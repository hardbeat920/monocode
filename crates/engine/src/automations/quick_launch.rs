//! The `QuickLaunch` entity: launches the floating quick composer hands to a
//! workspace window. Ports the queue and window choice of src-tauri
//! (`quick_composer_submit`, `quick_composer_take`, `quick_composer_ack`,
//! and quick_composer/delivery.rs) and the window side,
//! useQuickComposerLaunches.ts.
//!
//! The panel calls `submit`. The launch waits in the queue, owned by the
//! chosen window, until that window's receiver accepted it into the
//! workspace and acknowledged it. A window that closes hands its launches
//! to the next window that asks.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::rc::Rc;

use gpui::{App, Context, EventEmitter, Task, WeakEntity};
use monocode_core::harness::harness_supports_attachments;
use monocode_core::js;
use monocode_core::session::WorkspaceMode;
use monocode_core::{AttachmentKind, Platform};
use monocode_settings::Kv;
use monocode_settings::settings_store::{
    load_quick_composer_enabled, load_quick_composer_shortcut,
};
use serde_json::{Value, json};

use super::host::{LaunchHost, SessionPlacement};
use super::launch_delivery::{
    Accepted, Accepting, Delivery, LaunchError, LaunchReceiver, ReceiverOptions,
};
use super::quick_composer::{QuickLaunchRequest, quick_composer_supported};
use super::quick_launch_session::accept_quick_launch;

/// `MAX_PROMPT_BYTES`.
pub const MAX_PROMPT_BYTES: usize = 256 * 1024;
/// The queue holds at most this many launches.
pub const QUEUE_CAPACITY: usize = 64;

/// What the quick composer needs from the app: the global shortcut, the
/// panel, and new windows.
pub trait QuickLaunchApp {
    /// `quick_composer_set_enabled`: claim or release the global shortcut.
    fn set_shortcut(&self, _enabled: bool, _shortcut: &str, _cx: &mut App) -> Result<(), String> {
        Ok(())
    }

    /// `open_session_window`: no window can take a launch, so make one and
    /// return its label.
    fn open_session_window(&self, _reveal: bool, _cx: &mut App) -> Result<String, String> {
        Err("No workspace window is open.".into())
    }

    /// Hide the panel and its git popup after a submit.
    fn hide_panel(&self, _cx: &mut App) {}
}

/// The default `QuickLaunchApp`.
pub struct NoQuickLaunchApp;

impl QuickLaunchApp for NoQuickLaunchApp {}

/// One queued launch.
#[derive(Debug, Clone, PartialEq)]
pub struct QueuedLaunch {
    pub id: String,
    pub request: QuickLaunchRequest,
}

impl QueuedLaunch {
    /// The envelope `quick_composer_take` returned.
    pub fn envelope(&self) -> Value {
        json!({ "id": self.id, "request": self.request })
    }
}

struct Pending {
    delivery: QueuedLaunch,
    owner: String,
}

/// `LaunchQueue` from src-tauri.
#[derive(Default)]
pub struct LaunchQueue(VecDeque<Pending>);

impl LaunchQueue {
    pub fn has_capacity(&self) -> bool {
        self.0.len() < QUEUE_CAPACITY
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn push(&mut self, request: QuickLaunchRequest, owner: String) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.0.push_back(Pending {
            delivery: QueuedLaunch {
                id: id.clone(),
                request,
            },
            owner,
        });
        id
    }

    /// Claims repeat until acknowledged. Only the owner may take a launch;
    /// a window that no longer exists is replaced by the one asking.
    pub fn claim(&mut self, window: &str, exists: impl Fn(&str) -> bool) -> Option<QueuedLaunch> {
        let next = self
            .0
            .iter_mut()
            .find(|entry| entry.owner == window || !exists(&entry.owner))?;
        next.owner = window.to_string();
        Some(next.delivery.clone())
    }

    pub fn acknowledge(&mut self, window: &str, id: &str) {
        self.0
            .retain(|entry| !(entry.owner == window && entry.delivery.id == id));
    }

    /// The window that owns a launch.
    pub fn owner_of(&self, id: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|entry| entry.delivery.id == id)
            .map(|entry| entry.owner.as_str())
    }
}

/// What happened, for the app and the views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickLaunchEvent {
    /// `quick_composer_launch`: this window has a launch to take.
    Launch { owner: String },
    /// A delivery failed and stays queued.
    DeliveryFailed { owner: String, error: String },
}

struct Window {
    host: Rc<dyn LaunchHost>,
    receiver: LaunchReceiver,
    disposed: Rc<Cell<bool>>,
}

/// The receipts of one window label. They outlive a remount, so a failed
/// acknowledgement never starts a second session.
#[derive(Clone, Default)]
struct Receipts {
    accepted: Accepted,
    accepting: Accepting,
}

