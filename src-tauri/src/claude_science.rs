//! Claude Science as a client of the CC Switch gateway.
//!
//! Science has no provider file to write. Its daemon reads
//! `ANTHROPIC_BASE_URL`, `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` from
//! its own environment (measured in 0.1.63: the client is constructed with
//! `baseURL` from `process.env.ANTHROPIC_BASE_URL`, and the URL check allows
//! plain `http` only for literal loopback hosts — 127.0.0.1 / [::1]). That
//! makes it a natural client of the local gateway: point it at
//! `http://127.0.0.1:15723/claude-science`, hand it the gateway token, and
//! provider switching, failover and usage all happen in CC Switch as they do
//! for Claude Desktop.
//!
//! The process is the other half. Science runs a daemon on its data dir and
//! keeps the web UI attached to it, so a provider change is
//! `stop` → `serve --detached` with the new environment; there is no live
//! config reload to hook into. Restarting invalidates the UI's one-time
//! sign-in link, which is why this is an explicit action and not something
//! that happens per keystroke.
//!
//! This module is the foundation: environment construction, URL validation,
//! process control and the tests for the pure parts. Wiring it into
//! `AppType`, the proxy prefix and the provider form is the next slice; see
//! `docs/claude-science.md`.

use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use url::Url;

use crate::error::AppError;

/// App id used in the database and in the proxy path.
pub const APP_ID: &str = "claude-science";
/// Override for tests and non-default installs.
pub const EXE_ENV: &str = "CLAUDE_SCIENCE_EXE";
/// The proxy route this client talks to. Mirrors `/claude-desktop`.
pub const PROXY_PREFIX: &str = "/claude-science";
/// Science accepts code from the browser; the daemon and the UI live here.
pub const DEFAULT_PORT: u16 = 8000;

/// What the daemon has to be launched with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScienceGateway {
    pub base_url: String,
    pub api_key: Option<String>,
    pub auth_token: Option<String>,
}

impl ScienceGateway {
    /// The gateway route CC Switch serves for this app.
    pub fn via_gateway(proxy_base_url: &str, token: &str) -> Result<Self, AppError> {
        let base = normalise_base_url(proxy_base_url)?;
        Ok(Self {
            base_url: format!("{}{PROXY_PREFIX}", base.trim_end_matches('/')),
            api_key: None,
            auth_token: Some(token.to_string()),
        })
    }
}

/// A provider's `settings_config.env`, in the shape Claude providers use.
pub fn gateway_from_provider(settings_config: &Value) -> Result<ScienceGateway, AppError> {
    let env = settings_config
        .get("env")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AppError::InvalidInput(
                "Claude Science provider needs an env block with ANTHROPIC_BASE_URL".into(),
            )
        })?;

    let base = env
        .get("ANTHROPIC_BASE_URL")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let base_url = normalise_base_url(base)?;

    let api_key = non_empty(env.get("ANTHROPIC_API_KEY"));
    let auth_token = non_empty(env.get("ANTHROPIC_AUTH_TOKEN"));
    if api_key.is_none() && auth_token.is_none() {
        return Err(AppError::InvalidInput(
            "Claude Science provider needs ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN".into(),
        ));
    }

    Ok(ScienceGateway {
        base_url,
        api_key,
        auth_token,
    })
}

fn non_empty(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Science's own rule: https anywhere, plain http only on loopback.
///
/// The daemon refuses anything else at startup, so failing here is the only
/// way the UI can say why before the daemon dies with a console message.
pub fn normalise_base_url(raw: &str) -> Result<String, AppError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(AppError::InvalidInput(
            "ANTHROPIC_BASE_URL is empty".into(),
        ));
    }
    let url = Url::parse(raw)
        .map_err(|_| AppError::InvalidInput("ANTHROPIC_BASE_URL is not a valid URL".into()))?;

    match url.scheme() {
        "https" => {}
        "http" => {
            // `Url` normalises every textual loopback spelling (127.0.0.1,
            // [::1], 127.0.0.2) into an Ip address; asking the address is
            // both simpler and closer to Science's own check than matching
            // strings.
            let loopback = match url.host() {
                Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
                Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
                _ => false,
            };
            if !loopback {
                return Err(AppError::InvalidInput(
                    "Plain http is allowed only for loopback hosts (127.0.0.1 or [::1])".into(),
                ));
            }
        }
        other => {
            return Err(AppError::InvalidInput(format!(
                "ANTHROPIC_BASE_URL must be http(s), not {other}"
            )))
        }
    }

    // Science normalises the value to origin + pathname. A host-only URL
    // keeps the slash `Url` gives it; a path's trailing slash is dropped so
    // the stored value matches what the daemon reports back.
    let mut out = url.to_string();
    if url.path() != "/" {
        while out.ends_with('/') {
            out.pop();
        }
    }
    Ok(out)
}

/// Where the installed CLI/daemon lives.
pub fn exe_path() -> Option<PathBuf> {
    resolve_exe(
        std::env::var(EXE_ENV).ok().as_deref(),
        std::env::var("LOCALAPPDATA").ok().as_deref(),
    )
}

fn resolve_exe(override_path: Option<&str>, local_appdata: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = override_path.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(path));
    }
    local_appdata.map(|dir| {
        Path::new(dir)
            .join("Programs")
            .join("ClaudeScience")
            .join("claude-science.exe")
    })
}

