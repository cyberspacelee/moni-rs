//! Wire protocol shared by the agent (native) and the Cloudflare worker (wasm).
//! Must stay free of runtime-specific dependencies.

use serde::{Deserialize, Serialize};

pub mod api;

pub const PROTOCOL_VERSION: u16 = 2;

/// WebSocket close code sent when the protocol version is incompatible.
pub const CLOSE_INCOMPATIBLE: u16 = 4001;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct HostInfo {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub cpu_model: String,
    pub cpu_cores: u32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub boot_time_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct MetricsFrame {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub seq: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub ts_ms: i64,
    pub cpu_pct: f32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub mem_used: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub mem_total: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub swap_used: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub swap_total: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub disk_used: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub disk_total: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub net_rx_bps: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub net_tx_bps: u64,
    pub load1: f32,
    pub load5: f32,
    pub load15: f32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub uptime_s: u64,
    pub process_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    Hello {
        protocol: u16,
        agent_version: String,
        host: HostInfo,
    },
    Metrics(MetricsFrame),
    Rollup(MinuteRollup),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct MinuteRollup {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub minute_ms: i64,
    pub samples: u32,
    pub cpu_avg: f32,
    pub cpu_max: f32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub mem_used_avg: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub mem_total: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub swap_used_avg: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub swap_total: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub disk_used: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub disk_total: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub rx_bytes: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub tx_bytes: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub rx_bps_max: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub tx_bps_max: u64,
    pub load1_avg: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HubMessage {
    Welcome {
        server_id: String,
        report_interval_s: u32,
    },
    Ack {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        minute_ms: i64,
    },
}

#[cfg(test)]
mod tests;

#[cfg(all(test, feature = "ts"))]
mod export;
