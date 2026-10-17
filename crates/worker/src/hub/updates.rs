use futures_util::StreamExt;
use moni_protocol::{
    HubMessage, UpdateError, UpdateState,
    api::{AgentUpdateRequest, AgentUpdateStatus, ApiErrorCode},
    release,
};
use worker::*;

use super::{AgentConn, Hub, TAG_AGENT, agent_conn, save};
use crate::{auth, error};

const QUEUED_TTL_MS: i64 = 86_400_000;
const ACCEPTED_TTL_MS: i64 = 600_000;
const RETRY_MS: i64 = 60_000;

fn state_name(state: UpdateState) -> &'static str {
    match state {
        UpdateState::Queued => "queued",
        UpdateState::Accepted => "accepted",
        UpdateState::Succeeded => "succeeded",
        UpdateState::Failed => "failed",
        UpdateState::Unsupported => "unsupported",
    }
}

fn active(state: UpdateState) -> bool {
    matches!(state, UpdateState::Queued | UpdateState::Accepted)
}

impl Hub {
    pub(super) fn agent_updates(&self) -> Result<Vec<AgentUpdateStatus>> {
        self.state
            .storage()
            .sql()
            .exec(
                "SELECT * FROM agent_updates ORDER BY requested_ms DESC, server_id",
                None,
            )?
            .to_array()
    }