/// Command lines for the two operations. Separate from execution so the
/// arguments are testable without a daemon.
pub fn stop_args() -> [&'static str; 1] {
    ["stop"]
}

pub fn serve_args() -> [&'static str; 4] {
    // Detached: the switch action returns to the UI. No browser: the user
    // already has a tab, and a new one would only show a stale sign-in.
    // No auto-update: an updater replacing the binary mid-switch is not a
    // thing the user asked for.
    ["serve", "--no-browser", "--detached", "--no-auto-update"]
}

/// `claude-science status` — always exits 0, prints JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct ScienceStatus {
    pub running: bool,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub version: Option<String>,
}

pub fn status() -> Result<ScienceStatus, AppError> {
    let exe = exe_path().ok_or_else(missing_exe)?;
    let out = Command::new(&exe)
        .arg("status")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| AppError::io(&exe, e))?;
    serde_json::from_slice(&out.stdout).map_err(|e| AppError::json(&exe, e))
}

/// Restart the daemon with this environment. The old one is stopped first:
/// two daemons on one data dir is exactly what Science's single-instance
/// lock refuses, and a failed stop must not leave the new one unattached.
pub fn apply(gateway: &ScienceGateway) -> Result<(), AppError> {
    let exe = exe_path().ok_or_else(missing_exe)?;

    // Not running is the normal case on the first switch; the exit status is
    // not worth surfacing.
    let _ = Command::new(&exe)
        .args(stop_args())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let mut cmd = Command::new(&exe);
    cmd.args(serve_args())
        .env("ANTHROPIC_BASE_URL", &gateway.base_url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(key) = &gateway.api_key {
        cmd.env("ANTHROPIC_API_KEY", key);
    }
    if let Some(token) = &gateway.auth_token {
        cmd.env("ANTHROPIC_AUTH_TOKEN", token);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let out = cmd.output().map_err(|e| AppError::io(&exe, e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(AppError::Message(format!(
            "claude-science serve failed ({}): {}",
            out.status,
            stderr.trim()
        )));
    }
    Ok(())
}

fn missing_exe() -> AppError {
    AppError::Message(
        "claude-science.exe was not found; install Claude Science or set CLAUDE_SCIENCE_EXE".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_https_with_an_auth_token() {
        let gateway = gateway_from_provider(&json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "sk-test"
            }
        }))
        .unwrap();
        assert_eq!(gateway.base_url, "https://api.deepseek.com/anthropic");
        assert_eq!(gateway.api_key, None);
        assert_eq!(gateway.auth_token.as_deref(), Some("sk-test"));
    }

    #[test]
    fn accepts_plain_http_on_loopback_only() {
        assert!(normalise_base_url("http://127.0.0.1:15723/claude-science").is_ok());
        assert!(normalise_base_url("http://[::1]:15723/claude-science").is_ok());
        assert!(normalise_base_url("http://192.168.1.10:15723").is_err());
        assert!(normalise_base_url("http://example.com").is_err());
    }

    #[test]
    fn trims_the_trailing_slash_science_drops_itself() {
        assert_eq!(
            normalise_base_url("https://api.deepseek.com/anthropic/").unwrap(),
            "https://api.deepseek.com/anthropic"
        );
        // The root keeps its slash: "https://host" and "https://host/" are
        // the same origin, and stripping past it would mangle IPv6/port
        // forms that Url already normalised.
        assert_eq!(normalise_base_url("https://example.com").unwrap(), "https://example.com/");
    }

    #[test]
    fn rejects_a_provider_without_a_url_or_key() {
        assert!(gateway_from_provider(&json!({ "env": {} })).is_err());
        assert!(gateway_from_provider(&json!({
            "env": { "ANTHROPIC_BASE_URL": "https://example.com" }
        }))
        .is_err());
        assert!(gateway_from_provider(&json!({
            "env": { "ANTHROPIC_BASE_URL": "https://example.com", "ANTHROPIC_API_KEY": "  " }
        }))
        .is_err());
    }

    #[test]
    fn gateway_route_appends_the_science_prefix() {
        let gateway = ScienceGateway::via_gateway("http://127.0.0.1:15723/", "gw-token").unwrap();
        assert_eq!(gateway.base_url, "http://127.0.0.1:15723/claude-science");
        assert_eq!(gateway.auth_token.as_deref(), Some("gw-token"));
        assert!(gateway.api_key.is_none());
    }

    #[test]
    fn exe_path_prefers_the_override_and_falls_back_to_local_appdata() {
        assert_eq!(
            resolve_exe(Some("C:/custom/claude-science.exe"), Some("C:/Users/x/AppData/Local")),
            Some(PathBuf::from("C:/custom/claude-science.exe"))
        );
        assert_eq!(
            resolve_exe(None, Some("C:/Users/x/AppData/Local")).unwrap(),
            PathBuf::from("C:/Users/x/AppData/Local/Programs/ClaudeScience/claude-science.exe")
        );
        assert_eq!(resolve_exe(Some("  "), None), None);
    }

    #[test]
    fn serve_is_detached_and_never_opens_a_browser_or_updates() {
        assert_eq!(
            serve_args(),
            ["serve", "--no-browser", "--detached", "--no-auto-update"]
        );
        assert_eq!(stop_args(), ["stop"]);
    }
}
