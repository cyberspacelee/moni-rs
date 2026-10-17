use moni_protocol::api::{
    ActiveAlert, AlertEvent, AlertKind, AlertMetric, AlertRule, AlertRuleInput, AlertRulePatch,
    DashboardEvent, MetricsFrame,
};
use serde::Deserialize;
use serde_json::{Value, json};
use worker::*;

use super::{AgentConn, Hub, TAG_AGENT, agent_conn, save, servers::ServerRow};
use crate::{
    alert_logic::{self, Action},
    auth::now_ms,
    error,
    notifications::{self, Notification},
};

const ACTIVE_SQL: &str = "SELECT a.rule_id, r.name AS rule_name, a.server_id,
    s.name AS server_name, r.metric, a.value, a.since_ms FROM alert_state a
    JOIN alert_rules r ON r.id=a.rule_id JOIN servers s ON s.id=a.server_id";

impl Hub {
    pub(super) fn rules(&self) -> Result<Vec<AlertRule>> {
        if let Some(rules) = self.rules_cache.borrow().as_ref() {
            return Ok(rules.clone());
        }
        let rows = self
            .state
            .storage()
            .sql()
            .exec("SELECT * FROM alert_rules ORDER BY created_ms, id", None)?
            .to_array::<Value>()?;
        let mut rules = Vec::with_capacity(rows.len());
        for mut row in rows {
            let enabled = row.get("enabled").and_then(Value::as_i64) == Some(1);
            if let Some(object) = row.as_object_mut() {
                object.insert("enabled".into(), Value::Bool(enabled));
            }
            rules.push(serde_json::from_value(row)?);
        }
        *self.rules_cache.borrow_mut() = Some(rules.clone());
        Ok(rules)
    }

    pub(super) fn active_alerts(&self) -> Result<Vec<ActiveAlert>> {
        self.state
            .storage()
            .sql()
            .exec(
                &format!("{ACTIVE_SQL} ORDER BY a.since_ms, a.rule_id, a.server_id"),
                None,
            )?
            .to_array()
    }

