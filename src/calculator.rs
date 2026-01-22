use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::prime_cache::{new_shared_cache, SharedPrimeCache};
use crate::prime_worker::{process_batch, process_first_batch, BatchResult};
use crate::thread_pool::ThreadPool;
use crate::wheel::WHEEL_PERIOD;

/// Configuration for the prime calculator
#[derive(Debug, Clone)]
pub struct CalculatorConfig {
    /// Maximum number to check for primality
    pub max_n: u64,
    /// Number of worker threads (None = use CPU count)
    pub num_threads: Option<usize>,
}

impl CalculatorConfig {
    pub fn new(max_n: u64) -> Self {
        Self {
            max_n,
            num_threads: None,
        }
    }

    pub fn with_threads(mut self, threads: usize) -> Self {
        self.num_threads = Some(threads);
        self
    }
}

/// Statistics from a calculation run
#[derive(Debug)]
pub struct CalculatorStats {
    pub max_n: u64,
    pub primes_found: usize,
    pub total_duration: Duration,
    pub batches_processed: usize,
    pub threads_used: usize,
}

/// Coordinates parallel prime calculation across worker threads.
pub struct PrimeCalculator {
    config: CalculatorConfig,
    cache: SharedPrimeCache,
}

impl PrimeCalculator {
    pub fn new(config: CalculatorConfig) -> Self {
        let cache = new_shared_cache(config.max_n);
        Self { config, cache }
    }

    /// Run the calculation and return statistics.
    pub fn run(&self) -> CalculatorStats {
        let start = Instant::now();

        let num_threads = self.config.num_threads.unwrap_or_else(num_cpus::get);

        // Phase 1: Process first batch single-threaded (incremental updates)
        // This avoids deadlock where later numbers need primes found earlier in same batch
        let _first_result = process_first_batch(self.config.max_n, &self.cache);
        let mut batches_processed = 1;

        // If max_n <= 230, we're done (first batch covers everything)
        if self.config.max_n < WHEEL_PERIOD {
            return CalculatorStats {
                max_n: self.config.max_n,
                primes_found: self.cache.prime_count(),
                total_duration: start.elapsed(),
                batches_processed,
                threads_used: 1,
            };
        }

        // Phase 2: Process remaining batches with thread pool
        let pool = ThreadPool::new(num_threads);
        let (result_tx, result_rx) = mpsc::channel::<BatchResult>();

        // Calculate number of remaining batches (starting from batch 1)
        let num_batches = (self.config.max_n / WHEEL_PERIOD) + 1;

        for batch_idx in 1..num_batches {
            let base = batch_idx * WHEEL_PERIOD;
            let max = self.config.max_n;
            let cache = self.cache.clone();
            let tx = result_tx.clone();

            pool.execute(move || {
                let result = process_batch(base, max, &cache);
                let _ = tx.send(result);
            });
        }

        // Drop our sender so the receiver knows when all batches are done
        drop(result_tx);

        // Collect all results
        for _result in result_rx {
            batches_processed += 1;
        }

        // Wait for pool to finish (happens on drop)
        drop(pool);

        CalculatorStats {
            max_n: self.config.max_n,
            primes_found: self.cache.prime_count(),
            total_duration: start.elapsed(),
            batches_processed,
            threads_used: num_threads,
        }
    }

    /// Get all discovered primes after calculation
    pub fn get_primes(&self) -> Vec<u64> {
        self.cache.get_primes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_primes_to_100() {
        let config = CalculatorConfig::new(100).with_threads(2);
        let calc = PrimeCalculator::new(config);
        let stats = calc.run();

        let primes = calc.get_primes();

        // Primes up to 100: 2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47,
        //                   53, 59, 61, 67, 71, 73, 79, 83, 89, 97
        // That's 25 primes
        assert_eq!(stats.primes_found, 25);
        assert_eq!(primes.len(), 25);

        // Verify first and last
        assert_eq!(primes[0], 2);
        assert_eq!(*primes.last().unwrap(), 97);
    }

    #[test]
    fn calculates_primes_to_1000() {
        let config = CalculatorConfig::new(1000).with_threads(4);
        let calc = PrimeCalculator::new(config);
        let stats = calc.run();

        // There are 168 primes <= 1000
        assert_eq!(stats.primes_found, 168);
    }
}
