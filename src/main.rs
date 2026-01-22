mod calculator;
mod prime_cache;
mod prime_worker;
mod thread_pool;
mod wheel;

use std::env;
use std::process;

use calculator::{CalculatorConfig, PrimeCalculator};

fn print_usage(program: &str) {
    eprintln!("Usage: {} <max_number> [options]", program);
    eprintln!();
    eprintln!("Find all prime numbers from 2 to <max_number>");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --threads=N     Number of worker threads (default: number of CPU cores)");
    eprintln!("  --progress[=N]  Show progress every N seconds (default: 5)");
    eprintln!("  --list          Print all found primes");
    eprintln!("  --help          Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {} 1000                  Find primes up to 1000", program);
    eprintln!("  {} 1000000 --threads=8   Use 8 threads", program);
    eprintln!("  {} 1000000 --progress    Show progress every 5 seconds", program);
    eprintln!("  {} 100 --list            Print all primes up to 100", program);
}

struct Args {
    max_n: u64,
    threads: Option<usize>,
    progress_interval: Option<u64>,
    list_primes: bool,
}

fn parse_args() -> Result<Args, String> {
    let args: Vec<String> = env::args().collect();
    let program = &args[0];

    if args.len() < 2 {
        print_usage(program);
        return Err("Missing required argument: max_number".to_string());
    }

    let mut max_n: Option<u64> = None;
    let mut threads: Option<usize> = None;
    let mut progress_interval: Option<u64> = None;
    let mut list_primes = false;

    for arg in &args[1..] {
        if arg == "--help" || arg == "-h" {
            print_usage(program);
            process::exit(0);
        } else if arg == "--list" {
            list_primes = true;
        } else if arg == "--progress" {
            progress_interval = Some(5); // Default 5 seconds
        } else if let Some(value) = arg.strip_prefix("--progress=") {
            let secs: u64 = value
                .parse()
                .map_err(|_| format!("Invalid progress interval: {}", value))?;
            if secs == 0 {
                return Err("Progress interval must be at least 1 second".to_string());
            }
            progress_interval = Some(secs);
        } else if let Some(value) = arg.strip_prefix("--threads=") {
            threads = Some(
                value
                    .parse()
                    .map_err(|_| format!("Invalid thread count: {}", value))?,
            );
            if threads == Some(0) {
                return Err("Thread count must be at least 1".to_string());
            }
        } else if arg.starts_with("--") {
            return Err(format!("Unknown option: {}", arg));
        } else if arg.starts_with('-') {
            return Err(format!("Unknown option: {}", arg));
        } else {
            // Positional argument - should be max_n
            if max_n.is_some() {
                return Err("Too many positional arguments".to_string());
            }
            max_n = Some(
                arg.parse()
                    .map_err(|_| format!("Invalid number: {}", arg))?,
            );
        }
    }

    let max_n = max_n.ok_or("Missing required argument: max_number")?;

    if max_n < 2 {
        return Err("max_number must be at least 2".to_string());
    }

    Ok(Args {
        max_n,
        threads,
        progress_interval,
        list_primes,
    })
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    };

    // Build configuration
    let mut config = CalculatorConfig::new(args.max_n);
    if let Some(threads) = args.threads {
        config = config.with_threads(threads);
    }
    if let Some(interval) = args.progress_interval {
        config = config.with_progress_interval(interval);
    }

    // Report configuration
    let threads_desc = args
        .threads
        .map(|t| t.to_string())
        .unwrap_or_else(|| format!("{} (auto)", num_cpus::get()));

    println!("Finding primes up to {}...", args.max_n);
    println!("Using {} threads", threads_desc);
    println!();

    // Run calculation
    let calc = PrimeCalculator::new(config);
    let stats = calc.run();

    // Report results
    println!("Results:");
    println!("  Primes found: {}", stats.primes_found);
    println!("  Batches processed: {}", stats.batches_processed);
    println!("  Total time: {:.3}ms", stats.total_duration.as_secs_f64() * 1000.0);
    println!(
        "  Rate: {:.0} numbers/sec",
        args.max_n as f64 / stats.total_duration.as_secs_f64()
    );

    if args.list_primes {
        println!();
        println!("Primes:");
        let primes = calc.get_primes();
        for (i, p) in primes.iter().enumerate() {
            if i > 0 && i % 10 == 0 {
                println!();
            }
            print!("{:>8}", p);
        }
        println!();
    }
}
