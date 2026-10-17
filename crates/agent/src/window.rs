use std::{collections::BTreeMap, time::Duration};

use moni_protocol::MetricsFrame;

#[derive(Clone, Copy, Debug, Default)]
pub struct Traffic {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub elapsed: Duration,
}

pub fn bytes_per_second(bytes: u64, elapsed: Duration) -> u64 {
    if elapsed.is_zero() {
        0
    } else {
        (bytes as f64 / elapsed.as_secs_f64()) as u64
    }
}

#[derive(Default)]
pub struct Counters {
    previous: BTreeMap<String, (u64, u64)>,
}

impl Counters {
    pub fn update(
        &mut self,
        counters: impl IntoIterator<Item = (String, u64, u64)>,
        elapsed: Duration,
    ) -> Traffic {
        let mut traffic = Traffic {
            elapsed,
            ..Traffic::default()
        };
        let mut current = BTreeMap::new();
        for (name, rx, tx) in counters {
            if let Some(&(previous_rx, previous_tx)) = self.previous.get(&name) {
                traffic.rx_bytes = traffic
                    .rx_bytes
                    .saturating_add(rx.saturating_sub(previous_rx));
                traffic.tx_bytes = traffic
                    .tx_bytes
                    .saturating_add(tx.saturating_sub(previous_tx));
            }
            current.insert(name, (rx, tx));
        }
        self.previous = current;
        traffic
    }
}

pub struct Sample {
    pub frame: MetricsFrame,
    pub traffic: Traffic,
}

#[derive(Default)]
pub struct Window {
    latest: Option<MetricsFrame>,
    samples: u32,
    cpu: f64,
    mem: u128,
    swap: u128,
    disk_used: u128,
    disk_total: u128,
    load: [f64; 3],
    processes: u64,
    rx: u128,
    tx: u128,
    elapsed: Duration,
}

impl Window {
    pub fn push(&mut self, sample: &Sample) {
        let frame = &sample.frame;
        self.samples += 1;
        self.cpu += f64::from(frame.cpu_pct);
        self.mem += u128::from(frame.mem_used);
        self.swap += u128::from(frame.swap_used);
        self.disk_used += u128::from(frame.disk_used);
        self.disk_total += u128::from(frame.disk_total);
        self.load[0] += f64::from(frame.load1);
        self.load[1] += f64::from(frame.load5);
        self.load[2] += f64::from(frame.load15);
        self.processes += u64::from(frame.process_count);
        self.rx += u128::from(sample.traffic.rx_bytes);
        self.tx += u128::from(sample.traffic.tx_bytes);
        self.elapsed += sample.traffic.elapsed;
        self.latest = Some(frame.clone());
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }

    pub fn take(&mut self, seq: u64) -> Option<MetricsFrame> {
        let window = std::mem::take(self);
        let mut frame = window.latest?;
        let n = u128::from(window.samples);
        frame.seq = seq;
        frame.cpu_pct = (window.cpu / window.samples as f64) as f32;
        frame.mem_used = (window.mem / n) as u64;
        frame.swap_used = (window.swap / n) as u64;
        frame.disk_used = (window.disk_used / n) as u64;
        frame.disk_total = (window.disk_total / n) as u64;
        frame.load1 = (window.load[0] / window.samples as f64) as f32;
        frame.load5 = (window.load[1] / window.samples as f64) as f32;
        frame.load15 = (window.load[2] / window.samples as f64) as f32;
        frame.process_count = (window.processes / u64::from(window.samples)) as u32;
        let seconds = window.elapsed.as_secs_f64();
        frame.net_rx_bps = if seconds > 0.0 {
            (window.rx as f64 / seconds) as u64
        } else {
            0
        };
        frame.net_tx_bps = if seconds > 0.0 {
            (window.tx as f64 / seconds) as u64
        } else {
            0
        };
        Some(frame)
    }
}

#[cfg(test)]
pub fn sample(ts_ms: i64, cpu: f32, rx: u64, tx: u64, seconds: u64) -> Sample {
    Sample {
        frame: MetricsFrame {
            ts_ms,
            cpu_pct: cpu,
            mem_used: cpu as u64 * 10,
            mem_total: 1_000,
            swap_used: cpu as u64,
            swap_total: 100,
            disk_used: cpu as u64 * 100,
            disk_total: 10_000,
            load1: cpu / 10.0,
            process_count: cpu as u32,
            ..MetricsFrame::default()
        },
        traffic: Traffic {
            rx_bytes: rx,
            tx_bytes: tx,
            elapsed: Duration::from_secs(seconds),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counters(values: &[(&str, u64, u64)]) -> Vec<(String, u64, u64)> {
        values
            .iter()
            .map(|&(name, rx, tx)| (name.into(), rx, tx))
            .collect()
    }

    #[test]
    fn counters_reset_per_interface_and_reappearance_has_no_delta() {
        let mut tracker = Counters::default();
        let elapsed = Duration::from_secs(1);
        let first = tracker.update(
            counters(&[("eth0", 100, 200), ("eth1", 1000, 2000)]),
            elapsed,
        );
        assert_eq!(first.rx_bytes, 0);
        let reset = tracker.update(counters(&[("eth0", 10, 220), ("eth1", 1100, 10)]), elapsed);
        assert_eq!((reset.rx_bytes, reset.tx_bytes), (100, 20));
        tracker.update(counters(&[("eth1", 1200, 20)]), elapsed);
        let reappeared = tracker.update(
            counters(&[("eth0", 5000, 5000), ("eth1", 1300, 30)]),
            elapsed,
        );
        assert_eq!((reappeared.rx_bytes, reappeared.tx_bytes), (100, 10));
    }

    #[test]
    fn rate_handles_subsecond_delayed_and_zero_samples() {
        assert_eq!(bytes_per_second(100, Duration::ZERO), 0);
        assert_eq!(bytes_per_second(100, Duration::from_millis(500)), 200);
        assert_eq!(bytes_per_second(100, Duration::from_secs(2)), 50);
    }

    #[test]
    fn frame_averages_gauges_and_uses_elapsed_time_for_rate() {
        let mut window = Window::default();
        assert!(window.take(1).is_none());
        window.push(&sample(1_000, 10.0, 100, 20, 1));
        window.push(&sample(4_000, 30.0, 900, 180, 3));
        let frame = window.take(7).unwrap();
        assert_eq!((frame.seq, frame.ts_ms), (7, 4_000));
        assert_eq!(frame.cpu_pct, 20.0);
        assert_eq!(
            (frame.mem_used, frame.swap_used, frame.process_count),
            (200, 20, 20)
        );
        assert_eq!((frame.disk_used, frame.disk_total), (2_000, 10_000));
        assert_eq!((frame.net_rx_bps, frame.net_tx_bps), (250, 50));
        assert!(window.take(8).is_none());
    }
}
