//! The Tauri app's side of the browser module. The module itself lives in
//! `crates/monocode-browser`, shared with Monocode Native; this file gives it
//! the app's database, windows and job object ([`TauriEmbedder`]) and wraps
//! its host in the `browser_*` Tauri commands the frontend calls. The
//! [`protocol::STATE_EVENT`] event carries a [`protocol::BrowserState`] to
//! every window after each change.

pub mod register;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

pub use monocode_browser::*;

use monocode_browser::protocol::{
    AuditEntry, BrowserSettings, BrowserState, TabId, TabOverview, STATE_EVENT,
};

/// The app as the browser module sees it.
struct TauriEmbedder {
    app: AppHandle,
}

impl Embedder for TauriEmbedder {
    fn with_db(&self, f: &mut dyn FnMut(&rusqlite::Connection)) {
        let Some(store) = self.app.try_state::<crate::session_store::SessionStore>() else {
            return;
        };
        let Ok(conn) = store.lock_conn() else {
            return;
        };
        f(&conn);
    }

    fn state_changed(&self, state: &BrowserState) {
        let _ = self.app.emit(STATE_EVENT, state);
    }

    fn display_scale(&self) -> Option<f64> {
        self.app
            .get_webview_window("main")
            .and_then(|window| window.scale_factor().ok())
            .or_else(|| {
                self.app
                    .primary_monitor()
                    .ok()
                    .flatten()
                    .map(|monitor| monitor.scale_factor())
            })
    }

    fn browser_started(&self, child: &std::process::Child) {
        // Windows: the browser ends with the app even after a crash. Best
        // effort; without the job, OFF and app exit still stop it.
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            let _ = crate::windows::assign_child(child.as_raw_handle());
        }
        let _ = child;
    }
}

