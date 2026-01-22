# Design Notes

This document captures the evolution of design decisions for the prime calculator.

## Wheel Factorization Discussion

### Initial Approach: Standard Wheel (2, 3, 5, 7)

The conventional wheel factorization uses consecutive primes starting from 2:

| Wheel Primes | Period | Candidates | Efficiency |
|--------------|--------|------------|------------|
| 2 | 2 | 1 | 50.0% |
| 2, 3 | 6 | 2 | 33.3% |
| 2, 3, 5 | 30 | 8 | 26.7% |
| 2, 3, 5, 7 | 210 | 48 | 22.9% |

This requires storing 48 offsets and iterating or looking up against them.

### Alternative: Wheel (3, 7, 11) with Runtime Filtering

The insight: **filtering for 2 and 5 is nearly free at runtime**, so we can use a different wheel that may have other advantages.

Using primes 3 × 7 × 11 = 231:
- φ(231) = 2 × 6 × 10 = 120 candidates coprime to 3, 7, 11
- After filtering evens: 60 candidates
- After filtering multiples of 5: 48 candidates

**Result:** 48/231 ≈ 20.8% — slightly better than the standard 22.9%

### Why Skip 2 and 5 in the Wheel?

1. **Skipping evens is free**: The main loop can use `step_by(2)` or check `n & 1 == 0`
2. **Skipping multiples of 5 is cheap**: Check `n % 5 == 0` or `n % 10 == 5` (since n is odd)
3. **No lookup required**: Iterate through wheel offsets directly, filter at runtime

### Iteration vs Lookup

**Original concern:** Using a bitmap or lookup table to check if a number is a wheel candidate requires:
- Computing `n % 231` (modulo operation)
- Array/bitmap access (potential cache miss)

**Better approach:** Iterate directly through the wheel offsets:

```rust
const WHEEL_231: [u16; 120] = [1, 2, 4, 5, 8, 10, ...];

for &offset in &WHEEL_231 {
    let n = base + offset;
    if 0 == n & 1 { continue; }   // Skip even
    if n % 5 == 0 { continue; }   // Skip multiples of 5
    // ... trial division
}
```

This is:
- **Cache-friendly**: Sequential array traversal
- **Branch-predictor friendly**: The even/mod-5 pattern repeats predictably
- **No modulo for wheel position**: Just iterate

### Batching Strategy

The wheel period (231) naturally defines a batch size for parallel dispatch:

```rust
// Main thread
for base in (0..max).step_by(231) {
    thread_pool.submit(base);
}
```

**Benefits:**
- One dispatch message per 231 numbers (not per candidate)
- ~48 candidates per batch (predictable work size)
- Workers wait for √(base+231) primes once per batch, not per candidate

### Summary of Design Choice

| Aspect | Standard Wheel (2,3,5,7) | Our Wheel (3,7,11) + Filter |
|--------|--------------------------|----------------------------|
| Period | 210 | 231 |
| Stored offsets | 48 | 120 |
| Candidates per period | 48 (22.9%) | 48 (20.8%) |
| Even filtering | In wheel | Runtime `n & 1` |
| Mod-5 filtering | In wheel | Runtime `n % 5` |
| Access pattern | Lookup or iterate | Iterate only |

The (3, 7, 11) wheel with runtime filtering provides:
- Slightly better candidate ratio (20.8% vs 22.9%)
- Simpler iteration model (no lookup)
- Natural batch size for parallel dispatch

## Thread Pool Rationale

### Why a Thread Pool?

**Alternative 1: Spawn a thread per candidate**
```rust
for n in candidates {
    std::thread::spawn(move || check_prime(n));
}
```
Problems:
- Thread creation overhead (~10-50μs per spawn on Linux)
- OS scheduler thrashing with thousands of threads
- Stack allocation per thread (~2MB default on Linux)
- For 1 million candidates: 1M threads = ~2TB virtual memory

**Alternative 2: Single-threaded**
- Leaves N-1 cores idle
- Primality testing is CPU-bound and embarrassingly parallel
- Wasted hardware

