mod hub;

use worker::*;

pub use hub::Hub;

pub(crate) const H_SERVER_ID: &str = "x-moni-server-id";

#[event(start)]
fn start() {
    console_error_panic_hook::set_once();
}

/// Constant-time-ish comparison; good enough for PoC shared secrets.
fn secret_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn bearer(req: &Request) -> Option<String> {
    let h = req.headers().get("authorization").ok().flatten()?;
    h.strip_prefix("Bearer ").map(str::to_owned)
}

fn query_token(req: &Request) -> Option<String> {
    req.url()
        .ok()?
        .query_pairs()
        .find(|(k, _)| k == "token")
        .map(|(_, v)| v.into_owned())
}

/// PoC agent token format: `<server_id>:<AGENT_SECRET>` (see ADR-0005).
fn auth_agent(req: &Request, env: &Env) -> Option<String> {
    let secret = env.secret("AGENT_SECRET").ok()?.to_string();
    let token = bearer(req)?;
    let (server_id, s) = token.split_once(':')?;
    let valid_id = !server_id.is_empty()
        && server_id.len() <= 64
        && server_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    (valid_id && secret_eq(s, &secret)).then(|| server_id.to_owned())
}

fn auth_admin(req: &Request, env: &Env) -> bool {
    let Ok(secret) = env.secret("ADMIN_TOKEN") else {
        return false;
    };
    let secret = secret.to_string();
    bearer(req)
        .or_else(|| query_token(req))
        .is_some_and(|t| secret_eq(&t, &secret))
}

async fn to_hub(req: Request, env: &Env, server_id: Option<&str>) -> Result<Response> {
    let mut req = req.clone_mut()?;
    // Never trust a client-supplied identity header.
    req.headers_mut()?.delete(H_SERVER_ID)?;
    if let Some(id) = server_id {
        req.headers_mut()?.set(H_SERVER_ID, id)?;
    }
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

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let path = req.path();
    match path.as_str() {
        "/api/health" => Response::ok("ok"),
        "/ws/agent" => {
            if !is_upgrade(&req) {
                return Response::error("expected websocket", 426);
            }
            match auth_agent(&req, &env) {
                Some(id) => to_hub(req, &env, Some(&id)).await,
                None => Response::error("unauthorized", 401),
            }
        }
        "/ws/dashboard" => {
            if !is_upgrade(&req) {
                return Response::error("expected websocket", 426);
            }
            if !auth_admin(&req, &env) {
                return Response::error("unauthorized", 401);
            }
            to_hub(req, &env, None).await
        }
        p if p.starts_with("/api/") => {
            if !auth_admin(&req, &env) {
                return Response::error("unauthorized", 401);
            }
            to_hub(req, &env, None).await
        }
        _ => env.assets("ASSETS")?.fetch_request(req).await,
    }
}
