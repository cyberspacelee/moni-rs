//! Alert decisions are independent of storage and the Worker runtime.
use moni_protocol::api::{AlertMetric, AlertOp, AlertRule, MetricsFrame};

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Ok,
    Pending(i64),
    Firing,
    KeepFiring,
    Resolved,
}

pub fn evaluate(
    rule: &AlertRule,
    value: f64,
    pending_since: Option<i64>,
    firing: bool,
    now_ms: i64,
) -> Action {
    if !rule.enabled {
        return if firing { Action::Resolved } else { Action::Ok };
    }
    if !value.is_finite() {
        return if firing {
            Action::KeepFiring
        } else {
            Action::Ok
        };
    }
    if firing {
        let recover = rule.recover_threshold.unwrap_or(match rule.op {
            AlertOp::Gt => rule.threshold * 0.9,
            AlertOp::Lt => rule.threshold * 1.1,
        });
        let recovered = match rule.op {
            AlertOp::Gt => value < recover,
            AlertOp::Lt => value > recover,
        };
        return if recovered {
            Action::Resolved
        } else {
            Action::KeepFiring
        };
    }
    let breached = match rule.op {
        AlertOp::Gt => value > rule.threshold,
        AlertOp::Lt => value < rule.threshold,
    };
    if !breached {
        return Action::Ok;
    }
    let since = pending_since.unwrap_or(now_ms);
    if now_ms.saturating_sub(since) >= i64::from(rule.duration_s) * 1000 {
        Action::Firing
    } else {
        Action::Pending(since)
    }
}

pub fn metric_value(metric: AlertMetric, frame: &MetricsFrame) -> f64 {
    match metric {
        AlertMetric::Cpu => f64::from(frame.cpu_pct),
        AlertMetric::Mem => crate::logic::percentage(frame.mem_used, frame.mem_total),
        AlertMetric::Swap => crate::logic::percentage(frame.swap_used, frame.swap_total),
        AlertMetric::Disk => crate::logic::percentage(frame.disk_used, frame.disk_total),
        AlertMetric::Load1 => f64::from(frame.load1),
        AlertMetric::NetRx => frame.net_rx_bps as f64,
        AlertMetric::NetTx => frame.net_tx_bps as f64,
        AlertMetric::Offline => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> AlertRule {
        AlertRule {
            id: "cpu".into(),
            name: "CPU".into(),
            server_id: None,
            metric: AlertMetric::Cpu,
            op: AlertOp::Gt,
            threshold: 90.0,
            recover_threshold: None,
            duration_s: 5,
            enabled: true,
            created_ms: 0,
        }
    }

    #[test]
    fn duration_requires_continuous_breach_and_uses_inclusive_deadline() {
        let r = rule();
        assert_eq!(evaluate(&r, 91.0, None, false, 1000), Action::Pending(1000));
        assert_eq!(
            evaluate(&r, 91.0, Some(1000), false, 5999),
            Action::Pending(1000)
        );
        assert_eq!(evaluate(&r, 91.0, Some(1000), false, 6000), Action::Firing);
        assert_eq!(evaluate(&r, 90.0, Some(1000), false, 6000), Action::Ok);
        assert_eq!(evaluate(&r, 91.0, None, false, 7000), Action::Pending(7000));
    }

    #[test]
    fn hysteresis_keeps_firing_until_recovery_threshold_is_crossed() {
        let mut r = rule();
        for value in [95.0, 89.0, 81.0] {
            assert_eq!(evaluate(&r, value, None, true, 0), Action::KeepFiring);
        }
        assert_eq!(evaluate(&r, 80.0, None, true, 0), Action::Resolved);
        r.recover_threshold = Some(85.0);
        assert_eq!(evaluate(&r, 84.0, None, true, 0), Action::Resolved);
    }

    #[test]
    fn lt_duration_zero_disabled_and_invalid_values() {
        let mut r = rule();
        r.op = AlertOp::Lt;
        r.duration_s = 0;
        assert_eq!(evaluate(&r, 89.0, None, false, 0), Action::Firing);
        assert_eq!(evaluate(&r, 90.0, None, false, 0), Action::Ok);
        assert_eq!(evaluate(&r, 98.0, None, true, 0), Action::KeepFiring);
        assert_eq!(evaluate(&r, 100.0, None, true, 0), Action::Resolved);
        r.enabled = false;
        assert_eq!(evaluate(&r, 89.0, Some(0), false, 5000), Action::Ok);
        assert_eq!(evaluate(&r, 89.0, None, true, 0), Action::Resolved);
        r.enabled = true;
        assert_eq!(evaluate(&r, f64::NAN, None, false, 0), Action::Ok);
        assert_eq!(
            evaluate(&r, f64::INFINITY, None, true, 0),
            Action::KeepFiring
        );
    }
}
