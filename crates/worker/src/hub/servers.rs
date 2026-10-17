use moni_protocol::api::{
    ApiErrorCode, CreateServerRequest, CreateServerResponse, DashboardEvent, HostInfo,
    ServerStatus, TokenResponse, UpdateServerRequest,
};
use serde::Deserialize;
use worker::*;

use super::{Hub, TAG_AGENT, agent_conn};
use crate::{auth, error, logic::slug};

#[derive(Deserialize)]
pub(super) struct ServerRow {
    pub id: String,
    pub name: String,
    pub created_ms: i64,
    pub last_seen_ms: Option<i64>,
    pub host_json: Option<String>,
    pub agent_version: Option<String>,
    pub token_hash: String,
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
}

impl Hub {
    pub(super) fn server_row(&self, id: &str) -> Result<Option<ServerRow>> {
        Ok(self
            .state
            .storage()
            .sql()
            .exec("SELECT * FROM servers WHERE id = ?", vec![id.into()])?
            .to_array::<ServerRow>()?
            .into_iter()
            .next())
    }

    fn status(&self, row: ServerRow) -> Result<ServerStatus> {
        let mut live = None;
        for ws in self.state.get_websockets_with_tag(&super::tag_for(&row.id)) {
            if let Some(mut conn) = agent_conn(&ws).filter(|c| !c.closing) {
                self.refresh_traffic(&ws, &mut conn)?;
                live = Some(conn);
                break;
            }
        }
        let traffic_month = live
            .as_ref()
            .and_then(|c| c.traffic_month.clone())
            .unwrap_or_else(|| self.safe_traffic_month(&row.id));
        let alerts_firing = self.firing_count(&row.id)?;
        Ok(ServerStatus {
            id: row.id,
            name: row.name,
            created_ms: row.created_ms,
            online: live.is_some(),
            last_seen_ms: live.as_ref().map(|c| c.last_seen_ms).or(row.last_seen_ms),
            agent_version: row.agent_version,
            host: row
                .host_json
                .and_then(|json| serde_json::from_str::<HostInfo>(&json).ok()),
            metrics: live.and_then(|c| c.latest),
            traffic_month: traffic_month.into(),
            alerts_firing,
        })
    }

    pub(super) fn snapshot(&self) -> Result<Vec<ServerStatus>> {
        self.state
            .storage()
            .sql()
            .exec("SELECT * FROM servers ORDER BY name, id", None)?
            .to_array::<ServerRow>()?
            .into_iter()
            .map(|row| self.status(row))
            .collect()
    }

    pub(super) fn upsert_event(&self, id: &str) -> Result<()> {
        if let Some(row) = self.server_row(id)? {
            self.broadcast(&DashboardEvent::ServerUpsert {
                server: self.status(row)?,
            });
        }
        Ok(())
    }

    pub(super) async fn create_server(&self, mut req: Request) -> Result<Response> {
        let Ok(input) = req.json::<CreateServerRequest>().await else {
            return error::bad_request("Expected a server name and optional id");
        };
        if !valid_name(&input.name) {
            return error::bad_request("Name must not be empty");
        }
        let id = if let Some(id) = input.id {
            if !auth::valid_id(&id) {
                return error::bad_request("Id must match [a-z0-9-]{1,32}");
            }
            if self.server_row(&id)?.is_some() {
                return error::error(409, ApiErrorCode::Conflict, "Server id already exists");
            }
            id
        } else {
            let base = slug(&input.name);
            let mut candidate = base.clone();
            let mut suffix = 2u64;
            while self.server_row(&candidate)?.is_some() {
                let tail = format!("-{suffix}");
                let prefix: String = base
                    .chars()
                    .take(32usize.saturating_sub(tail.len()))
                    .collect();
                candidate = format!("{}{tail}", prefix.trim_end_matches('-'));
                suffix = suffix.checked_add(1).ok_or("Server id space exhausted")?;
            }
            candidate
        };
        let secrets = auth::Secrets::load(&self.env).ok_or("Missing secrets")?;
        let token = auth::new_token(&id)?;
        self.state.storage().sql().exec(
            "INSERT INTO servers (id, name, token_hash, created_ms) VALUES (?, ?, ?, ?)",
            vec![
                id.as_str().into(),
                input.name.trim().into(),
                auth::token_hash(&secrets.key, &token)?.into(),
                auth::now_ms().into(),
            ],
        )?;
        if self.state.storage().get_alarm().await?.is_none() {
            self.schedule_alarm().await?;
        }
        let row = self.server_row(&id)?.ok_or("Created server is missing")?;
        let server = self.status(row)?;
        self.broadcast(&DashboardEvent::ServerUpsert {
            server: server.clone(),
        });
        Response::from_json(&CreateServerResponse { server, token }).map(|r| r.with_status(201))
    }

    pub(super) async fn update_server(&self, id: &str, mut req: Request) -> Result<Response> {
        let Ok(input) = req.json::<UpdateServerRequest>().await else {
            return error::bad_request("Expected an optional server name");
        };
        if self.server_row(id)?.is_none() {
            return error::not_found();
        }
        if let Some(name) = input.name {
            if !valid_name(&name) {
                return error::bad_request("Name must not be empty");
            }
            self.state.storage().sql().exec(
                "UPDATE servers SET name = ? WHERE id = ?",
                vec![name.trim().into(), id.into()],
            )?;
        }
        let row = self.server_row(id)?.ok_or("Updated server is missing")?;
        let server = self.status(row)?;
        self.broadcast(&DashboardEvent::ServerUpsert {
            server: server.clone(),
        });
        Response::from_json(&server)
    }

    pub(super) fn rotate_token(&self, id: &str) -> Result<Response> {
        if self.server_row(id)?.is_none() {
            return error::not_found();
        }
        let secrets = auth::Secrets::load(&self.env).ok_or("Missing secrets")?;
        let token = auth::new_token(id)?;
        self.state.storage().sql().exec(
            "UPDATE servers SET token_hash = ? WHERE id = ?",
            vec![auth::token_hash(&secrets.key, &token)?.into(), id.into()],
        )?;
        self.revoke_connections(id)?;
        Response::from_json(&TokenResponse { token })
    }

    pub(super) fn delete_server(&self, id: &str) -> Result<Response> {
        if self.server_row(id)?.is_none() {
            return error::not_found();
        }
        self.revoke_connections(id)?;
        let sql = self.state.storage().sql();
        for table in [
            "alert_rules",
            "alert_state",
            "alert_events",
            "notification_queue",
            "metrics_1m",
            "metrics_1h",
            "dirty_hours",
        ] {
            sql.exec(
                &format!("DELETE FROM {table} WHERE server_id = ?"),
                vec![id.into()],
            )?;
        }
        sql.exec("DELETE FROM servers WHERE id = ?", vec![id.into()])?;
        *self.rules_cache.borrow_mut() = None;
        self.broadcast(&DashboardEvent::ServerRemoved { id: id.into() });
        Response::empty().map(|r| r.with_status(204))
    }

    fn revoke_connections(&self, id: &str) -> Result<()> {
        for ws in self.state.get_websockets_with_tag(&super::tag_for(id)) {
            self.on_disconnect(&ws)?;
            let _ = ws.close(Some(4003), Some("token revoked"));
        }
        // A pending timer is retained for offline detection and future maintenance (B4/B5).
        Ok(())
    }

    pub(super) fn has_agents(&self) -> bool {
        self.state
            .get_websockets_with_tag(TAG_AGENT)
            .iter()
            .any(|ws| agent_conn(ws).is_some_and(|c| !c.closing))
    }
}
