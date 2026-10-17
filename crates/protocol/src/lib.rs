//! Wire protocol shared by the agent (native) and the Cloudflare worker (wasm).
//! Must stay free of runtime-specific dependencies.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

/// WebSocket close code sent when the protocol version is incompatible.
pub const CLOSE_INCOMPATIBLE: u16 = 4001;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostInfo {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub cpu_cores: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MetricsFrame {
    /// Monotonic per agent process, for dedup / gap detection.
    pub seq: u64,
    pub ts_ms: i64,
    pub cpu_pct: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    /// Average rate over the reporting window.
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
    /// Raw interface counters (for traffic accounting).
    pub net_rx_total: u64,
    pub net_tx_total: u64,
    pub load1: f32,
    pub load5: f32,
    pub load15: f32,
    pub uptime_s: u64,
}

/// Agent -> Hub
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    Hello {
        protocol: u16,
        agent_version: String,
        host: HostInfo,
    },
    Metrics(MetricsFrame),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerStatus {
    pub server_id: String,
    pub online: bool,
    pub last_seen_ms: Option<i64>,
    pub host: Option<HostInfo>,
    pub metrics: Option<MetricsFrame>,
}

/// Hub -> Dashboard
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    Snapshot {
        servers: Vec<ServerStatus>,
    },
    Online {
        server_id: String,
    },
    Offline {
        server_id: String,
        last_seen_ms: i64,
    },
    Metrics {
        server_id: String,
        frame: MetricsFrame,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_message_roundtrip() {
        let m = AgentMessage::Metrics(MetricsFrame {
            seq: 7,
            cpu_pct: 12.5,
            ..Default::default()
        });
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.starts_with(r#"{"type":"metrics","seq":7"#), "{s}");
        assert_eq!(serde_json::from_str::<AgentMessage>(&s).unwrap(), m);
    }
}
