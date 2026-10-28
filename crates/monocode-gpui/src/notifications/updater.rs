//! Shared app updater: one check/install across windows; no I/O on the UI thread.
use crate::{
    runtime::{Database, locked},
    theme::*,
};
use gpui::{App, Context, Entity, EventEmitter, Global, Role, div, prelude::*, px};
use monocode_core::app_update::{self, Config, Installation, Installed, Update};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone)]
pub(crate) struct Shared(pub Entity<Updater>, Arc<Mutex<InstallGate>>);
impl Global for Shared {}
#[derive(Default)]
struct InstallGate {
    installing: bool,
    quitting: bool,
}

/// A normal quit can cancel a download, but must not interrupt a file swap or
/// a privileged package install. Keep this gate outside the entity borrow so
/// successful installation can call the regular quit path from its callback.
pub(crate) fn defer_quit(cx: &App) -> bool {
    let Some(shared) = cx.try_global::<Shared>() else {
        return false;
    };
    let mut gate = shared.1.lock().unwrap_or_else(|e| e.into_inner());
    gate.quitting = true;
    gate.installing
}
pub(crate) enum Event {
    Changed,
    Error(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Disabled,
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Installing,
}
enum Progress {
    Download(u64, Option<u64>),
    Installing,
}

pub(crate) fn version() -> &'static str {
    option_env!("MONOCODE_NATIVE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}
pub(crate) fn shared(store: Database, cx: &mut App) -> Entity<Updater> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return shared.0.clone();
    }
    let entity = cx.new(|cx| Updater::new(store, cx));
    let gate = entity.read(cx).gate.clone();
    cx.set_global(Shared(entity.clone(), gate));
    entity
}

