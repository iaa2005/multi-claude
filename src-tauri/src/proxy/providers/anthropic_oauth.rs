//! Anthropic OAuth (claude.ai account sign-in)
//!
//! Claude Code / Claude Desktop sign in without an API key: an OAuth exchange
//! writes `~/.claude/.credentials.json` with an access token whose scope includes
//! `user:inference`. That is exactly what `POST /v1/messages` on api.anthropic.com
//! needs, so a provider can serve the real Claude models over the same 1P
//! connection the user already has.
//!
//! Auth headers differ from an API key: OAuth goes on `Authorization: Bearer`
//! together with `anthropic-beta: oauth-2025-04-20`, never on `x-api-key`.
//!
//! ## Token lifetime
//! Claude Code refreshes this file on its own while it runs. When the gateway
//! runs standalone nothing else refreshes it, so this module also speaks the
//! refresh_token grant. The endpoint is fixed; the client id must match the one
//! that minted the token — set `ANTHROPIC_OAUTH_CLIENT_ID` if yours differs from
//! [`DEFAULT_CLIENT_ID`], and check the log line on a refresh failure.

use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use crate::proxy::error::ProxyError;

/// Token endpoint for the Anthropic OAuth refresh_token grant.
const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
/// OAuth client id that minted the token. Overridable because a mismatch here is
/// the one thing that makes refresh fail while the stored token still works.
const DEFAULT_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
/// Re-read the credentials file at most this often; Claude Code rewrites it on
/// refresh, so we must not hold a stale token for long.
const RELOAD_INTERVAL: Duration = Duration::from_secs(30);
/// Refresh this long before the token actually expires.
const EXPIRY_SKEW_MS: u64 = 60_000;

#[derive(Debug, Clone, Deserialize)]
struct StoredOauth {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<u64>,
}

#[derive(Debug, Clone)]
struct Cached {
    token: String,
    loaded_at: SystemTime,
    expires_at_ms: Option<u64>,
}

static CACHE: RwLock<Option<Cached>> = RwLock::new(None);

/// `~/.claude/.credentials.json` — the file Claude Code writes on sign-in.
pub fn credentials_path() -> Result<PathBuf, ProxyError> {
    let home = crate::config::get_home_dir();
    Ok(home.join(".claude").join(".credentials.json"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn client_id() -> String {
    std::env::var("ANTHROPIC_OAUTH_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string())
}

/// Read `claudeAiOauth` out of the credentials file.
fn read_stored() -> Result<StoredOauth, ProxyError> {
    let path = credentials_path()?;
    let raw = std::fs::read_to_string(&path).map_err(|error| {
        ProxyError::AuthError(format!(
            "Anthropic OAuth: cannot read {}: {error} — sign in to Claude Code once",
            path.display()
        ))
    })?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|error| ProxyError::AuthError(format!("Anthropic OAuth: bad credentials JSON: {error}")))?;
    let oauth = value.get("claudeAiOauth").cloned().unwrap_or(Value::Null);
    let mut stored: StoredOauth = serde_json::from_value(oauth).map_err(|_| {
        ProxyError::AuthError(
            "Anthropic OAuth: claudeAiOauth is missing accessToken — sign in to Claude Code once"
                .to_string(),
        )
    })?;
    stored.access_token = stored.access_token.trim().to_string();
    if stored.access_token.is_empty() {
        return Err(ProxyError::AuthError(
            "Anthropic OAuth: accessToken is empty — sign in to Claude Code once".to_string(),
        ));
    }
    Ok(stored)
}

/// A token that is still usable (with skew).
fn fresh(cached: &Cached) -> bool {
    let Some(expires_at_ms) = cached.expires_at_ms else {
        // Unknown expiry: use it, and re-read on the reload interval.
        return true;
    };
    now_ms() + EXPIRY_SKEW_MS < expires_at_ms
}

/// Access token for `Authorization: Bearer`. Reads the credentials file (bounded
/// by [`RELOAD_INTERVAL`]), refreshing through the token endpoint when the
/// stored one has expired.
pub async fn access_token() -> Result<String, ProxyError> {
    {
        let guard = CACHE
            .read()
            .map_err(|_| ProxyError::AuthError("Anthropic OAuth: cache poisoned".to_string()))?;
        if let Some(cached) = guard.as_ref() {
            if fresh(cached) && cached.loaded_at.elapsed().unwrap_or(RELOAD_INTERVAL) < RELOAD_INTERVAL
            {
                return Ok(cached.token.clone());
            }
        }
    }

    let stored = read_stored()?;
    let expires_at_ms = stored.expires_at;
    let expired = expires_at_ms
        .map(|at| now_ms() + EXPIRY_SKEW_MS >= at)
        .unwrap_or(false);

    let token = if expired {
        match refresh(&stored).await {
            Ok(token) => token,
            Err(error) => {
                // Fall back to the stored token: it may still be accepted, and
                // Claude Code will rewrite the file with a fresh one.
                log::warn!("[AnthropicOAuth] refresh failed, using stored token: {error}");
                stored.access_token.clone()
            }
        }
    } else {
        stored.access_token.clone()
    };

    *CACHE.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Cached {
        token: token.clone(),
        loaded_at: SystemTime::now(),
        expires_at_ms,
    });
    Ok(token)
}

/// refresh_token grant against [`TOKEN_URL`].
async fn refresh(stored: &StoredOauth) -> Result<String, ProxyError> {
    let refresh_token = stored.refresh_token.as_deref().filter(|t| !t.trim().is_empty()).ok_or_else(|| {
        ProxyError::AuthError(
            "Anthropic OAuth: token expired and no refreshToken is stored — sign in to Claude Code again"
                .to_string(),
        )
    })?;

    let response = reqwest::Client::new()
        .post(TOKEN_URL)
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": client_id(),
        }))
        .send()
        .await
        .map_err(|error| ProxyError::AuthError(format!("Anthropic OAuth: refresh request failed: {error}")))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_default();
    if !status.is_success() {
        return Err(ProxyError::AuthError(format!(
            "Anthropic OAuth: refresh got {status}: {body}"
        )));
    }

    let value: Value = serde_json::from_str(&body).map_err(|error| {
        ProxyError::AuthError(format!("Anthropic OAuth: refresh returned bad JSON: {error}"))
    })?;
    let token = value
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            ProxyError::AuthError("Anthropic OAuth: refresh response has no access_token".to_string())
        })?;
    Ok(token)
}
