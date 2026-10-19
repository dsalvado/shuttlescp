//! Speed and time-left estimates for transfers, and progress fractions.

use std::time::{Duration, Instant};

use crate::format::{format_duration, format_size};

/// Speed is measured over at least this long, so one burst does not swing the estimate.
const MIN_SAMPLE: Duration = Duration::from_millis(250);
/// How much each new speed sample moves the smoothed speed (0 = never, 1 = jump to it).
const SMOOTHING: f64 = 0.3;
/// A time-left estimate is shown once a transfer has run this long...
const ESTIMATE_AFTER: Duration = Duration::from_secs(2);
/// ...and only if it looks like taking at least this long in total.
const ESTIMATE_MIN_TOTAL: Duration = Duration::from_secs(10);

/// Measures a transfer's speed and estimates the time left, from progress reports.
#[derive(Debug, Clone)]
pub struct RateMeter {
    started: Instant,
    /// Last sample used for the speed: when, and bytes done then.
    last: (Instant, u64),
    /// Smoothed speed in bytes per second, once measured.
    rate: Option<f64>,
}

impl RateMeter {
    /// Start measuring a transfer that begins at `now` with nothing done.
    pub fn new(now: Instant) -> Self {
        Self {
            started: now,
            last: (now, 0),
            rate: None,
        }
    }

    /// Start measuring a transfer that already has `done` bytes done at `now` (e.g. a
    /// file whose first progress report just arrived).
    pub fn starting_from(now: Instant, done: u64) -> Self {
        Self {
            started: now,
            last: (now, done),
            rate: None,
        }
    }

    /// Count `bytes` as done before measuring began (e.g. the part of a file copied
    /// before an interruption), so they do not show up as speed.
    pub fn add_done_earlier(&mut self, bytes: u64) {
        self.last.1 += bytes;
    }

    /// Note that `done` bytes are done at `now`.
    pub fn record(&mut self, done: u64, now: Instant) {
        let (then, done_then) = self.last;
        let elapsed = now.saturating_duration_since(then);
        if elapsed < MIN_SAMPLE {
            return;
        }
        let sample = done.saturating_sub(done_then) as f64 / elapsed.as_secs_f64();
        self.rate = Some(
            self.rate
                .map_or(sample, |rate| rate + SMOOTHING * (sample - rate)),
        );
        self.last = (now, done);
    }

    /// Smoothed speed in bytes per second, once there is enough data.
    pub fn bytes_per_second(&self) -> Option<f64> {
        self.rate
    }

    /// Estimated time left, shown only for long transfers: after [`ESTIMATE_AFTER`],
    /// and when the whole transfer looks like taking at least [`ESTIMATE_MIN_TOTAL`].
    ///
    /// # Arguments
    ///
    /// * `done` - Bytes done
    /// * `total` - Bytes in all
    /// * `now` - Current time
    pub fn time_left(&self, done: u64, total: u64, now: Instant) -> Option<Duration> {
        let rate = self.rate.filter(|rate| *rate > 0.0)?;
        let elapsed = now.saturating_duration_since(self.started);
        let left = Duration::from_secs_f64(total.saturating_sub(done) as f64 / rate);
        (elapsed >= ESTIMATE_AFTER && elapsed + left >= ESTIMATE_MIN_TOTAL).then_some(left)
    }

    /// Progress as one line, e.g. "45% · 12.3 MiB of 27.0 MiB · 11.2 MiB/s · 0:12 left".
    /// The speed and time left are added once known.
    ///
    /// # Arguments
    ///
    /// * `done` - Bytes done
    /// * `total` - Bytes in all
    /// * `now` - Current time
    pub fn describe(&self, done: u64, total: u64, now: Instant) -> String {
        let mut text = format!(
            "{}% · {} of {}",
            percent(done, total),
            format_size(done),
            format_size(total)
        );
        if let Some(rate) = self.rate {
            // Rounded to whole bytes for display.
            text.push_str(&format!(" · {}/s", format_size(rate as u64)));
        }
        if let Some(left) = self.time_left(done, total, now) {
            text.push_str(&format!(" · {} left", format_duration(left)));
        }
        text
    }
}

/// Whole percent of `total` that `done` is; an empty total counts as complete.
pub fn percent(done: u64, total: u64) -> u64 {
    if total == 0 {
        100
    } else {
        (u128::from(done.min(total)) * 100 / u128::from(total)) as u64
    }
}

/// `done` as a fraction of `total` (0.0 to 1.0) for a progress bar.
pub fn fraction(done: u64, total: u64) -> f64 {
    if total == 0 {
        1.0
    } else {
        done.min(total) as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_meter_estimates_only_long_transfers() {
        let start = Instant::now();
        let mut meter = RateMeter::new(start);
        let total = 100_000_000;

        // 1 MB/s: 100 s in all, so an estimate appears after 2 s.
        meter.record(1_000_000, start + Duration::from_secs(1));
        assert_eq!(
            meter.time_left(1_000_000, total, start + Duration::from_secs(1)),
            None
        );
        meter.record(3_000_000, start + Duration::from_secs(3));
        let left = meter
            .time_left(3_000_000, total, start + Duration::from_secs(3))
            .expect("estimate");
        assert_eq!(left.as_secs(), 97);

        // A 5 MB file at the same speed finishes in 5 s: no estimate.
        assert_eq!(
            meter.time_left(3_000_000, 5_000_000, start + Duration::from_secs(3)),
            None
        );
    }

    #[test]
    fn bytes_done_earlier_do_not_count_as_speed() {
        // A resumed copy: 200 MB were done before; 2 MB more arrive in the first second.
        let start = Instant::now();
        let mut meter = RateMeter::new(start);
        meter.add_done_earlier(200_000_000);

        meter.record(202_000_000, start + Duration::from_secs(1));

        assert_eq!(
            meter.bytes_per_second().map(|rate| rate as u64),
            Some(2_000_000)
        );
    }

    #[test]
    fn rate_meter_describes_progress() {
        let start = Instant::now();
        let mut meter = RateMeter::new(start);
        assert_eq!(meter.describe(0, 2048, start), "0% · 0 B of 2.0 KiB");
        meter.record(1024, start + Duration::from_secs(1));
        assert_eq!(
            meter.describe(1024, 2048, start + Duration::from_secs(1)),
            "50% · 1.0 KiB of 2.0 KiB · 1.0 KiB/s"
        );
    }

    #[test]
    fn percent_and_fraction_handle_empty_totals() {
        assert_eq!(percent(0, 0), 100);
        assert_eq!(percent(1, 3), 33);
        assert!((fraction(1, 4) - 0.25).abs() < f64::EPSILON);
        assert!((fraction(5, 0) - 1.0).abs() < f64::EPSILON);
    }
}
