/// Wheel factorization using primes 3 × 7 × 11 = 231
///
/// This array contains all numbers from 1 to 231 that are coprime to 3, 7, and 11.
/// Workers iterate through these offsets and filter out evens (n & 1 == 0) and
/// multiples of 5 (n % 5 == 0) at runtime.
///
/// Total: 120 offsets, of which 48 survive the runtime filters.
/// Efficiency: 48/231 ≈ 20.8% of numbers need trial division.

pub const WHEEL_PERIOD: u64 = 231;

pub const WHEEL_231: [u64; 120] = [
    1, 2, 4, 5, 8, 10, 13, 16, 17, 19, 20, 23,
    25, 26, 29, 31, 32, 34, 37, 38, 40, 41, 43, 46,
    47, 50, 52, 53, 58, 59, 61, 62, 64, 65, 67, 68,
    71, 73, 74, 76, 79, 80, 82, 83, 85, 86, 89, 92,
    94, 95, 97, 100, 101, 103, 104, 106, 107, 109, 113, 115,
    116, 118, 122, 124, 125, 127, 128, 130, 131, 134, 136, 137,
    139, 142, 145, 146, 148, 149, 151, 152, 155, 157, 158, 160,
    163, 164, 166, 167, 169, 170, 172, 173, 178, 179, 181, 184,
    185, 188, 190, 191, 193, 194, 197, 199, 200, 202, 205, 206,
    208, 211, 212, 214, 215, 218, 221, 223, 226, 227, 229, 230,
];

/// Iterator over prime candidates in a batch.
/// Yields numbers that pass the wheel filter and runtime checks for 2 and 5.
pub struct WheelBatchIter {
    base: u64,
    max: u64,
    index: usize,
}

impl WheelBatchIter {
    pub fn new(base: u64, max: u64) -> Self {
        Self { base, max, index: 0 }
    }
}

impl Iterator for WheelBatchIter {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < WHEEL_231.len() {
            let offset = WHEEL_231[self.index];
            self.index += 1;

            let n = self.base + offset;
            if n > self.max {
                return None;
            }

            // Skip even numbers
            if 0 == n & 1 {
                continue;
            }

            // Skip multiples of 5
            if n % 5 == 0 {
                continue;
            }

            return Some(n);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_offsets_are_coprime_to_231() {
        for &offset in &WHEEL_231 {
            assert!(offset > 0 && offset <= WHEEL_PERIOD);
            assert!(offset % 3 != 0, "{} is divisible by 3", offset);
            assert!(offset % 7 != 0, "{} is divisible by 7", offset);
            assert!(offset % 11 != 0, "{} is divisible by 11", offset);
        }
    }

    #[test]
    fn wheel_has_120_offsets() {
        assert_eq!(WHEEL_231.len(), 120);
    }

    #[test]
    fn batch_iter_filters_correctly() {
        let candidates: Vec<u64> = WheelBatchIter::new(0, 231).collect();

        for &n in &candidates {
            assert!(n & 1 == 1, "{} is even", n);
            assert!(n % 5 != 0, "{} is divisible by 5", n);
            assert!(n % 3 != 0, "{} is divisible by 3", n);
            assert!(n % 7 != 0, "{} is divisible by 7", n);
            assert!(n % 11 != 0, "{} is divisible by 11", n);
        }

        // Should be ~48 candidates per period
        assert_eq!(candidates.len(), 48);
    }
}
