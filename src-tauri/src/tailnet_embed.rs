// Embedded tailnet node (tailscale-rs) — desktop-only, EXPERIMENTAL.
//
// MERGE NOTE: desktop-gated module (`mod` is cfg'd out on mobile), calls
// existing code without changing it. Depends on the `tailscale` crate, which
// is itself desktop-only in Cargo.toml.
//
// What this is: MonoCode joins the official tailnet as its own "monocode"
// device and serves the companion WebSocket protocol directly on its tailnet
// IP — no Tailscale CLI, daemon, or `tailscale serve` needed on the Mac.
// Google SSO happens through the interactive login URL the node returns
// (open it in a browser, log in with Google), or via a pasted auth key.
//
// Hard limits (from Tailscale's own docs — not our choice):
// - tailscale-rs is pre-1.0, unaudited, DERP-only: treat this transport as
//   experimental. Frame auth (pairing token) still applies, but there is no
//   TLS on this path yet. System `tailscale serve` is opt-in; this node
//   is the default Google-login path.
// - iOS is unsupported by this crate. The iPad uses userspace tsnet
//   (`tsnet_mobile.rs`) with the same Google SSO flow, no Tailscale app.
// - No MagicDNS in the crate: the iPad pairs with the node's tailnet IP
//   (stable per node, shown in Settings).

use tauri::{AppHandle, Manager};

use crate::remote::{EmbedSnapshot, RemoteState};

const CONFIG_FILE: &str = "tailnet.json";
const KEY_FILE: &str = "tailnet_keys.json";
const CLIENT_NAME: &str = "monocode";

/// Required by the crate until its third-party audit lands. Set
/// programmatically so the desktop app, not the user's shell, owns it.
const EXPERIMENT_ENV: &str = "TS_RS_EXPERIMENT";
const EXPERIMENT_VALUE: &str = "this_is_unstable_software";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct EmbedConfig {
    #[serde(default)]
    auth_key: Option<String>,
    /// Set on start, cleared on stop: boot restores a wanted node.
    #[serde(default)]
    wanted: bool,
    /// Advertised ACL tags, e.g. ["tag:monocode"]. Empty = untagged node.
    #[serde(default)]
    tags: Vec<String>,
}

/// Tag syntax enforced client-side so control-plane rejections (and the
/// admin approval they trigger) never come as a surprise.
pub fn validate_tag(tag: &str) -> Result<(), String> {
    let trimmed = tag.trim();
    if !trimmed.starts_with("tag:") {
        return Err(format!("Tag must look like tag:name, got {trimmed:?}"));
    }
    let name = &trimmed["tag:".len()..];
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(format!(
            "Tag names use lowercase letters, digits, and dashes: {trimmed:?}"
        ));
    }
    Ok(())
}

fn config_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("companion");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(CONFIG_FILE))
}

fn key_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("companion");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(KEY_FILE))
}

