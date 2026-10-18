use crate::{auth, error, logic};
use moni_protocol::api::{
    ApiErrorCode, AppConfig, Appearance, AuthMeResponse, Notifications, PublicInfo, Retention,
};
use worker::*;

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
    var_u32(env, "REPORT_INTERVAL_S", 2)
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

// Opt-in by config: a missing variable keeps the dashboard private.
fn public_dashboard(env: &Env) -> bool {
    env.var("PUBLIC_DASHBOARD")
        .is_ok_and(|v| v.to_string().eq_ignore_ascii_case("true"))
}

/// Anonymous history reads are served from the edge cache so visitors cannot drain the Hub's
/// request and row-read budget; a minute matches the rollup granularity.
async fn public_history(req: Request, env: &Env) -> Result<Response> {
    let url = req.url()?;
    let range = url
        .query_pairs()
        .find(|(key, _)| key == "range")
        .map(|(_, value)| value.into_owned());
    if logic::history_plan(range.as_deref()).is_none() {
        return error::bad_request("Unknown history range");
    }
    let key = format!(
        "{}{}?range={}",
        url.origin().ascii_serialization(),
        url.path(),
        range.as_deref().unwrap_or("24h")
    );
    let cache = Cache::default();
    if let Some(hit) = cache.get(key.as_str(), false).await? {
        return Ok(hit);
    }
    let mut upstream = to_hub(req, env).await?;
    if upstream.status_code() != 200 {
        return Ok(upstream);
    }
    let headers = Headers::new();
    headers.set("content-type", "application/json")?;
    headers.set("cache-control", "public, max-age=60")?;
    let mut response = Response::from_bytes(upstream.bytes().await?)?.with_headers(headers);
    if let Err(error) = cache.put(key.as_str(), response.cloned()?).await {
        console_warn!("Public history cache write failed: {}", error);
    }
    Ok(response)
}

fn public_cache_key(req: &Request) -> Result<String> {
    Ok(format!(
        "{}/api/public",
        req.url()?.origin().ascii_serialization()
    ))
}

/// Every page load asks for this, so it is edge-cached like public history; a stale minute after
/// an appearance change is fine, and the admin's own colo is purged on save.
async fn public_info(req: &Request, env: &Env) -> Result<Response> {
    let key = public_cache_key(req)?;
    let cache = Cache::default();
    if let Some(hit) = cache.get(key.as_str(), false).await? {
        return Ok(hit);
    }
    let url = format!(
        "{}/api/appearance",
        req.url()?.origin().ascii_serialization()
    );
    let appearance = match to_hub(Request::new(&url, Method::Get)?, env).await {
        Ok(mut res) if res.status_code() == 200 => res.json::<Appearance>().await.ok(),
        _ => None,
    };
    let cacheable = appearance.is_some();
    let mut response = Response::from_json(&PublicInfo {
        enabled: public_dashboard(env),
        appearance: appearance.unwrap_or_default(),
    })?;
    if cacheable {
        response
            .headers_mut()
            .set("cache-control", "public, max-age=60")?;
        if let Err(error) = cache.put(key.as_str(), response.cloned()?).await {
            console_warn!("Public info cache write failed: {}", error);
        }
    }
    Ok(response)
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
    if path == "/api/public" && req.method() == Method::Get {
        return public_info(&req, &env).await;
    }
    let Some(secrets) = auth::Secrets::load(&env) else {
        return error::error(
            503,
            ApiErrorCode::NotConfigured,
            "ADMIN_PASSWORD and AUTH_SECRET (at least 32 bytes) are required",
        );
    };
    let country = logic::country_code(req.cf().and_then(|cf| cf.country()).as_deref());
    let mut req = req.clone_mut()?;
    for internal in [
        auth::H_SERVER_ID,
        auth::H_TOKEN_HASH,
        auth::H_COUNTRY,
        auth::H_ADMIN,
    ] {
        req.headers_mut()?.delete(internal)?;
    }
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
        if let Some(country) = country {
            req.headers_mut()?.set(auth::H_COUNTRY, &country)?;
        }
        // The Hub needs only the keyed hash, never the original credential.
        req.headers_mut()?.delete("authorization")?;
        return to_hub(req, &env).await;
    }
    let login = path == "/api/auth/login" && req.method() == Method::Post;
    let bearer_admin = auth::bearer(&req).is_some_and(|v| auth::secret_eq(&v, &secrets.password));
    let admin = auth::is_admin(&req, &secrets);
    if !login && !admin {
        if public_dashboard(&env) && logic::public_route(req.method() == Method::Get, &path) {
            if path == "/ws/dashboard" {
                if !is_upgrade(&req) {
                    return error::bad_request("Expected WebSocket upgrade");
                }
                return to_hub(req, &env).await;
            }
            return public_history(req, &env).await;
        }
        return error::unauthorized();
    }
    let write = req.method() != Method::Get && path.starts_with("/api/");
    if !bearer_admin && (login || write || path == "/ws/dashboard") {
        let origin = req.headers().get("origin")?;
        let target = req.url()?.origin().ascii_serialization();
        if !logic::origin_allowed(origin.as_deref(), &target, !login) {
            return error::error(403, ApiErrorCode::Forbidden, "Same-origin request required");
        }
    }
    if write
        && logic::request_has_body(
            req.headers().get("content-length")?.as_deref(),
            req.headers().get("transfer-encoding")?.as_deref(),
        )
        && !logic::json_content_type(req.headers().get("content-type")?.as_deref())
    {
        return error::error(415, ApiErrorCode::BadRequest, "Expected application/json");
    }
    if login {
        return to_hub(req, &env).await;
    }
    match (req.method(), path.as_str()) {
        (Method::Post, "/api/auth/logout") => {
            auth::session_response(&req, &secrets.session_key, true)
        }
        (Method::Get, "/api/auth/me") => Response::from_json(&AuthMeResponse {
            authenticated: true,
        }),
        (Method::Get, "/api/config") => Response::from_json(&config(&env)),
        (Method::Get, "/ws/dashboard") if !is_upgrade(&req) => {
            error::bad_request("Expected WebSocket upgrade")
        }
        (Method::Get, "/ws/dashboard") => {
            req.headers_mut()?.set(auth::H_ADMIN, "1")?;
            to_hub(req, &env).await
        }
        (Method::Put, "/api/appearance") => {
            let key = public_cache_key(&req)?;
            let response = to_hub(req, &env).await?;
            if response.status_code() == 200
                && let Err(error) = Cache::default().delete(key.as_str(), false).await
            {
                console_warn!("Public info cache purge failed: {}", error);
            }
            Ok(response)
        }
        _ => to_hub(req, &env).await,
    }
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    error::finish(route(req, env).await)
}
