mod auth;
mod error;
mod hub;

use moni_protocol::api::{ApiErrorCode, AppConfig, AuthMeResponse, Notifications, Retention};
use worker::*;

pub use hub::Hub;

#[event(start)]
fn start() {
    console_error_panic_hook::set_once();
}

pub(crate) fn var_u32(env: &Env, name: &str, default: u32) -> u32 {
    env.var(name)
        .ok()
        .and_then(|v| v.to_string().parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

pub(crate) fn report_interval(env: &Env) -> u32 {
    var_u32(env, "REPORT_INTERVAL_S", 5)
}

async fn to_hub(req: Request, env: &Env) -> Result<Response> {
    env.durable_object("HUB")?
        .get_by_name("hub")?
        .fetch_with_request(req)
        .await
}

fn is_upgrade(req: &Request) -> bool {
    req.headers()
        .get("upgrade")
        .ok()
        .flatten()
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
}

fn config(env: &Env) -> AppConfig {
    AppConfig {
        version: env!("CARGO_PKG_VERSION").into(),
        report_interval_s: report_interval(env),
        agent_release_repo: env
            .var("AGENT_RELEASE_REPO")
            .ok()
            .map(|v| v.to_string())
            .filter(|v| !v.is_empty()),
        retention: Retention {
            minute_days: var_u32(env, "RETENTION_MINUTE_DAYS", 7),
            hour_days: var_u32(env, "RETENTION_HOUR_DAYS", 90),
        },
        notifications: Notifications {
            telegram: env.secret("TG_BOT_TOKEN").is_ok() && env.secret("TG_CHAT_ID").is_ok(),
            webhook: env.secret("WEBHOOK_URL").is_ok(),
        },
    }
}

async fn route(req: Request, env: Env) -> Result<Response> {
    let path = req.path();
    if !(path == "/api" || path.starts_with("/api/") || path == "/ws" || path.starts_with("/ws/")) {
        return env.assets("ASSETS")?.fetch_request(req).await;
    }
    if path == "/api/health" && req.method() == Method::Get {
        return Response::from_json(&"ok");
    }
    let Some(secrets) = auth::Secrets::load(&env) else {
        return error::error(
            503,
            ApiErrorCode::NotConfigured,
            "ADMIN_PASSWORD and AUTH_SECRET (at least 32 bytes) are required",
        );
    };
    let mut req = req.clone_mut()?;
    req.headers_mut()?.delete(auth::H_SERVER_ID)?;
    req.headers_mut()?.delete(auth::H_TOKEN_HASH)?;
    if path == "/ws/agent" && req.method() == Method::Get {
        let Some(token) = auth::bearer(&req) else {
            return error::unauthorized();
        };
        let Some(id) = auth::agent_id(&token) else {
            return error::unauthorized();
        };
        if !is_upgrade(&req) {
            return error::bad_request("Expected WebSocket upgrade");
        }
        req.headers_mut()?.set(auth::H_SERVER_ID, id)?;
        req.headers_mut()?
            .set(auth::H_TOKEN_HASH, &auth::token_hash(&secrets.key, &token)?)?;
        // The Hub needs only the keyed hash, never the original credential.
        req.headers_mut()?.delete("authorization")?;
        return to_hub(req, &env).await;
    }
    if path == "/api/auth/login" && req.method() == Method::Post {
        return to_hub(req, &env).await;
    }
    if !auth::is_admin(&req, &secrets) {
        return error::unauthorized();
    }
    match (req.method(), path.as_str()) {
        (Method::Post, "/api/auth/logout") => auth::session_response(&req, &secrets.key, true),
        (Method::Get, "/api/auth/me") => Response::from_json(&AuthMeResponse {
            authenticated: true,
        }),
        (Method::Get, "/api/config") => Response::from_json(&config(&env)),
        (Method::Get, "/ws/dashboard") if !is_upgrade(&req) => {
            error::bad_request("Expected WebSocket upgrade")
        }
        _ => to_hub(req, &env).await,
    }
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    error::finish(route(req, env).await)
}
