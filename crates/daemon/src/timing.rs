//! How long the daemon takes to handle things, for `flowbuddy status`.

use std::collections::VecDeque;
use std::time::Duration;

/// The latest handling times, in microseconds.
#[derive(Default)]
pub struct Timings {
    samples: VecDeque<u32>,
    count: u64,
    max_us: u32,
}

/// Samples kept for the percentiles.
const KEEP: usize = 2000;

impl Timings {
    pub fn record(&mut self, took: Duration) {
        let us = took.as_micros().min(u128::from(u32::MAX)) as u32;
        if self.samples.len() == KEEP {
            self.samples.pop_front();
        }
        self.samples.push_back(us);
        self.count += 1;
        self.max_us = self.max_us.max(us);
    }

    /// "p50 0.04 ms, p95 0.21 ms, slowest 1.90 ms over 1234".
    pub fn summary(&self) -> Option<String> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted: Vec<u32> = self.samples.iter().copied().collect();
        sorted.sort_unstable();
        let at = |p: usize| sorted[(sorted.len() - 1) * p / 100];
        let ms = |us: u32| f64::from(us) / 1000.0;
        Some(format!(
            "p50 {:.2} ms, p95 {:.2} ms, slowest {:.2} ms over {}",
            ms(at(50)),
            ms(at(95)),
            ms(self.max_us),
            self.count
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles() {
        let mut t = Timings::default();
        assert_eq!(t.summary(), None);
        for us in 1..=100 {
            t.record(Duration::from_micros(us * 10));
        }
        assert_eq!(
            t.summary().unwrap(),
            "p50 0.50 ms, p95 0.95 ms, slowest 1.00 ms over 100"
        );
    }
}
