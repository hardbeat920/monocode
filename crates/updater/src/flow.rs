//! Port of src/app/model/updater.ts: the check, ask, install, and relaunch
//! flow behind "Check for Updates" and the Settings update row.
//!
//! The TypeScript called Tauri plugins and dialogs directly. Here those calls
//! go through `UpdaterHost`, which the app implements: `check` and
//! `download_and_install` wrap `Updater::check_async` and
//! `Update::download_and_install_async`, and the dialog, sound, marker, and
//! relaunch methods use the app's own UI. The module-level `pendingUpdate`
//! becomes the `UpdaterFlow` value, which the app keeps for the session.

use std::future::Future;
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::updater::{DownloadEvent, Update};

/// `UpdaterPhase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdaterPhase {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "checking")]
    Checking,
    #[serde(rename = "current")]
    Current,
    #[serde(rename = "available")]
    Available,
    #[serde(rename = "downloading")]
    Downloading,
    #[serde(rename = "error")]
    Error,
}

/// `UpdaterSnapshot`: what the update UI shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterSnapshot {
    pub phase: UpdaterPhase,
    pub current_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_version: Option<String>,
    /// Download percent, 0 to 100, when the size is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl UpdaterSnapshot {
    fn new(phase: UpdaterPhase, current_version: &str) -> Self {
        Self {
            phase,
            current_version: current_version.to_string(),
            available_version: None,
            progress: None,
            error: None,
        }
    }
}

/// The dialog `kind` the TypeScript passed to `ask`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Info,
    Warning,
    Error,
}

/// Dialog options: `{ title }` or `{ title, kind }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogOptions {
    pub title: &'static str,
    pub kind: Option<DialogKind>,
}

const MONOCODE_DIALOG: DialogOptions = DialogOptions {
    title: "MonoCode",
    kind: None,
};

const UPDATE_AVAILABLE_DIALOG: DialogOptions = DialogOptions {
    title: "Update available",
    kind: Some(DialogKind::Info),
};

/// Where releases live when this build has no updater feed.
pub const RELEASES_URL: &str = "https://github.com/hardbeat920/monocode/releases/latest";

/// What the flow needs from the app.
pub trait UpdaterHost {
    /// `getVersion()`: the running app's version.
    fn app_version(&self) -> String;

    /// `check()` from the updater plugin. Usually
    /// `Updater::new(UpdaterConfig::from_build_env(version))?.check_async()`.
    fn check(&self) -> impl Future<Output = Result<Option<Update>>>;

    /// `update.downloadAndInstall(onEvent)`. Usually
    /// `update.download_and_install_async(on_event)`.
    fn download_and_install(
        &self,
        update: &Update,
        on_event: &mut dyn FnMut(DownloadEvent),
    ) -> impl Future<Output = Result<()>>;

    /// `message(text, options)`: an alert with one button.
    fn message(&self, text: &str, options: DialogOptions) -> impl Future<Output = ()>;

    /// `ask(text, options)`: a yes or no question. `true` means yes.
    fn ask(&self, text: &str, options: DialogOptions) -> impl Future<Output = bool>;

    /// `announceUpdateAvailable(version)` from the sounds model.
    fn announce_update_available(&self, version: &str);

    /// `rememberInstalledUpdate(version)`, so the next launch shows the
    /// release notes. `update_notice::remember_installed_update` with the
    /// app's `Kv` does it.
    fn remember_installed_update(&self, version: &str);

    /// `relaunch()`. Usually `relaunch::relaunch()` and then quitting.
    fn relaunch(&self) -> impl Future<Output = Result<()>>;
}

/// The flow and its pending update. Methods take `&self`, so the app can
/// share one value between tasks.
pub struct UpdaterFlow<H> {
    host: H,
    pending_update: Mutex<Option<Update>>,
}

impl<H: UpdaterHost> UpdaterFlow<H> {
    pub fn new(host: H) -> Self {
        Self {
            host,
            pending_update: Mutex::new(None),
        }
    }

    pub fn host(&self) -> &H {
        &self.host
    }

    /// The update the last check found, if it has not been installed.
    pub fn pending_update(&self) -> Option<Update> {
        self.pending().clone()
    }

