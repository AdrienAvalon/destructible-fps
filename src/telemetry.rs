//! Bounded latency distributions used by runtime and benchmark instrumentation.

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistributionSummary {
    pub samples: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug)]
pub struct SampleWindow {
    capacity: usize,
    samples: VecDeque<f64>,
}

impl SampleWindow {
    /// Creates a bounded window retaining the newest samples.
    ///
    /// # Panics
    ///
    /// Panics when `capacity` is zero.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "a telemetry window needs positive capacity");
        Self {
            capacity,
            samples: VecDeque::with_capacity(capacity),
        }
    }

    pub fn record_ms(&mut self, value: f64) {
        if !value.is_finite() || value < 0.0 {
            return;
        }
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(value);
    }

    #[must_use]
    pub fn summary(&self) -> Option<DistributionSummary> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted: Vec<_> = self.samples.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        let max_ms = sorted.last().copied()?;
        Some(DistributionSummary {
            samples: sorted.len(),
            p50_ms: percentile(&sorted, 50, 100),
            p95_ms: percentile(&sorted, 95, 100),
            p99_ms: percentile(&sorted, 99, 100),
            max_ms,
        })
    }
}

fn percentile(sorted: &[f64], numerator: usize, denominator: usize) -> f64 {
    let rank = sorted.len().saturating_mul(numerator).div_ceil(denominator);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_bounded_and_keeps_recent_samples() {
        let mut window = SampleWindow::new(3);
        for sample in [100.0, 1.0, 2.0, 3.0] {
            window.record_ms(sample);
        }

        assert_eq!(
            window.summary(),
            Some(DistributionSummary {
                samples: 3,
                p50_ms: 2.0,
                p95_ms: 3.0,
                p99_ms: 3.0,
                max_ms: 3.0,
            })
        );
    }

    #[test]
    fn invalid_samples_do_not_poison_reports() {
        let mut window = SampleWindow::new(4);
        window.record_ms(f64::NAN);
        window.record_ms(f64::INFINITY);
        window.record_ms(-1.0);

        assert_eq!(window.summary(), None);
    }
}
