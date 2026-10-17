//! Singleton Hub Durable Object (ADR-0002 / ADR-0003).
//!
//! Agents and dashboards connect through the WebSocket Hibernation API. The runtime may evict the
//! instance between any two messages (verified in PoC), so per-connection state — including the
//! latest frame and the current minute's rollup — lives in the socket attachment, not in fields.
//! Per-minute rollups are written to the DO's SQLite storage.

use std::cell::Cell;
use std::time::Duration;

use moni_protocol::{
    AgentMessage, CLOSE_INCOMPATIBLE, MetricsFrame, PROTOCOL_VERSION, ServerEvent, ServerStatus,
};
use serde::{Deserialize, Serialize};
use worker::*;

use crate::H_SERVER_ID;

const TAG_AGENT: &str = "agent";
const TAG_DASH: &str = "dash";
const ALARM_PERIOD_MS: i64 = 30_000;
const CLOSE_STALE: u16 = 4000;
const CLOSE_REPLACED: u16 = 4002;

/// Per-connection state; survives hibernation via serialize_attachment (max 16 KiB).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "role", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // short-lived (de)serialization value
enum Attachment {
    Agent(AgentConn),
    Dash { connected_ms: i64 },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct AgentConn {
    server_id: String,
    connected_ms: i64,
    last_seen_ms: i64,
    /// Set once we have closed the socket and announced offline; the peer may not ack the close
    /// for a long time (half-open), so the socket can linger in `get_websockets`.
    #[serde(default)]
    closing: bool,
    #[serde(default)]
    latest: Option<MetricsFrame>,
    #[serde(default)]
    minute: Option<MinuteAcc>,
}

/// Accumulates frames of one server within one wall-clock minute.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct MinuteAcc {
    minute: i64,
    n: u32,
    cpu_sum: f64,
    cpu_max: f32,
    rx_sum: u64,
    rx_max: u64,
    tx_sum: u64,
    tx_max: u64,
}

impl MinuteAcc {
    fn new(minute: i64) -> Self {
        Self {
            minute,
            n: 0,
            cpu_sum: 0.0,
            cpu_max: 0.0,
            rx_sum: 0,
            rx_max: 0,
            tx_sum: 0,
            tx_max: 0,
        }
    }

    fn add(&mut self, f: &MetricsFrame) {
        self.n += 1;
        self.cpu_sum += f.cpu_pct as f64;
        self.cpu_max = self.cpu_max.max(f.cpu_pct);
        self.rx_sum += f.net_rx_bps;
        self.rx_max = self.rx_max.max(f.net_rx_bps);
        self.tx_sum += f.net_tx_bps;
        self.tx_max = self.tx_max.max(f.net_tx_bps);
    }
}

fn minute_of(ts_ms: i64) -> i64 {
    ts_ms / 60_000 * 60
}

fn pct(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        used as f64 * 100.0 / total as f64
    }
}

fn tag_for(server_id: &str) -> String {
    format!("server:{server_id}")
}

fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

fn agent_conn(ws: &WebSocket) -> Option<AgentConn> {
    match ws.deserialize_attachment().ok().flatten() {
        Some(Attachment::Agent(c)) => Some(c),
        _ => None,
    }
}

fn save(ws: &WebSocket, conn: AgentConn) -> Result<()> {
    ws.serialize_attachment(Attachment::Agent(conn))
}

#[derive(Serialize)]
struct DebugInfo {
    /// Changes every time the runtime constructs a new instance (cold start / after eviction).
    instance_started_ms: i64,
    constructs_total: i64,
    messages_this_instance: u64,
    agents: usize,
    dashboards: usize,
    db_size_bytes: usize,
}

#[durable_object]
pub struct Hub {
    state: State,
    env: Env,
    started_ms: i64,
    messages: Cell<u64>,
    last_cleanup_ms: Cell<i64>,
}

