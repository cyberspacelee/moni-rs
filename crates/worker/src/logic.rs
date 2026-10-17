//! Runtime-independent query plans and time rules shared by the Hub and native tests.
use moni_protocol::HostInfo;
use moni_protocol::api::{DashboardEvent, HistoryRange, ServerStatus};

pub const MINUTE_MS: i64 = 60_000;
pub const HOUR_MS: i64 = 60 * MINUTE_MS;
pub const DAY_MS: i64 = 24 * HOUR_MS;

#[derive(Debug, PartialEq, Eq)]
pub struct HistoryPlan {
    pub range: HistoryRange,
    pub table: &'static str,
    pub duration_ms: i64,
    pub step_ms: i64,
}

pub fn history_plan(range: Option<&str>) -> Option<HistoryPlan> {
    let (range, hours, step_ms) = match range.unwrap_or("24h") {
        "1h" => (HistoryRange::OneHour, 1, MINUTE_MS),
        "6h" => (HistoryRange::SixHours, 6, MINUTE_MS),
        "24h" => (HistoryRange::TwentyFourHours, 24, MINUTE_MS),
        "7d" => (HistoryRange::SevenDays, 7 * 24, 10 * MINUTE_MS),
        "30d" => (HistoryRange::ThirtyDays, 30 * 24, HOUR_MS),
        "90d" => (HistoryRange::NinetyDays, 90 * 24, HOUR_MS),
        _ => return None,
    };
    Some(HistoryPlan {
        range,
        table: if step_ms == HOUR_MS {
            "metrics_1h"
        } else {
            "metrics_1m"
        },
        duration_ms: hours * HOUR_MS,
        step_ms,
    })
}

pub fn bucket(ts_ms: i64, step_ms: i64) -> i64 {
    ts_ms.div_euclid(step_ms) * step_ms
}

pub fn reasonable_minute(minute_ms: i64, now_ms: i64, retention_days: u32) -> bool {
    minute_ms >= 0
        && minute_ms % MINUTE_MS == 0
        && minute_ms <= now_ms.saturating_add(2 * MINUTE_MS)
        && minute_ms >= now_ms.saturating_sub(i64::from(retention_days) * DAY_MS)
}

pub fn hourly_start(last_hourly_ms: i64, now_ms: i64, retention_days: u32) -> i64 {
    let recent = bucket(now_ms, HOUR_MS) - 2 * HOUR_MS;
    let last = if last_hourly_ms == 0 {
        recent
    } else {
        last_hourly_ms.min(recent)
    };
    last.max(bucket(now_ms - i64::from(retention_days) * DAY_MS, HOUR_MS))
}

pub fn percentage(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64 * 100.0).min(100.0)
    }
}

pub fn month_start(ts_ms: i64) -> i64 {
    // Gregorian 400-year eras keep UTC month calculation independent of the runtime/timezone.
    let days = ts_ms.div_euclid(DAY_MS);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month + 2) / 5;
    (days - day) * DAY_MS
}

pub fn warning_due(last_ms: Option<i64>, now_ms: i64) -> bool {
    last_ms.is_none_or(|last| now_ms.saturating_sub(last) >= MINUTE_MS)
}

pub fn safe_parse_error(error: &serde_json::Error) -> String {
    // Serde data errors can echo arbitrary input, including credentials; omit their quoted values.
    let message = error.to_string();
    let quoted = |ch: char| matches!(ch, '"' | '\'' | '`');
    let safe = match (message.find(quoted), message.rfind(quoted)) {
        (Some(first), Some(last)) => format!(
            "{}[redacted]{}",
            message.get(..first).unwrap_or_default(),
            message.get(last + 1..).unwrap_or_default()
        ),
        _ => message,
    };
    safe.chars().take(200).collect()
}
pub fn slug(name: &str) -> String {
    let mut id = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if id.len() == 32 {
                break;
            }
            id.push(ch.to_ascii_lowercase());
        } else if !id.is_empty() && !id.ends_with('-') && id.len() < 32 {
            id.push('-');
        }
    }
    let id = id.trim_end_matches('-');
    if id.is_empty() {
        "server".into()
    } else {
        id.into()
    }
}