    fn pending(&self) -> MutexGuard<'_, Option<Update>> {
        self.pending_update
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn set_pending(&self, update: Option<Update>) {
        *self.pending() = update;
    }

    /// `probeForUpdate`: a quiet check that remembers what it found.
    pub async fn probe_for_update(&self) -> Result<Option<Update>> {
        let update = self.host.check().await?;
        self.set_pending(update.clone());
        if let Some(update) = &update {
            self.host.announce_update_available(&update.version);
        }
        Ok(update)
    }

    /// `runUpdateFlow`. A manual run shows the result, asks before
    /// installing, and installs on yes. An automatic run only reports.
    pub async fn run_update_flow(
        &self,
        manual: bool,
        mut on_progress: impl FnMut(&UpdaterSnapshot),
    ) -> UpdaterSnapshot {
        let current_version = self.host.app_version();
        let base = UpdaterSnapshot::new(UpdaterPhase::Checking, &current_version);
        on_progress(&base);

        let update = match self.host.check().await {
            Ok(update) => update,
            Err(err) => {
                return self
                    .check_failed(manual, &current_version, err, on_progress)
                    .await;
            }
        };

        let Some(update) = update else {
            self.set_pending(None);
            let current = UpdaterSnapshot::new(UpdaterPhase::Current, &current_version);
            on_progress(&current);
            if manual {
                self.host
                    .message("You're on the latest version.", MONOCODE_DIALOG)
                    .await;
            }
            return current;
        };

        self.set_pending(Some(update.clone()));
        self.host.announce_update_available(&update.version);
        let available = UpdaterSnapshot {
            available_version: Some(update.version.clone()),
            ..UpdaterSnapshot::new(UpdaterPhase::Available, &current_version)
        };
        on_progress(&available);

        if !manual {
            return available;
        }

        let notes = update.body.as_deref().map(str::trim).unwrap_or_default();
        let detail = if notes.is_empty() {
            String::new()
        } else {
            format!("\n\n{notes}")
        };
        let yes = self
            .host
            .ask(
                &format!(
                    "MonoCode {} is available (you have {current_version}).{detail}\n\nInstall now?",
                    update.version
                ),
                UPDATE_AVAILABLE_DIALOG,
            )
            .await;
        if !yes {
            return available;
        }

        self.install_pending_update(on_progress).await
    }

    /// The `catch` branch of `runUpdateFlow`.
    async fn check_failed(
        &self,
        manual: bool,
        current_version: &str,
        err: Error,
        mut on_progress: impl FnMut(&UpdaterSnapshot),
    ) -> UpdaterSnapshot {
        let error = err.to_string();
        if is_updater_not_configured_error(&error) {
            self.set_pending(None);
            let idle = UpdaterSnapshot::new(UpdaterPhase::Idle, current_version);
            on_progress(&idle);
            if manual {
                self.host
                    .message(
                        &format!(
                            "Automatic updates aren't configured for this build.\n\nDownload releases at {RELEASES_URL}"
                        ),
                        MONOCODE_DIALOG,
                    )
                    .await;
            }
            return idle;
        }

        let failed = UpdaterSnapshot {
            error: Some(error.clone()),
            ..UpdaterSnapshot::new(UpdaterPhase::Error, current_version)
        };
        on_progress(&failed);
        if manual {
            self.host
                .message(
                    &format!("Couldn't check for updates.\n\n{error}"),
                    MONOCODE_DIALOG,
                )
                .await;
        }
        failed
    }

