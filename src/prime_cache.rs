use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};

/// Thread-safe cache of discovered prime numbers.
///
/// Design optimizations:
/// - `RwLock<Vec<u64>>` allows multiple readers during trial division
/// - `AtomicU64` for progress tracking avoids lock contention
/// - `Condvar` for sleeping (not spinning) when waiting for primes
pub struct PrimeCache {
    /// The discovered primes, in ascending order
    primes: RwLock<Vec<u64>>,

    /// Highest number for which all primes have been confirmed.
    /// Workers can proceed if confirmed_up_to >= sqrt(n).
    confirmed_up_to: AtomicU64,

    /// Minimum batch base currently in progress (for status reporting)
    /// Uses u64::MAX to indicate no batches in progress
    min_in_progress: AtomicU64,

    /// Condition variable for workers waiting on progress
    progress_condvar: Condvar,
    progress_mutex: Mutex<()>,
}

impl PrimeCache {
    /// Create a new cache, optionally pre-seeded with small primes.
    pub fn new() -> Self {
        Self {
            primes: RwLock::new(vec![2, 3, 5, 7, 11]),
            confirmed_up_to: AtomicU64::new(11),
            min_in_progress: AtomicU64::new(u64::MAX),
            progress_condvar: Condvar::new(),
            progress_mutex: Mutex::new(()),
        }
    }

    /// Create a cache with estimated capacity based on prime number theorem.
    /// For n numbers, expect approximately n / ln(n) primes.
    pub fn with_capacity(max_n: u64) -> Self {
        let estimated_primes = if max_n > 10 {
            (max_n as f64 / (max_n as f64).ln()) as usize
        } else {
            10
        };

        let mut primes = Vec::with_capacity(estimated_primes);
        primes.extend_from_slice(&[2, 3, 5, 7, 11]);

        Self {
            primes: RwLock::new(primes),
            confirmed_up_to: AtomicU64::new(11),
            min_in_progress: AtomicU64::new(u64::MAX),
            progress_condvar: Condvar::new(),
            progress_mutex: Mutex::new(()),
        }
    }

    /// Wait until we have confirmed all primes up to at least sqrt(n).
    /// Uses fast atomic check, falls back to condvar sleep if needed.
    pub fn wait_for_sqrt(&self, n: u64) {
        let sqrt_n = (n as f64).sqrt() as u64;

        // Fast path: check atomically without any lock
        if self.confirmed_up_to.load(Ordering::Acquire) >= sqrt_n {
            return;
        }

        // Slow path: need to wait
        let mut guard = self.progress_mutex.lock().unwrap();
        loop {
            // Re-check after acquiring mutex (avoid missed wakeup)
            if self.confirmed_up_to.load(Ordering::Acquire) >= sqrt_n {
                return;
            }
            guard = self.progress_condvar.wait(guard).unwrap();
        }
    }

    /// Check if n is prime using trial division against cached primes.
    /// Caller must ensure wait_for_sqrt(n) has returned first.
    pub fn is_prime(&self, n: u64) -> bool {
        if n < 2 {
            return false;
        }
        if n == 2 {
            return true;
        }
        if n & 1 == 0 {
            return false;
        }

        let sqrt_n = (n as f64).sqrt() as u64;
        let primes = self.primes.read().unwrap();

        for &p in primes.iter() {
            if p > sqrt_n {
                break;
            }
            if n % p == 0 {
                return false;
            }
        }

        true
    }

    /// Add newly discovered primes from a completed batch.
    /// Maintains sorted order by inserting at the correct position.
    /// Uses efficient bulk insert: sort batch, find position, splice once.
    pub fn add_primes(&self, new_primes: &[u64]) {
        if new_primes.is_empty() {
            return;
        }

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

    /// Mark that all numbers up to `up_to` have been checked.
    /// Wakes any workers waiting for these primes.
    pub fn confirm_up_to(&self, up_to: u64) {
        // Only update if this is progress
        let current = self.confirmed_up_to.load(Ordering::Acquire);
        if up_to > current {
            self.confirmed_up_to.store(up_to, Ordering::Release);
            self.progress_condvar.notify_all();
        }
    }

    /// Get current confirmation progress
    pub fn get_confirmed_up_to(&self) -> u64 {
        self.confirmed_up_to.load(Ordering::Acquire)
    }

    /// Update min_in_progress if this batch is lower than current min
    pub fn update_min_in_progress(&self, batch_base: u64) {
        self.min_in_progress.fetch_min(batch_base, Ordering::AcqRel);
    }

    /// Clear min_in_progress (set to MAX) - call when all batches done
    pub fn clear_min_in_progress(&self) {
        self.min_in_progress.store(u64::MAX, Ordering::Release);
    }

    /// Get current min_in_progress (returns None if no batches in progress)
    pub fn get_min_in_progress(&self) -> Option<u64> {
        let val = self.min_in_progress.load(Ordering::Acquire);
        if val == u64::MAX { None } else { Some(val) }
    }

    /// Get the number of primes found so far
    pub fn prime_count(&self) -> usize {
        self.primes.read().unwrap().len()
    }

    /// Get a copy of all primes (for final output)
    pub fn get_primes(&self) -> Vec<u64> {
        self.primes.read().unwrap().clone()
    }
}

impl Default for PrimeCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared reference to a PrimeCache for use across threads
pub type SharedPrimeCache = Arc<PrimeCache>;

pub fn new_shared_cache(max_n: u64) -> SharedPrimeCache {
    Arc::new(PrimeCache::with_capacity(max_n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_starts_with_small_primes() {
        let cache = PrimeCache::new();
        let primes = cache.get_primes();
        assert_eq!(primes, vec![2, 3, 5, 7, 11]);
    }

    #[test]
    fn is_prime_works_for_small_numbers() {
        let cache = PrimeCache::new();

        assert!(!cache.is_prime(0));
        assert!(!cache.is_prime(1));
        assert!(cache.is_prime(2));
        assert!(cache.is_prime(3));
        assert!(!cache.is_prime(4));
        assert!(cache.is_prime(5));
        assert!(!cache.is_prime(6));
        assert!(cache.is_prime(7));
        assert!(!cache.is_prime(9));
        assert!(cache.is_prime(11));
    }

    #[test]
    fn add_primes_extends_cache() {
        let cache = PrimeCache::new();
        cache.add_primes(&[13, 17, 19]);

        let primes = cache.get_primes();
        assert_eq!(primes, vec![2, 3, 5, 7, 11, 13, 17, 19]);
    }

    #[test]
    fn add_primes_maintains_sorted_order_out_of_order_batches() {
        let cache = PrimeCache::new();
        // Simulate batches completing out of order
        cache.add_primes(&[101, 103, 107]);  // Higher batch finishes first
        cache.add_primes(&[13, 17, 19]);     // Lower batch finishes later

        let primes = cache.get_primes();
        assert_eq!(primes, vec![2, 3, 5, 7, 11, 13, 17, 19, 101, 103, 107]);
    }

    #[test]
    fn wait_for_sqrt_returns_immediately_when_ready() {
        let cache = PrimeCache::new();
        // confirmed_up_to is 11, sqrt(100) = 10, so should return immediately
        cache.wait_for_sqrt(100);
    }
}