/// The quick composer's launches.
pub struct QuickLaunch {
    kv: Kv,
    platform: Platform,
    app: Rc<dyn QuickLaunchApp>,
    queue: LaunchQueue,
    windows: BTreeMap<String, Window>,
    receipts: HashMap<String, Receipts>,
}

impl EventEmitter<QuickLaunchEvent> for QuickLaunch {}

impl QuickLaunch {
    pub fn new(kv: Kv, platform: Platform, _cx: &mut Context<Self>) -> Self {
        Self {
            kv,
            platform,
            app: Rc::new(NoQuickLaunchApp),
            queue: LaunchQueue::default(),
            windows: BTreeMap::new(),
            receipts: HashMap::new(),
        }
    }

    /// The app's shortcut, panel, and window calls.
    pub fn set_app(&mut self, app: Rc<dyn QuickLaunchApp>) {
        self.app = app;
    }

    /// Launches waiting for a window.
    pub fn queue(&self) -> &LaunchQueue {
        &self.queue
    }

    /// The panel's platform check.
    pub fn supported(&self) -> bool {
        quick_composer_supported(self.platform)
    }

    /// `setQuickComposerShortcut(loadQuickComposerEnabled())`: claim the
    /// global shortcut per the setting. Call at start and when the setting
    /// changes.
    pub fn apply_shortcut(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        if !self.supported() {
            return Ok(());
        }
        let enabled = load_quick_composer_enabled(&self.kv);
        let shortcut = load_quick_composer_shortcut(&self.kv);
        self.app.set_shortcut(enabled, &shortcut, cx)
    }

    // The panel side.

    /// `quick_composer_submit`: check the launch, queue it for the window
    /// the user last looked at (or a new one), hide the panel, and tell the
    /// window.
    pub fn submit(
        &mut self,
        request: QuickLaunchRequest,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let attachments = request.attachments.as_deref().unwrap_or(&[]);
        if js::trim(&request.prompt).is_empty() && attachments.is_empty() {
            return Err("Write a prompt first.".into());
        }
        if request.prompt.len() > MAX_PROMPT_BYTES {
            return Err("That prompt is too long for the quick composer.".into());
        }
        if js::trim(&request.cwd).is_empty() {
            return Err("Pick a project first.".into());
        }
        validate_workspace(&request)?;
        validate_attachments(&request)?;
        if !self.queue.has_capacity() {
            return Err("Too many sessions are waiting to start. Please try again shortly.".into());
        }
        let target = match self.launch_target(cx) {
            Some(target) => target,
            None => self.app.open_session_window(request.reveal, cx)?,
        };
        let reveal = request.reveal;
        let id = self.queue.push(request, target.clone());
        self.app.hide_panel(cx);
        if reveal && let Some(window) = self.windows.get(&target) {
            window.host.bring_forward(cx);
        }
        cx.emit(QuickLaunchEvent::Launch {
            owner: target.clone(),
        });
        self.receive(&target, cx);
        Ok(id)
    }

    /// `launch_target`: the focused window, else a visible one, else the
    /// first.
    fn launch_target(&self, cx: &App) -> Option<String> {
        self.windows
            .iter()
            .find(|(_, window)| window.host.is_focused(cx))
            .or_else(|| {
                self.windows
                    .iter()
                    .find(|(_, window)| window.host.is_visible(cx))
            })
            .or_else(|| self.windows.iter().next())
            .map(|(label, _)| label.clone())
    }

    /// `quick_composer_take`.
    pub fn take(&mut self, window: &str) -> Option<QueuedLaunch> {
        let windows = &self.windows;
        self.queue
            .claim(window, |label| windows.contains_key(label))
    }

    /// `quick_composer_ack`.
    pub fn acknowledge(&mut self, window: &str, id: &str) {
        self.queue.acknowledge(window, id);
    }

    // The window side.

    /// `useQuickComposerLaunches` mounting: start this window's receiver
    /// and drain whatever is waiting for it.
    pub fn attach_window(&mut self, label: &str, host: Rc<dyn LaunchHost>, cx: &mut Context<Self>) {
        self.detach_window(label);
        let receipts = self.receipts.entry(label.to_string()).or_default().clone();
        let disposed = Rc::new(Cell::new(false));
        let this = cx.entity().downgrade();
        let receiver = LaunchReceiver::new(ReceiverOptions {
            take: take_fn(this.clone(), label),
            accept: accept_fn(host.clone()),
            ack: ack_fn(this, label),
            disposed: {
                let disposed = disposed.clone();
                Rc::new(move || disposed.get())
            },
            accepted: receipts.accepted,
            accepting: receipts.accepting,
        });
        self.windows.insert(
            label.to_string(),
            Window {
                host,
                receiver,
                disposed,
            },
        );
        self.receive(label, cx);
    }

