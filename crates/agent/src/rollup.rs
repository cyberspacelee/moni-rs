use moni_protocol::MinuteRollup;

use crate::window::{Sample, Window, bytes_per_second};

#[derive(Default)]
pub struct Rollup {
    current: Option<MinuteRollup>,
    window: Window,
}

impl Rollup {
    pub fn push(&mut self, sample: &Sample) -> Option<MinuteRollup> {
        let minute_ms = sample.frame.ts_ms.div_euclid(60_000) * 60_000;
        let completed = if self
            .current
            .as_ref()
            .is_some_and(|r| r.minute_ms != minute_ms)
        {
            self.finish()
        } else {
            None
        };
        let current = self.current.get_or_insert_with(|| MinuteRollup {
            minute_ms,
            ..MinuteRollup::default()
        });
        let frame = &sample.frame;
        current.cpu_max = current.cpu_max.max(frame.cpu_pct);
        current.mem_total = frame.mem_total;
        current.swap_total = frame.swap_total;
        current.disk_used = frame.disk_used;
        current.disk_total = frame.disk_total;
        current.rx_bytes = current.rx_bytes.saturating_add(sample.traffic.rx_bytes);
        current.tx_bytes = current.tx_bytes.saturating_add(sample.traffic.tx_bytes);
        current.rx_bps_max = current.rx_bps_max.max(bytes_per_second(
            sample.traffic.rx_bytes,
            sample.traffic.elapsed,
        ));
        current.tx_bps_max = current.tx_bps_max.max(bytes_per_second(
            sample.traffic.tx_bytes,
            sample.traffic.elapsed,
        ));
        self.window.push(sample);
        completed
    }

    fn finish(&mut self) -> Option<MinuteRollup> {
        let mut rollup = self.current.take()?;
        rollup.samples = self.window.samples();
        let average = self.window.take(0)?;
        rollup.cpu_avg = average.cpu_pct;
        rollup.mem_used_avg = average.mem_used;
        rollup.swap_used_avg = average.swap_used;
        rollup.load1_avg = average.load1;
        Some(rollup)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::window::{Counters, sample};

    use super::*;

    #[test]
    fn minute_switch_finishes_averages_peaks_bytes_and_last_disk_snapshot() {
        let mut rollup = Rollup::default();
        assert!(rollup.push(&sample(58_000, 10.0, 100, 20, 1)).is_none());
        assert!(rollup.push(&sample(59_000, 30.0, 900, 180, 3)).is_none());
        let minute = rollup.push(&sample(60_000, 90.0, 5, 5, 1)).unwrap();
        assert_eq!((minute.minute_ms, minute.samples), (0, 2));
        assert_eq!(
            (minute.cpu_avg, minute.cpu_max, minute.load1_avg),
            (20.0, 30.0, 2.0)
        );
        assert_eq!((minute.mem_used_avg, minute.swap_used_avg), (200, 20));
        assert_eq!((minute.disk_used, minute.disk_total), (3_000, 10_000));
        assert_eq!((minute.rx_bytes, minute.tx_bytes), (1000, 200));
        assert_eq!((minute.rx_bps_max, minute.tx_bps_max), (300, 60));
        let next = rollup.push(&sample(180_000, 0.0, 0, 0, 1)).unwrap();
        assert_eq!(
            (next.minute_ms, next.samples, next.rx_bytes),
            (60_000, 1, 5)
        );
    }

    #[test]
    fn counter_reset_contributes_zero_bytes_to_rollup() {
        let mut counters = Counters::default();
        let mut rollup = Rollup::default();
        for (ts_ms, rx, tx) in [(0, 100, 100), (1000, 10, 20), (2000, 30, 50)] {
            let mut sample = sample(ts_ms, 10.0, 0, 0, 1);
            sample.traffic = counters.update([("eth0".into(), rx, tx)], Duration::from_secs(1));
            rollup.push(&sample);
        }
        let minute = rollup.push(&sample(60_000, 0.0, 0, 0, 1)).unwrap();
        assert_eq!((minute.rx_bytes, minute.tx_bytes), (20, 30));
        assert_eq!((minute.rx_bps_max, minute.tx_bps_max), (20, 30));
    }
}