/// Create the browser host on the app's session store. Called from
/// `lib.rs` setup after `session_store::init`.
pub fn init(app: &AppHandle) -> Result<(), String> {
    if app
        .try_state::<crate::session_store::SessionStore>()
        .is_none()
    {
        return Err("Session store must be initialised before the browser module".into());
    }
    let data_dir = app.path().app_data_dir().ok();
    monocode_browser::init(
        Arc::new(TauriEmbedder { app: app.clone() }),
        data_dir.as_deref(),
    )
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

fn require_host() -> Result<Arc<BrowserHost>, String> {
    host().ok_or_else(|| "Browser module is not initialised".to_string())
}

/// Run a command's work on a blocking thread, one command at a time: most
/// of them talk to the browser (detach, close, adopt) and wait for its
/// answers, which must not freeze the UI.
async fn on_host<T: Send + 'static>(
    work: impl FnOnce(&Arc<BrowserHost>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let host = require_host()?;
    tauri::async_runtime::spawn_blocking(move || host.serialized(|| work(&host)))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn browser_toggle(on: bool) -> Result<BrowserState, String> {
    on_host(move |host| {
        host.set_toggle(on);
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub fn browser_status() -> Result<BrowserState, String> {
    Ok(require_host()?.state())
}

#[tauri::command]
pub fn browser_tabs() -> Result<Vec<TabOverview>, String> {
    Ok(require_host()?.policy().read().overview())
}

#[tauri::command]
pub async fn browser_revoke(session_id: String) -> Result<BrowserState, String> {
    on_host(move |host| {
        host.revoke(&session_id);
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_transfer(
    tab: String,
    from_session: String,
    to_session: String,
) -> Result<BrowserState, String> {
    on_host(move |host| {
        host.transfer(&TabId(tab), &from_session, &to_session)
            .map_err(|reason| reason.message().to_string())?;
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_release_tab(tab: String) -> Result<BrowserState, String> {
    on_host(move |host| {
        host.release_tab(&TabId(tab))?;
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_connect() -> Result<BrowserState, String> {
    on_host(|host| {
        host.connect();
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_disconnect() -> Result<BrowserState, String> {
    on_host(|host| {
        host.disconnect();
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_set_session_grant(
    window: WebviewWindow,
    session_id: String,
    enabled: bool,
) -> Result<BrowserState, String> {
    let label = window.label().to_string();
    on_host(move |host| {
        host.set_grant(&session_id, &label, enabled);
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_answer_request(
    window: WebviewWindow,
    session_id: String,
    allow: bool,
) -> Result<BrowserState, String> {
    let label = window.label().to_string();
    on_host(move |host| {
        host.answer_request(&session_id, &label, allow);
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_user_tabs() -> Result<Vec<targets::FreeTab>, String> {
    on_host(|host| host.user_tabs()).await
}

#[tauri::command]
pub async fn browser_give_tab(
    window: WebviewWindow,
    free: String,
    session_id: String,
) -> Result<BrowserState, String> {
    let label = window.label().to_string();
    on_host(move |host| {
        host.give_tab(&free, &session_id, &label)?;
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_open_app(
    window: WebviewWindow,
    session_id: String,
    url: String,
) -> Result<BrowserState, String> {
    let host = require_host()?;
    let label = window.label().to_string();
    let state = host.clone();
    tauri::async_runtime::spawn_blocking(move || host.open_app(&session_id, &label, &url))
        .await
        .map_err(|e| e.to_string())??;
    Ok(state.state())
}

/// "New tab" in a thread's browser pane. Returns the tab's id.
#[tauri::command]
pub async fn browser_new_tab(
    window: WebviewWindow,
    session_id: String,
    url: Option<String>,
) -> Result<String, String> {
    let host = require_host()?;
    let label = window.label().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        host.new_tab(&session_id, &label, url.as_deref())
            .map(|tab| tab.0)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn browser_pick_element(
    session_id: String,
    tab: String,
    in_pane: Option<bool>,
) -> Result<Option<pick::PickedElement>, String> {
    let host = require_host()?;
    tauri::async_runtime::spawn_blocking(move || {
        host.pick_element(&session_id, &TabId(tab), in_pane.unwrap_or(false))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn browser_cancel_pick() -> Result<(), String> {
    require_host()?.cancel_pick();
    Ok(())
}

/// Start the view of `view`. Returns when it ends. Pictures are not sent:
/// the pane asks for each with [`browser_live_frame`].
#[tauri::command]
pub async fn browser_live_start(
    session_id: String,
    tab: String,
    view: String,
    width: u32,
    height: u32,
    scale: Option<f64>,
) -> Result<live::LiveEnd, String> {
    let host = require_host()?;
    let size = live::PanelSize::new(width, height, scale.unwrap_or(1.0));
    tauri::async_runtime::spawn_blocking(move || {
        host.live_view(&session_id, &TabId(tab), &view, size, |_| {})
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The next picture of `view` after `after` (its `seq`), as
/// `live::LiveFrame::packed` bytes; empty when none came within a second
/// or the view is over. One request at a time per pane: the pane asks when
/// it is ready to draw, so it always gets what the tab shows now, and the
/// picture crosses to the webview once, as bytes, without a script run or
/// a callback in between.
#[tauri::command]
pub async fn browser_live_frame(view: String, after: u64) -> Result<tauri::ipc::Response, String> {
    let host = require_host()?;
    tauri::async_runtime::spawn_blocking(move || host.live_take(&view, after))
        .await
        .map(tauri::ipc::Response::new)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_live_stop(view: Option<String>) -> Result<(), String> {
    require_host()?.stop_live(view.as_deref());
    Ok(())
}

#[tauri::command]
pub fn browser_live_resize(
    view: String,
    width: u32,
    height: u32,
    scale: Option<f64>,
) -> Result<(), String> {
    require_host()?.resize_live(
        &view,
        live::PanelSize::new(width, height, scale.unwrap_or(1.0)),
    );
    Ok(())
}

#[tauri::command]
pub async fn browser_live_navigate(
    session_id: String,
    tab: String,
    nav: live::LiveNav,
) -> Result<(), String> {
    let host = require_host()?;
    tauri::async_runtime::spawn_blocking(move || host.live_navigate(&session_id, &TabId(tab), &nav))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn browser_live_input(
    session_id: String,
    tab: String,
    input: live::LiveInput,
) -> Result<(), String> {
    let host = require_host()?;
    tauri::async_runtime::spawn_blocking(move || host.live_input(&session_id, &TabId(tab), &input))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn browser_open_window() -> Result<BrowserState, String> {
    on_host(|host| {
        host.open_window()?;
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_close_tab(tab: String) -> Result<BrowserState, String> {
    on_host(move |host| {
        host.close_tab_by_user(&TabId(tab))?;
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_close_all_tabs() -> Result<BrowserState, String> {
    on_host(|host| {
        host.close_all_tabs();
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub fn browser_audit_list(
    session_id: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<AuditEntry>, String> {
    Ok(require_host()?.audit_list(
        session_id.as_deref(),
        limit.unwrap_or(audit::DEFAULT_LIST_LIMIT),
    ))
}

#[tauri::command]
pub fn browser_events_list(limit: Option<u32>) -> Result<Vec<audit::EventEntry>, String> {
    Ok(require_host()?.events_list(limit.unwrap_or(audit::DEFAULT_LIST_LIMIT)))
}

#[tauri::command]
pub fn browser_audit_clear() -> Result<(), String> {
    require_host()?.audit_clear();
    Ok(())
}

#[tauri::command]
pub async fn browser_set_settings(settings: BrowserSettings) -> Result<BrowserState, String> {
    on_host(move |host| {
        host.set_settings(settings);
        Ok(host.state())
    })
    .await
}
