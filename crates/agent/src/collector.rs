use std::time::Instant;

use moni_protocol::{HostInfo, MetricsFrame};
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

/// Interfaces that do not represent real traffic in/out of the host.
fn is_virtual_iface(name: &str) -> bool {
    name == "lo"
        || [
            "docker", "veth", "br-", "virbr", "cni", "flannel", "tun", "wg",
        ]
        .iter()
        .any(|p| name.starts_with(p))
}

pub struct Collector {
    sys: System,
    disks: Disks,
    networks: Networks,
    seq: u64,
    /// (rx_total, tx_total, when) at the start of the current reporting window.
    window_start: Option<(u64, u64, Instant)>,
}

impl Collector {
    pub fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                .with_memory(MemoryRefreshKind::everything()),
        );
        Self {
            sys,
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            seq: 0,
            window_start: None,
        }
    }

    pub fn host_info(&self) -> HostInfo {
        HostInfo {
            hostname: System::host_name().unwrap_or_default(),
            os: System::long_os_version().unwrap_or_default(),
            kernel: System::kernel_version().unwrap_or_default(),
            arch: System::cpu_arch(),
            cpu_cores: self.sys.cpus().len() as u32,
        }
    }

    fn net_totals(&mut self) -> (u64, u64) {
        self.networks.refresh(true);
        self.networks
            .iter()
            .filter(|(name, _)| !is_virtual_iface(name))
            .fold((0, 0), |(rx, tx), (_, d)| {
                (rx + d.total_received(), tx + d.total_transmitted())
            })
    }

    /// Produce one frame covering the time since the previous call.
    pub fn sample(&mut self, now_ms: i64) -> MetricsFrame {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.disks.refresh(true);

        let (rx_total, tx_total) = self.net_totals();
        let now = Instant::now();
        let (rx_bps, tx_bps) = match self.window_start {
            Some((rx0, tx0, t0)) => {
                let secs = now.duration_since(t0).as_secs_f64().max(0.001);
                // Counters can go backwards when an interface disappears or resets.
                (
                    (rx_total.saturating_sub(rx0) as f64 / secs) as u64,
                    (tx_total.saturating_sub(tx0) as f64 / secs) as u64,
                )
            }
            None => (0, 0),
        };
        self.window_start = Some((rx_total, tx_total, now));

        let mut seen = std::collections::HashSet::new();
        let (disk_used, disk_total) = self
            .disks
            .iter()
            .filter(|d| d.name().to_string_lossy().starts_with("/dev/"))
            .filter(|d| seen.insert(d.name().to_owned()))
            .fold((0, 0), |(u, t), d| {
                (
                    u + d.total_space() - d.available_space(),
                    t + d.total_space(),
                )
            });

        let load = System::load_average();
        self.seq += 1;
        MetricsFrame {
            seq: self.seq,
            ts_ms: now_ms,
            cpu_pct: self.sys.global_cpu_usage(),
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            disk_used,
            disk_total,
            net_rx_bps: rx_bps,
            net_tx_bps: tx_bps,
            net_rx_total: rx_total,
            net_tx_total: tx_total,
            load1: load.one as f32,
            load5: load.five as f32,
            load15: load.fifteen as f32,
            uptime_s: System::uptime(),
        }
    }
}
