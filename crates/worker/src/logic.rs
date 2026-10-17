//! Runtime-independent query plans and time rules shared by the Hub and native tests.
use moni_protocol::api::HistoryRange;

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
}
