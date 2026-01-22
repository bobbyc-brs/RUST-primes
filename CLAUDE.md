# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build System and Common Commands

```bash
cargo build              # Build debug version
cargo build --release    # Build optimized release version
cargo run                # Build and run debug version
cargo run --release      # Build and run optimized version
cargo test               # Run all tests
cargo clippy             # Run linter
cargo fmt                # Format code
```

## Project Purpose/Goals

Create a prime number calculator in Rust that uses multiple (e.g. all of the) cores on a system via multi-threading.
We want to have an efficient design with as little copying of data as necessary while still keeping the system secure (and the borrow checker happy).

## Project Architecture

### Overview

A main thread dispatches batched calculations to worker threads using wheel factorization. Batches of 231 consecutive numbers are dispatched to workers, who iterate through wheel offsets to find prime candidates.

### Wheel Factorization Strategy

We use a wheel based on primes 3 × 7 × 11 = 231, combined with runtime filtering:

```rust
// Main thread - dispatcher
for base in (0..max).step_by(231) {
    thread_pool.submit(base);
}

// Worker thread - processes one batch
fn collect_primes(base: u64, cache: &PrimeCache) -> Vec<PrimeResult> {
    let mut results = Vec::new();

    for &offset in &WHEEL_231 {
        let n = base + offset;
        if 0 == n & 1 { continue; }   // Skip even
        if n % 5 == 0 { continue; }   // Skip multiples of 5

        cache.wait_for_sqrt(n);
        let is_prime = check_prime(n, cache);
        results.push(PrimeResult { n, is_prime, ... });
    }
    results
}
```

**Layered filtering:**

| Layer | Mechanism | Cost |
|-------|-----------|------|
| Skip ×3, ×7, ×11 | Wheel iteration (not lookup) | Sequential array traversal |
| Skip evens | `n & 1 == 0` | Single AND instruction |
| Skip ×5 | `n % 5 == 0` | One modulo (or check last digit == 5) |

**Efficiency:** 48 candidates checked per 231 numbers ≈ 20.8%

### Core Components

#### 1. ThreadPool (`struct ThreadPool`)

A generic thread pool implementation:
- Configurable number of worker threads (defaults to `num_cpus`)
- Can be specified via command-line argument
- Receives batch base values, not individual candidates

#### 2. PrimeCalculator (`struct PrimeCalculator`)

A struct that manages prime calculation tasks, tracking:
- Current busy thread count
- Minimum batch base in progress
- Maximum batch base in progress
- Calculation start time
- Calculation end time

Returns a `PrimeResult` containing:
- `is_prime: bool`
- `calculation_time: Duration`

#### 3. PrimeCache (`struct PrimeCache`)

A shared data structure holding discovered primes:
- Uses `Arc<RwLock<Vec<u64>>>` for thread-safe shared access
- Tracks `confirmed_up_to: AtomicU64` - the highest number for which all primes have been confirmed
- Workers wait (via `Condvar`) until `confirmed_up_to >= sqrt(n)` before checking
- New primes are appended atomically; readers don't block other readers

#### 4. Wheel (`const WHEEL_231`)

A compile-time constant array of 120 offsets (numbers 1-231 coprime to 3, 7, 11):
- Workers iterate directly through offsets (no lookup/bitmap)
- Runtime checks filter evens (`n & 1`) and multiples of 5 (`n % 5`)
- Cache-friendly sequential access pattern

### Synchronization Strategy

```
Main Thread                    Worker Threads (n)
    │                               │
    ├─── dispatch batch base ───────┼──► receive base
    │    (one per 231 numbers)      │
    │                               ├──► iterate WHEEL_231 offsets
    │                               │
    │                               ├──► wait on Condvar (once per batch)
    │                               │
    │                               ├──► trial division using RwLock read
    │                               │
    │                               ├──► if prime: RwLock write, notify Condvar
    │                               │
    ◄── collect batch results ──────┤
```

### Key Rust Constructs

- `std::sync::Arc` - shared ownership across threads
- `std::sync::RwLock` - multiple readers OR single writer
- `std::sync::atomic::AtomicU64` - lock-free progress tracking
- `std::sync::Condvar` - workers wait for sufficient primes
- `std::sync::mpsc` or `crossbeam::channel` - task/result passing
- `std::thread::spawn` or `rayon` for thread pool

### Module Structure

```
src/
├── main.rs           # CLI parsing, orchestration
├── lib.rs            # Public API
├── wheel.rs          # WHEEL_231 constant and iteration helpers
├── thread_pool.rs    # Generic ThreadPool implementation
├── prime_cache.rs    # PrimeCache with synchronization
├── prime_worker.rs   # Worker logic and PrimeResult
└── calculator.rs     # PrimeCalculator coordinator
```

### Performance Considerations

1. **Batch dispatch**: One message per 231 numbers reduces channel overhead
2. **Wheel iteration**: Sequential array traversal, not random-access lookup
3. **Warm-up phase**: Early primes (2, 3, 5, 7...) computed nearly sequentially
4. **Parallelism increases**: Once primes up to √(base+231) exist, batches run independently
5. **Memory**: Storing all primes up to N uses approximately N / ln(N) entries

See [DESIGN_NOTES.md](DESIGN_NOTES.md) for the evolution of this design.
