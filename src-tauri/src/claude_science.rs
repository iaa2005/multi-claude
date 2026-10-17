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
/// Isolated data dir for a test instance (`claude-science --data-dir`).
pub const DATA_DIR_ENV: &str = "CLAUDE_SCIENCE_DATA_DIR";
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

/// Science's own data directory (`~/.claude-science`), also the natural
/// target for "open config folder" in the UI.
pub fn data_dir() -> PathBuf {
    crate::config::get_home_dir().join(".claude-science")
}

/// A scratch data dir for a test instance, set by the launcher.
fn configured_data_dir() -> Option<PathBuf> {
    std::env::var_os(DATA_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// `--data-dir` is a global flag on every Science command; passing it to
/// `stop` and `status` matters as much as to `serve`, otherwise they address
/// the real install instead of the test one.
fn data_dir_args(dir: Option<&Path>) -> Vec<String> {
    dir.map(|path| {
        vec![
            "--data-dir".to_string(),
            path.display().to_string(),
        ]
    })
    .unwrap_or_default()
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
    let out = hidden(&mut Command::new(&exe))
        .arg("status")
        .args(data_dir_args(configured_data_dir().as_deref()))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| AppError::io(&exe, e))?;
    serde_json::from_slice(&out.stdout).map_err(|e| AppError::json(&exe, e))
}

/// The seeded "Claude Science Official" card: no endpoint override, the
/// daemon runs on the claude.ai account it is signed in to.
pub fn is_official(provider: &crate::provider::Provider) -> bool {
    provider.id == crate::database::CLAUDE_SCIENCE_OFFICIAL_PROVIDER_ID
}

/// Check a provider before it is saved or switched to.
pub fn validate_provider(provider: &crate::provider::Provider) -> Result<(), AppError> {
    if is_official(provider) {
        return Ok(());
    }
    gateway_from_provider(&provider.settings_config).map(|_| ())
}

/// Direct mode: only the official card. Science signs every request with
/// its claude.ai token and ignores ANTHROPIC_API_KEY / ANTHROPIC_AUTH_TOKEN
/// ("API keys are not supported" in its client), so pointing it straight at
/// another vendor only earns a 401. Third-party providers go through the
/// gateway (Route mode), which swaps in the provider's key.
pub fn apply_provider(provider: &crate::provider::Provider) -> Result<(), AppError> {
    if is_official(provider) {
        return restart_official();
    }
    Err(AppError::Message(DIRECT_THIRD_PARTY.into()))
}

/// Leaving the gateway: a third-party direct pointer cannot be served
/// directly (see `apply_provider`), so Science goes back to claude.ai.
pub fn apply_direct_or_official(provider: &crate::provider::Provider) -> Result<(), AppError> {
    if !is_official(provider) {
        log::warn!(
            "[claude-science] direct provider {} cannot be used without the gateway; restarting on claude.ai",
            provider.id
        );
    }
    restart_official()
}

const DIRECT_THIRD_PARTY: &str = "Claude Science signs requests with your claude.ai sign-in and ignores API keys, so a third-party provider only works through the gateway: switch Claude Science to Route mode and pick this provider there.";

/// Restart the daemon with this environment.
pub fn apply(gateway: &ScienceGateway) -> Result<(), AppError> {
    apply_with_profile(gateway, "gateway")
}

/// Restart on the gateway, swapping in the model settings last used in
/// `profile` ("route", "stack"): each mode publishes different model ids.
pub fn apply_with_profile(gateway: &ScienceGateway, profile: &str) -> Result<(), AppError> {
    restart(Some(gateway), profile)
}

/// Restart with no override at all, which is how Science goes back to the
/// claude.ai account it is signed in to.
pub fn restart_official() -> Result<(), AppError> {
    restart(None, OFFICIAL_PROFILE)
}

/// The old daemon is stopped first: two of them on one data dir is exactly
/// what Science's single-instance lock refuses, and a failed stop must not
/// leave the new one unattached.
fn restart(gateway: Option<&ScienceGateway>, profile: &str) -> Result<(), AppError> {
    let exe = exe_path().ok_or_else(missing_exe)?;
    let data_dir = configured_data_dir();
    match gateway {
        Some(gateway) => log::info!(
            "[claude-science] restart: base={} api_key={} auth_token={} data_dir={}",
            gateway.base_url,
            gateway.api_key.is_some(),
            gateway.auth_token.is_some(),
            data_dir.as_ref().map_or("<default>".to_string(), |p| p.display().to_string())
        ),
        None => log::info!(
            "[claude-science] restart: official account, data_dir={}",
            data_dir.as_ref().map_or("<default>".to_string(), |p| p.display().to_string())
        ),
    }

    // Not running is the normal case on the first switch; the exit status is
    // not worth surfacing.
    if let Ok(status) = hidden(&mut Command::new(&exe))
        .args(stop_args())
        .args(data_dir_args(data_dir.as_deref()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        if !status.success() {
            log::warn!("[claude-science] stop exited with {status} (may not have been running)");
        }
    }

    // The daemon is down: its preference files can be swapped without it
    // writing them back. A failure here only costs the remembered models.
    let science_dir = data_dir.clone().unwrap_or_else(self::data_dir);
    if let Err(error) = swap_model_settings(&science_dir, &model_profiles_path(), profile) {
        log::warn!("[claude-science] could not swap model settings to {profile}: {error}");
    }

    let mut cmd = Command::new(&exe);
    hidden(&mut cmd)
        .args(serve_args())
        .args(data_dir_args(data_dir.as_deref()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // The parent (CC Switch) must not leak its own Anthropic settings
        // into a process that is supposed to be back on claude.ai.
        .env_remove("ANTHROPIC_BASE_URL")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN");
    for (key, value) in env_pairs(gateway) {
        cmd.env(key, value);
    }

    let out = cmd.output().map_err(|e| AppError::io(&exe, e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        log::error!(
            "[claude-science] serve failed ({}): {}",
            out.status,
            stderr.trim()
        );
        return Err(AppError::Message(format!(
            "claude-science serve failed ({}): {}",
            out.status,
            stderr.trim()
        )));
    }
    log::info!("[claude-science] daemon started");
    Ok(())
}

const OFFICIAL_PROFILE: &str = "official";

/// Where MultiClaude keeps each mode's Science model settings.
fn model_profiles_path() -> PathBuf {
    crate::config::get_app_config_dir().join("claude-science-models.json")
}

/// Science's per-org model settings that live on this machine (the default
/// model is kept on the account server and survives a gateway visit by
/// itself). Every `*Model` key counts, so a new one is covered too.
fn is_model_setting(key: &str) -> bool {
    key.ends_with("Model") || key.ends_with("Models")
}

/// Save the model settings of the mode that was running and put back the
/// ones last used in `profile`. Profiles are keyed by org directory, so two
/// signed-in orgs keep separate choices.
///
/// File shape: `{"active": "<profile>", "profiles": {"<profile>": {"<org>": {..}}}}`.
fn swap_model_settings(science_dir: &Path, store: &Path, profile: &str) -> Result<(), AppError> {
    let mut state: Value = std::fs::read(store)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    // Before MultiClaude ever switched, Science ran on its account.
    let previous = state
        .get("active")
        .and_then(Value::as_str)
        .unwrap_or(OFFICIAL_PROFILE)
        .to_string();

    let Ok(entries) = std::fs::read_dir(science_dir.join("orgs")) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let prefs_path = entry.path().join("preferences.json");
        let Ok(bytes) = std::fs::read(&prefs_path) else {
            continue;
        };
        let Ok(Value::Object(mut prefs)) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let org = entry.file_name().to_string_lossy().to_string();

        let saved: serde_json::Map<String, Value> = prefs
            .iter()
            .filter(|(key, _)| is_model_setting(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        state["profiles"][&previous][&org] = Value::Object(saved);

        if previous == profile {
            continue;
        }
        prefs.retain(|key, _| !is_model_setting(key));
        if let Some(Value::Object(restored)) =
            state["profiles"].get(profile).and_then(|p| p.get(&org))
        {
            for (key, value) in restored {
                prefs.insert(key.clone(), value.clone());
            }
        }
        let text = serde_json::to_vec_pretty(&Value::Object(prefs))
            .map_err(|e| AppError::json(&prefs_path, e))?;
        crate::config::atomic_write(&prefs_path, &text)?;
    }

    state["active"] = Value::String(profile.to_string());
    let text = serde_json::to_vec_pretty(&state).map_err(|e| AppError::json(store, e))?;
    crate::config::atomic_write(store, &text)
}

/// `claude-science.exe` is a console program. Spawned from the GUI without
/// this flag, every call opens a console window, and the status bar polls
/// `status` every five seconds.
fn hidden(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// The environment a restart contributes. Empty means "official account".
fn env_pairs(gateway: Option<&ScienceGateway>) -> Vec<(&'static str, String)> {
    let Some(gateway) = gateway else {
        return Vec::new();
    };
    let mut pairs = vec![("ANTHROPIC_BASE_URL", gateway.base_url.clone())];
    if let Some(key) = &gateway.api_key {
        pairs.push(("ANTHROPIC_API_KEY", key.clone()));
    }
    if let Some(token) = &gateway.auth_token {
        pairs.push(("ANTHROPIC_AUTH_TOKEN", token.clone()));
    }
    pairs
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

    fn write_prefs(dir: &Path, org: &str, prefs: serde_json::Value) {
        let org_dir = dir.join("orgs").join(org);
        std::fs::create_dir_all(&org_dir).unwrap();
        std::fs::write(org_dir.join("preferences.json"), prefs.to_string()).unwrap();
    }

    fn read_prefs(dir: &Path, org: &str) -> serde_json::Value {
        let bytes = std::fs::read(dir.join("orgs").join(org).join("preferences.json")).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn model_settings_come_back_when_science_returns_to_its_account() {
        let tmp = tempfile::tempdir().unwrap();
        let science = tmp.path().join("science");
        let store = tmp.path().join("models.json");
        write_prefs(
            &science,
            "org1",
            serde_json::json!({"reviewerModel": "claude-sonnet-5-5", "autoMode": false}),
        );

        // Into the gateway: the official choice is put aside, nothing to restore yet.
        swap_model_settings(&science, &store, "route").unwrap();
        let prefs = read_prefs(&science, "org1");
        assert!(prefs.get("reviewerModel").is_none());
        assert_eq!(prefs["autoMode"], false);

        // The user picks a gateway model while routed.
        write_prefs(
            &science,
            "org1",
            serde_json::json!({"reviewerModel": "claude-opus-5", "autoMode": false}),
        );

        // Back to the account: the official choice returns.
        swap_model_settings(&science, &store, OFFICIAL_PROFILE).unwrap();
        assert_eq!(read_prefs(&science, "org1")["reviewerModel"], "claude-sonnet-5-5");

        // And the routed one waits for the next visit.
        swap_model_settings(&science, &store, "route").unwrap();
        assert_eq!(read_prefs(&science, "org1")["reviewerModel"], "claude-opus-5");
    }

    #[test]
    fn restarting_in_the_same_mode_keeps_the_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let science = tmp.path().join("science");
        let store = tmp.path().join("models.json");
        write_prefs(&science, "org1", serde_json::json!({"reviewerModel": "claude-sonnet-5-5"}));
        swap_model_settings(&science, &store, OFFICIAL_PROFILE).unwrap();
        assert_eq!(read_prefs(&science, "org1")["reviewerModel"], "claude-sonnet-5-5");
    }

    #[test]
    fn serve_is_detached_and_never_opens_a_browser_or_updates() {
        assert_eq!(
            serve_args(),
            ["serve", "--no-browser", "--detached", "--no-auto-update"]
        );
        assert_eq!(stop_args(), ["stop"]);
    }

    #[test]
    fn data_dir_flag_reaches_every_command() {
        assert!(data_dir_args(None).is_empty());
        assert_eq!(
            data_dir_args(Some(Path::new("C:/tmp/science-test"))),
            vec!["--data-dir", "C:/tmp/science-test"]
        );
    }

    #[test]
    fn official_restart_contributes_no_environment() {
        assert!(env_pairs(None).is_empty());
    }

    #[test]
    fn gateway_restart_carries_url_and_each_credential_present() {
        let gateway = ScienceGateway {
            base_url: "https://example.com/anthropic".into(),
            api_key: Some("key".into()),
            auth_token: Some("token".into()),
        };
        assert_eq!(
            env_pairs(Some(&gateway)),
            vec![
                ("ANTHROPIC_BASE_URL", "https://example.com/anthropic".into()),
                ("ANTHROPIC_API_KEY", "key".into()),
                ("ANTHROPIC_AUTH_TOKEN", "token".into()),
            ]
        );
    }
}