impl DurableObject for Hub {
    fn new(state: State, env: Env) -> Self {
        let schema = "
            CREATE TABLE IF NOT EXISTS servers (
              id TEXT PRIMARY KEY, created_ms INTEGER NOT NULL, last_seen_ms INTEGER, host_json TEXT
            );
            CREATE TABLE IF NOT EXISTS metrics_1m (
              server_id TEXT NOT NULL, ts INTEGER NOT NULL,
              cpu_avg REAL, cpu_max REAL, mem_pct REAL, disk_pct REAL,
              rx_bps_avg INTEGER, rx_bps_max INTEGER, tx_bps_avg INTEGER, tx_bps_max INTEGER,
              load1 REAL,
              PRIMARY KEY (server_id, ts)
            ) WITHOUT ROWID;
            CREATE TABLE IF NOT EXISTS poc_meta (k TEXT PRIMARY KEY, v INTEGER NOT NULL);
            INSERT INTO poc_meta (k, v) VALUES ('constructs', 1)
              ON CONFLICT(k) DO UPDATE SET v = v + 1;
        ";
        if let Err(e) = state.storage().sql().exec(schema, None) {
            console_error!("schema init failed: {e}");
        }
        Self {
            state,
            env,
            started_ms: now_ms(),
            messages: Cell::new(0),
            last_cleanup_ms: Cell::new(0),
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let path = req.path();
        match path.as_str() {
            "/ws/agent" => {
                let server_id = req.headers().get(H_SERVER_ID)?.ok_or("missing server id")?;
                self.accept_agent(server_id).await
            }
            "/ws/dashboard" => self.accept_dash(),
            "/api/servers" => Response::from_json(&self.snapshot()?),
            "/api/debug" => Response::from_json(&self.debug_info()?),
            p => match p
                .strip_prefix("/api/servers/")
                .and_then(|r| r.strip_suffix("/history"))
            {
                Some(id) => self.history(id),
                None => Response::error("not found", 404),
            },
        }
    }

    async fn websocket_message(
        &self,
        ws: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        self.messages.set(self.messages.get() + 1);
        // Dashboards are receive-only; frames on a socket we already closed are ignored.
        let Some(mut conn) = agent_conn(&ws).filter(|c| !c.closing) else {
            return Ok(());
        };
        let WebSocketIncomingMessage::String(text) = message else {
            return Ok(());
        };
        conn.last_seen_ms = now_ms();

        match serde_json::from_str::<AgentMessage>(&text) {
            Ok(AgentMessage::Hello { protocol, host, .. }) => {
                if protocol != PROTOCOL_VERSION {
                    ws.close(Some(CLOSE_INCOMPATIBLE), Some("incompatible protocol"))?;
                    return Ok(());
                }
                self.state.storage().sql().exec(
                    "UPDATE servers SET host_json = ? WHERE id = ?",
                    vec![
                        serde_json::to_string(&host)?.into(),
                        conn.server_id.as_str().into(),
                    ],
                )?;
            }
            Ok(AgentMessage::Metrics(frame)) => {
                let minute = minute_of(frame.ts_ms);
                if conn.minute.as_ref().is_some_and(|m| m.minute != minute) {
                    self.flush_minute(&mut conn)?;
                }
                conn.minute
                    .get_or_insert_with(|| MinuteAcc::new(minute))
                    .add(&frame);
                conn.latest = Some(frame.clone());
                self.broadcast(&ServerEvent::Metrics {
                    server_id: conn.server_id.clone(),
                    frame,
                });
            }
            Err(e) => console_warn!("bad agent message from {}: {e}", conn.server_id),
        }
        save(&ws, conn)
    }

    async fn websocket_close(
        &self,
        ws: WebSocket,
        _code: usize,
        _reason: String,
        _clean: bool,
    ) -> Result<()> {
        self.on_disconnect(&ws)
    }

    async fn websocket_error(&self, ws: WebSocket, _error: Error) -> Result<()> {
        self.on_disconnect(&ws)
    }

    async fn alarm(&self) -> Result<Response> {
        let now = now_ms();
        let interval_ms = self.var_i64("REPORT_INTERVAL_S", 5) * 1000;
        let current_minute = minute_of(now);

        for ws in self.state.get_websockets_with_tag(TAG_AGENT) {
            let Some(mut conn) = agent_conn(&ws).filter(|c| !c.closing) else {
                continue;
            };
            if now - conn.last_seen_ms > 3 * interval_ms {
                // Half-open connection: no frame for 3 intervals.
                self.on_disconnect(&ws)?;
                let _ = ws.close(Some(CLOSE_STALE), Some("stale"));
            } else if conn
                .minute
                .as_ref()
                .is_some_and(|m| m.minute < current_minute)
            {
                // Flush a finished minute even if the next frame is late.
                self.flush_minute(&mut conn)?;
                save(&ws, conn)?;
            }
        }

        if now - self.last_cleanup_ms.get() > 3_600_000 {
            self.cleanup(now)?;
        }
        if !self.state.get_websockets_with_tag(TAG_AGENT).is_empty() {
            self.schedule_alarm().await?;
        }
        Response::ok("ok")
    }
}

impl Hub {
    fn var_i64(&self, name: &str, default: i64) -> i64 {
        self.env
            .var(name)
            .ok()
            .and_then(|v| v.to_string().parse().ok())
            .unwrap_or(default)
    }

