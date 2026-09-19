use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::window::Color;
#[cfg(target_os = "windows")]
use tauri::window::{Effect, EffectsBuilder};
use tauri::{AppHandle, Emitter, EventTarget, Manager, WebviewWindow, WebviewWindowBuilder};

static WINDOW_COUNTER: AtomicU32 = AtomicU32::new(1);
static ALLOW_EXIT: AtomicBool = AtomicBool::new(false);

const QUIT_POLL: &str = "quit_poll";
const QUIT_CONFIRM: &str = "quit_confirm";
const QUIT_COMMIT: &str = "quit_commit";
const QUIT_ABORTED: &str = "quit_aborted";

/// A wedged webview must not strand the app. Missing the poll deadline is safe
/// because silence counts as busy, so it can be short.
const POLL_TIMEOUT: Duration = Duration::from_secs(2);
const COMMIT_TIMEOUT: Duration = Duration::from_secs(10);
/// Confirming waits on a person, so this is a backstop rather than a deadline:
/// a dialog that never arrives, or an answer that never gets back, would
/// otherwise leave Quit dead for the life of the process. Expiring only abandons
/// the run, so a later Quit starts a fresh one.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Polling,
    Confirming,
    Committing,
}

/// One quit at a time, decided in one place. Quit events reach every window, so
/// letting each answer for itself let an idle window exit the app while a busy
/// one was still asking - killing agents nobody agreed to stop.
struct QuitRun {
    id: u32,
    stage: Stage,
    /// Windows still owing a reply for the current stage.
    pending: HashSet<String>,
    /// Windows that answered the poll, so their listeners are known to be live.
    replied: HashSet<String>,
    in_flight: u32,
    /// The window showing the dialog, so its close can abort the quit.
    prompt: Option<String>,
}

static QUIT_RUN: Mutex<Option<QuitRun>> = Mutex::new(None);
static QUIT_COUNTER: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct QuitConfirm {
    id: u32,
    in_flight: u32,
}

pub fn open_new_window(app: &AppHandle) -> Result<(), String> {
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .ok_or("missing main window config")?
        .clone();

    let id = WINDOW_COUNTER.fetch_add(1, Ordering::Relaxed);
    config.label = format!("window-{id}");

    let window = WebviewWindowBuilder::from_config(app, &config)
        .map_err(|err| err.to_string())?
        .build()
        .map_err(|err| err.to_string())?;

    #[cfg(target_os = "macos")]
    crate::macos::install(&window);

    #[cfg(not(target_os = "macos"))]
    {
        let _ = window.set_decorations(false);
        let _ = window.set_shadow(true);
    }

    let _ = window.set_focus();
    Ok(())
}