    /// `installPendingUpdate`: download with progress, install, record the
    /// version for the next launch's release notes, and relaunch.
    pub async fn install_pending_update(
        &self,
        mut on_progress: impl FnMut(&UpdaterSnapshot),
    ) -> UpdaterSnapshot {
        let current_version = self.host.app_version();
        let Some(update) = self.pending_update() else {
            let idle = UpdaterSnapshot::new(UpdaterPhase::Idle, &current_version);
            on_progress(&idle);
            return idle;
        };

        let downloading = |progress: Option<i64>| UpdaterSnapshot {
            available_version: Some(update.version.clone()),
            progress,
            ..UpdaterSnapshot::new(UpdaterPhase::Downloading, &current_version)
        };
        on_progress(&downloading(Some(0)));

        let mut downloaded: u64 = 0;
        let mut content_length: u64 = 0;
        let installed = {
            let mut on_event = |event: DownloadEvent| {
                match event {
                    DownloadEvent::Started {
                        content_length: length,
                    } => {
                        content_length = length.unwrap_or(0);
                        downloaded = 0;
                    }
                    DownloadEvent::Progress { chunk_length } => {
                        downloaded += chunk_length as u64;
                    }
                    DownloadEvent::Finished => {}
                }
                let progress = (content_length > 0).then(|| {
                    let percent = (downloaded as f64 / content_length as f64 * 100.0).round();
                    (percent as i64).min(100)
                });
                on_progress(&downloading(progress));
            };
            self.host.download_and_install(&update, &mut on_event).await
        };

        let result = match installed {
            Ok(()) => {
                self.host.remember_installed_update(&update.version);
                self.set_pending(None);
                self.host.relaunch().await
            }
            Err(err) => Err(err),
        };

        match result {
            Ok(()) => UpdaterSnapshot::new(UpdaterPhase::Current, &update.version),
            Err(err) => {
                let error = err.to_string();
                let failed = UpdaterSnapshot {
                    available_version: Some(update.version.clone()),
                    error: Some(error.clone()),
                    ..UpdaterSnapshot::new(UpdaterPhase::Error, &current_version)
                };
                on_progress(&failed);
                self.host
                    .message(
                        &format!("Couldn't install the update.\n\n{error}"),
                        MONOCODE_DIALOG,
                    )
                    .await;
                failed
            }
        }
    }
}

/// `isUpdaterNotConfiguredError`.
fn is_updater_not_configured_error(text: &str) -> bool {
    text.to_lowercase()
        .contains("updater does not have any endpoints set")
}

#[cfg(test)]
mod tests {
    //! Port of src/app/model/updater.test.ts and updaterConfig.test.ts.

    use std::cell::RefCell;
    use std::collections::VecDeque;