    fn agent_update(&self, id: &str) -> Result<Option<AgentUpdateStatus>> {
        Ok(self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT * FROM agent_updates WHERE server_id = ?",
                vec![id.into()],
            )?
            .to_array::<AgentUpdateStatus>()?
            .into_iter()
            .next())
    }

    pub(super) async fn request_updates(&self, mut req: Request) -> Result<Response> {
        let Some(repo) = self
            .env
            .var("AGENT_RELEASE_REPO")
            .ok()
            .map(|value| value.to_string())
            .filter(|value| release::valid_repo(value))
        else {
            return error::error(
                503,
                ApiErrorCode::NotConfigured,
                "A trusted AGENT_RELEASE_REPO is required",
            );
        };
        let Ok(mut stream) = req.stream() else {
            return error::bad_request("Expected a JSON update request");
        };
        let mut body = Vec::new();
        while let Some(chunk) = stream.next().await {
            let Ok(chunk) = chunk else {
                return error::bad_request("Cannot read update request");
            };
            if body.len().saturating_add(chunk.len()) > 16_384 {
                return error::error(
                    413,
                    ApiErrorCode::BadRequest,
                    "Update request exceeds 16 KiB",
                );
            }
            body.extend_from_slice(&chunk);
        }
        // workers-rs JSON deserialization discards unknown fields before serde sees them.
        let Ok(input) = serde_json::from_slice::<AgentUpdateRequest>(&body) else {
            return error::bad_request("Expected a version and optional server_ids");
        };
        if !input.version.starts_with('v') || release::version(&input.version).is_none() {
            return error::bad_request("Expected a stable vX.Y.Z release version");
        }
        let ids = match input.server_ids {
            Some(ids) if !ids.is_empty() && ids.len() <= 100 => ids,
            Some(_) => return error::bad_request("server_ids must contain 1..100 servers"),
            None => {
                #[derive(serde::Deserialize)]
                struct Id {
                    id: String,
                }
                let rows = self
                    .state
                    .storage()
                    .sql()
                    .exec("SELECT id FROM servers ORDER BY id LIMIT 101", None)?
                    .to_array::<Id>()?;
                if rows.len() > 100 {
                    return error::bad_request("Select at most 100 servers");
                }
                rows.into_iter().map(|row| row.id).collect()
            }
        };
        let mut servers = Vec::with_capacity(ids.len());
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            if !auth::valid_id(&id) || !seen.insert(id.clone()) {
                return error::bad_request("Invalid or duplicate server ID");
            }
            let Some(server) = self.server_row(&id)? else {
                return error::not_found();
            };
            if let Some(job) = self.agent_update(&id)?
                && active(job.state)
                && (job.version != input.version || job.repo != repo)
            {
                return error::error(
                    409,
                    ApiErrorCode::Conflict,
                    "A server already has an active update",
                );
            }
            servers.push(server);
        }
        let now = auth::now_ms();
        for server in &servers {
            if self.agent_update(&server.id)?.is_some_and(|job| {
                job.repo == repo && job.version == input.version && active(job.state)
            }) {
                continue;
            }
            let mut random = [0_u8; 16];
            getrandom::fill(&mut random)
                .map_err(|_| Error::RustError("Cannot generate update ID".into()))?;
            let job_id: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
            let current = server.agent_version.as_deref().and_then(release::version);
            let target = release::version(&input.version);
            let (state, failure) = if current.is_some() && current == target {
                (UpdateState::Succeeded, None)
            } else if current > target {
                (UpdateState::Failed, Some(UpdateError::Downgrade))
            } else {
                (UpdateState::Queued, None)
            };
            self.state.storage().sql().exec(
                "INSERT INTO agent_updates (server_id,job_id,repo,version,state,error,requested_ms,updated_ms)
                 VALUES (?,?,?,?,?,?,?,?) ON CONFLICT(server_id) DO UPDATE SET
                 job_id=excluded.job_id,repo=excluded.repo,version=excluded.version,state=excluded.state,
                 error=excluded.error,requested_ms=excluded.requested_ms,updated_ms=excluded.updated_ms",
                vec![server.id.as_str().into(),job_id.into(),repo.as_str().into(),input.version.as_str().into(),
                    state_name(state).into(),error_name(failure)?.into(),now.into(),now.into()],
            )?;
        }
        for ws in self.state.get_websockets_with_tag(TAG_AGENT) {
            let Some(mut conn) =
                agent_conn(&ws).filter(|conn| !conn.closing && conn.hello_received)
            else {
                continue;
            };
            if seen.contains(&conn.server_id) {
                self.deliver_update(&ws, &mut conn)?;
                save(&ws, conn)?;
            }
        }
        // Offline jobs need expiry even if the Hub currently has no Agent alarm scheduled.
        self.heal_alarm(now).await;
        let jobs: Vec<_> = self
            .agent_updates()?
            .into_iter()
            .filter(|job| seen.contains(&job.server_id))
            .collect();
        Response::from_json(&jobs).map(|response| response.with_status(202))
    }

    fn set_update_state(
        &self,
        job: &AgentUpdateStatus,
        state: UpdateState,
        failure: Option<UpdateError>,
    ) -> Result<()> {
        let failure = error_name(failure)?;
        self.state.storage().sql().exec(
            "UPDATE agent_updates SET state=?,error=?,updated_ms=? WHERE server_id=? AND job_id=?
             AND state IN ('queued','accepted') AND (state != ? OR error IS NOT ?)",
            vec![
                state_name(state).into(),
                failure.clone().into(),
                auth::now_ms().into(),
                job.server_id.as_str().into(),
                job.job_id.as_str().into(),
                state_name(state).into(),
                failure.into(),
            ],
        )?;
        Ok(())
    }

    pub(super) fn confirm_update(&self, server_id: &str, agent_version: &str) -> Result<()> {
        let Some(job) = self.agent_update(server_id)? else {
            return Ok(());
        };
        let Some(current) = release::version(agent_version) else {
            return Ok(());
        };
        let target = release::version(&job.version);
        if active(job.state) && Some(current) == target {
            self.set_update_state(&job, UpdateState::Succeeded, None)?;
        } else if job.state == UpdateState::Succeeded && Some(current) < target {
            // A replacement can connect successfully and then fail the installer's health check.
            self.state.storage().sql().exec(
                "UPDATE agent_updates SET state='failed',error='request_failed',updated_ms=?
                 WHERE server_id=? AND job_id=? AND state='succeeded'",
                vec![auth::now_ms().into(), server_id.into(), job.job_id.into()],
            )?;
        }
        Ok(())
    }

    pub(super) fn deliver_update(&self, ws: &WebSocket, conn: &mut AgentConn) -> Result<()> {
        let Some(job) = self
            .agent_update(&conn.server_id)?
            .filter(|job| job.state == UpdateState::Queued)
        else {
            return Ok(());
        };
        let Some(repo) = &conn.update_repo else {
            return self.set_update_state(
                &job,
                UpdateState::Unsupported,
                Some(UpdateError::Unsupported),
            );
        };
        if repo != &job.repo {
            return self.set_update_state(
                &job,
                UpdateState::Failed,
                Some(UpdateError::RepoMismatch),
            );
        }
        let now = auth::now_ms();
        if conn.update_sent_job_id.as_deref() == Some(&job.job_id) {
            if conn
                .update_sent_ms
                .is_some_and(|last| now.saturating_sub(last) < RETRY_MS)
            {
                return Ok(());
            }
            if conn.update_attempts >= 3 {
                return self.set_update_state(
                    &job,
                    UpdateState::Failed,
                    Some(UpdateError::Timeout),
                );
            }
        } else {
            conn.update_attempts = 0;
        }
        ws.send(&HubMessage::Update {
            job_id: job.job_id.clone(),
            version: job.version,
            repo: job.repo,
        })?;
        conn.update_sent_job_id = Some(job.job_id);
        conn.update_sent_ms = Some(now);
        conn.update_attempts = conn.update_attempts.saturating_add(1);
        Ok(())
    }

    pub(super) fn receive_update_status(
        &self,
        conn: &AgentConn,
        job_id: &str,
        version: &str,
        state: UpdateState,
        failure: Option<UpdateError>,
    ) -> Result<()> {
        if !conn.hello_received
            || conn.update_sent_job_id.as_deref() != Some(job_id)
            || !matches!(state, UpdateState::Accepted | UpdateState::Failed)
        {
            return Ok(());
        }
        let Some(job) = self.agent_update(&conn.server_id)? else {
            return Ok(());
        };
        if job.job_id != job_id
            || job.version != version
            || conn.update_repo.as_deref() != Some(job.repo.as_str())
        {
            return Ok(());
        }
        let failure = if state == UpdateState::Failed {
            Some(failure.unwrap_or(UpdateError::RequestFailed))
        } else {
            None
        };
        self.set_update_state(&job, state, failure)
    }

    pub(super) fn maintain_updates(&self, now: i64) -> Result<()> {
        for job in self
            .agent_updates()?
            .into_iter()
            .filter(|job| active(job.state))
        {
            let expired = match job.state {
                UpdateState::Queued => now.saturating_sub(job.requested_ms) > QUEUED_TTL_MS,
                UpdateState::Accepted => now.saturating_sub(job.updated_ms) > ACCEPTED_TTL_MS,
                _ => false,
            };
            if expired {
                self.set_update_state(&job, UpdateState::Failed, Some(UpdateError::Timeout))?;
            }
        }
        for ws in self.state.get_websockets_with_tag(TAG_AGENT) {
            let Some(mut conn) =
                agent_conn(&ws).filter(|conn| !conn.closing && conn.hello_received)
            else {
                continue;
            };
            self.deliver_update(&ws, &mut conn)?;
            save(&ws, conn)?;
        }
        Ok(())
    }
}

fn error_name(failure: Option<UpdateError>) -> Result<Option<String>> {
    Ok(failure
        .map(serde_json::to_value)
        .transpose()?
        .and_then(|value| value.as_str().map(str::to_owned)))
}
