//! Persistent state lives in SQLite; live connection state survives hibernation in attachments.

mod alerts;
mod data;
mod schema;
mod servers;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    time::Duration,
};

use moni_protocol::api::{ApiErrorCode, DashboardEvent, LoginRequest, MetricsFrame, Traffic};
use moni_protocol::{AgentMessage, CLOSE_INCOMPATIBLE, HubMessage, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use worker::*;

use crate::{
    auth::{self, H_SERVER_ID, H_TOKEN_HASH, now_ms},
    error, logic, report_interval,
};

const TAG_AGENT: &str = "agent";
const TAG_DASH: &str = "dash";
const ALARM_PERIOD_MS: i64 = 30_000;

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "role", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
enum Attachment {
    Agent(AgentConn),
    Dash,
}

#[derive(Serialize, Deserialize, Clone)]
struct AgentConn {
    server_id: String,
    last_seen_ms: i64,
    closing: bool,
    latest: Option<MetricsFrame>,
    #[serde(default)]
    parse_warn_ms: Option<i64>,
    #[serde(default)]
    pending: HashMap<String, i64>,
    #[serde(default)]
    pending_warn_ms: Option<i64>,
    #[serde(default)]
    traffic_month: Option<CachedTraffic>,
}

// Attachments use JS numbers; serializing a u64 above 2^53 through workers-rs fails.
#[derive(Serialize, Deserialize, Clone)]
struct CachedTraffic {
    rx_bytes: f64,
    tx_bytes: f64,
    since_ms: i64,
}

impl From<CachedTraffic> for Traffic {
    fn from(value: CachedTraffic) -> Self {
        Self {
            rx_bytes: value.rx_bytes as u64,
            tx_bytes: value.tx_bytes as u64,
            since_ms: value.since_ms,
        }
    }
}

fn tag_for(id: &str) -> String {
    format!("server:{id}")
}

fn agent_conn(ws: &WebSocket) -> Option<AgentConn> {
    match ws.deserialize_attachment().ok().flatten() {
        Some(Attachment::Agent(conn)) => Some(conn),
        _ => None,
    }
}

fn save(ws: &WebSocket, conn: AgentConn) -> Result<()> {
    ws.serialize_attachment(Attachment::Agent(conn))
}

#[durable_object]
pub struct Hub {
    state: State,
    env: Env,
    login_failures: RefCell<logic::LoginFailures>,
    rules_cache: RefCell<Option<Vec<moni_protocol::api::AlertRule>>>,
    alarm_check_ms: Cell<Option<i64>>,
}

impl DurableObject for Hub {
    fn new(state: State, env: Env) -> Self {
        if schema::migrate(&state.storage().sql()).is_err() {
            console_error!("Hub schema migration failed");
        }
        let hub = Self {
            state,
            env,
            login_failures: RefCell::default(),
            rules_cache: RefCell::default(),
            alarm_check_ms: Cell::new(None),
        };
        if hub.rules().is_err() {
            console_error!("Hub alert rule loading failed");
        }
        hub
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let is_test = req.path() == "/api/alerts/test";
        let result = self.route(req).await;
        if !is_test && let Err(error) = self.dispatch_notifications() {
            console_error!("Hub notification dispatch failed: {}", error);
        }
        error::finish(result)
    }

    async fn websocket_message(
        &self,
        ws: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        let Some(mut conn) = agent_conn(&ws).filter(|c| !c.closing) else {
            return Ok(());
        };
        let WebSocketIncomingMessage::String(text) = message else {
            return Ok(());
        };
        let message = match serde_json::from_str::<AgentMessage>(&text) {
            Ok(message) => message,
            Err(error) => {
                let now = now_ms();
                if logic::warning_due(conn.parse_warn_ms, now) {
                    console_warn!(
                        "Invalid Agent message server={}: {}",
                        conn.server_id,
                        logic::safe_parse_error(&error)
                    );
                    conn.parse_warn_ms = Some(now);
                    save(&ws, conn)?;
                }
                return Ok(());
            }
        };
        conn.last_seen_ms = now_ms();
        self.heal_alarm(conn.last_seen_ms).await;
        self.refresh_traffic(&ws, &mut conn)?;
        match message {
            AgentMessage::Hello {
                protocol,
                agent_version,
                host,
            } => {
                if protocol != PROTOCOL_VERSION {
                    self.on_disconnect(&ws)?;
                    ws.close(Some(CLOSE_INCOMPATIBLE), Some("incompatible protocol"))?;
                    return Ok(());
                }
                // Bound untrusted metadata before storage; no host data is kept in attachments.
                if serde_json::to_string(&host)?.len() > 8192 || agent_version.len() > 128 {
                    self.on_disconnect(&ws)?;
                    ws.close(Some(1009), Some("host metadata too large"))?;
                    return Ok(());
                }
                self.state.storage().sql().exec(
                    "UPDATE servers SET host_json = ?, agent_version = ? WHERE id = ?",
                    vec![
                        serde_json::to_string(&host)?.into(),
                        agent_version.as_str().into(),
                        conn.server_id.as_str().into(),
                    ],
                )?;
                save(&ws, conn.clone())?;
                self.broadcast(&DashboardEvent::Online {
                    id: conn.server_id.clone(),
                    agent_version: Some(agent_version),
                    host: Some(host),
                });
                self.upsert_event(&conn.server_id)?;
            }
            AgentMessage::Metrics(frame) => {
                conn.latest = Some(frame.clone());
                self.evaluate_metrics(&mut conn, &frame)?;
                save(&ws, conn.clone())?;
                self.broadcast(&DashboardEvent::Metrics {
                    id: conn.server_id,
                    frame,
                });
            }
            AgentMessage::Rollup(rollup) => {
                let stored = match self.store_rollup(&mut conn, &rollup).and_then(|stored| {
                    save(&ws, conn.clone())?;
                    Ok(stored)
                }) {
                    Ok(stored) => stored,
                    Err(error) => {
                        console_error!(
                            "Hub rollup persistence failed server={}: {}",
                            conn.server_id,
                            error
                        );
                        if let Err(error) = self.on_disconnect(&ws) {
                            console_error!("Hub disconnect after rollup failure failed: {}", error);
                        }
                        ws.close(Some(1011), Some("rollup persistence failed"))?;
                        return Err(error);
                    }
                };
                if stored && let Some(traffic) = conn.traffic_month {
                    self.broadcast(&DashboardEvent::Traffic {
                        id: conn.server_id,
                        traffic: traffic.into(),
                    });
                }
                ws.send(&HubMessage::Ack {
                    minute_ms: rollup.minute_ms,
                })?;
            }
        }
        self.dispatch_notifications()?;
        Ok(())
    }

    async fn websocket_close(
        &self,
        ws: WebSocket,
        _code: usize,
        _reason: String,
        _clean: bool,
    ) -> Result<()> {
        self.on_disconnect(&ws)?;
        ws.close(Some(1000), None::<&str>)
    }

    async fn websocket_error(&self, ws: WebSocket, _error: Error) -> Result<()> {
        self.on_disconnect(&ws)?;
        ws.close(Some(1011), Some("connection error"))
    }

    async fn alarm(&self) -> Result<Response> {
        // Reschedule before fallible work so a bad row or connection cannot stop maintenance.
        let had_agents = self.has_agents();
        let scheduled = match self.schedule_alarm().await {
            Ok(()) => true,
            Err(error) => {
                console_error!("Hub alarm scheduling failed: {}", error);
                false
            }
        };
        let now = now_ms();
        let interval_ms = i64::from(report_interval(&self.env)) * 1000;
        for ws in self.state.get_websockets_with_tag(TAG_AGENT) {
            let Some(conn) = agent_conn(&ws).filter(|c| !c.closing) else {
                continue;
            };
            if now - conn.last_seen_ms > 3 * interval_ms {
                if let Err(error) = self.on_disconnect(&ws) {
                    console_error!("Hub stale disconnect failed: {}", error);
                }
                let _ = ws.close(Some(4000), Some("stale"));
            }
        }
        if let Err(error) = self.evaluate_offline(now) {
            console_error!("Hub offline alerts failed: {}", error);
        }
        if let Err(error) = self.dispatch_notifications() {
            console_error!("Hub notification dispatch failed: {}", error);
        }
        if let Err(error) = self.maintain(now) {
            console_error!("Hub maintenance failed: {}", error);
        }
        if (!scheduled || had_agents != self.has_agents())
            && let Err(error) = self.schedule_alarm().await
        {
            console_error!("Hub alarm scheduling failed: {}", error);
            return Err(error);
        }
        Response::from_json(&"ok")
    }
}

impl Hub {
    async fn route(&self, req: Request) -> Result<Response> {
        schema::ready(&self.state.storage().sql())?;
        let path = req.path();
        match (req.method(), path.as_str()) {
            (Method::Post, "/api/auth/login") => self.login(req).await,
            (Method::Get, "/ws/agent") => self.accept_agent(req).await,
            (Method::Get, "/ws/dashboard") => self.accept_dash(),
            (Method::Get, "/api/alerts/rules") => Response::from_json(&self.rules()?),
            (Method::Post, "/api/alerts/rules") => self.create_rule(req).await,
            (Method::Get, "/api/alerts/active") => Response::from_json(&self.active_alerts()?),
            (Method::Get, "/api/alerts/events") => self.alert_events(&req),
            (Method::Post, "/api/alerts/test") => self.test_notifications().await,
            (Method::Get, "/api/servers") => Response::from_json(&self.snapshot()?),
            (Method::Post, "/api/servers") => self.create_server(req).await,
            _ => {
                if let Some(id) = path.strip_prefix("/api/alerts/rules/") {
                    return match req.method() {
                        Method::Patch => self.update_rule(id, req).await,
                        Method::Delete => self.delete_rule(id),
                        _ => error::not_found(),
                    };
                }
                let Some(rest) = path.strip_prefix("/api/servers/") else {
                    return error::not_found();
                };
                if let Some(id) = rest.strip_suffix("/token") {
                    if req.method() == Method::Post && auth::valid_id(id) {
                        return self.rotate_token(id);
                    }
                } else if let Some(id) = rest.strip_suffix("/history") {
                    if req.method() == Method::Get && auth::valid_id(id) {
                        return self.history(id, &req);
                    }
                } else if auth::valid_id(rest) {
                    match req.method() {
                        Method::Patch => return self.update_server(rest, req).await,
                        Method::Delete => return self.delete_server(rest),
                        _ => {}
                    }
                }
                error::not_found()
            }
        }
    }

    async fn login(&self, mut req: Request) -> Result<Response> {
        let secrets = auth::Secrets::load(&self.env).ok_or("Missing secrets")?;
        let Ok(input) = req.json::<LoginRequest>().await else {
            return error::bad_request("Expected a password");
        };
        let ip = req
            .headers()
            .get("CF-Connecting-IP")?
            .unwrap_or_else(|| "local".into());
        let valid = auth::secret_eq(&input.password, &secrets.password);
        if self
            .login_failures
            .borrow_mut()
            .limited(&ip, now_ms(), !valid)
        {
            return error::error(
                429,
                ApiErrorCode::RateLimited,
                "Too many failed logins; retry in one minute",
            );
        }
        if !valid {
            return error::unauthorized();
        }
        auth::session_response(&req, &secrets.key, false)
    }

    async fn schedule_alarm(&self) -> Result<()> {
        let seconds = if self.has_agents() { 30 } else { 300 };
        self.state
            .storage()
            .set_alarm(Duration::from_secs(seconds))
            .await
    }

    async fn heal_alarm(&self, now: i64) {
        if self
            .alarm_check_ms
            .get()
            .is_some_and(|last| now.saturating_sub(last) < 300_000)
        {
            return;
        }
        // This throttle may be lost on eviction; correctness lives in the platform alarm.
        self.alarm_check_ms.set(Some(now));
        let result = async {
            if self.state.storage().get_alarm().await?.is_none() {
                self.schedule_alarm().await?;
            }
            Ok::<(), Error>(())
        }
        .await;
        if let Err(error) = result {
            console_error!("Hub alarm recovery failed: {}", error);
        }
    }

    async fn accept_agent(&self, req: Request) -> Result<Response> {
        let Some(id) = req.headers().get(H_SERVER_ID)? else {
            return error::unauthorized();
        };
        let Some(hash) = req.headers().get(H_TOKEN_HASH)? else {
            return error::unauthorized();
        };
        let Some(row) = self.server_row(&id)? else {
            return error::unauthorized();
        };
        if !auth::secret_eq(&hash, &row.token_hash) {
            return error::unauthorized();
        }

        let tag = tag_for(&id);
        for old in self.state.get_websockets_with_tag(&tag) {
            if let Some(mut conn) = agent_conn(&old).filter(|c| !c.closing) {
                // Mark before close so the later callback cannot announce the replacement offline.
                conn.closing = true;
                save(&old, conn)?;
            }
            let _ = old.close(Some(4002), Some("replaced by new connection"));
        }
        let pair = WebSocketPair::new()?;
        self.state
            .accept_websocket_with_tags(&pair.server, &[TAG_AGENT, &tag]);
        let now = now_ms();
        save(
            &pair.server,
            AgentConn {
                server_id: id.clone(),
                last_seen_ms: now,
                closing: false,
                latest: None,
                parse_warn_ms: None,
                pending: HashMap::new(),
                pending_warn_ms: None,
                traffic_month: Some(self.safe_traffic_month(&id)),
            },
        )?;
        pair.server.send(&HubMessage::Welcome {
            server_id: id.clone(),
            report_interval_s: report_interval(&self.env),
        })?;
        self.state.storage().sql().exec(
            "UPDATE servers SET last_seen_ms = ? WHERE id = ?",
            vec![now.into(), id.as_str().into()],
        )?;
        self.broadcast(&DashboardEvent::Online {
            id: id.clone(),
            agent_version: row.agent_version,
            host: row
                .host_json
                .and_then(|json| serde_json::from_str(&json).ok()),
        });
        self.resolve_server_alerts(&id, true)?;
        self.upsert_event(&id)?;
        let alarm = self.state.storage().get_alarm().await?;
        if alarm.is_none_or(|t| t > now + 2 * ALARM_PERIOD_MS) {
            self.schedule_alarm().await?;
        }
        Response::from_websocket(pair.client)
    }

    fn accept_dash(&self) -> Result<Response> {
        let pair = WebSocketPair::new()?;
        self.state
            .accept_websocket_with_tags(&pair.server, &[TAG_DASH]);
        pair.server.serialize_attachment(Attachment::Dash)?;
        pair.server.send(&DashboardEvent::Snapshot {
            servers: self.snapshot()?,
            alerts: self.active_alerts()?,
        })?;
        Response::from_websocket(pair.client)
    }

    fn on_disconnect(&self, ws: &WebSocket) -> Result<()> {
        let Some(mut conn) = agent_conn(ws).filter(|c| !c.closing) else {
            return Ok(());
        };
        conn.closing = true;
        let id = conn.server_id.clone();
        let last_seen_ms = conn.last_seen_ms;
        save(ws, conn)?;
        if self
            .state
            .get_websockets_with_tag(&tag_for(&id))
            .iter()
            .any(|other| agent_conn(other).is_some_and(|c| !c.closing))
        {
            return Ok(());
        }
        self.state.storage().sql().exec(
            "UPDATE servers SET last_seen_ms = ? WHERE id = ?",
            vec![last_seen_ms.into(), id.as_str().into()],
        )?;
        self.broadcast(&DashboardEvent::Offline { id, last_seen_ms });
        Ok(())
    }

    fn broadcast(&self, event: &DashboardEvent) {
        let Ok(json) = serde_json::to_string(event) else {
            return;
        };
        for ws in self.state.get_websockets_with_tag(TAG_DASH) {
            let _ = ws.send_with_str(&json);
        }
    }
}
