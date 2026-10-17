//! Persistent state lives in SQLite; live connection state survives hibernation in attachments.

mod schema;
mod servers;

use std::{cell::RefCell, collections::HashMap, time::Duration};

use moni_protocol::api::{ApiErrorCode, DashboardEvent, LoginRequest, MetricsFrame};
use moni_protocol::{AgentMessage, CLOSE_INCOMPATIBLE, HubMessage, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use worker::*;

use crate::{
    auth::{self, H_SERVER_ID, H_TOKEN_HASH, now_ms},
    error, report_interval,
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

#[derive(Default)]
struct LoginFailures {
    by_ip: HashMap<String, (i64, u8)>,
}

impl LoginFailures {
    fn limited(&mut self, ip: &str, now: i64, failed: bool) -> bool {
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

#[durable_object]
pub struct Hub {
    state: State,
    env: Env,
    login_failures: RefCell<LoginFailures>,
}

impl DurableObject for Hub {
    fn new(state: State, env: Env) -> Self {
        if schema::migrate(&state.storage().sql()).is_err() {
            console_error!("Hub schema migration failed");
        }
        Self {
            state,
            env,
            login_failures: RefCell::default(),
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        error::finish(self.route(req).await)
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
        let Ok(message) = serde_json::from_str::<AgentMessage>(&text) else {
            return Ok(());
        };
        conn.last_seen_ms = now_ms();
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
                save(&ws, conn.clone())?;
                self.broadcast(&DashboardEvent::Metrics {
                    id: conn.server_id,
                    frame,
                });
            }
            AgentMessage::Rollup(rollup) => {
                save(&ws, conn)?;
                // shortcut: B3 acknowledges without storage; B4 must persist before Ack.
                ws.send(&HubMessage::Ack {
                    minute_ms: rollup.minute_ms,
                })?;
            }
        }
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
        let now = now_ms();
        let interval_ms = i64::from(report_interval(&self.env)) * 1000;
        for ws in self.state.get_websockets_with_tag(TAG_AGENT) {
            let Some(conn) = agent_conn(&ws).filter(|c| !c.closing) else {
                continue;
            };
            if now - conn.last_seen_ms > 3 * interval_ms {
                self.on_disconnect(&ws)?;
                let _ = ws.close(Some(4000), Some("stale"));
            }
        }
        self.schedule_alarm().await?;
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
            (Method::Get, "/api/servers") => Response::from_json(&self.snapshot()?),
            (Method::Post, "/api/servers") => self.create_server(req).await,
            _ => {
                let Some(rest) = path.strip_prefix("/api/servers/") else {
                    return error::not_found();
                };
                if let Some(id) = rest.strip_suffix("/token") {
                    if req.method() == Method::Post && auth::valid_id(id) {
                        return self.rotate_token(id);
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
            alerts: Vec::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
