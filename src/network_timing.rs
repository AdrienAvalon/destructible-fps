//! Bounded client-side timing for ordered-delta repair.
//!
//! The authority does not depend on wall-clock timing. This estimator only controls when a client
//! repeats a repair request after observing a complete future delta.

use std::time::Duration;

pub const MIN_REPAIR_RTO: Duration = Duration::from_millis(100);
pub const INITIAL_REPAIR_RTO: Duration = Duration::from_millis(250);
pub const MAX_REPAIR_RTO: Duration = Duration::from_secs(2);
pub const MIN_REORDER_GRACE: Duration = Duration::from_millis(50);
pub const INITIAL_REORDER_GRACE: Duration = Duration::from_millis(100);
pub const MAX_REORDER_GRACE: Duration = Duration::from_millis(500);

const MIN_CLOCK_GRANULARITY_US: u64 = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RepairProbe {
    sequence: u64,
    first_sent_us: u64,
    last_sent_us: u64,
    retransmitted: bool,
}

/// A Jacobson/Karels RTT estimator with Karn filtering and bounded exponential backoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdaptiveRepairTimer {
    smoothed_rtt_us: Option<u64>,
    rtt_variance_us: u64,
    retransmission_timeout_us: u64,
    samples: u64,
    probe: Option<RepairProbe>,
}

impl Default for AdaptiveRepairTimer {
    fn default() -> Self {
        Self {
            smoothed_rtt_us: None,
            rtt_variance_us: 0,
            retransmission_timeout_us: duration_us(INITIAL_REPAIR_RTO),
            samples: 0,
            probe: None,
        }
    }
}

impl AdaptiveRepairTimer {
    #[must_use]
    pub fn reorder_grace(self) -> Duration {
        Duration::from_micros(
            self.smoothed_rtt_us
                .unwrap_or_else(|| duration_us(INITIAL_REORDER_GRACE))
                .clamp(
                    duration_us(MIN_REORDER_GRACE),
                    duration_us(MAX_REORDER_GRACE),
                ),
        )
    }

    #[must_use]
    pub const fn retransmission_timeout(self) -> Duration {
        Duration::from_micros(self.retransmission_timeout_us)
    }

    #[must_use]
    pub fn smoothed_rtt(self) -> Option<Duration> {
        self.smoothed_rtt_us.map(Duration::from_micros)
    }

    #[must_use]
    pub const fn sample_count(self) -> u64 {
        self.samples
    }

    /// Returns true when there is no request for this sequence in flight or its bounded RTO elapsed.
    #[must_use]
    pub fn send_due(self, sequence: u64, now: Duration) -> bool {
        let now_us = duration_us(now);
        self.probe.is_none_or(|probe| {
            probe.sequence != sequence
                || now_us.saturating_sub(probe.last_sent_us) >= self.retransmission_timeout_us
        })
    }

    /// Records a request that was emitted after [`Self::send_due`] returned true.
    pub fn record_send(&mut self, sequence: u64, now: Duration) {
        let now_us = duration_us(now);
        if let Some(probe) = &mut self.probe
            && probe.sequence == sequence
        {
            probe.last_sent_us = probe.last_sent_us.max(now_us);
            probe.retransmitted = true;
            self.retransmission_timeout_us = self
                .retransmission_timeout_us
                .saturating_mul(2)
                .min(duration_us(MAX_REPAIR_RTO));
            return;
        }
        self.probe = Some(RepairProbe {
            sequence,
            first_sent_us: now_us,
            last_sent_us: now_us,
            retransmitted: false,
        });
    }

    /// Completes a probe once the ordered inbox has advanced past it.
    ///
    /// Retransmitted probes are deliberately excluded because their acknowledgement is ambiguous.
    /// The returned duration is the accepted clean RTT sample, when one exists.
    pub fn observe_sequence(
        &mut self,
        next_expected_sequence: u64,
        now: Duration,
    ) -> Option<Duration> {
        let probe = self
            .probe
            .filter(|probe| probe.sequence < next_expected_sequence)?;
        self.probe = None;
        if probe.retransmitted {
            return None;
        }
        let sample_us = duration_us(now).checked_sub(probe.first_sent_us)?.max(1);
        self.update_estimate(sample_us);
        Some(Duration::from_micros(sample_us))
    }