    pub(super) fn firing_count(&self, id: &str) -> Result<u32> {
        #[derive(Deserialize)]
        struct Count {
            n: u32,
        }
        Ok(self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT COUNT(*) AS n FROM alert_state WHERE server_id = ?",
                vec![id.into()],
            )?
            .one::<Count>()?
            .n)
    }

    pub(super) fn alert_events(&self, req: &Request) -> Result<Response> {
        let mut limit = 50i64;
        let mut before = i64::MAX;
        for (key, value) in req.url()?.query_pairs() {
            match key.as_ref() {
                "limit" => match value.parse::<i64>() {
                    Ok(n) if n > 0 => limit = n.min(200),
                    _ => return error::bad_request("limit must be a positive integer"),
                },
                "before_id" => match value.parse::<i64>() {
                    Ok(n) if n > 0 => before = n,
                    _ => return error::bad_request("before_id must be a positive integer"),
                },
                _ => {}
            }
        }
        let events = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT e.id,e.rule_id,
            COALESCE(e.rule_name,r.name,e.rule_id) AS rule_name,e.server_id,
            COALESCE(e.server_name,s.name,e.server_id) AS server_name,e.kind,e.value,e.ts_ms
            FROM alert_events e LEFT JOIN alert_rules r ON r.id=e.rule_id
            LEFT JOIN servers s ON s.id=e.server_id WHERE e.id < ? ORDER BY e.id DESC LIMIT ?",
                vec![before.into(), limit.into()],
            )?
            .to_array::<AlertEvent>()?;
        Response::from_json(&events)
    }

    fn validate_rule(&self, rule: &AlertRule) -> Result<Option<&'static str>> {
        if rule.name.trim().is_empty() || rule.name.len() > 256 {
            return Ok(Some("Rule name must contain 1–256 bytes"));
        }
        if !rule.threshold.is_finite() || rule.recover_threshold.is_some_and(|v| !v.is_finite()) {
            return Ok(Some("Thresholds must be finite"));
        }
        if rule.duration_s > 86_400 {
            return Ok(Some("duration_s must be between 0 and 86400"));
        }
        if let Some(id) = &rule.server_id
            && self.server_row(id)?.is_none()
        {
            return Ok(Some("Unknown server_id"));
        }
        Ok(None)
    }

    fn write_rule(&self, rule: &AlertRule) -> Result<()> {
        let metric = serde_json::to_value(rule.metric)?;
        let op = serde_json::to_value(rule.op)?;
        self.state.storage().sql().exec("INSERT INTO alert_rules
            (id,name,server_id,metric,op,threshold,recover_threshold,duration_s,enabled,created_ms)
            VALUES (?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET
            name=excluded.name,server_id=excluded.server_id,metric=excluded.metric,
            op=excluded.op,threshold=excluded.threshold,recover_threshold=excluded.recover_threshold,
            duration_s=excluded.duration_s,enabled=excluded.enabled",
            vec![rule.id.as_str().into(),rule.name.as_str().into(),rule.server_id.clone().into(),
                metric.as_str().ok_or("Invalid metric")?.into(),op.as_str().ok_or("Invalid op")?.into(),
                rule.threshold.into(),rule.recover_threshold.into(),i64::from(rule.duration_s).into(),
                i64::from(rule.enabled).into(),rule.created_ms.into()])?;
        *self.rules_cache.borrow_mut() = None;
        Ok(())
    }

    pub(super) async fn create_rule(&self, mut req: Request) -> Result<Response> {
        let Ok(input) = req.json::<AlertRuleInput>().await else {
            return error::bad_request("Expected a valid alert rule");
        };
        let rule = AlertRule {
            id: crate::auth::new_token("rule")?,
            name: input.name.trim().into(),
            server_id: input.server_id,
            metric: input.metric,
            op: input.op,
            threshold: input.threshold,
            recover_threshold: input.recover_threshold,
            duration_s: input.duration_s,
            enabled: input.enabled,
            created_ms: now_ms(),
        };
        if let Some(message) = self.validate_rule(&rule)? {
            return error::bad_request(message);
        }
        self.write_rule(&rule)?;
        if self.state.storage().get_alarm().await?.is_none() {
            self.schedule_alarm().await?;
        }
        Response::from_json(&rule).map(|response| response.with_status(201))
    }

    pub(super) async fn update_rule(&self, id: &str, mut req: Request) -> Result<Response> {
        let Ok(value) = req.json::<Value>().await else {
            return error::bad_request("Expected rule fields");
        };
        if !value.is_object()
            || ["name", "metric", "op", "threshold", "duration_s", "enabled"]
                .iter()
                .any(|key| value.get(key).is_some_and(Value::is_null))
        {
            return error::bad_request("Only server_id and recover_threshold may be null");
        }
        let Ok(patch) = serde_json::from_value::<AlertRulePatch>(value) else {
            return error::bad_request("Invalid rule fields");
        };
        let Some(mut rule) = self.rules()?.into_iter().find(|r| r.id == id) else {
            return error::not_found();
        };
        let old = rule.clone();
        if let Some(v) = patch.name {
            rule.name = v.trim().into();
        }
        if let Some(v) = patch.server_id {
            rule.server_id = v;
        }
        if let Some(v) = patch.metric {
            rule.metric = v;
        }
        if let Some(v) = patch.op {
            rule.op = v;
        }
        if let Some(v) = patch.threshold {
            rule.threshold = v;
        }
        if let Some(v) = patch.recover_threshold {
            rule.recover_threshold = v;
        }
        if let Some(v) = patch.duration_s {
            rule.duration_s = v;
        }
        if let Some(v) = patch.enabled {
            rule.enabled = v;
        }
        if let Some(message) = self.validate_rule(&rule)? {
            return error::bad_request(message);
        }
        // A timer accumulated under another condition must never fire the edited rule early.
        self.clear_pending(id)?;
        if !rule.enabled
            || rule.metric != old.metric
            || rule.op != old.op
            || rule.server_id != old.server_id
        {
            self.resolve_rule(id)?;
        }
        self.write_rule(&rule)?;
        Response::from_json(&rule)
    }

    pub(super) fn delete_rule(&self, id: &str) -> Result<Response> {
        if !self.rules()?.iter().any(|rule| rule.id == id) {
            return error::not_found();
        }
        self.resolve_rule(id)?;
        self.clear_pending(id)?;
        self.state
            .storage()
            .sql()
            .exec("DELETE FROM alert_rules WHERE id = ?", vec![id.into()])?;
        *self.rules_cache.borrow_mut() = None;
        Response::empty().map(|response| response.with_status(204))
    }

    fn clear_pending(&self, id: &str) -> Result<()> {
        for ws in self.state.get_websockets_with_tag(TAG_AGENT) {
            if let Some(mut conn) = agent_conn(&ws)
                && conn.pending.remove(id).is_some()
            {
                save(&ws, conn)?;
            }
        }
        Ok(())
    }

    fn resolve_rule(&self, id: &str) -> Result<()> {
        for alert in self
            .active_alerts()?
            .into_iter()
            .filter(|a| a.rule_id == id)
        {
            self.transition(AlertKind::Resolved, alert, now_ms())?;
        }
        Ok(())
    }

    pub(super) fn resolve_server_alerts(&self, id: &str, offline_only: bool) -> Result<()> {
        for alert in self
            .active_alerts()?
            .into_iter()
            .filter(|a| a.server_id == id && (!offline_only || a.metric == AlertMetric::Offline))
        {
            self.transition(AlertKind::Resolved, alert, now_ms())?;
        }
        Ok(())
    }

    pub(super) fn evaluate_metrics(
        &self,
        conn: &mut AgentConn,
        frame: &MetricsFrame,
    ) -> Result<()> {
        let now = now_ms();
        #[derive(Deserialize)]
        struct Firing {
            rule_id: String,
            since_ms: i64,
        }
        let active = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT rule_id,since_ms FROM alert_state WHERE server_id = ?",
                vec![conn.server_id.as_str().into()],
            )?
            .to_array::<Firing>()?;
        let rules = self.rules()?;
        conn.pending.retain(|id, _| {
            rules.iter().any(|r| {
                r.id == *id
                    && r.enabled
                    && r.metric != AlertMetric::Offline
                    && applies(r, &conn.server_id)
            })
        });
        for rule in rules
            .iter()
            .filter(|r| r.metric != AlertMetric::Offline && applies(r, &conn.server_id))
        {
            let firing = active.iter().find(|a| a.rule_id == rule.id);
            let value = alert_logic::metric_value(rule.metric, frame);
            match alert_logic::evaluate(
                rule,
                value,
                conn.pending.get(&rule.id).copied(),
                firing.is_some(),
                now,
            ) {
                Action::Pending(since) => {
                    if conn.pending.contains_key(&rule.id) || conn.pending.len() < 64 {
                        conn.pending.insert(rule.id.clone(), since);
                    } else if crate::logic::warning_due(conn.pending_warn_ms, now) {
                        console_warn!("Alert pending limit (64) reached server={}", conn.server_id);
                        conn.pending_warn_ms = Some(now);
                    }
                }
                action => {
                    let since = conn.pending.remove(&rule.id).unwrap_or(now);
                    match action {
                        Action::Firing | Action::Resolved => {
                            if let Some(server) = self.server_row(&conn.server_id)? {
                                let kind = if action == Action::Firing {
                                    AlertKind::Firing
                                } else {
                                    AlertKind::Resolved
                                };
                                let since = firing.map_or(since, |alert| alert.since_ms);
                                self.transition(
                                    kind,
                                    active_for(rule, &server, Some(value), since),
                                    now,
                                )?;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn evaluate_offline(&self, now: i64) -> Result<()> {
        let rules = self.rules()?;
        let active = self.active_alerts()?;
        let servers = self
            .state
            .storage()
            .sql()
            .exec("SELECT * FROM servers", None)?
            .to_array::<ServerRow>()?;
        for server in servers {
            let online = self
                .state
                .get_websockets_with_tag(&super::tag_for(&server.id))
                .iter()
                .any(|ws| agent_conn(ws).is_some_and(|c| !c.closing));
            for rule in rules
                .iter()
                .filter(|r| r.enabled && r.metric == AlertMetric::Offline && applies(r, &server.id))
            {
                let firing = active
                    .iter()
                    .find(|a| a.rule_id == rule.id && a.server_id == server.id);
                if online {
                    if let Some(alert) = firing {
                        self.transition(AlertKind::Resolved, alert.clone(), now)?;
                    }
                } else if firing.is_none()
                    && now.saturating_sub(server.last_seen_ms.unwrap_or(server.created_ms))
                        >= i64::from(rule.duration_s) * 1000
                {
                    self.transition(AlertKind::Firing, active_for(rule, &server, None, now), now)?;
                }
            }
        }
        Ok(())
    }

    fn transition(&self, kind: AlertKind, alert: ActiveAlert, now: i64) -> Result<()> {
        let sql = self.state.storage().sql();
        match kind {
            AlertKind::Firing => {
                sql.exec(
                    "INSERT INTO alert_state (rule_id,server_id,since_ms,value) VALUES (?,?,?,?)",
                    vec![
                        alert.rule_id.as_str().into(),
                        alert.server_id.as_str().into(),
                        alert.since_ms.into(),
                        alert.value.into(),
                    ],
                )?;
            }
            AlertKind::Resolved => {
                sql.exec(
                    "DELETE FROM alert_state WHERE rule_id = ? AND server_id = ?",
                    vec![
                        alert.rule_id.as_str().into(),
                        alert.server_id.as_str().into(),
                    ],
                )?;
            }
        }
        let event = match kind {
            AlertKind::Firing => "firing",
            AlertKind::Resolved => "resolved",
        };
        let rule = self
            .rules()?
            .into_iter()
            .find(|r| r.id == alert.rule_id)
            .ok_or("Missing alert rule")?;
        let notice = Notification {
            event: event.into(),
            rule: json!(rule),
            server: json!({"id": alert.server_id, "name": alert.server_name}),
            value: alert.value,
            ts_ms: now,
        };
        sql.exec(
            "INSERT INTO alert_events (rule_id,server_id,kind,value,ts_ms,rule_name,server_name)
            VALUES (?,?,?,?,?,?,?)",
            vec![
                alert.rule_id.as_str().into(),
                alert.server_id.as_str().into(),
                event.into(),
                alert.value.into(),
                now.into(),
                alert.rule_name.as_str().into(),
                alert.server_name.as_str().into(),
            ],
        )?;
        sql.exec(
            "INSERT INTO notification_queue (server_id,notification_json) VALUES (?,?)",
            vec![
                alert.server_id.as_str().into(),
                serde_json::to_string(&notice)?.into(),
            ],
        )?;
        self.broadcast(&DashboardEvent::Alert { kind, alert });
        Ok(())
    }

    pub(super) fn dispatch_notifications(&self) -> Result<()> {
        #[derive(Deserialize)]
        struct Queued {
            id: i64,
            notification_json: String,
        }
        let sql = self.state.storage().sql();
        // A transition uses at most two requests; leave room below the Free invocation limit of 50.
        let rows = sql
            .exec(
                "SELECT id,notification_json FROM notification_queue ORDER BY id LIMIT 24",
                None,
            )?
            .to_array::<Queued>()?;
        for row in rows {
            let notice = serde_json::from_str::<Notification>(&row.notification_json)?;
            // Claim before awaiting; delivery failures are logged, never retried.
            sql.exec(
                "DELETE FROM notification_queue WHERE id = ?",
                vec![row.id.into()],
            )?;
            let env = self.env.clone();
            self.state.wait_until(async move {
                notifications::send(env, notice).await;
            });
        }
        Ok(())
    }

    pub(super) async fn test_notifications(&self) -> Result<Response> {
        Response::from_json(
            &notifications::send(
                self.env.clone(),
                Notification {
                    event: "test".into(),
                    rule: json!({"id":"test","name":"通知测试"}),
                    server: json!({"id":"test","name":"moni-rs"}),
                    value: None,
                    ts_ms: now_ms(),
                },
            )
            .await,
        )
    }
}

fn applies(rule: &AlertRule, id: &str) -> bool {
    rule.server_id.as_deref().is_none_or(|server| server == id)
}

fn active_for(
    rule: &AlertRule,
    server: &ServerRow,
    value: Option<f64>,
    since_ms: i64,
) -> ActiveAlert {
    ActiveAlert {
        rule_id: rule.id.clone(),
        rule_name: rule.name.clone(),
        server_id: server.id.clone(),
        server_name: server.name.clone(),
        metric: rule.metric,
        value,
        since_ms,
    }
}
