//! The shared updater flow and native prompts used by Settings.
use gpui::{App, AppContext as _, AsyncApp, Entity, Global, Task};
use monocode_app::boot::AppServices;
use monocode_engine::attention::Attention;
use monocode_settings::Kv;
use monocode_updater::flow::{DialogOptions, UpdaterFlow, UpdaterHost, UpdaterPhase};
use monocode_updater::{DownloadEvent, Result, Update, Updater, UpdaterConfig};
use monocode_view_settings::settings::host::UpdateReporter;
use monocode_view_settings::settings::{UpdatePhase, UpdaterSnapshot};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct UpdateState {
    pub snapshot: UpdaterSnapshot,
}
struct SharedState(Entity<UpdateState>);
impl Global for SharedState {}

pub fn state(cx: &mut App) -> Entity<UpdateState> {
    if let Some(state) = cx.try_global::<SharedState>() {
        return state.0.clone();
    }
    let state = cx.new(|_| UpdateState {
        snapshot: UpdaterSnapshot {
            current_version: env!("CARGO_PKG_VERSION").into(),
            ..Default::default()
        },
    });
    cx.set_global(SharedState(state.clone()));
    state
}

#[derive(Default)]
struct OperationGate(Rc<Cell<bool>>);
impl Global for OperationGate {}
struct Operation(Rc<Cell<bool>>);
impl Drop for Operation {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
fn begin(cx: &mut App) -> Option<Operation> {
    let gate = cx.default_global::<OperationGate>().0.clone();
    if gate.replace(true) {
        return None;
    }
    Some(Operation(gate))
}

fn publish(value: &monocode_updater::flow::UpdaterSnapshot, report: &UpdateReporter, cx: &mut App) {
    let value = snapshot(value);
    state(cx).update(cx, |state, cx| {
        state.snapshot = value.clone();
        cx.notify();
    });
    report(value, cx);
}

struct NativeUpdater {
    cx: RefCell<AsyncApp>,
    kv: Kv,
}
impl UpdaterHost for NativeUpdater {
    fn app_version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }
    async fn check(&self) -> Result<Option<Update>> {
        Updater::new(UpdaterConfig::from_build_env(self.app_version()))?
            .check_async()
            .await
    }
    async fn download_and_install(
        &self,
        update: &Update,
        event: &mut dyn FnMut(DownloadEvent),
    ) -> Result<()> {
        update.download_and_install_async(event).await
    }
    async fn message(&self, text: &str, _: DialogOptions) {
        self.cx
            .borrow_mut()
            .update(|cx| monocode_app::bridge::dialogs::alert(text, false, cx));
    }
    async fn ask(&self, text: &str, _: DialogOptions) -> bool {
        let prompt = self
            .cx
            .borrow_mut()
            .update(|cx| monocode_app::bridge::dialogs::confirm(text, "Install", cx));
        prompt.await
    }
    fn announce_update_available(&self, version: &str) {
        self.cx.borrow_mut().update(|cx| {
            let notifier = Attention::global(cx).notifier.clone();
            notifier.update(cx, |notifier, _| {
                notifier.announce_update_available(Some(version))
            });
        });
    }
    fn remember_installed_update(&self, version: &str) {
        monocode_updater::update_notice::remember_installed_update(version, &self.kv);
    }
    async fn relaunch(&self) -> Result<()> {
        let approval = self.cx.borrow_mut().update(|cx| {
            let lifecycle = monocode_engine::runtime::Engine::lifecycle(cx);
            let running = lifecycle.update(cx, |lifecycle, cx| lifecycle.in_flight_count(cx));
            if running > 0 {
                lifecycle.update(cx, |lifecycle, cx| {
                    lifecycle.ask_quit_confirmation(running, cx)
                })
            } else {
                Task::ready(true)
            }
        });
        if !approval.await {
            return Err(monocode_updater::Error::AuthenticationFailed);
        }
        let persisted = self.cx.borrow_mut().update(|cx| {
            monocode_engine::runtime::Engine::lifecycle(cx)
                .update(cx, |lifecycle, cx| lifecycle.commit_quit(cx))
        });
        if !persisted.await {
            return Err(monocode_updater::Error::AuthenticationFailed);
        }
        monocode_updater::relaunch()?;
        self.cx.borrow_mut().update(|cx| cx.quit());
        Ok(())
    }
}
struct SharedUpdater(Rc<UpdaterFlow<NativeUpdater>>);
impl Global for SharedUpdater {}

fn flow(cx: &mut App) -> Rc<UpdaterFlow<NativeUpdater>> {
    if let Some(updater) = cx.try_global::<SharedUpdater>() {
        return updater.0.clone();
    }
    let flow = Rc::new(UpdaterFlow::new(NativeUpdater {
        cx: RefCell::new(cx.to_async()),
        kv: AppServices::global(cx).kv.clone(),
    }));
    cx.set_global(SharedUpdater(flow.clone()));
    flow
}
fn snapshot(value: &monocode_updater::flow::UpdaterSnapshot) -> UpdaterSnapshot {
    UpdaterSnapshot {
        phase: match value.phase {
            UpdaterPhase::Idle => UpdatePhase::Idle,
            UpdaterPhase::Checking => UpdatePhase::Checking,
            UpdaterPhase::Current => UpdatePhase::Current,
            UpdaterPhase::Available => UpdatePhase::Available,
            UpdaterPhase::Downloading => UpdatePhase::Downloading,
            UpdaterPhase::Error => UpdatePhase::Error,
        },
        current_version: value.current_version.clone(),
        available_version: value.available_version.clone(),
        progress: value.progress,
        error: value.error.clone(),
    }
}
pub fn run(manual: bool, report: UpdateReporter, cx: &mut App) -> Task<()> {
    let Some(operation) = begin(cx) else {
        return Task::ready(());
    };
    let flow = flow(cx);
    cx.spawn(async move |cx| {
        let _operation = operation;
        flow.run_update_flow(manual, |value| cx.update(|cx| publish(value, &report, cx)))
            .await;
    })
}
pub fn install(report: UpdateReporter, cx: &mut App) -> Task<()> {
    let Some(operation) = begin(cx) else {
        return Task::ready(());
    };
    let flow = flow(cx);
    cx.spawn(async move |cx| {
        let _operation = operation;
        flow.install_pending_update(|value| cx.update(|cx| publish(value, &report, cx)))
            .await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn overlapping_update_actions_keep_the_first_operation_until_it_finishes(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            let operation = begin(cx).unwrap();
            assert!(begin(cx).is_none());
            assert!(begin(cx).is_none());
            drop(operation);
            let next = begin(cx).unwrap();
            drop(next);
            assert!(begin(cx).is_some());
        });
    }

    #[gpui::test]
    fn progress_updates_reach_the_shared_rail_state_and_settings_callback(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let seen = Rc::new(RefCell::new(Vec::new()));
            let captured = seen.clone();
            let report: UpdateReporter =
                Rc::new(move |snapshot, _| captured.borrow_mut().push(snapshot));
            let value = monocode_updater::flow::UpdaterSnapshot {
                phase: UpdaterPhase::Downloading,
                current_version: "0.6.0".into(),
                available_version: Some("0.6.1".into()),
                progress: Some(37),
                error: None,
            };
            publish(&value, &report, cx);
            assert_eq!(state(cx).read(cx).snapshot, seen.borrow()[0]);
            assert_eq!(state(cx).read(cx).snapshot.progress, Some(37));
        });
    }
}
