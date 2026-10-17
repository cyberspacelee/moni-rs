use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use worker::{Env, Request, Response, Result};

pub(crate) const H_SERVER_ID: &str = "x-moni-server-id";
pub(crate) const H_TOKEN_HASH: &str = "x-moni-token-hash";
const SESSION_MS: i64 = 30 * 86_400_000;

pub(crate) struct Secrets {
    pub password: String,
    pub key: String,
}

impl Secrets {
    pub fn load(env: &Env) -> Option<Self> {
        let password = env.secret("ADMIN_PASSWORD").ok()?.to_string();
        let key = env.secret("AUTH_SECRET").ok()?.to_string();
        (!password.is_empty() && key.len() >= 32).then_some(Self { password, key })
    }
}

pub(crate) fn now_ms() -> i64 {
    worker::Date::now().as_millis() as i64
}

pub(crate) fn secret_eq(a: &str, b: &str) -> bool {
    // Compare fixed-size digests so neither a matching prefix nor secret length shortens comparison.
    bool::from(Sha256::digest(a.as_bytes()).ct_eq(&Sha256::digest(b.as_bytes())))
}

fn sign(key: &str, value: &str) -> Result<Vec<u8>> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
        .map_err(|_| worker::Error::RustError("Invalid signing key".into()))?;
    mac.update(value.as_bytes());
    Ok(mac.finalize().into_bytes().to_vec())
}

pub(crate) fn token_hash(key: &str, token: &str) -> Result<String> {
    use std::fmt::Write;
    let mut hex = String::with_capacity(64);
    for byte in sign(key, token)? {
        write!(hex, "{byte:02x}")
            .map_err(|_| worker::Error::RustError("Cannot encode token hash".into()))?;
    }
    Ok(hex)
}

pub(crate) fn bearer(req: &Request) -> Option<String> {
    req.headers()
        .get("authorization")
        .ok()
        .flatten()?
        .strip_prefix("Bearer ")
        .map(str::to_owned)
}

pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub(crate) fn agent_id(token: &str) -> Option<&str> {
    let (id, secret) = token.strip_prefix("mn_")?.split_once('_')?;
    (valid_id(id)
        && secret.len() == 43
        && secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && URL_SAFE_NO_PAD.decode(secret).ok()?.len() == 32)
        .then_some(id)
}

pub(crate) fn new_token(id: &str) -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| worker::Error::RustError("Cannot generate token".into()))?;
    Ok(format!("mn_{id}_{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn valid_session(value: &str, key: &str, now: i64) -> bool {
    let Some((expiry, signature)) = value.split_once('.') else {
        return false;
    };
    let Ok(expiry_ms) = expiry.parse::<i64>() else {
        return false;
    };
    if expiry_ms <= now || expiry_ms > now.saturating_add(SESSION_MS) {
        return false;
    }
    let Ok(signature) = URL_SAFE_NO_PAD.decode(signature) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(key.as_bytes()) else {
        return false;
    };
    mac.update(format!("session:{expiry}").as_bytes());
    mac.verify_slice(&signature).is_ok()
}

pub(crate) fn is_admin(req: &Request, secrets: &Secrets) -> bool {
    if bearer(req).is_some_and(|value| secret_eq(&value, &secrets.password)) {
        return true;
    }
    req.headers()
        .get("cookie")
        .ok()
        .flatten()
        .is_some_and(|cookies| {
            cookies
                .split(';')
                .filter_map(|c| c.trim().split_once('='))
                .any(|(name, value)| {
                    name == "moni_session" && valid_session(value, &secrets.key, now_ms())
                })
        })
}

pub(crate) fn session_response(req: &Request, key: &str, clear: bool) -> Result<Response> {
    let value = if clear {
        String::new()
    } else {
        let expiry = now_ms() + SESSION_MS;
        let signature = URL_SAFE_NO_PAD.encode(sign(key, &format!("session:{expiry}"))?);
        format!("{expiry}.{signature}")
    };
    let secure = if req.url()?.scheme() == "https" {
        "; Secure"
    } else {
        ""
    };
    let max_age = if clear { 0 } else { SESSION_MS / 1000 };
    let mut response = Response::empty()?.with_status(204);
    response.headers_mut().set(
        "set-cookie",
        &format!(
            "moni_session={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{secure}"
        ),
    )?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_and_agent_tokens_reject_tampering() -> Result<()> {
        let key = "a signing key that is at least 32 bytes";
        let signature = URL_SAFE_NO_PAD.encode(sign(key, "session:2000")?);
        assert!(valid_session(&format!("2000.{signature}"), key, 1000));
        assert!(!valid_session(&format!("2001.{signature}"), key, 1000));
        assert!(!valid_session(&format!("2000.{signature}"), key, 2000));
        assert!(!valid_session(
            &format!("2000.{signature}"),
            "wrong key",
            1000
        ));
        assert!(secret_eq("password", "password"));
        assert!(!secret_eq("password", "password-extra"));
        let token = format!("mn_server-2_{}", URL_SAFE_NO_PAD.encode([7u8; 32]));
        assert_eq!(agent_id(&token), Some("server-2"));
        assert_eq!(agent_id("mn_server_invalid"), None);
        assert_eq!(agent_id(&token.replace("server-2", "BAD")), None);
        assert_eq!(
            token_hash("key", "The quick brown fox jumps over the lazy dog")?,
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
        Ok(())
    }
}