    /// The window closed or remounts: stop its receiver. Its launches stay
    /// queued for the next window that asks.
    pub fn detach_window(&mut self, label: &str) {
        if let Some(window) = self.windows.remove(label) {
            window.disposed.set(true);
            window.receiver.dispose();
        }
    }

    /// The window gained focus: drain its queue.
    pub fn window_focused(&mut self, label: &str, cx: &mut Context<Self>) {
        self.receive(label, cx);
    }

    /// Drain one window's queue. Failures stay queued; transient ones retry
    /// on their own.
    pub fn receive(&mut self, label: &str, cx: &mut Context<Self>) -> Option<Delivery> {
        let receiver = self.windows.get(label)?.receiver.clone();
        let delivery = receiver.receive(cx);
        let watch = delivery.clone();
        let owner = label.to_string();
        cx.spawn(async move |this, cx| {
            if let Err(error) = watch.await {
                log::warn!("Quick session delivery failed; session remains queued: {error}");
                this.update(cx, |_, cx| {
                    cx.emit(QuickLaunchEvent::DeliveryFailed {
                        owner,
                        error: error.message().to_string(),
                    });
                })
                .ok();
            }
        })
        .detach();
        Some(delivery)
    }

    /// `launchQuickSession`: accept a launch into a window's workspace
    /// directly, as the app API does, optionally split beside a pane.
    pub fn accept(
        &mut self,
        label: &str,
        launch: QuickLaunchRequest,
        id: String,
        placement: Option<SessionPlacement>,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), LaunchError>> {
        let Some(host) = self.windows.get(label).map(|window| window.host.clone()) else {
            return Task::ready(Err(LaunchError::Failed(
                "The window is no longer open.".into(),
            )));
        };
        cx.spawn(async move |_, cx| accept_quick_launch(launch, id, host, placement, cx).await)
    }

    /// The receiver of a window, for tests and diagnostics.
    pub fn receiver(&self, label: &str) -> Option<&LaunchReceiver> {
        self.windows.get(label).map(|window| &window.receiver)
    }
}

fn take_fn(this: WeakEntity<QuickLaunch>, label: &str) -> super::launch_delivery::TakeFn {
    let label = label.to_string();
    Rc::new(move |cx: &mut App| {
        let taken = this
            .update(cx, |this, _| {
                this.take(&label).map(|launch| launch.envelope())
            })
            .map_err(|error| LaunchError::Failed(error.to_string()));
        Task::ready(taken)
    })
}

fn ack_fn(this: WeakEntity<QuickLaunch>, label: &str) -> super::launch_delivery::AckFn {
    let label = label.to_string();
    Rc::new(move |id: String, cx: &mut App| {
        let acked = this
            .update(cx, |this, _| this.acknowledge(&label, &id))
            .map_err(|error| LaunchError::Failed(error.to_string()));
        Task::ready(acked)
    })
}

fn accept_fn(host: Rc<dyn LaunchHost>) -> super::launch_delivery::AcceptFn {
    Rc::new(move |launch, id, cx: &mut App| {
        let host = host.clone();
        cx.spawn(async move |cx| accept_quick_launch(launch, id, host, None, cx).await)
    })
}

/// `validate_workspace` from src-tauri.
fn validate_workspace(request: &QuickLaunchRequest) -> Result<(), String> {
    let worktree = request.workspace_mode == Some(WorkspaceMode::Worktree);
    if request
        .worktree_base
        .as_deref()
        .is_some_and(|base| js::trim(base).is_empty() || !worktree)
        || (worktree && request.worktree_cwd.is_some())
    {
        return Err("Select a valid workspace for this session.".into());
    }
    if let Some(path) = &request.worktree_cwd
        && (js::trim(path).is_empty() || !std::path::Path::new(path).is_dir())
    {
        return Err("This worktree is no longer available. Select another working copy.".into());
    }
    Ok(())
}

/// `validate_attachments` from src-tauri: only files on disk cross windows.
fn validate_attachments(request: &QuickLaunchRequest) -> Result<(), String> {
    let files = request.attachments.as_deref().unwrap_or(&[]);
    if files.len() > 20 {
        return Err("You can attach up to 20 files.".into());
    }
    if !files.is_empty() && !harness_supports_attachments(request.harness) {
        return Err("This provider does not support attachments.".into());
    }
    for file in files {
        let on_disk = file
            .path
            .as_deref()
            .is_some_and(|path| std::path::Path::new(path).is_file());
        if file.id.is_empty()
            || file.name.is_empty()
            || file.mime_type.is_empty()
            || !matches!(
                file.kind,
                AttachmentKind::Image | AttachmentKind::Audio | AttachmentKind::File
            )
            || !on_disk
        {
            return Err(format!("Could not read attachment: {}", file.name));
        }
    }
    Ok(())
}