fn load_config(app: &AppHandle) -> EmbedConfig {
    let Ok(path) = config_path(app) else {
        return EmbedConfig::default();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return EmbedConfig::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}
fn save_config(app: &AppHandle, config: &EmbedConfig) -> Result<(), String> {
    let path = config_path(app)?;
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, raw).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Flip the boot-restore flag without touching keys or tags. Used by link
/// disable so a stopped link stays stopped across restarts.
pub(crate) fn set_wanted(app: &AppHandle, wanted: bool) {
    let mut saved = load_config(app);
    saved.wanted = wanted;
    let _ = save_config(app, &saved);
}

fn secret_path(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Start (or restart) the embedded node. `None` keeps the saved auth key.
/// Always joins the official tailnet (login.tailscale.com).
pub async fn start(
    app: AppHandle,
    auth_key: Option<String>,
    tags: Vec<String>,
) -> Result<EmbedSnapshot, String> {
    let mut saved = load_config(&app);
    if auth_key.is_some() {
        saved.auth_key = auth_key.filter(|key| !key.trim().is_empty());
    }
    let mut seen = std::collections::HashSet::new();
    let mut clean_tags = Vec::new();
    for tag in &tags {
        validate_tag(tag)?;
        let normalized = tag.trim().to_string();
        if seen.insert(normalized.clone()) {
            clean_tags.push(normalized);
        }
    }
    saved.tags = clean_tags;
    saved.wanted = true;
    save_config(&app, &saved)?;

    let state: tauri::State<'_, RemoteState> = app.state();
    if let Some(previous) = state.embed_take_task() {
        previous.abort();
    }
    state.embed_update_snapshot(EmbedSnapshot {
        running: true,
        ..EmbedSnapshot::default()
    });
    std::env::set_var(EXPERIMENT_ENV, EXPERIMENT_VALUE);
    let task = tauri::async_runtime::spawn(run(app.clone()));
    state.embed_replace_task(task);
    Ok(state.embed_snapshot())
}

/// Stop the embedded node. The tailnet forgets nothing; restarting reuses
/// the saved keys and rejoins as the same device. Note: aborting drops the
/// Device without a graceful shutdown, so the experimental runtime may log
/// teardown panics in background threads on stop — contained noise, the app
/// itself is unaffected.
pub async fn stop(app: AppHandle) -> Result<EmbedSnapshot, String> {
    let state: tauri::State<'_, RemoteState> = app.state();
    if let Some(task) = state.embed_take_task() {
        task.abort();
    }
    let mut saved = load_config(&app);
    saved.wanted = false;
    let _ = save_config(&app, &saved);
    state.embed_update_snapshot(EmbedSnapshot::default());
    Ok(state.embed_snapshot())
}

/// Forget this device on the tailnet and bring Google sign-in back.
pub async fn logout(app: AppHandle) -> Result<EmbedSnapshot, String> {
    let _ = stop(app.clone()).await;
    if let Ok(path) = key_path(&app) {
        let _ = std::fs::remove_file(path);
    }
    let mut saved = load_config(&app);
    saved.auth_key = None;
    saved.wanted = true;
    let _ = save_config(&app, &saved);
    start(app, None, Vec::new()).await
}

/// Boot-time restore: if the node was wanted when the app last ran, start
/// it again without opening Settings. Called from setup().
pub fn autostart(app: &AppHandle) {
    if !load_config(app).wanted {
        return;
    }
    let state: tauri::State<'_, RemoteState> = app.state();
    if state.embed_snapshot().running {
        return;
    }
    state.embed_update_snapshot(EmbedSnapshot {
        running: true,
        ..EmbedSnapshot::default()
    });
    std::env::set_var(EXPERIMENT_ENV, EXPERIMENT_VALUE);
    // tauri::spawn, not tokio::spawn: setup() runs on the main thread with
    // no runtime yet, where tokio::spawn panics ("no reactor running").
    let task = tauri::async_runtime::spawn(run(app.clone()));
    if let Some(previous) = state.embed_replace_task(task) {
        previous.abort();
    }
}

async fn run(app: AppHandle) {
    if let Err(error) = run_inner(&app).await {
        let state: tauri::State<'_, RemoteState> = app.state();
        let mut snapshot = state.embed_snapshot();
        snapshot.running = false;
        snapshot.error = Some(error);
        state.embed_update_snapshot(snapshot);
    }
}

async fn run_inner(app: &AppHandle) -> Result<(), String> {
    // Pin the process-wide rustls provider BEFORE any TLS: the tree enables
    // multiple providers, and without an explicit default the first control
    // connection panics ("could not automatically determine CryptoProvider").
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let saved = load_config(app);
    let key_file = secret_path(&key_path(app)?);
    let mut config = tailscale::Config::default_with_key_file(&key_file)
        .await
        .map_err(|e| format!("Tailnet key storage failed: {e}"))?;
    // Official control plane only. Hostname/client name identify the node.
    config.requested_hostname = Some(crate::remote::node_hostname());
    config.client_name = Some(CLIENT_NAME.to_string());
    // Tags gate the node into tailnet ACLs (tagOwners + grants). Requesting
    // an unpermitted tag fails registration with a clear control error.
    config.requested_tags = saved.tags.clone();

    let device = tailscale::Device::new(&config, saved.auth_key.clone())
        .await
        .map_err(|e| format!("Tailnet node failed to start: {e}"))?;

    // Interactive auth: poll until the control plane authorizes us. Each
    // poll returns the current state plus the browser URL for Google SSO.
    loop {
        match device.is_authorized().await {
            Ok(tailscale::AuthState::Authorized) => break,
            Ok(tailscale::AuthState::NotAuthorized(url)) => {
                set_snapshot(app, |snapshot| {
                    snapshot.running = true;
                    snapshot.authorized = false;
                    snapshot.login_url = Some(url.to_string());
                    snapshot.error = None;
                    snapshot.hostname = Some(crate::remote::node_hostname());
                });
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            Err(error) => {
                return Err(format!("Tailnet authorization failed: {error}"));
            }
        }
    }

    let ip = device
        .ipv4_addr()
        .await
        .map_err(|e| format!("Tailnet address unavailable: {e}"))?;
    set_snapshot(app, |snapshot| {
        snapshot.running = true;
        snapshot.authorized = true;
        snapshot.tailnet_ip = Some(ip.to_string());
        snapshot.login_url = None;
        snapshot.error = None;
        snapshot.hostname = Some(crate::remote::node_hostname());
    });

    let state: tauri::State<'_, RemoteState> = app.state();
    let port = state.companion_port();
    let token = state.pairing_token(app)?;
    let listener = device
        .tcp_listen((ip, port).into())
        .await
        .map_err(|e| format!("Tailnet listen on {ip}:{port} failed: {e}"))?;

    loop {
        let stream = listener
            .accept()
            .await
            .map_err(|e| format!("Tailnet accept failed: {e}"))?;
        eprintln!("companion: tailnet accept {ip}:{port}");
        let app = app.clone();
        let token = token.clone();
        let shared = state.companion_shared();
        tokio::spawn(async move {
            crate::remote_server::serve_connection(app, stream, token, shared).await;
        });
    }
}

fn set_snapshot(app: &AppHandle, update: impl FnOnce(&mut EmbedSnapshot)) {
    let state: tauri::State<'_, RemoteState> = app.state();
    let mut snapshot = state.embed_snapshot();
    update(&mut snapshot);
    state.embed_update_snapshot(snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_validation() {
        assert!(validate_tag("tag:monocode").is_ok());
        assert!(validate_tag("  tag:monocode-2  ").is_ok());
        assert!(validate_tag("monocode").is_err());
        assert!(validate_tag("tag:").is_err());
        assert!(validate_tag("tag:HasCaps").is_err());
        assert!(validate_tag("tag:has space").is_err());
        assert!(validate_tag("tag:has_underscore").is_err());
    }
}