    /// NB: workers-rs treats a bare `i64` as an offset from now, unlike the JS API's epoch millis.
    async fn schedule_alarm(&self) -> Result<()> {
        self.state
            .storage()
            .set_alarm(Duration::from_millis(ALARM_PERIOD_MS as u64))
            .await
    }

    async fn accept_agent(&self, server_id: String) -> Result<Response> {
        let now = now_ms();
        let tag = tag_for(&server_id);
        // One live connection per server: the newest wins, carrying over the open minute.
        let mut carried_minute = None;
        for old in self.state.get_websockets_with_tag(&tag) {
            if let Some(mut c) = agent_conn(&old).filter(|c| !c.closing) {
                carried_minute = c.minute.take();
                c.closing = true;
                save(&old, c)?;
            }
            let _ = old.close(Some(CLOSE_REPLACED), Some("replaced by new connection"));
        }

        let pair = WebSocketPair::new()?;
        self.state
            .accept_websocket_with_tags(&pair.server, &[TAG_AGENT, &tag]);
        save(
            &pair.server,
            AgentConn {
                server_id: server_id.clone(),
                connected_ms: now,
                last_seen_ms: now,
                closing: false,
                latest: None,
                minute: carried_minute,
            },
        )?;

        self.state.storage().sql().exec(
            "INSERT INTO servers (id, created_ms, last_seen_ms) VALUES (?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET last_seen_ms = excluded.last_seen_ms",
            vec![server_id.as_str().into(), now.into(), now.into()],
        )?;
        self.broadcast(&ServerEvent::Online { server_id });

        // Also repair an alarm scheduled absurdly far ahead (e.g. by a past bug).
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
        pair.server.serialize_attachment(Attachment::Dash {
            connected_ms: now_ms(),
        })?;
        pair.server.send(&ServerEvent::Snapshot {
            servers: self.snapshot()?,
        })?;
        Response::from_websocket(pair.client)
    }

    /// Idempotent: a socket already marked `closing` has been handled.
    fn on_disconnect(&self, ws: &WebSocket) -> Result<()> {
        let Some(mut conn) = agent_conn(ws).filter(|c| !c.closing) else {
            return Ok(());
        };
        self.flush_minute(&mut conn)?;
        conn.closing = true;
        let (server_id, last_seen_ms) = (conn.server_id.clone(), conn.last_seen_ms);
        save(ws, conn)?;

        // A replacement connection may already be live for this server.
        let still_connected = self
            .state
            .get_websockets_with_tag(&tag_for(&server_id))
            .iter()
            .any(|other| agent_conn(other).is_some_and(|c| !c.closing));
        if still_connected {
            return Ok(());
        }
        self.state.storage().sql().exec(
            "UPDATE servers SET last_seen_ms = ? WHERE id = ?",
            vec![last_seen_ms.into(), server_id.as_str().into()],
        )?;
        self.broadcast(&ServerEvent::Offline {
            server_id,
            last_seen_ms,
        });
        Ok(())
    }

    fn broadcast(&self, event: &ServerEvent) {
        let Ok(json) = serde_json::to_string(event) else {
            return;
        };
        for ws in self.state.get_websockets_with_tag(TAG_DASH) {
            let _ = ws.send_with_str(&json);
        }
    }