/// Desktop blur goes on after the first UI paint and only in dark mode.
#[tauri::command]
pub fn set_window_glass_enabled(window: WebviewWindow, enabled: bool) {
    #[cfg(target_os = "macos")]
    {
        if enabled {
            let _ = window.set_background_color(Some(Color(0, 0, 0, 3)));
            crate::macos::enable_glass(&window);
        } else {
            crate::macos::disable_glass(&window);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if enabled {
            let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
            let _ = window.set_effects(EffectsBuilder::new().effect(Effect::Acrylic).build());
        } else {
            let _ = window.set_effects(None);
            let _ = window.set_background_color(Some(Color(247, 247, 247, 255)));
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (window, enabled);
    }
}

/// Close with a running chat hides the webview so the harness child keeps going.
#[tauri::command]
pub fn hide_window(window: WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|err| err.to_string())
}

/// Finish an idle close. `destroy` skips CloseRequested so the JS handler
/// does not loop; `close` would fire it again.
#[tauri::command]
pub fn destroy_window(window: WebviewWindow) -> Result<(), String> {
    window.destroy().map_err(|err| err.to_string())
}

/// Dock click / Cmd-click with no visible windows: bring hidden ones back.
pub fn show_hidden_or_open_new(app: &AppHandle) -> Result<(), String> {
    let mut windows: Vec<WebviewWindow> = app.webview_windows().into_values().collect();
    if windows.is_empty() {
        return open_new_window(app);
    }
    windows.sort_by(|a, b| a.label().cmp(b.label()));
    for window in &windows {
        let _ = window.unminimize();
        let _ = window.show();
    }
    windows
        .first()
        .ok_or_else(|| "missing window".to_string())?
        .set_focus()
        .map_err(|err| err.to_string())
}

/// window-state can restore a window as hidden after a quit-while-hidden.
pub fn ensure_launch_window_visible(app: &AppHandle) {
    let windows: Vec<WebviewWindow> = app.webview_windows().into_values().collect();
    if windows.is_empty() {
        return;
    }
    let any_visible = windows
        .iter()
        .any(|window| window.is_visible().unwrap_or(false));
    if any_visible {
        return;
    }
    let _ = show_hidden_or_open_new(app);
}

pub fn allow_exit() -> bool {
    ALLOW_EXIT.load(Ordering::SeqCst)
}

/// What the coordinator should do once a stage's bookkeeping is settled.
#[derive(Debug, PartialEq)]
enum Next {
    Wait,
    Confirm,
    Exit,
    Abort,
}

/// A quit already under way owns the decision. Starting a second one would
/// orphan the open dialog: its answer arrives for a run that no longer exists.
fn begin_run(slot: &mut Option<QuitRun>, id: u32, labels: Vec<String>) -> bool {
    if slot.is_some() {
        return false;
    }
    *slot = Some(QuitRun {
        id,
        stage: Stage::Polling,
        pending: labels.into_iter().collect(),
        replied: HashSet::new(),
        in_flight: 0,
        prompt: None,
    });
    true
}

fn record_reply(slot: &mut Option<QuitRun>, id: u32, label: &str, in_flight: u32) -> Next {
    let Some(run) = slot.as_mut() else {
        return Next::Wait;
    };
    if run.id != id || run.stage != Stage::Polling {
        return Next::Wait;
    }
    run.pending.remove(label);
    run.replied.insert(label.to_string());
    run.in_flight += in_flight;
    if run.pending.is_empty() {
        Next::Confirm
    } else {
        Next::Wait
    }
}

/// Silence counts as busy. A window that never answered may well have a turn
/// running, and guessing idle is what killed agents in the first place.
///
/// Closes once: a final reply and the poll timeout can land together, and
/// asking twice would send a second dialog that cancels the first.
fn close_poll(slot: &mut Option<QuitRun>, id: u32) -> Option<u32> {
    let run = slot
        .as_mut()
        .filter(|run| run.id == id && run.stage == Stage::Polling)?;
    run.in_flight += run.pending.len() as u32;
    run.pending.clear();
    run.stage = Stage::Confirming;
    Some(run.in_flight)
}

fn open_commit(slot: &mut Option<QuitRun>, id: u32, labels: Vec<String>) -> bool {
    let Some(run) = slot.as_mut().filter(|run| run.id == id) else {
        return false;
    };
    run.stage = Stage::Committing;
    run.pending = labels.into_iter().collect();
    true
}

fn record_ready(slot: &mut Option<QuitRun>, id: u32, label: &str) -> Next {
    let Some(run) = slot.as_mut() else {
        return Next::Wait;
    };
    if run.id != id || run.stage != Stage::Committing {
        return Next::Wait;
    }
    run.pending.remove(label);
    if run.pending.is_empty() {
        Next::Exit
    } else {
        Next::Wait
    }
}

fn drop_window(slot: &mut Option<QuitRun>, label: &str) -> Next {
    let Some(run) = slot.as_mut() else {
        return Next::Wait;
    };
    if run.stage == Stage::Confirming {
        return if run.prompt.as_deref() == Some(label) {
            Next::Abort
        } else {
            Next::Wait
        };
    }
    run.pending.remove(label);
    if !run.pending.is_empty() {
        return Next::Wait;
    }
    if run.stage == Stage::Polling {
        Next::Confirm
    } else {
        Next::Exit
    }
}

/// Ask every window what it has running, then decide once for all of them.
pub fn request_quit(app: &AppHandle) {
    let labels: Vec<String> = app.webview_windows().keys().cloned().collect();
    if labels.is_empty() {
        confirm_quit(app.clone());
        return;
    }
    let id = QUIT_COUNTER.fetch_add(1, Ordering::Relaxed);
    if !begin_run(&mut QUIT_RUN.lock().unwrap(), id, labels) {
        resurface_prompt(app);
        return;
    }
    // One webview tearing down fails the whole emit, and exiting on that would
    // kill every other window's work without asking. Let the poll time out
    // instead: silence counts as busy, so the user is still asked.
    let _ = app.emit(QUIT_POLL, id);
    watch_stage(app, id, Stage::Polling, POLL_TIMEOUT);
}

/// One window's live turn count, counted before anything is killed.
#[tauri::command]
pub fn quit_poll_reply(app: AppHandle, window: WebviewWindow, id: u32, in_flight: u32) {
    let next = record_reply(&mut QUIT_RUN.lock().unwrap(), id, window.label(), in_flight);
    if next == Next::Confirm {
        start_confirm(&app, id);
    }
}

/// Bring a parked dialog back into view. The run cannot be restarted, and with
/// close-to-tray the user cannot close that window to clear it either.
fn resurface_prompt(app: &AppHandle) {
    let label = {
        let guard = QUIT_RUN.lock().unwrap();
        let Some(run) = guard.as_ref() else { return };
        if run.stage != Stage::Confirming {
            return;
        }
        run.prompt.clone()
    };
    let Some(window) = label.and_then(|label| app.get_webview_window(&label)) else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// The answer to the one dialog the whole app gets to show.
#[tauri::command]
pub fn quit_decision(app: AppHandle, window: WebviewWindow, id: u32, confirmed: bool) {
    {
        let guard = QUIT_RUN.lock().unwrap();
        let Some(run) = guard.as_ref() else { return };
        if run.id != id || run.stage != Stage::Confirming {
            return;
        }
        // Only the window that was asked gets to answer for everyone.
        if run.prompt.as_deref() != Some(window.label()) {
            return;
        }
    }
    if confirmed {
        start_commit(&app, id);
    } else {
        clear_run(id);
    }
}

/// One window has persisted. The last one out turns the lights off, so a slow
/// window cannot lose its workspace to a faster window's exit.
#[tauri::command]
pub fn quit_ready(app: AppHandle, window: WebviewWindow, id: u32, persisted: bool) {
    // A window that could not save its workspace keeps the app open, the way a
    // failed persist did before the handshake existed. The windows that did
    // save are staying too, so take them back out of quitting.
    if !persisted {
        clear_run(id);
        let _ = app.emit(QUIT_ABORTED, ());
        return;
    }
    let next = record_ready(&mut QUIT_RUN.lock().unwrap(), id, window.label());
    if next == Next::Exit {
        confirm_quit(app);
    }
}

/// A window that closes mid-quit must not be waited on forever.
pub fn forget_quit_window(app: &AppHandle, label: &str) {
    let (id, next) = {
        let mut guard = QUIT_RUN.lock().unwrap();
        let Some(id) = guard.as_ref().map(|run| run.id) else {
            return;
        };
        (id, drop_window(&mut guard, label))
    };
    match next {
        Next::Confirm => start_confirm(app, id),
        Next::Exit => confirm_quit(app.clone()),
        Next::Abort => clear_run(id),
        Next::Wait => {}
    }
}

fn start_confirm(app: &AppHandle, id: u32) {
    let Some(in_flight) = close_poll(&mut QUIT_RUN.lock().unwrap(), id) else {
        return;
    };
    if in_flight == 0 {
        start_commit(app, id);
        return;
    }
    if app.webview_windows().is_empty() {
        clear_run(id);
        confirm_quit(app.clone());
        return;
    }
    // Tray Quit arrives with every window hidden, and a dialog parented to a
    // hidden window cannot be answered.
    let _ = show_hidden_or_open_new(app);
    let replied = {
        let guard = QUIT_RUN.lock().unwrap();
        guard
            .as_ref()
            .filter(|run| run.id == id)
            .map(|run| run.replied.clone())
            .unwrap_or_default()
    };
    let Some(label) = prompt_window(app, &replied) else {
        clear_run(id);
        confirm_quit(app.clone());
        return;
    };
    if let Some(run) = QUIT_RUN.lock().unwrap().as_mut() {
        if run.id == id {
            run.prompt = Some(label.clone());
        }
    }
    let payload = QuitConfirm { id, in_flight };
    if app
        .emit_to(EventTarget::webview_window(&label), QUIT_CONFIRM, payload)
        .is_err()
    {
        clear_run(id);
        return;
    }
    watch_stage(app, id, Stage::Confirming, CONFIRM_TIMEOUT);
}

fn start_commit(app: &AppHandle, id: u32) {
    let labels: Vec<String> = app.webview_windows().keys().cloned().collect();
    let empty = labels.is_empty();
    if !open_commit(&mut QUIT_RUN.lock().unwrap(), id, labels) {
        return;
    }
    if empty {
        confirm_quit(app.clone());
        return;
    }
    // Same here: the windows that did receive it still deserve their save, so
    // the commit timeout is the backstop rather than exiting on the spot.
    let _ = app.emit(QUIT_COMMIT, id);
    watch_stage(app, id, Stage::Committing, COMMIT_TIMEOUT);
}

/// Prefer the window the user is looking at; any window beats none.
///
/// Only among windows that answered the poll, though: replying proves their
/// listeners are live. A window still booting would swallow the dialog, and
/// confirming has nothing to time out on.
fn prompt_window(app: &AppHandle, replied: &HashSet<String>) -> Option<String> {
    let windows = app.webview_windows();
    let mut labels: Vec<String> = windows.keys().cloned().collect();
    labels.sort();
    let answered: Vec<String> = labels
        .iter()
        .filter(|label| replied.contains(*label))
        .cloned()
        .collect();
    let pool = if answered.is_empty() {
        &labels
    } else {
        &answered
    };
    let focused = pool.iter().find(|label| {
        windows
            .get(*label)
            .is_some_and(|window| window.is_focused().unwrap_or(false))
    });
    if let Some(label) = focused {
        return Some(label.clone());
    }
    let visible = pool.iter().find(|label| {
        windows
            .get(*label)
            .is_some_and(|window| window.is_visible().unwrap_or(false))
    });
    if let Some(label) = visible {
        return Some(label.clone());
    }
    pool.first().cloned()
}

/// Nothing waits forever: a webview that never answers still lets the app quit.
fn watch_stage(app: &AppHandle, id: u32, stage: Stage, wait: Duration) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        let stalled = QUIT_RUN
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|run| run.id == id && run.stage == stage);
        if !stalled {
            return;
        }
        match stage {
            Stage::Polling => start_confirm(&app, id),
            Stage::Committing => confirm_quit(app),
            Stage::Confirming => clear_run(id),
        }
    });
}

