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

A main thread dispatches calculations to worker threads. Numbers are dispatched in order from 2 to the highest number requested.

### Core Components

#### 1. ThreadPool (`struct ThreadPool`)

A generic thread pool implementation:
- Configurable number of worker threads (defaults to `num_cpus`)
- Can be specified via command-line argument

#### 2. PrimeCalculator (`struct PrimeCalculator`)

A struct that manages prime calculation tasks, tracking:
- Current busy thread count
- Minimum NIP (Number under Investigation for Primality) in progress
- Maximum NIP in progress
- Calculation start time
- Calculation end time

Returns a `PrimeResult` containing:
- `is_prime: bool`
- `calculation_time: Duration`

#### 3. PrimeCache (`struct PrimeCache`)

A shared data structure holding discovered primes:
- Uses `Arc<RwLock<Vec<u64>>>` for thread-safe shared access
- Tracks `confirmed_up_to: AtomicU64` - the highest number for which all primes have been confirmed
- Workers wait (via `Condvar`) until `confirmed_up_to >= sqrt(NIP)` before checking their number
- New primes are appended atomically; readers don't block other readers

#### 4. Worker Logic

Each worker thread:
1. Receives a number to check from a channel (`mpsc` or `crossbeam`)
2. Waits until `PrimeCache` has all primes up to `sqrt(NIP)`
3. Performs trial division against cached primes
4. Returns `PrimeResult` with timing data
5. If prime, appends to `PrimeCache` and signals waiting workers

### Synchronization Strategy

```
Main Thread                    Worker Threads (n)
    │                               │
    ├─── dispatch NIP via channel ──┼──► receive NIP
    │                               │
    │                               ├──► wait on Condvar if primes insufficient
    │                               │
    │                               ├──► trial division using RwLock read
    │                               │
    │                               ├──► if prime: RwLock write, notify Condvar
    │                               │
    ◄── collect results via channel ┤
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
├── thread_pool.rs    # Generic ThreadPool implementation
├── prime_cache.rs    # PrimeCache with synchronization
├── prime_worker.rs   # Worker logic and PrimeResult
└── calculator.rs     # PrimeCalculator coordinator
```

### Performance Considerations

1. **Warm-up phase**: Early primes (2, 3, 5, 7...) are computed nearly sequentially due to dependencies
2. **Parallelism increases**: Once primes up to ~1000 are found, workers rarely block
3. **Batch optimization**: Consider checking multiple numbers per task to reduce channel overhead
4. **Memory**: Storing all primes up to N uses approximately N / ln(N) entries (prime number theorem)
