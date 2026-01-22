use std::time::{Duration, Instant};

use crate::prime_cache::SharedPrimeCache;
use crate::wheel::{WheelBatchIter, WHEEL_PERIOD};

/// Result of checking a single number for primality
#[derive(Debug, Clone)]
pub struct PrimeResult {
    pub number: u64,
    pub is_prime: bool,
    pub duration: Duration,
}

/// Results from processing a batch of numbers
#[derive(Debug)]
pub struct BatchResult {
    pub base: u64,
    pub results: Vec<PrimeResult>,
    pub primes_found: Vec<u64>,
    pub total_duration: Duration,
}

/// Process the first batch (0-230) with incremental cache updates.
///
/// This must be called single-threaded before starting the thread pool.
/// It updates the cache after each prime is found to avoid deadlock
/// (later numbers in this batch need primes found earlier in the same batch).
pub fn process_first_batch(max: u64, cache: &SharedPrimeCache) -> BatchResult {
    let batch_start = Instant::now();
    let mut results = Vec::new();
    let mut primes_found = Vec::new();

    let batch_end = max.min(WHEEL_PERIOD - 1);

    for n in WheelBatchIter::new(0, batch_end) {
        // Skip numbers already seeded in cache (2, 3, 5, 7, 11)
        if n <= 11 {
            continue;
        }

        let start = Instant::now();

        // For first batch, we may need primes found earlier in this batch
        // Wait is usually instant since we update incrementally
        cache.wait_for_sqrt(n);

        let is_prime = cache.is_prime(n);
        let duration = start.elapsed();

        if is_prime {
            primes_found.push(n);
            // Immediately add to cache so later numbers can use it
            cache.add_primes(&[n]);
        }

        // Update confirmed_up_to after each number
        cache.confirm_up_to(n);

        results.push(PrimeResult {
            number: n,
            is_prime,
            duration,
        });
    }

    BatchResult {
        base: 0,
        results,
        primes_found,
        total_duration: batch_start.elapsed(),
    }
}

/// Process a batch of numbers (for batches 1+, used by thread pool).
///
/// Waits for sufficient primes at the start of the batch, then processes
/// all numbers without incremental updates (safe because needed primes
/// are already in cache from earlier batches).
pub fn process_batch(base: u64, max: u64, cache: &SharedPrimeCache) -> BatchResult {
    debug_assert!(base > 0, "Use process_first_batch for batch 0");

    let batch_start = Instant::now();
    let mut results = Vec::new();
    let mut primes_found = Vec::new();

    let batch_end = max.min(base + WHEEL_PERIOD - 1);

    // Wait once at batch start for primes up to sqrt(batch_end)
    cache.wait_for_sqrt(batch_end);

    for n in WheelBatchIter::new(base, batch_end) {
        let start = Instant::now();
        let is_prime = cache.is_prime(n);
        let duration = start.elapsed();

        if is_prime {
            primes_found.push(n);
        }

        results.push(PrimeResult {
            number: n,
            is_prime,
            duration,
        });
    }

    // Batch-level update at end (not per-number)
    cache.add_primes(&primes_found);
    cache.confirm_up_to(batch_end);

    BatchResult {
        base,
        results,
        primes_found,
        total_duration: batch_start.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prime_cache::new_shared_cache;

    #[test]
    fn process_first_batch_finds_primes() {
        let cache = new_shared_cache(1000);
        let result = process_first_batch(230, &cache);

        // Primes from 13-229 (2,3,5,7,11 are pre-seeded)
        let expected_primes: Vec<u64> = vec![
            13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97, 101,
            103, 107, 109, 113, 127, 131, 137, 139, 149, 151, 157, 163, 167, 173, 179, 181, 191,
            193, 197, 199, 211, 223, 227, 229,
        ];

        assert_eq!(result.primes_found.len(), expected_primes.len());
        for p in &result.primes_found {
            assert!(expected_primes.contains(p), "{} is not expected", p);
        }
    }

    #[test]
    fn first_batch_updates_cache() {
        let cache = new_shared_cache(1000);
        process_first_batch(230, &cache);

        // Cache should now have primes up to 229
        assert!(cache.get_confirmed_up_to() >= 229);

        // Should include prime 229
        let primes = cache.get_primes();
        assert!(primes.contains(&229));
    }

    #[test]
    fn second_batch_works_after_first() {
        let cache = new_shared_cache(1000);

        // Process first batch
        process_first_batch(230, &cache);

        // Now process second batch (231-461)
        let result = process_batch(231, 461, &cache);

        // Should find some primes
        assert!(!result.primes_found.is_empty());

        // 233 is prime
        assert!(result.primes_found.contains(&233));
    }
}