fn clear_run(id: u32) {
    let mut guard = QUIT_RUN.lock().unwrap();
    if guard.as_ref().is_some_and(|run| run.id == id) {
        *guard = None;
    }
}

/// Persist already happened in JS. Show windows so window-state doesn't save hidden.
pub fn confirm_quit(app: AppHandle) {
    *QUIT_RUN.lock().unwrap() = None;
    ALLOW_EXIT.store(true, Ordering::SeqCst);
    for window in app.webview_windows().values() {
        let _ = window.show();
    }
    // Belt and braces. `RunEvent::Exit` reaps too, and it also runs before the
    // process is gone, but a macOS terminate that skips the run loop would not
    // reach it — and `kill_all`'s SIGKILL wait only works while we're alive.
    if let Some(host) = app.try_state::<crate::harness::HarnessHost>() {
        host.kill_all();
    }
    if let Some(host) = app.try_state::<crate::pty::PtyHost>() {
        host.kill_all();
    }
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polling(labels: &[&str]) -> Option<QuitRun> {
        let mut slot = None;
        let owned = labels.iter().map(|label| label.to_string()).collect();
        assert!(begin_run(&mut slot, 1, owned));
        slot
    }

    fn in_flight(slot: &Option<QuitRun>) -> Option<u32> {
        slot.as_ref().map(|run| run.in_flight)
    }

    #[test]
    fn one_window_answering_does_not_decide_for_the_others() {
        let mut slot = polling(&["main", "window-2"]);
        assert_eq!(record_reply(&mut slot, 1, "main", 0), Next::Wait);
        assert_eq!(record_reply(&mut slot, 1, "window-2", 2), Next::Confirm);
        assert_eq!(close_poll(&mut slot, 1), Some(2));
    }

    #[test]
    fn a_window_that_never_answers_counts_as_busy() {
        let mut slot = polling(&["main", "window-2"]);
        record_reply(&mut slot, 1, "main", 0);
        // The poll timed out with window-2 still owing an answer.
        assert_eq!(close_poll(&mut slot, 1), Some(1));
    }

    #[test]
    fn replies_from_a_stale_run_are_ignored() {
        let mut slot = polling(&["main"]);
        assert_eq!(record_reply(&mut slot, 99, "main", 5), Next::Wait);
        assert_eq!(in_flight(&slot), Some(0));
    }

    #[test]
    fn a_second_quit_while_polling_is_ignored() {
        let mut slot = polling(&["main"]);
        assert!(!begin_run(&mut slot, 2, vec!["main".to_string()]));
        assert_eq!(slot.as_ref().map(|run| run.id), Some(1));
    }

    #[test]
    fn a_second_quit_leaves_the_open_dialog_in_charge() {
        let mut slot = polling(&["main"]);
        record_reply(&mut slot, 1, "main", 1);
        close_poll(&mut slot, 1);
        assert!(!begin_run(&mut slot, 2, vec!["main".to_string()]));
        assert_eq!(slot.as_ref().map(|run| run.id), Some(1));
    }

    #[test]
    fn only_windows_that_answered_are_offered_the_dialog() {
        let mut slot = polling(&["main", "window-2"]);
        record_reply(&mut slot, 1, "window-2", 1);
        // main never answered, so it is still booting or wedged: asking it
        // would leave the dialog unshown and the quit with nothing to await.
        let replied = slot.as_ref().map(|run| run.replied.clone());
        assert_eq!(replied, Some(HashSet::from(["window-2".to_string()])));
    }

    #[test]
    fn a_final_reply_and_the_timeout_cannot_both_close_the_poll() {
        let mut slot = polling(&["main"]);
        record_reply(&mut slot, 1, "main", 1);
        assert_eq!(close_poll(&mut slot, 1), Some(1));
        assert_eq!(close_poll(&mut slot, 1), None);
    }

    #[test]
    fn the_app_exits_only_once_every_window_has_persisted() {
        let mut slot = polling(&["main", "window-2"]);
        record_reply(&mut slot, 1, "main", 1);
        record_reply(&mut slot, 1, "window-2", 0);
        close_poll(&mut slot, 1);
        let labels = vec!["main".to_string(), "window-2".to_string()];
        assert!(open_commit(&mut slot, 1, labels));
        assert_eq!(record_ready(&mut slot, 1, "main"), Next::Wait);
        assert_eq!(record_ready(&mut slot, 1, "window-2"), Next::Exit);
    }

    #[test]
    fn a_window_closing_mid_poll_is_not_waited_on() {
        let mut slot = polling(&["main", "window-2"]);
        record_reply(&mut slot, 1, "main", 0);
        assert_eq!(drop_window(&mut slot, "window-2"), Next::Confirm);
    }

    #[test]
    fn closing_the_window_holding_the_dialog_aborts_the_quit() {
        let mut slot = polling(&["main"]);
        record_reply(&mut slot, 1, "main", 1);
        close_poll(&mut slot, 1);
        if let Some(run) = slot.as_mut() {
            run.prompt = Some("main".to_string());
        }
        assert_eq!(drop_window(&mut slot, "main"), Next::Abort);
    }

    #[test]
    fn another_window_closing_leaves_the_dialog_alone() {
        let mut slot = polling(&["main", "window-2"]);
        record_reply(&mut slot, 1, "main", 1);
        record_reply(&mut slot, 1, "window-2", 0);
        close_poll(&mut slot, 1);
        if let Some(run) = slot.as_mut() {
            run.prompt = Some("main".to_string());
        }
        assert_eq!(drop_window(&mut slot, "window-2"), Next::Wait);
    }
}