**Alternative 3: Thread pool (chosen)**
- Fixed number of threads (typically = CPU cores)
- Threads are reused across many tasks
- Work is distributed via channels
- Minimal overhead: one channel send/receive per batch

### Pool Size Considerations

Default: `num_cpus::get()` (logical cores including hyperthreads)

Hyperthreading tradeoff:
- Trial division is ALU-heavy, benefits somewhat from HT
- Memory access (reading prime cache) may benefit from HT hiding latency
- Recommendation: default to logical cores, allow CLI override for tuning

### Work Stealing vs Fixed Assignment

**Fixed assignment** (simpler):
- Each batch goes to one worker
- Workers pull from a shared queue
- Potential imbalance if some batches take longer

**Work stealing** (more complex):
- Workers can steal from other workers' queues
- Better load balancing
- Libraries like `rayon` provide this

For our design: Start with fixed assignment via channels. The batch size (231 numbers, ~48 candidates) is small enough that imbalance is minimal. Consider `rayon` if profiling shows load imbalance.

## PrimeCache Design

### The Core Problem

Workers need read access to discovered primes for trial division, but also need to write new primes when found. This is a classic readers-writer problem with an ordering constraint.

### Naive Approach: `Mutex<Vec<u64>>`

```rust
let cache = Arc::new(Mutex::new(Vec::new()));
```

Problems:
- Every read locks out all other readers
- Trial division reads many primes per candidate
- Workers serialize, destroying parallelism

### Better: `RwLock<Vec<u64>>`

```rust
let cache = Arc::new(RwLock::new(Vec::new()));
```

- Multiple readers can hold the lock simultaneously
- Only writers need exclusive access
- Much better for read-heavy workloads

**But still has issues:**
- Writers block all readers (and vice versa)
- Frequent small writes (each new prime) cause contention

### Optimized Design: Separate Concerns

Split the cache into components with different synchronization needs:

```rust
struct PrimeCache {
    // The actual prime storage - rarely written in bulk
    primes: RwLock<Vec<u64>>,

    // Progress tracking - updated atomically, read frequently
    confirmed_up_to: AtomicU64,

    // Notification for waiting workers
    progress_condvar: Condvar,
    progress_mutex: Mutex<()>,  // Condvar requires a mutex
}
```

**Why this helps:**

1. **`confirmed_up_to: AtomicU64`**
   - Workers check `confirmed_up_to >= sqrt(n)` before proceeding
   - Atomic read: no lock, no contention
   - Updated only when a batch completes (not per-prime)

2. **`primes: RwLock<Vec<u64>>`**
   - Readers acquire read lock, iterate for trial division
   - Writers acquire write lock only to append new primes
   - Writes are batched: worker collects primes from batch, writes once

3. **`Condvar` for waiting**
   - Workers processing high numbers wait for lower batches to complete
   - `Condvar::wait()` sleeps the thread (no busy-waiting)
   - `Condvar::notify_all()` wakes waiters when progress is made

### Memory Layout Considerations

```rust
primes: Vec<u64>
```

- Contiguous memory: cache-friendly iteration
- Append-only: no reallocation churn once capacity stabilizes
- Pre-allocate based on prime number theorem: `capacity ≈ max / ln(max)`

### Batch Writes to Reduce Contention

Instead of:
```rust
// Bad: lock per prime
for n in batch_results {
    if n.is_prime {
        cache.primes.write().push(n.value);  // Lock acquired/released each time
    }
}
```

Do:
```rust
// Good: collect then write once
let new_primes: Vec<u64> = batch_results
    .iter()
    .filter(|r| r.is_prime)
    .map(|r| r.value)
    .collect();

{
    let mut primes = cache.primes.write();
    primes.extend(new_primes);
}  // Lock released

cache.confirmed_up_to.store(batch_end, Ordering::Release);
cache.progress_condvar.notify_all();
```

### The Ordering Constraint

Workers checking number `n` need all primes up to `√n`. This creates a dependency:

