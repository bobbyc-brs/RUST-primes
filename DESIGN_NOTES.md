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