#[derive(Default)]
pub struct LoginFailures {
    by_ip: std::collections::HashMap<String, (i64, u8)>,
}

impl LoginFailures {
    pub fn limited(&mut self, ip: &str, now: i64, failed: bool) -> bool {
        self.by_ip.retain(|_, (start, _)| now - *start < 60_000);
        if self.by_ip.get(ip).is_some_and(|(_, count)| *count >= 10) {
            return true;
        }
        if failed {
            let (_, count) = self.by_ip.entry(ip.into()).or_insert((now, 0));
            *count += 1;
        }
        false
    }
}

pub fn origin_allowed(origin: Option<&str>, target: &str, required: bool) -> bool {
    match origin {
        Some(value) => value != "null" && value == target,
        None => !required,
    }
}

pub fn request_has_body(content_length: Option<&str>, transfer_encoding: Option<&str>) -> bool {
    transfer_encoding.is_some()
        || content_length
            .is_some_and(|value| value.trim().parse::<u64>().is_ok_and(|length| length > 0))
}

pub fn json_content_type(value: Option<&str>) -> bool {
    value
        .and_then(|v| v.split(';').next())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
}

pub fn valid_rollup(r: &moni_protocol::MinuteRollup) -> bool {
    r.rx_bytes <= 1 << 50
        && r.tx_bytes <= 1 << 50
        && r.rx_bps_max <= 1 << 47
        && r.tx_bps_max <= 1 << 47
        && (0.0..=100.0).contains(&r.cpu_avg)
        && (0.0..=100.0).contains(&r.cpu_max)
        && r.mem_used_avg <= r.mem_total
        && r.swap_used_avg <= r.swap_total
        && r.disk_used <= r.disk_total
        && r.load1_avg.is_finite()
}

/// Cloudflare reports `XX` for unknown and `T1` for Tor; neither names a place.
pub fn country_code(value: Option<&str>) -> Option<String> {
    value
        .filter(|v| v.len() == 2 && v.bytes().all(|b| b.is_ascii_uppercase()))
        .filter(|v| *v != "XX" && *v != "T1")
        .map(str::to_owned)
}

/// Requests anonymous visitors may make when the public dashboard is enabled.
pub fn public_route(get: bool, path: &str) -> bool {
    get && (path == "/ws/dashboard"
        || path
            .strip_prefix("/api/servers/")
            .and_then(|rest| rest.strip_suffix("/history"))
            .is_some_and(crate::auth::valid_id))
}

/// Strips what helps an attacker more than a visitor: exact host identity, kernel and Agent
/// versions, and alert state (rule names and thresholds are operator configuration).
pub fn redact_server(mut server: ServerStatus) -> ServerStatus {
    if let Some(host) = server.host.as_mut() {
        redact_host(host);
    }
    server.agent_version = None;
    server.alerts_firing = 0;
    server
}

fn redact_host(host: &mut HostInfo) {
    host.hostname.clear();
    host.kernel.clear();
}

#[allow(clippy::large_enum_variant)]
pub enum PublicView {
    Same,
    Redacted(DashboardEvent),
    Hidden,
}