    use futures::executor::block_on;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Announce(String),
        Message(String, DialogOptions),
        Ask(String),
        Remember(String),
        Relaunch,
        DownloadAndInstall(String),
    }

    /// The vi.mock stand-ins: scripted results and a call log in order.
    struct MockHost {
        version: String,
        checks: RefCell<VecDeque<Result<Option<Update>>>>,
        install_result: RefCell<Option<Result<()>>>,
        install_events: Vec<DownloadEvent>,
        answer: bool,
        calls: RefCell<Vec<Call>>,
    }

    impl MockHost {
        fn new(version: &str) -> Self {
            Self {
                version: version.into(),
                checks: RefCell::new(VecDeque::new()),
                install_result: RefCell::new(None),
                install_events: Vec::new(),
                answer: false,
                calls: RefCell::new(Vec::new()),
            }
        }

        fn checking(self, result: Result<Option<Update>>) -> Self {
            self.checks.borrow_mut().push_back(result);
            self
        }

        fn installing(self, result: Result<()>) -> Self {
            *self.install_result.borrow_mut() = Some(result);
            self
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.borrow().clone()
        }

        fn count(&self, matches: impl Fn(&Call) -> bool) -> usize {
            self.calls
                .borrow()
                .iter()
                .filter(|call| matches(call))
                .count()
        }
    }

    impl UpdaterHost for MockHost {
        fn app_version(&self) -> String {
            self.version.clone()
        }

        async fn check(&self) -> Result<Option<Update>> {
            self.checks
                .borrow_mut()
                .pop_front()
                .expect("an unexpected check")
        }

        async fn download_and_install(
            &self,
            update: &Update,
            on_event: &mut dyn FnMut(DownloadEvent),
        ) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(Call::DownloadAndInstall(update.version.clone()));
            for event in &self.install_events {
                on_event(*event);
            }
            self.install_result.borrow_mut().take().unwrap_or(Ok(()))
        }

        async fn message(&self, text: &str, options: DialogOptions) {
            self.calls
                .borrow_mut()
                .push(Call::Message(text.into(), options));
        }

        async fn ask(&self, text: &str, _options: DialogOptions) -> bool {
            self.calls.borrow_mut().push(Call::Ask(text.into()));
            self.answer
        }

        fn announce_update_available(&self, version: &str) {
            self.calls.borrow_mut().push(Call::Announce(version.into()));
        }

        fn remember_installed_update(&self, version: &str) {
            self.calls.borrow_mut().push(Call::Remember(version.into()));
        }

        async fn relaunch(&self) -> Result<()> {
            self.calls.borrow_mut().push(Call::Relaunch);
            Ok(())
        }
    }

    fn no_progress(_: &UpdaterSnapshot) {}

    /// `updaterWithPendingUpdate`.
    fn flow_with_pending_update(install: Result<()>) -> UpdaterFlow<MockHost> {
        let host = MockHost::new("0.1.22")
            .checking(Ok(Some(Update::for_test("0.1.23", None))))
            .installing(install);
        let flow = UpdaterFlow::new(host);
        block_on(flow.probe_for_update()).unwrap();
        flow
    }

    // installPendingUpdate

    #[test]
    fn records_a_successful_installation_before_relaunching() {
        let flow = flow_with_pending_update(Ok(()));

        block_on(flow.install_pending_update(no_progress));

        let calls = flow.host().calls();
        let remember = calls
            .iter()
            .position(|call| *call == Call::Remember("0.1.23".into()))
            .expect("remembered 0.1.23");
        let relaunch = calls
            .iter()
            .position(|call| *call == Call::Relaunch)
            .expect("relaunched");
        assert_eq!(flow.host().count(|call| *call == Call::Relaunch), 1);
        assert!(remember < relaunch);
    }

    #[test]
    fn does_not_record_or_relaunch_after_installation_fails() {
        let flow = flow_with_pending_update(Err(Error::Network("install failed".into())));

        let result = block_on(flow.install_pending_update(no_progress));

        assert_eq!(result.phase, UpdaterPhase::Error);
        assert_eq!(
            flow.host()
                .count(|call| matches!(call, Call::Remember(_) | Call::Relaunch)),
            0
        );
    }

    #[test]
    fn does_not_record_when_no_update_is_pending() {
        let flow = UpdaterFlow::new(MockHost::new("0.1.22"));

        assert_eq!(
            block_on(flow.install_pending_update(no_progress)).phase,
            UpdaterPhase::Idle
        );
        assert_eq!(
            flow.host()
                .count(|call| matches!(call, Call::Remember(_) | Call::Relaunch)),
            0
        );
    }

    // updater (updaterConfig.test.ts)

    #[test]
    fn keeps_automatic_checks_quiet_when_updater_endpoints_are_missing() {
        let flow = UpdaterFlow::new(MockHost::new("0.1.23").checking(Err(Error::EmptyEndpoints)));

        let result = block_on(flow.run_update_flow(false, no_progress));

        assert_eq!(
            result,
            UpdaterSnapshot::new(UpdaterPhase::Idle, "0.1.23"),
            "phase idle with the current version and nothing else"
        );
        assert_eq!(
            flow.host().count(|call| matches!(call, Call::Message(..))),
            0
        );
    }

    #[test]
    fn points_manual_checks_without_updater_endpoints_to_github_releases() {
        let flow = UpdaterFlow::new(MockHost::new("0.1.23").checking(Err(Error::EmptyEndpoints)));

        let result = block_on(flow.run_update_flow(true, no_progress));

        assert_eq!(result, UpdaterSnapshot::new(UpdaterPhase::Idle, "0.1.23"));
        let calls = flow.host().calls();
        let [Call::Message(text, options)] = calls.as_slice() else {
            panic!("expected one message, got {calls:?}");
        };
        assert!(text.contains("https://github.com/hardbeat920/monocode/releases/latest"));
        assert_eq!(*options, MONOCODE_DIALOG);
    }

    #[test]
    fn still_reports_real_updater_failures() {
        let flow = UpdaterFlow::new(
            MockHost::new("0.1.23").checking(Err(Error::Http("network failed".into()))),
        );

        let result = block_on(flow.run_update_flow(true, no_progress));

        assert_eq!(result.phase, UpdaterPhase::Error);
        assert_eq!(result.error.as_deref(), Some("network failed"));
        assert_eq!(
            flow.host().count(|call| matches!(call, Call::Message(..))),
            1
        );
    }

    // Behavior the TypeScript had without a test.

    #[test]
    fn manual_check_on_the_latest_version_says_so() {
        let flow = UpdaterFlow::new(MockHost::new("0.1.23").checking(Ok(None)));

        let result = block_on(flow.run_update_flow(true, no_progress));

        assert_eq!(result.phase, UpdaterPhase::Current);
        assert_eq!(
            flow.host().calls(),
            [Call::Message(
                "You're on the latest version.".into(),
                MONOCODE_DIALOG
            )]
        );
    }

    #[test]
    fn automatic_check_announces_and_reports_without_asking() {
        let flow = UpdaterFlow::new(
            MockHost::new("0.1.22").checking(Ok(Some(Update::for_test("0.1.23", None)))),
        );
        let mut seen = Vec::new();

        let result = block_on(flow.run_update_flow(false, |snapshot| seen.push(snapshot.phase)));

        assert_eq!(result.phase, UpdaterPhase::Available);
        assert_eq!(result.available_version.as_deref(), Some("0.1.23"));
        assert_eq!(seen, [UpdaterPhase::Checking, UpdaterPhase::Available]);
        assert_eq!(flow.host().calls(), [Call::Announce("0.1.23".into())]);
        assert!(flow.pending_update().is_some());
    }

    #[test]
    fn manual_check_asks_with_the_notes_and_installs_on_yes() {
        let mut host = MockHost::new("0.1.22")
            .checking(Ok(Some(Update::for_test("0.1.23", Some("  Fixes.  ")))));
        host.answer = true;
        host.install_events = vec![
            DownloadEvent::Started {
                content_length: Some(200),
            },
            DownloadEvent::Progress { chunk_length: 50 },
            DownloadEvent::Progress { chunk_length: 150 },
            DownloadEvent::Finished,
        ];
        let flow = UpdaterFlow::new(host);
        let mut progress = Vec::new();

        let result = block_on(flow.run_update_flow(true, |snapshot| {
            if snapshot.phase == UpdaterPhase::Downloading {
                progress.push(snapshot.progress);
            }
        }));

        assert_eq!(
            result,
            UpdaterSnapshot::new(UpdaterPhase::Current, "0.1.23")
        );
        assert_eq!(progress, [Some(0), Some(0), Some(25), Some(100), Some(100)]);
        let calls = flow.host().calls();
        assert_eq!(
            calls[1],
            Call::Ask(
                "MonoCode 0.1.23 is available (you have 0.1.22).\n\nFixes.\n\nInstall now?".into()
            )
        );
        assert!(flow.pending_update().is_none());
    }

    #[test]
    fn manual_check_keeps_the_update_pending_on_no() {
        let flow = UpdaterFlow::new(
            MockHost::new("0.1.22").checking(Ok(Some(Update::for_test("0.1.23", None)))),
        );

        let result = block_on(flow.run_update_flow(true, no_progress));

        assert_eq!(result.phase, UpdaterPhase::Available);
        assert_eq!(
            flow.host().calls()[1],
            Call::Ask("MonoCode 0.1.23 is available (you have 0.1.22).\n\nInstall now?".into())
        );
        assert_eq!(
            flow.host()
                .count(|call| matches!(call, Call::DownloadAndInstall(_))),
            0
        );
        assert!(flow.pending_update().is_some());
    }

    #[test]
    fn unknown_download_size_reports_no_percent() {
        let mut host = MockHost::new("0.1.22").checking(Ok(Some(Update::for_test("0.1.23", None))));
        host.install_events = vec![
            DownloadEvent::Started {
                content_length: None,
            },
            DownloadEvent::Progress { chunk_length: 10 },
        ];
        let flow = UpdaterFlow::new(host);
        block_on(flow.probe_for_update()).unwrap();
        let mut progress = Vec::new();

        block_on(flow.install_pending_update(|snapshot| progress.push(snapshot.progress)));

        assert_eq!(progress, [Some(0), None, None]);
    }

    #[test]
    fn snapshot_serializes_like_the_typescript() {
        let snapshot = UpdaterSnapshot {
            available_version: Some("0.1.23".into()),
            progress: Some(40),
            ..UpdaterSnapshot::new(UpdaterPhase::Downloading, "0.1.22")
        };
        assert_eq!(
            serde_json::to_string(&snapshot).unwrap(),
            r#"{"phase":"downloading","currentVersion":"0.1.22","availableVersion":"0.1.23","progress":40}"#
        );
    }
}