    /// Writes the open minute (if any) as one `metrics_1m` row; the caller persists `conn`.
    fn flush_minute(&self, conn: &mut AgentConn) -> Result<()> {
        let (Some(acc), Some(l)) = (conn.minute.take(), conn.latest.as_ref()) else {
            return Ok(());
        };
        if acc.n == 0 {
            return Ok(());
        }
        let n = acc.n as u64;
        self.state.storage().sql().exec(
            "INSERT OR REPLACE INTO metrics_1m
               (server_id, ts, cpu_avg, cpu_max, mem_pct, disk_pct, rx_bps_avg, rx_bps_max, tx_bps_avg, tx_bps_max, load1)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![
                conn.server_id.as_str().into(),
                acc.minute.into(),
                (acc.cpu_sum / n as f64).into(),
                (acc.cpu_max as f64).into(),
                pct(l.mem_used, l.mem_total).into(),
                pct(l.disk_used, l.disk_total).into(),
                ((acc.rx_sum / n) as i64).into(),
                (acc.rx_max as i64).into(),
                ((acc.tx_sum / n) as i64).into(),
                (acc.tx_max as i64).into(),
                (l.load1 as f64).into(),
            ],
        )?;
        Ok(())
    }

    /// Range-delete per server so the PK index is used and rows read stay proportional to rows deleted.
    fn cleanup(&self, now: i64) -> Result<()> {
        let cutoff = (now - self.var_i64("RETENTION_DAYS", 7) * 86_400_000) / 1000;
        let sql = self.state.storage().sql();
        #[derive(Deserialize)]
        struct Id {
            id: String,
        }
        for Id { id } in sql.exec("SELECT id FROM servers", None)?.to_array::<Id>()? {
            sql.exec(
                "DELETE FROM metrics_1m WHERE server_id = ? AND ts < ?",
                vec![id.into(), cutoff.into()],
            )?;
        }
        self.last_cleanup_ms.set(now);
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<ServerStatus>> {
        #[derive(Deserialize)]
        struct Row {
            id: String,
            last_seen_ms: Option<i64>,
            host_json: Option<String>,
        }
        let live: Vec<AgentConn> = self
            .state
            .get_websockets_with_tag(TAG_AGENT)
            .iter()
            .filter_map(agent_conn)
            .filter(|c| !c.closing)
            .collect();
        let rows = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT id, last_seen_ms, host_json FROM servers ORDER BY id",
                None,
            )?
            .to_array::<Row>()?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let conn = live.iter().find(|c| c.server_id == r.id);
                ServerStatus {
                    online: conn.is_some(),
                    last_seen_ms: conn.map(|c| c.last_seen_ms).or(r.last_seen_ms),
                    host: r.host_json.and_then(|j| serde_json::from_str(&j).ok()),
                    metrics: conn.and_then(|c| c.latest.clone()),
                    server_id: r.id,
                }
            })
            .collect())
    }

    fn history(&self, server_id: &str) -> Result<Response> {
        #[derive(Serialize, Deserialize)]
        struct Point {
            ts: i64,
            cpu_avg: f64,
            cpu_max: f64,
            mem_pct: f64,
            disk_pct: f64,
            rx_bps_avg: i64,
            rx_bps_max: i64,
            tx_bps_avg: i64,
            tx_bps_max: i64,
            load1: f64,
        }
        let since = now_ms() / 1000 - 86_400;
        let points = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT ts, cpu_avg, cpu_max, mem_pct, disk_pct, rx_bps_avg, rx_bps_max, tx_bps_avg, tx_bps_max, load1
                 FROM metrics_1m WHERE server_id = ? AND ts >= ? ORDER BY ts",
                vec![server_id.into(), since.into()],
            )?
            .to_array::<Point>()?;
        Response::from_json(&points)
    }

    fn debug_info(&self) -> Result<DebugInfo> {
        #[derive(Deserialize)]
        struct V {
            v: i64,
        }
        let sql = self.state.storage().sql();
        let constructs = sql
            .exec("SELECT v FROM poc_meta WHERE k = 'constructs'", None)?
            .one::<V>()?
            .v;
        Ok(DebugInfo {
            instance_started_ms: self.started_ms,
            constructs_total: constructs,
            messages_this_instance: self.messages.get(),
            agents: self.state.get_websockets_with_tag(TAG_AGENT).len(),
            dashboards: self.state.get_websockets_with_tag(TAG_DASH).len(),
            db_size_bytes: sql.database_size(),
        })
    }
}