```
Batch 0 (0-230)    → finds primes 2, 3, 5, 7, 11, ...
Batch 1 (231-461)  → needs primes up to √461 ≈ 21 → must wait for batch 0
Batch 2 (462-692)  → needs primes up to √692 ≈ 26 → must wait for batch 0
...
Batch 44 (10164-10394) → needs primes up to √10394 ≈ 102 → must wait for batches finding primes up to 102
```

**Implication:** Early batches are sequential, but parallelism increases rapidly. By batch ~44, workers rarely wait because primes up to ~100 are long since confirmed.

### Wait Strategy

```rust
fn wait_for_sqrt(&self, n: u64) {
    let sqrt_n = (n as f64).sqrt() as u64;

    loop {
        // Fast path: atomic check, no lock
        if self.confirmed_up_to.load(Ordering::Acquire) >= sqrt_n {
            return;
        }

        // Slow path: sleep until progress
        let guard = self.progress_mutex.lock().unwrap();
        // Re-check after acquiring mutex (avoid race)
        if self.confirmed_up_to.load(Ordering::Acquire) >= sqrt_n {
            return;
        }
        self.progress_condvar.wait(guard).unwrap();
    }
}
```

**Why this pattern:**
1. Fast path avoids mutex entirely (common case once warmed up)
2. Slow path sleeps instead of spinning (saves CPU)
3. Re-check after mutex prevents missed-wakeup race

### Sorted Insert Optimization

Batches may complete out of order. A batch processing numbers 1000-1230 might finish before a batch processing 500-730. If we just append primes, the list becomes unsorted.

**Naive fix:** Sort on every read — expensive, O(n log n) per read.

**Better fix:** Maintain sorted order on insert.

**Naive insert:** Insert each prime individually with `Vec::insert()` — O(n) per prime, O(n × m) total.

**Efficient insert:** Bulk insert with single shift:

```rust
pub fn add_primes(&self, new_primes: &[u64]) {
    // Sort incoming batch
    let mut sorted_new = new_primes.to_vec();
    sorted_new.sort_unstable();

    let mut primes = self.primes.write().unwrap();

    // Find insertion point for smallest new prime (search from end)
    let first = sorted_new[0];
    let mut insert_pos = primes.len();
    while insert_pos > 0 && primes[insert_pos - 1] > first {
        insert_pos -= 1;
    }

    // Bulk insert: splice shifts tail once, inserts all new primes
    primes.splice(insert_pos..insert_pos, sorted_new);
}
```

**Why this works:**
- All primes from one batch are contiguous in the number line
- They all insert at the same position in the sorted list
- `splice` does one shift of the tail, then copies the batch in
- O(n) for the shift + O(m log m) for sorting the batch

**Searching from the end:** New primes are typically larger than most existing ones, so searching backwards finds the insertion point faster.

### Safe List Portion for Readers (TODO)

**Problem:** When a reader is iterating the prime list for trial division, a writer might insert primes in the middle (due to out-of-order batch completion). This could cause issues if the reader is mid-iteration.

**Current mitigation:** `RwLock` prevents concurrent read/write.

**Future optimization:** Track `min_in_progress` — the minimum number currently being calculated by any worker thread. Primes below this value are "stable" (no batch will insert there). Readers doing trial division only need primes up to `√n`, which is much smaller than `n`. If `√n < min_in_progress`, the reader can safely iterate without lock contention.

**Implementation sketch:**
```rust
struct PrimeCache {
    primes: RwLock<Vec<u64>>,
    confirmed_up_to: AtomicU64,
    min_in_progress: AtomicU64,  // Track minimum batch base across all workers
    // ...
}
```

Workers would:
1. Register their batch base with `min_in_progress` on start
2. Deregister on completion
3. Readers check if `√n < min_in_progress` — if so, safe to read without full lock

This optimization is deferred until profiling shows lock contention is a bottleneck.

## Progress Reporting

### Design Goals

For long-running calculations, users need visibility into progress without impacting performance.

### Implementation: Separate Thread

```rust
let progress_handle = config.progress_interval.map(|interval_secs| {
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(interval);
            // Report progress...
        }
    })
});
```

**Why a separate thread:**
- Doesn't block or slow down worker threads
- Can sleep independently of calculation pace
- Clean shutdown via `AtomicBool` flag

