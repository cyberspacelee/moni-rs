//! HTTP DTOs and dashboard WebSocket events from docs/api.md.

use serde::{Deserialize, Serialize};

pub use crate::{HostInfo, MetricsFrame, MinuteRollup};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct AppConfig {
    pub version: String,
    pub report_interval_s: u32,
    pub agent_release_repo: Option<String>,
    pub retention: Retention,
    pub notifications: Notifications,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct Retention {
    pub minute_days: u32,
    pub hour_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct Notifications {
    pub telegram: bool,
    pub webhook: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct Traffic {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub rx_bytes: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub tx_bytes: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub since_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct ServerStatus {
    pub id: String,
    pub name: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub created_ms: i64,
    pub online: bool,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub last_seen_ms: Option<i64>,
    pub agent_version: Option<String>,
    pub host: Option<HostInfo>,
    pub metrics: Option<MetricsFrame>,
    pub traffic_month: Traffic,
    pub alerts_firing: u32,
    /// ISO 3166-1 alpha-2 code of the Agent's egress IP, from Cloudflare's edge geolocation.
    #[serde(default)]
    pub country: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub enum HistoryRange {
    #[serde(rename = "1h")]
    OneHour,
    #[serde(rename = "6h")]
    SixHours,
    #[default]
    #[serde(rename = "24h")]
    TwentyFourHours,
    #[serde(rename = "7d")]
    SevenDays,
    #[serde(rename = "30d")]
    ThirtyDays,
    #[serde(rename = "90d")]
    NinetyDays,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct HistoryPoint {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub ts_ms: i64,
    pub cpu_avg: f32,
    pub cpu_max: f32,
    pub mem_pct: f32,
    pub swap_pct: f32,
    pub disk_pct: f32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub rx_bytes: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub tx_bytes: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub rx_bps_max: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub tx_bps_max: u64,
    pub load1: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct History {
    pub range: HistoryRange,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub step_ms: u64,
    pub points: Vec<HistoryPoint>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
#[serde(rename_all = "snake_case")]
pub enum AlertMetric {
    Cpu,
    Mem,
    Swap,
    Disk,
    Load1,
    NetRx,
    NetTx,
    Offline,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
#[serde(rename_all = "snake_case")]
pub enum AlertOp {
    Gt,
    Lt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub server_id: Option<String>,
    pub metric: AlertMetric,
    pub op: AlertOp,
    pub threshold: f64,
    pub recover_threshold: Option<f64>,
    pub duration_s: u32,
    pub enabled: bool,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub created_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct AlertRuleInput {
    pub name: String,
    pub server_id: Option<String>,
    pub metric: AlertMetric,
    pub op: AlertOp,
    pub threshold: f64,
    pub recover_threshold: Option<f64>,
    pub duration_s: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct AlertRulePatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub server_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub metric: Option<AlertMetric>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub op: Option<AlertOp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub threshold: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub recover_threshold: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub duration_s: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub enabled: Option<bool>,
}

// PATCH must distinguish an omitted field from explicitly clearing a nullable value.
fn deserialize_nullable_patch<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    Firing,
    Resolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct AlertEvent {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub id: i64,
    pub rule_id: String,
    pub rule_name: String,
    pub server_id: String,
    pub server_name: String,
    pub kind: AlertKind,
    pub value: Option<f64>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub ts_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct ActiveAlert {
    pub rule_id: String,
    pub rule_name: String,
    pub server_id: String,
    pub server_name: String,
    pub metric: AlertMetric,
    pub value: Option<f64>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub since_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum DashboardEvent {
    Snapshot {
        servers: Vec<ServerStatus>,
        alerts: Vec<ActiveAlert>,
    },
    ServerUpsert {
        server: ServerStatus,
    },
    ServerRemoved {
        id: String,
    },
    Online {
        id: String,
        agent_version: Option<String>,
        host: Option<HostInfo>,
    },
    Offline {
        id: String,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        last_seen_ms: i64,
    },
    Metrics {
        id: String,
        frame: MetricsFrame,
    },
    Traffic {
        id: String,
        traffic: Traffic,
    },
    Alert {
        kind: AlertKind,
        alert: ActiveAlert,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct LoginRequest {
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct AuthMeResponse {
    pub authenticated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct PublicInfo {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct CreateServerRequest {
    pub name: String,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct CreateServerResponse {
    pub server: ServerStatus,
    pub token: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct UpdateServerRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct TokenResponse {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct AlertTestResponse {
    pub telegram: String,
    pub webhook: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    Unauthorized,
    Forbidden,
    NotFound,
    BadRequest,
    Conflict,
    RateLimited,
    NotConfigured,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export_to = "web/src/api/generated/"))]
pub struct ApiError {
    pub error: ApiErrorCode,
    pub message: String,
}