pub fn public_view(event: &DashboardEvent) -> PublicView {
    match event {
        DashboardEvent::Snapshot { servers, .. } => {
            PublicView::Redacted(DashboardEvent::Snapshot {
                servers: servers.iter().cloned().map(redact_server).collect(),
                alerts: Vec::new(),
            })
        }
        DashboardEvent::ServerUpsert { server } => {
            PublicView::Redacted(DashboardEvent::ServerUpsert {
                server: redact_server(server.clone()),
            })
        }
        DashboardEvent::Online { id, host, .. } => PublicView::Redacted(DashboardEvent::Online {
            id: id.clone(),
            agent_version: None,
            host: host.clone().map(|mut host| {
                redact_host(&mut host);
                host
            }),
        }),
        DashboardEvent::Alert { .. } => PublicView::Hidden,
        DashboardEvent::ServerRemoved { .. }
        | DashboardEvent::Offline { .. }
        | DashboardEvent::Metrics { .. }
        | DashboardEvent::Traffic { .. } => PublicView::Same,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moni_protocol::MinuteRollup;

    #[test]
    fn ranges_match_the_api_and_reject_unknown_values() -> Result<(), &'static str> {
        for (range, hours, step, table) in [
            ("1h", 1, MINUTE_MS, "metrics_1m"),
            ("6h", 6, MINUTE_MS, "metrics_1m"),
            ("24h", 24, MINUTE_MS, "metrics_1m"),
            ("7d", 168, 600_000, "metrics_1m"),
            ("30d", 720, HOUR_MS, "metrics_1h"),
            ("90d", 2160, HOUR_MS, "metrics_1h"),
        ] {
            let plan = history_plan(Some(range)).ok_or("Missing query plan")?;
            assert_eq!(
                (plan.duration_ms, plan.step_ms, plan.table),
                (hours * HOUR_MS, step, table)
            );
        }
        assert_eq!(history_plan(None), history_plan(Some("24h")));
        for range in ["", "2h", "7D", " 1h", "1h;DROP TABLE servers"] {
            assert!(history_plan(Some(range)).is_none());
        }
        Ok(())
    }

    #[test]
    fn minute_bounds_are_inclusive_and_require_alignment() {
        let now = 20 * DAY_MS;
        assert!(reasonable_minute(now - 7 * DAY_MS, now, 7));
        assert!(!reasonable_minute(now - 7 * DAY_MS - MINUTE_MS, now, 7));
        assert!(reasonable_minute(now + 2 * MINUTE_MS, now, 7));
        assert!(!reasonable_minute(now + 3 * MINUTE_MS, now, 7));
        assert!(!reasonable_minute(now + 1, now, 7));
        assert!(!reasonable_minute(-MINUTE_MS, now, u32::MAX));
        assert!(!reasonable_minute(i64::MAX, now, 7));
        assert_eq!(bucket(HOUR_MS + MINUTE_MS, HOUR_MS), HOUR_MS);
        assert_eq!(bucket(599_999, 600_000), 0);
        assert_eq!(bucket(600_000, 600_000), 600_000);
    }

    #[test]
    fn hourly_work_covers_recent_hours_and_recovers_missed_alarms() {
        let now = 20 * DAY_MS + 3 * HOUR_MS + MINUTE_MS;
        let hour = bucket(now, HOUR_MS);
        assert_eq!(hourly_start(0, now, 7), hour - 2 * HOUR_MS);
        assert_eq!(hourly_start(hour - HOUR_MS, now, 7), hour - 2 * HOUR_MS);
        assert_eq!(hourly_start(hour - 5 * HOUR_MS, now, 7), hour - 5 * HOUR_MS);
        assert_eq!(hourly_start(HOUR_MS, now, 7), hour - 7 * DAY_MS);
    }

    #[test]
    fn utc_month_boundaries_include_leap_days_and_year_changes() {
        assert_eq!(month_start(0), 0);
        assert_eq!(month_start(31 * DAY_MS - 1), 0);
        assert_eq!(month_start(31 * DAY_MS), 31 * DAY_MS);
        assert_eq!(month_start(1_709_251_199_999), 1_706_745_600_000); // 2024-02-29
        assert_eq!(month_start(1_709_251_200_000), 1_709_251_200_000); // 2024-03-01
        assert_eq!(month_start(1_735_689_600_000), 1_735_689_600_000); // 2025-01-01
    }

    #[test]
    fn percentages_and_slugs_handle_empty_and_extreme_inputs() {
        assert_eq!(percentage(1, 2), 50.0);
        assert_eq!(percentage(10, 0), 0.0);
        assert_eq!(percentage(u64::MAX, 1), 100.0);
        assert_eq!(slug(" My VPS / 2 "), "my-vps-2");
        assert_eq!(slug("中文"), "server");
        for name in ["---", "", "LONG", &"a".repeat(100)] {
            let id = slug(name);
            assert!(!id.is_empty() && id.len() <= 32);
            assert!(
                id.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            );
        }
    }

    #[test]
    fn parse_warnings_are_throttled_and_never_echo_string_values() -> Result<(), &'static str> {
        assert!(warning_due(None, 0));
        assert!(!warning_due(Some(0), 59_999));
        assert!(warning_due(Some(0), 60_000));
        for value in [
            "secret-token",
            "secret`token",
            "secret\"token",
            "密钥",
            &"x".repeat(500),
        ] {
            let input = serde_json::json!({"type": value}).to_string();
            let error = serde_json::from_str::<moni_protocol::AgentMessage>(&input)
                .err()
                .ok_or("Invalid message was accepted")?;
            let safe = safe_parse_error(&error);
            assert!(safe.chars().count() <= 200);
            assert!(!safe.contains("secret"));
            assert!(!safe.contains("密钥"));
            assert!(safe.contains("unknown variant"));
        }
        Ok(())
    }

    #[test]
    fn failed_logins_are_limited_per_ip_and_expire() {
        let mut failures = LoginFailures::default();
        for _ in 0..10 {
            assert!(!failures.limited("one", 1000, true));
        }
        assert!(failures.limited("one", 1001, false));
        assert!(!failures.limited("two", 1001, true));
        assert!(!failures.limited("one", 61_000, true));
    }

    #[test]
    fn request_body_detection_uses_framing_headers() {
        for length in [
            None,
            Some("0"),
            Some("00"),
            Some(""),
            Some("invalid"),
            Some("-1"),
        ] {
            assert!(!request_has_body(length, None), "{length:?}");
        }
        for length in [
            Some("1"),
            Some("42"),
            Some(" 42 "),
            Some("18446744073709551615"),
        ] {
            assert!(request_has_body(length, None), "{length:?}");
        }
        for length in [None, Some("0"), Some("42")] {
            for encoding in ["chunked", "gzip, chunked", ""] {
                assert!(request_has_body(length, Some(encoding)));
            }
        }
    }

    #[test]
    fn csrf_and_json_media_types() {
        let target = "https://moni.example.com";
        assert!(origin_allowed(Some(target), target, true));
        for origin in [
            None,
            Some("null"),
            Some("https://evil.example.com"),
            Some("http://moni.example.com"),
            Some("https://moni.example.com:444"),
        ] {
            assert!(!origin_allowed(origin, target, true));
        }
        assert!(origin_allowed(None, target, false));
        assert!(json_content_type(Some("Application/JSON; charset=utf-8")));
        for value in [None, Some("text/plain"), Some("application/jsonp")] {
            assert!(!json_content_type(value));
        }
    }

    #[test]
    fn rollup_numeric_limits() {
        let valid = MinuteRollup {
            rx_bytes: 1 << 50,
            tx_bytes: 1 << 50,
            rx_bps_max: 1 << 47,
            tx_bps_max: 1 << 47,
            cpu_avg: 100.0,
            cpu_max: 100.0,
            ..Default::default()
        };
        assert!(valid_rollup(&valid));
        for bad in [
            MinuteRollup {
                rx_bytes: (1 << 50) + 1,
                ..valid.clone()
            },
            MinuteRollup {
                tx_bps_max: (1 << 47) + 1,
                ..valid.clone()
            },
            MinuteRollup {
                cpu_avg: -1.0,
                ..valid.clone()
            },
            MinuteRollup {
                cpu_max: 100.1,
                ..valid.clone()
            },
            MinuteRollup {
                load1_avg: f32::INFINITY,
                ..valid.clone()
            },
            MinuteRollup {
                cpu_avg: f32::NAN,
                ..valid.clone()
            },
            MinuteRollup {
                mem_used_avg: 1,
                ..valid.clone()
            },
        ] {
            assert!(!valid_rollup(&bad));
        }
    }

    #[test]
    fn country_codes_accept_only_real_iso_codes() {
        assert_eq!(country_code(Some("JP")).as_deref(), Some("JP"));
        for bad in [
            None,
            Some(""),
            Some("XX"),
            Some("T1"),
            Some("jp"),
            Some("JPN"),
            Some("日本"),
        ] {
            assert_eq!(country_code(bad), None);
        }
    }

    #[test]
    fn public_routes_are_read_only_dashboard_and_history() {
        assert!(public_route(true, "/ws/dashboard"));
        assert!(public_route(true, "/api/servers/vps-1/history"));
        for (get, path) in [
            (false, "/ws/dashboard"),
            (false, "/api/servers/vps-1/history"),
            (true, "/api/servers"),
            (true, "/api/servers/vps-1"),
            (true, "/api/servers/vps-1/token"),
            (true, "/api/servers/VPS/history"),
            (true, "/api/servers//history"),
            (true, "/api/servers/a/b/history"),
            (true, "/api/alerts/rules"),
            (true, "/api/alerts/events"),
            (true, "/api/config"),
        ] {
            assert!(!public_route(get, path), "{path}");
        }
    }

    #[test]
    fn public_view_hides_identity_versions_and_alerts() {
        let host = HostInfo {
            hostname: "db-primary.internal".into(),
            os: "Debian 13".into(),
            kernel: "6.12.0-1-amd64".into(),
            arch: "x86_64".into(),
            cpu_model: "EPYC".into(),
            cpu_cores: 2,
            boot_time_ms: 1,
        };
        let server = ServerStatus {
            id: "vps-1".into(),
            name: "VPS 1".into(),
            created_ms: 0,
            online: true,
            last_seen_ms: Some(1),
            agent_version: Some("0.1.0".into()),
            host: Some(host.clone()),
            metrics: None,
            traffic_month: moni_protocol::api::Traffic {
                rx_bytes: 1,
                tx_bytes: 2,
                since_ms: 0,
            },
            alerts_firing: 2,
            country: Some("JP".into()),
        };
        let PublicView::Redacted(DashboardEvent::Snapshot { servers, alerts }) =
            public_view(&DashboardEvent::Snapshot {
                servers: vec![server.clone()],
                alerts: Vec::new(),
            })
        else {
            panic!("snapshot must be redacted");
        };
        assert!(alerts.is_empty());
        let public = &servers[0];
        let json = serde_json::to_string(public).unwrap_or_default();
        for secret in ["db-primary", "6.12.0", "0.1.0"] {
            assert!(!json.contains(secret), "{secret} leaked");
        }
        assert_eq!(public.alerts_firing, 0);
        assert_eq!(public.country.as_deref(), Some("JP"));
        assert_eq!(
            public.host.as_ref().map(|h| h.os.as_str()),
            Some("Debian 13")
        );
        let PublicView::Redacted(DashboardEvent::Online {
            agent_version,
            host,
            ..
        }) = public_view(&DashboardEvent::Online {
            id: "vps-1".into(),
            agent_version: Some("0.1.0".into()),
            host: Some(host),
        })
        else {
            panic!("online must be redacted");
        };
        assert_eq!(agent_version, None);
        assert_eq!(host.map(|h| h.hostname), Some(String::new()));
        assert!(matches!(
            public_view(&DashboardEvent::ServerUpsert { server }),
            PublicView::Redacted(DashboardEvent::ServerUpsert { server }) if server.agent_version.is_none()
        ));
        let alert = moni_protocol::api::ActiveAlert {
            rule_id: "r".into(),
            rule_name: "secret rule".into(),
            server_id: "vps-1".into(),
            server_name: "VPS 1".into(),
            metric: moni_protocol::api::AlertMetric::Cpu,
            value: None,
            since_ms: 0,
        };
        assert!(matches!(
            public_view(&DashboardEvent::Alert {
                kind: moni_protocol::api::AlertKind::Firing,
                alert
            }),
            PublicView::Hidden
        ));
    }
}