    /// Cancels an in-flight delta probe after an atomic snapshot replaces the ordered cursor.
    pub const fn clear_probe(&mut self) {
        self.probe = None;
    }

    fn update_estimate(&mut self, sample_us: u64) {
        if let Some(smoothed) = self.smoothed_rtt_us {
            let error = smoothed.abs_diff(sample_us);
            self.rtt_variance_us = weighted_average(self.rtt_variance_us, 3, error, 1, 4);
            self.smoothed_rtt_us = Some(weighted_average(smoothed, 7, sample_us, 1, 8));
        } else {
            self.smoothed_rtt_us = Some(sample_us);
            self.rtt_variance_us = sample_us / 2;
        }
        let variation = self
            .rtt_variance_us
            .saturating_mul(4)
            .max(MIN_CLOCK_GRANULARITY_US);
        self.retransmission_timeout_us = self
            .smoothed_rtt_us
            .unwrap_or(sample_us)
            .saturating_add(variation)
            .clamp(duration_us(MIN_REPAIR_RTO), duration_us(MAX_REPAIR_RTO));
        self.samples = self.samples.saturating_add(1);
    }
}

fn duration_us(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

const fn weighted_average(
    first: u64,
    first_weight: u64,
    second: u64,
    second_weight: u64,
    divisor: u64,
) -> u64 {
    first
        .saturating_mul(first_weight)
        .saturating_add(second.saturating_mul(second_weight))
        / divisor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_samples_adapt_reorder_grace_and_rto() {
        let mut timer = AdaptiveRepairTimer::default();
        assert_eq!(timer.reorder_grace(), Duration::from_millis(100));
        assert_eq!(timer.retransmission_timeout(), Duration::from_millis(250));

        timer.record_send(7, Duration::ZERO);
        assert_eq!(
            timer.observe_sequence(8, Duration::from_millis(80)),
            Some(Duration::from_millis(80))
        );
        assert_eq!(timer.smoothed_rtt(), Some(Duration::from_millis(80)));
        assert_eq!(timer.reorder_grace(), Duration::from_millis(80));
        assert_eq!(timer.retransmission_timeout(), Duration::from_millis(240));

        timer.record_send(8, Duration::from_secs(1));
        timer.observe_sequence(9, Duration::from_millis(1_120));
        assert_eq!(timer.smoothed_rtt(), Some(Duration::from_millis(85)));
        assert_eq!(timer.retransmission_timeout(), Duration::from_millis(245));
        assert_eq!(timer.sample_count(), 2);
    }

    #[test]
    fn retransmission_is_karn_filtered_and_backs_off() {
        let mut timer = AdaptiveRepairTimer::default();
        timer.record_send(3, Duration::ZERO);
        assert!(!timer.send_due(3, Duration::from_millis(249)));
        assert!(timer.send_due(3, Duration::from_millis(250)));
        timer.record_send(3, Duration::from_millis(250));
        assert_eq!(timer.retransmission_timeout(), Duration::from_millis(500));
        assert_eq!(timer.observe_sequence(4, Duration::from_millis(300)), None);
        assert_eq!(timer.smoothed_rtt(), None);
        assert_eq!(timer.sample_count(), 0);
    }

    #[test]
    fn estimator_and_clock_inputs_remain_bounded() {
        let mut timer = AdaptiveRepairTimer::default();
        timer.record_send(1, Duration::from_millis(20));
        assert_eq!(timer.observe_sequence(2, Duration::from_millis(10)), None);

        timer.record_send(2, Duration::ZERO);
        timer.observe_sequence(3, Duration::from_micros(1));
        assert_eq!(timer.retransmission_timeout(), MIN_REPAIR_RTO);

        timer.record_send(3, Duration::ZERO);
        timer.observe_sequence(4, Duration::from_secs(10));
        assert!(timer.retransmission_timeout() <= MAX_REPAIR_RTO);
        assert!(timer.reorder_grace() <= MAX_REORDER_GRACE);
        timer.clear_probe();
        assert!(timer.send_due(4, Duration::ZERO));
    }
}