**Why `Option<u64>` for the interval:**

The `progress_interval` field uses `Option<u64>` rather than a sentinel value like `-1`:

| Approach | Memory | Runtime Cost |
|----------|--------|--------------|
| `Option<u64>` | 16 bytes | One-time check at startup |
| `i64` with `-1` sentinel | 8 bytes | Same one-time check |

The 8-byte overhead is negligible for a config field. Benefits of `Option`:
- Type-safe: can't accidentally use sentinel as real value
- Self-documenting: type signature shows "might be absent"
- Compiler-enforced: must handle `None` case

Once the thread spawns, it captures a plain `u64` interval—no `Option` overhead in the hot path.

## Square Root Implementation

### The Problem

Trial division needs `√n` to know when to stop checking factors. For `u64` values, we need a correct integer square root.

### Naive Approach: f64

```rust
let sqrt_n = (n as f64).sqrt() as u64;
```

**Fast** (~15 CPU cycles), but **loses precision above 2^53** because f64 has only 53 bits of mantissa. For `n > 2^53`, the conversion `n as f64` rounds, potentially giving wrong sqrt.

### Naive Approach: Pure Integer Newton

```rust
loop {
    let x1 = (x + n / x) / 2;
    if x1 >= x { return x; }
    x = x1;
}
```

**Correct** for all u64, but **slower** (~6 iterations × division cost).

### Chosen Approach: Hybrid with Monitoring

```rust
fn fast_sqrt(n: u64) -> u64 {
    let f64_guess = (n as f64).sqrt() as u64;

    // Try f64 with correction loop
    let mut sqrt_n = f64_guess;
    while sqrt_n * sqrt_n > n { sqrt_n -= 1; }
    while (sqrt_n + 1) * (sqrt_n + 1) <= n { sqrt_n += 1; }

    sqrt_n
}
```

**Benefits:**
- Fast path: f64 is exact for n < 2^53 (correction loops execute 0 times)
- Safe: correction handles any f64 imprecision
- Monitored: if corrections exceed threshold, switch to Newton

### Newton's Method: Two-Phase Design

When `fast_sqrt` falls back to Newton, we pass the f64 estimate as an initial guess:

```rust
fn integer_sqrt(n: u64, guess: Option<u64>) -> u64 {
    // Phase 1: If below sqrt(n), Newton steps upward until we're at/above
    while x.saturating_mul(x) < n {
        x = (x + n / x) / 2;
    }

    // Phase 2: Standard Newton convergence from above
    loop {
        let x1 = (x + n / x) / 2;
        if x1 >= x { return x; }
        x = x1;
    }
}
```

**Why two phases with the same formula?**

This is subtle and worth documenting. Newton's method for sqrt: `x_new = (x + n/x) / 2`

The standard termination `x1 >= x` assumes convergence from ABOVE:
- If `x > √n`: then `n/x < √n`, so `x1 = (x + n/x)/2 < x` (decreases)
- If `x = √n`: then `x1 = x` (stable)
- If `x < √n`: then `n/x > √n`, so `x1 > x` (increases!)

The problem: when starting below √n, the first iteration jumps UP, and `x1 >= x` triggers immediately—returning the wrong answer.

**Solution:** Two phases with different termination conditions:
- Phase 1: `x*x < n` — keep iterating until we're AT or ABOVE √n
- Phase 2: `x1 >= x` — standard Newton convergence from above

Both use the identical Newton formula. Phase 1 ensures we reach a valid starting point for phase 2's termination condition.

**Example trace for n=100, guess=9:**
```
Phase 1: x=9, x²=81 < 100
         x = (9 + 100/9) / 2 = 10
         x=10, x²=100, not < 100, exit phase 1
Phase 2: x1 = (10 + 100/10) / 2 = 10
         x1 >= x, return 10 ✓
```

**Example trace for n=10000, guess=1:**
```
Phase 1: x=1, x²=1 < 10000
         x = (1 + 10000/1) / 2 = 5000
         x²=25000000 > 10000, exit phase 1
Phase 2: converges 5000 → 2501 → 1252 → ... → 100 ✓
```
