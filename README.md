# Prime Number Calculator

A multi-threaded prime number calculator written in Rust, using wheel factorization and parallel batch processing.

## Building

```bash
cargo build --release
```

## Usage

```bash
# Find all primes up to 1 million
./target/release/primes 1000000

# Use 8 worker threads
./target/release/primes 1000000 --threads=8

# Show progress every 5 seconds (default)
./target/release/primes 100000000 --progress

# Show progress every 2 seconds
./target/release/primes 100000000 --progress=2

# List all found primes
./target/release/primes 1000 --list
```

### Options

| Option | Description |
|--------|-------------|
| `--threads=N` | Number of worker threads (default: CPU core count) |
| `--progress[=N]` | Show progress every N seconds (default: 5) |
| `--list` | Print all discovered primes |
| `--help` | Show usage information |

## Performance

The calculator uses:
- **Wheel factorization** (3, 7, 11) with runtime filtering for 2 and 5
- **Batch processing** — 231 numbers per batch dispatched to thread pool
- **RwLock-based prime cache** — multiple readers during trial division
- **Condvar waiting** — workers sleep (not spin) when waiting for dependencies

Typical throughput on a 4-core system: ~2-3 million numbers/second.

## Limitations

### Maximum Number: ~9 × 10^15 (practical)

While the calculator accepts `u64` values up to 2^64, **full precision is only guaranteed up to ~2^53** (9,007,199,254,740,992).

**Why?** The square root calculation uses `f64` for speed. The f64 type has 53 bits of mantissa precision, so numbers above 2^53 may have imprecise sqrt calculations.

**Mitigations in place:**
- Correction loop detects and fixes small sqrt errors
- Automatic fallback to integer Newton's method if corrections exceed threshold
- Warning printed if fallback is triggered

For numbers below 2^53, results are guaranteed correct.

### Memory Usage

The calculator stores all discovered primes in memory. Approximate memory usage:

| Max N | Primes | Memory |
|-------|--------|--------|
| 10^6 | ~78K | ~0.6 MB |
| 10^9 | ~50M | ~400 MB |
| 10^12 | ~37B | ~300 GB |

Memory is pre-allocated based on the prime number theorem estimate: `N / ln(N)`.

## Architecture

See [DESIGN_NOTES.md](DESIGN_NOTES.md) for detailed design rationale covering:
- Wheel factorization strategy
- Thread pool design
- PrimeCache synchronization
- Square root implementation choices

## Testing

```bash
cargo test
```

## License

[Add your license here]
