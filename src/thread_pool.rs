use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

type Job = Box<dyn FnOnce() + Send + 'static>;

/// A simple thread pool for executing jobs in parallel.
///
/// Workers pull jobs from a shared queue and execute them.
/// The pool shuts down gracefully when dropped.
pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: Option<Sender<Job>>,
}

impl ThreadPool {
    /// Create a new thread pool with the specified number of workers.
    ///
    /// # Panics
    /// Panics if size is 0.
    pub fn new(size: usize) -> Self {
        assert!(size > 0, "Thread pool size must be at least 1");

        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));

        let workers: Vec<Worker> = (0..size)
            .map(|id| Worker::new(id, Arc::clone(&receiver)))
            .collect();

        ThreadPool {
            workers,
            sender: Some(sender),
        }
    }

    /// Create a thread pool with one worker per CPU core.
    pub fn new_default() -> Self {
        Self::new(num_cpus::get())
    }

    /// Submit a job to be executed by the pool.
    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        self.sender.as_ref().unwrap().send(job).unwrap();
    }

    /// Get the number of workers in the pool.
    pub fn size(&self) -> usize {
        self.workers.len()
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        // Drop the sender to signal workers to shut down
        drop(self.sender.take());

        // Wait for all workers to finish
        for worker in &mut self.workers {
            if let Some(handle) = worker.handle.take() {
                handle.join().unwrap();
            }
        }
    }
}

struct Worker {
    #[allow(dead_code)]
    id: usize,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<Receiver<Job>>>) -> Self {
        let handle = thread::spawn(move || loop {
            // Lock, receive, then immediately unlock before executing
            let job = {
                let lock = receiver.lock().unwrap();
                lock.recv()
            };

            match job {
                Ok(job) => job(),
                Err(_) => break, // Channel closed, shut down
            }
        });

        Worker {
            id,
            handle: Some(handle),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn pool_executes_jobs() {
        let pool = ThreadPool::new(4);
        let counter = Arc::new(AtomicUsize::new(0));

        for _ in 0..100 {
            let counter = Arc::clone(&counter);
            pool.execute(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            });
        }

        drop(pool); // Wait for all jobs to complete
        assert_eq!(counter.load(Ordering::SeqCst), 100);
    }

    #[test]
    fn pool_uses_multiple_threads() {
        let pool = ThreadPool::new(4);
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));

        for _ in 0..8 {
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            pool.execute(move || {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                // Update max if current is higher
                let mut max = max_active.load(Ordering::SeqCst);
                while current > max {
                    match max_active.compare_exchange(
                        max,
                        current,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    ) {
                        Ok(_) => break,
                        Err(m) => max = m,
                    }
                }
                thread::sleep(Duration::from_millis(50));
                active.fetch_sub(1, Ordering::SeqCst);
            });
        }

        drop(pool);
        // Should have had multiple jobs running concurrently
        assert!(max_active.load(Ordering::SeqCst) > 1);
    }
}