pub(crate) struct Updater {
    config: Option<Config>,
    installation: Option<Installation>,
    store: Database,
    phase: Phase,
    pending: Option<Update>,
    progress: Option<u64>,
    message: Option<String>,
    poll: Option<gpui::Task<()>>,
    gate: Arc<Mutex<InstallGate>>,
}
impl EventEmitter<Event> for Updater {}
impl Updater {
    fn new(store: Database, cx: &mut Context<Self>) -> Self {
        let setup = (|| {
            let endpoint = option_env!("MONOCODE_NATIVE_UPDATER_ENDPOINT")
                .filter(|v| !v.is_empty())
                .ok_or("Automatic updates aren’t configured for this build")?;
            let key = option_env!("MONOCODE_NATIVE_UPDATER_PUBKEY")
                .filter(|v| !v.is_empty())
                .ok_or("This build has no update verification key")?;
            Config::new(endpoint, key, version())
        })();
        let (config, message) = match setup {
            Ok(config) => (Some(config), None),
            Err(error) => (None, Some(error)),
        };
        let mut this = Self {
            phase: if config.is_some() {
                Phase::Idle
            } else {
                Phase::Disabled
            },
            config,
            installation: None,
            store,
            pending: None,
            progress: None,
            message,
            poll: None,
            gate: Arc::new(Mutex::new(InstallGate::default())),
        };
        if this.config.is_some() {
            this.check(false, cx);
            this.poll = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_secs(4 * 60 * 60))
                        .await;
                    if this.update(cx, |this, cx| this.check(false, cx)).is_err() {
                        break;
                    }
                }
            }));
        }
        this
    }
    fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Checking | Phase::Downloading | Phase::Installing
        )
    }
    pub(crate) fn actionable(&self) -> bool {
        matches!(
            self.phase,
            Phase::Available | Phase::Downloading | Phase::Installing
        )
    }
    fn changed(&self, cx: &mut Context<Self>) {
        cx.emit(Event::Changed);
        cx.notify();
    }
    pub(crate) fn check(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let Some(config) = self.config.clone() else {
            self.changed(cx);
            return;
        };
        self.phase = Phase::Checking;
        self.message = None;
        self.changed(cx);
        let installation = self.installation.clone();
        let work = cx.background_executor().spawn(async move {
            let installation = match installation {
                Some(installation) => installation,
                None => Installation::detect()?,
            };
            let update = app_update::check(&config, &installation)?;
            Ok::<_, String>((installation, update))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((installation, update)) => {
                        this.installation = Some(installation);
                        this.phase = if update.is_some() {
                            Phase::Available
                        } else {
                            Phase::Current
                        };
                        this.pending = update;
                    }
                    Err(error) => {
                        this.phase = if this.pending.is_some() {
                            Phase::Available
                        } else {
                            Phase::Idle
                        };
                        this.message = Some(error.clone());
                        if manual {
                            cx.emit(Event::Error(format!("Couldn’t check for updates: {error}")));
                        }
                    }
                }
                this.changed(cx);
            });
        })
        .detach();
    }
    fn install(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let (Some(update), Some(installation)) = (self.pending.clone(), self.installation.clone())
        else {
            return;
        };
        self.phase = Phase::Downloading;
        self.progress = Some(0);
        self.message = None;
        self.changed(cx); // Close the double-click window before starting I/O.
        let store = self.store.clone();
        let gate = self.gate.clone();
        let (tx, rx) = async_channel::bounded(8);
        let work = cx.background_executor().spawn(async move {
            let result = (|| {
                let mut last_bytes = 0;
                let mut last_percent = None;
                let download = update.download(|n, total| {
                    let percent = total.filter(|n| *n > 0).map(|total| n.saturating_mul(100) / total);
                    if (percent.is_some() && percent != last_percent) || (percent.is_none() && n - last_bytes >= 1024 * 1024) {
                        let _ = tx.try_send(Progress::Download(n, total));
                        last_percent = percent;
                        last_bytes = n;
                    }
                })?;
                {
                    let mut gate = gate.lock().unwrap_or_else(|e| e.into_inner());
                    if gate.quitting { return Err("The app is closing; the update was not installed".into()); }
                    gate.installing = true;
                }
                let _ = tx.send_blocking(Progress::Installing);
                locked(&store)?.save_setting(
                    "monocode.installedUpdate",
                    &serde_json::json!({"version": update.version.to_string()}).to_string(),
                )?;
                locked(&store)?.save_setting("monocode.nativeReleaseNotes", &serde_json::json!({"version": update.version.to_string(), "body": update.notes}).to_string())?;
                update.install(download, &installation)
            })();
            if result.is_err() {
                let _ = locked(&store)
                    .and_then(|store| store.save_setting("monocode.installedUpdate", ""));
            }
            gate.lock().unwrap_or_else(|e| e.into_inner()).installing = false;
            result
        });
        cx.spawn(async move |this, cx| {
            while let Ok(progress) = rx.recv().await {
                if this
                    .update(cx, |this, cx| {
                        match progress {
                            Progress::Download(n, total) => {
                                this.progress = total
                                    .filter(|n| *n > 0)
                                    .map(|total| (n.saturating_mul(100) / total).min(100))
                            }
                            Progress::Installing => this.phase = Phase::Installing,
                        }
                        this.changed(cx);
                    })
                    .is_err()
                {
                    return;
                }
            }
            let result = work.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Installed::Relaunch(path)) => {
                    if let Err(error) = app_update::schedule_relaunch(&path) {
                        this.phase = Phase::Available;
                        this.message = Some(error.clone());
                        cx.emit(Event::Error(format!(
                            "The update was installed. Reopen Monocode Native to finish: {error}"
                        )));
                        this.changed(cx);
                    } else {
                        crate::quit(cx);
                    }
                }
                Ok(Installed::InstallerStarted) => crate::quit(cx),
                Err(error) => {
                    this.phase = Phase::Available;
                    this.message = Some(error.clone());
                    cx.emit(Event::Error(format!(
                        "Couldn’t install the update: {error}"
                    )));
                    this.changed(cx);
                    if this.gate.lock().unwrap_or_else(|e| e.into_inner()).quitting {
                        crate::quit(cx);
                    }
                }
            });
        })
        .detach();
    }
}
/// What the updater offers right now, for Settings' About button.
pub(crate) enum Action {
    Check,
    Install(String),
    Busy(String),
}
pub(crate) fn action(entity: &Entity<Updater>, cx: &App) -> Action {
    let state = entity.read(cx);
    match state.phase {
        Phase::Available => Action::Install(state.pending_version()),
        Phase::Checking => Action::Busy("Checking…".into()),
        Phase::Downloading | Phase::Installing => Action::Busy(state.progress_label()),
        Phase::Disabled | Phase::Idle | Phase::Current => Action::Check,
    }
}
/// A line for Settings' About description: the last check's outcome, if any.
pub(crate) fn status(entity: &Entity<Updater>, cx: &App) -> Option<String> {
    let state = entity.read(cx);
    match state.phase {
        Phase::Current => Some("You’re on the latest version.".into()),
        Phase::Checking => None,
        _ => state.message.clone(),
    }
}
pub(crate) fn install(entity: &Entity<Updater>, cx: &mut App) {
    entity.update(cx, |updater, cx| updater.install(cx));
}
impl Updater {
    fn pending_version(&self) -> String {
        self.pending
            .as_ref()
            .map(|u| u.version.to_string())
            .unwrap_or_default()
    }
    fn progress_label(&self) -> String {
        match self.phase {
            Phase::Installing => "Installing…".into(),
            _ => self
                .progress
                .map(|p| format!("Downloading {p}%"))
                .unwrap_or_else(|| "Downloading…".into()),
        }
    }
}
/// The sidebar's update card, shown while an update is available or installing.
pub(crate) fn row(entity: Entity<Updater>, cx: &App) -> gpui::AnyElement {
    let action = entity.clone();
    let state = entity.read(cx);
    let available = state.phase == Phase::Available;
    let title = if available {
        "Update available".to_string()
    } else {
        state.progress_label()
    };
    let detail = if available || state.busy() {
        format!("v{}", state.pending_version())
    } else {
        format!("v{}", version())
    };
    let mut button = div()
        .id("install-native-update")
        .role(Role::Button)
        .rounded(px(8.))
        .px_2()
        .py(px(6.))
        .flex()
        .items_center()
        .gap_2()
        .bg(content(0.06))
        .child(
            icon(
                if state.busy() {
                    "loader"
                } else {
                    "arrow-down-circle"
                },
                16.,
            )
            .text_color(content(if available { 0.9 } else { 0.5 })),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(content(0.9))
                        .child(title),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .line_height(px(14.))
                        .text_color(content(0.45))
                        .child(detail),
                ),
        );
    if available {
        button = button
            .aria_label("Install the available Monocode Native update")
            .cursor_pointer()
            .hover(|s| s.bg(content(0.1)))
            .on_click(move |_, _, cx| action.update(cx, |updater, cx| updater.install(cx)));
    }
    div()
        .id("native-app-update")
        .role(Role::Status)
        .w_full()
        .min_w_0()
        .child(button)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::sync::{Arc, Mutex};
    #[gpui::test]
    fn quit_waits_for_installation_to_finish(cx: &mut TestAppContext) {
        let store = Arc::new(Mutex::new(
            monocode_core::store::Store::open(std::path::Path::new(":memory:")).unwrap(),
        ));
        let updater = cx.update(|cx| shared(store, cx));
        let gate = updater.read_with(cx, |updater, _| updater.gate.clone());
        gate.lock().unwrap().installing = true;
        assert!(cx.update(|cx| defer_quit(cx)));
        assert!(gate.lock().unwrap().quitting);
        gate.lock().unwrap().installing = false;
        assert!(!cx.update(|cx| defer_quit(cx)));
    }
    #[gpui::test]
    fn windows_share_one_updater_and_busy_operations_cannot_start_again(cx: &mut TestAppContext) {
        let store = Arc::new(Mutex::new(
            monocode_core::store::Store::open(std::path::Path::new(":memory:")).unwrap(),
        ));
        let first = cx.update(|cx| shared(store.clone(), cx));
        let second = cx.update(|cx| shared(store, cx));
        assert_eq!(first.entity_id(), second.entity_id());
        first.update(cx, |view, cx| {
            assert!(!view.actionable());
            for phase in [Phase::Downloading, Phase::Installing] {
                view.phase = phase;
                view.progress = Some(42);
                view.install(cx);
                view.check(true, cx);
                assert_eq!(view.phase, phase);
                assert_eq!(view.progress, Some(42));
                assert!(view.actionable());
            }
            view.phase = Phase::Current;
            assert!(!view.actionable());
        });
    }
}
