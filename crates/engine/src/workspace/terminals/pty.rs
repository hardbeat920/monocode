//! Engine terminals: the PTY host behind every terminal file, and the
//! adapter that lets `monocode_terminal_view::TerminalView` attach to one.
//!
//! Ports the PTY side of src/platform/tauri/pty.ts (the output routing and
//! the replay buffer) and the parts of
//! src/features/terminal/ui/TerminalView.tsx that are not drawing: spawning
//! on attach, queueing input until the shell starts, the OSC 7 cwd scan,
//! and the once-a-second foreground title poll.
//!
//! A terminal is keyed by its `FilePaneTab` id, as the TypeScript keyed the
//! PTY. It runs until the workspace closes its file, so a view can detach
//! and attach again; output that arrives with no view attached is kept, up
//! to 256 KB, and replayed to the next view.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use gpui::{App, AppContext, Entity, EventEmitter, Global, Task};
use monocode_layout::terminal_tab::{TerminalMetaPatch, default_terminal_title, scan_osc_cwd};
use monocode_terminal::pty::{PtyEvents, PtyHost};
use monocode_terminal_view::{Pty, PtyEvent, PtySize};
use parking_lot::Mutex;

/// Replay budget for a terminal with no view attached. Chunks arrive at up
/// to 32 KB each, so the cap counts bytes and drops whole chunks oldest
/// first.
pub const MAX_BUFFERED_BYTES: usize = 256 * 1024;
/// At most this many chunks wait for a view.
pub const MAX_BUFFERED: usize = 200;
/// How often the foreground process poll runs.
pub const STATUS_POLL: Duration = Duration::from_secs(1);
/// The grid a terminal starts with, before its view measures itself. The
/// terminal view also starts at 80 by 24.
pub const INITIAL_SIZE: (u16, u16) = (80, 24);

/// `trimReplay`: how many leading chunks to drop to bring a replay buffer
/// back within budget, and the byte total that remains. Never drops the
/// newest chunk, even when it alone is over budget.
pub fn trim_replay(sizes: &[usize], bytes: usize) -> (usize, usize) {
    let mut drop = 0;
    let mut left = bytes;
    while drop + 1 < sizes.len() && (left > MAX_BUFFERED_BYTES || sizes.len() - drop > MAX_BUFFERED)
    {
        left -= sizes[drop];
        drop += 1;
    }
    (drop, left)
}

/// The PTY calls the engine makes. `HostPty` runs them on
/// `monocode_terminal::pty`; tests use a fake.
pub trait PtyBackend: Send + Sync {
    fn spawn(&self, id: &str, cwd: &str, cols: u16, rows: u16) -> Result<(), String>;
    fn write(&self, id: &str, data: &[u8]) -> Result<(), String>;
    fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String>;
    /// The foreground process when it is not the shell.
    fn status(&self, id: &str) -> Result<Option<String>, String>;
    fn kill(&self, id: &str) -> Result<(), String>;
    fn kill_all(&self) -> Result<(), String>;
}

/// The real PTYs.
pub struct HostPty(pub PtyHost);

impl PtyBackend for HostPty {
    fn spawn(&self, id: &str, cwd: &str, cols: u16, rows: u16) -> Result<(), String> {
        monocode_terminal::pty::pty_spawn(&self.0, id.into(), cwd.into(), cols, rows)
    }

    fn write(&self, id: &str, data: &[u8]) -> Result<(), String> {
        // TODO(port): pty_write takes a String, so bytes that are not UTF-8
        // (X10 mouse reports past column 95) are replaced.
        monocode_terminal::pty::pty_write(
            &self.0,
            id.into(),
            String::from_utf8_lossy(data).into_owned(),
        )
    }

    fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        monocode_terminal::pty::pty_resize(&self.0, id.into(), cols, rows)
    }

    fn status(&self, id: &str) -> Result<Option<String>, String> {
        let status = monocode_terminal::pty::pty_status(&self.0, id.into())?;
        // `PtyStatus` keeps its field private and only serializes it.
        let value = serde_json::to_value(status).map_err(|err| err.to_string())?;
        Ok(value
            .get("foreground")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string))
    }

    fn kill(&self, id: &str) -> Result<(), String> {
        monocode_terminal::pty::pty_kill(&self.0, id.into())
    }

    fn kill_all(&self) -> Result<(), String> {
        monocode_terminal::pty::pty_kill_all(&self.0)
    }
}

/// Something the router saw on a reader thread, for the UI thread.
enum RouterNote {
    /// OSC 7 reported a new working directory.
    Cwd { id: String, cwd: String },
    /// The child exited.
    Exited { id: String },
}

#[derive(Default)]
struct Route {
    sender: Option<async_channel::Sender<PtyEvent>>,
    buffer: VecDeque<Vec<u8>>,
    buffered_bytes: usize,
    osc_buffer: String,
}

impl Route {
    /// `pushBuffered`.
    fn push_buffered(&mut self, chunk: Vec<u8>) {
        self.buffered_bytes += chunk.len();
        self.buffer.push_back(chunk);
        let sizes: Vec<usize> = self.buffer.iter().map(Vec::len).collect();
        let (drop, bytes) = trim_replay(&sizes, self.buffered_bytes);
        self.buffer.drain(..drop);
        self.buffered_bytes = bytes;
    }
}

/// Receives output from the PTY reader threads and hands it to the
/// attached view, or buffers it. Only terminals this engine opened are
/// routed, like `openedPtys`.
pub struct PtyRouter {
    routes: Mutex<HashMap<String, Route>>,
    notes: async_channel::Sender<RouterNote>,
}

impl PtyRouter {
    fn new() -> (Arc<Self>, async_channel::Receiver<RouterNote>) {
        let (notes, receiver) = async_channel::unbounded();
        (
            Arc::new(Self {
                routes: Mutex::new(HashMap::new()),
                notes,
            }),
            receiver,
        )
    }

    fn open(&self, id: &str) {
        self.routes.lock().entry(id.to_string()).or_default();
    }

    fn close(&self, id: &str) {
        self.routes.lock().remove(id);
    }

    fn close_all(&self) {
        self.routes.lock().clear();
    }

    /// `subscribePty`: a new event stream for `id`, starting with the
    /// buffered output.
    fn attach(&self, id: &str) -> async_channel::Receiver<PtyEvent> {
        let (sender, receiver) = async_channel::unbounded();
        let mut routes = self.routes.lock();
        let route = routes.entry(id.to_string()).or_default();
        for chunk in route.buffer.drain(..) {
            let _ = sender.try_send(PtyEvent::Output(chunk));
        }
        route.buffered_bytes = 0;
        route.sender = Some(sender);
        receiver
    }

    /// Output that is not from the PTY, such as a spawn error.
    fn inject(&self, id: &str, bytes: Vec<u8>) {
        self.data(id, &bytes);
    }
}

impl PtyEvents for PtyRouter {
    fn data(&self, id: &str, bytes: &[u8]) {
        let mut routes = self.routes.lock();
        let Some(route) = routes.get_mut(id) else {
            return;
        };
        let text = String::from_utf8_lossy(bytes);
        let scanned = scan_osc_cwd(&text, &route.osc_buffer);
        route.osc_buffer = scanned.rest;
        if let Some(cwd) = scanned.cwd {
            let _ = self.notes.try_send(RouterNote::Cwd { id: id.into(), cwd });
        }
        if let Some(sender) = &route.sender {
            if sender.try_send(PtyEvent::Output(bytes.to_vec())).is_ok() {
                return;
            }
            // The view went away; keep output for the next one.
            route.sender = None;
        }
        route.push_buffered(bytes.to_vec());
    }

    fn exit(&self, id: &str, code: Option<i32>) {
        let routes = self.routes.lock();
        let Some(route) = routes.get(id) else {
            return;
        };
        if let Some(sender) = &route.sender {
            let _ = sender.try_send(PtyEvent::Exited(code));
        }
        let _ = self.notes.try_send(RouterNote::Exited { id: id.into() });
    }
}

/// One queued PTY call. A terminal runs its calls in order, so input waits
/// for the spawn the way the TypeScript chained `starting.then(..)`.
enum Command {
    Spawn { cwd: String, cols: u16, rows: u16 },
    Write(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Kill,
}

/// A terminal's live state on the UI thread.
struct Live {
    commands: async_channel::Sender<Command>,
    cwd: String,
    spawned: Rc<std::cell::Cell<bool>>,
    exited: bool,
    /// `runningProcessRef`: the last foreground process the poll saw.
    running_process: Option<String>,
    /// `lastForeground` of the poll effect.
    last_foreground: Option<String>,
    polling: bool,
    _pump: Task<()>,
}

/// A terminal reported a new title, cwd, or foreground process. The
/// workspace applies it to the dock or tab that holds the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalMetaChanged {
    pub file_id: String,
    pub patch: TerminalMetaPatch,
}

/// Emits `TerminalMetaChanged`.
pub struct TerminalSignal;

impl EventEmitter<TerminalMetaChanged> for TerminalSignal {}

struct TerminalsState {
    live: HashMap<String, Live>,
    hidden: bool,
}

/// The app's terminals. Install with `Terminals::init`.
#[derive(Clone)]
pub struct Terminals {
    backend: Arc<dyn PtyBackend>,
    router: Arc<PtyRouter>,
    state: Rc<RefCell<TerminalsState>>,
    /// Views and workspaces subscribe to this for `TerminalMetaChanged`.
    pub signal: Entity<TerminalSignal>,
}

impl Global for Terminals {}

impl Terminals {
    /// Real PTYs through `monocode_terminal`.
    pub fn init(cx: &mut App) {
        let (router, notes) = PtyRouter::new();
        let host = PtyHost::new(router.clone());
        Self::install(Arc::new(HostPty(host)), router, notes, cx);
    }

    /// Any backend. The caller routes its output through the returned
    /// sink, as `PtyHost` does with the `PtyEvents` it was given.
    pub fn init_with(
        backend: impl FnOnce(Arc<dyn PtyEvents>) -> Arc<dyn PtyBackend>,
        cx: &mut App,
    ) {
        let (router, notes) = PtyRouter::new();
        let backend = backend(router.clone());
        Self::install(backend, router, notes, cx);
    }

    fn install(
        backend: Arc<dyn PtyBackend>,
        router: Arc<PtyRouter>,
        notes: async_channel::Receiver<RouterNote>,
        cx: &mut App,
    ) {
        let terminals = Terminals {
            backend,
            router,
            state: Rc::new(RefCell::new(TerminalsState {
                live: HashMap::new(),
                hidden: false,
            })),
            signal: cx.new(|_| TerminalSignal),
        };
        // The loops look the global up each time, so they hold no entity
        // and stop with the app.
        cx.spawn(async move |cx| {
            while let Ok(note) = notes.recv().await {
                cx.update(|cx| {
                    if let Some(terminals) = Terminals::try_global(cx) {
                        terminals.handle_note(note, cx);
                    }
                });
            }
        })
        .detach();
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(STATUS_POLL).await;
                let alive = cx.update(|cx| {
                    let terminals = Terminals::try_global(cx);
                    if let Some(terminals) = &terminals {
                        terminals.poll_status(cx);
                    }
                    terminals.is_some()
                });
                if !alive {
                    break;
                }
            }
        })
        .detach();
        cx.set_global(terminals);
    }

    pub fn global(cx: &App) -> Terminals {
        cx.global::<Terminals>().clone()
    }

    pub fn try_global(cx: &App) -> Option<Terminals> {
        cx.try_global::<Terminals>().cloned()
    }

    /// Attach a view to the terminal for `file_id`, starting its shell in
    /// `cwd` on first attach. Hand the result to `TerminalView::new`.
    pub fn attach(&self, file_id: &str, cwd: &str, cx: &mut App) -> EnginePty {
        self.router.open(file_id);
        let commands = self.ensure_live(file_id, cwd, cx);
        let events = self.router.attach(file_id);
        EnginePty {
            commands,
            events: Some(events),
        }
    }

    fn ensure_live(
        &self,
        file_id: &str,
        cwd: &str,
        cx: &mut App,
    ) -> async_channel::Sender<Command> {
        if let Some(live) = self.state.borrow().live.get(file_id) {
            return live.commands.clone();
        }
        let (commands, queue) = async_channel::unbounded::<Command>();
        let spawned = Rc::new(std::cell::Cell::new(false));
        let backend = self.backend.clone();
        let router = self.router.clone();
        let id = file_id.to_string();
        let started = spawned.clone();
        let pump = cx.spawn(async move |cx| {
            let mut failed = false;
            while let Ok(command) = queue.recv().await {
                let backend = backend.clone();
                let id_for_call = id.clone();
                match command {
                    Command::Spawn { cwd, cols, rows } => {
                        let result = cx
                            .background_executor()
                            .spawn(async move { backend.spawn(&id_for_call, &cwd, cols, rows) })
                            .await;
                        match result {
                            Ok(()) => started.set(true),
                            Err(message) => {
                                failed = true;
                                router.inject(
                                    &id,
                                    format!("\x1b[31m{message}\x1b[0m\r\n").into_bytes(),
                                );
                            }
                        }
                    }
                    Command::Write(bytes) if !failed => {
                        let _ = cx
                            .background_executor()
                            .spawn(async move { backend.write(&id_for_call, &bytes) })
                            .await;
                    }
                    Command::Resize { cols, rows } if !failed => {
                        let _ = cx
                            .background_executor()
                            .spawn(async move { backend.resize(&id_for_call, cols, rows) })
                            .await;
                    }
                    Command::Kill => {
                        let _ = cx
                            .background_executor()
                            .spawn(async move { backend.kill(&id_for_call) })
                            .await;
                        break;
                    }
                    Command::Write(_) | Command::Resize { .. } => {}
                }
            }
        });
        let _ = commands.try_send(Command::Spawn {
            cwd: cwd.to_string(),
            cols: INITIAL_SIZE.0,
            rows: INITIAL_SIZE.1,
        });
        self.state.borrow_mut().live.insert(
            file_id.to_string(),
            Live {
                commands: commands.clone(),
                cwd: cwd.to_string(),
                spawned,
                exited: false,
                running_process: None,
                last_foreground: None,
                polling: false,
                _pump: pump,
            },
        );
        commands
    }

    /// Whether a terminal for `file_id` was started and not killed.
    pub fn is_open(&self, file_id: &str) -> bool {
        self.state.borrow().live.contains_key(file_id)
    }

    /// The ids of every terminal started and not killed.
    pub fn open_ids(&self) -> HashSet<String> {
        self.state.borrow().live.keys().cloned().collect()
    }

    /// `killPty`: stop the shell for `file_id` after its queued input.
    pub fn kill(&self, file_id: &str) {
        self.router.close(file_id);
        let removed = self.state.borrow_mut().live.remove(file_id);
        if let Some(live) = removed {
            let _ = live.commands.try_send(Command::Kill);
            // The pump task must outlive this handle to run the kill.
            let pump = live._pump;
            pump.detach();
        }
    }

    /// `killAllPtys`.
    pub fn kill_all(&self, cx: &mut App) -> Task<()> {
        self.router.close_all();
        let live: Vec<Live> = self
            .state
            .borrow_mut()
            .live
            .drain()
            .map(|(_, live)| live)
            .collect();
        drop(live);
        let backend = self.backend.clone();
        cx.background_executor().spawn(async move {
            let _ = backend.kill_all();
        })
    }

    /// The foreground process of each terminal, for the close prompts. A
    /// terminal that is gone reports `None`.
    pub fn foregrounds(
        &self,
        ids: Vec<String>,
        cx: &mut App,
    ) -> Task<HashMap<String, Option<String>>> {
        let open = self.open_ids();
        let lookups: Vec<_> = ids
            .into_iter()
            .map(|id| {
                let backend = self.backend.clone();
                let running = open.contains(&id);
                cx.background_executor().spawn(async move {
                    let foreground = if running {
                        backend.status(&id).ok().flatten()
                    } else {
                        None
                    };
                    (id, foreground)
                })
            })
            .collect();
        cx.background_executor()
            .spawn(async move { join_all(lookups).await.into_iter().collect() })
    }

    /// `document.hidden`: the poll pauses while every window is hidden, and
    /// runs at once when one shows again.
    pub fn set_hidden(&self, hidden: bool, cx: &mut App) {
        self.state.borrow_mut().hidden = hidden;
        if !hidden {
            self.poll_status(cx);
        }
    }

    fn handle_note(&self, note: RouterNote, cx: &mut App) {
        match note {
            RouterNote::Cwd { id, cwd } => {
                let patch = {
                    let mut state = self.state.borrow_mut();
                    let Some(live) = state.live.get_mut(&id) else {
                        return;
                    };
                    live.cwd = cwd.clone();
                    TerminalMetaPatch {
                        title: live
                            .running_process
                            .is_none()
                            .then(|| default_terminal_title(&cwd)),
                        cwd: Some(cwd),
                        foreground: None,
                    }
                };
                self.emit_meta(id, patch, cx);
            }
            RouterNote::Exited { id } => {
                if let Some(live) = self.state.borrow_mut().live.get_mut(&id) {
                    live.exited = true;
                }
            }
        }
    }

    fn emit_meta(&self, file_id: String, patch: TerminalMetaPatch, cx: &mut App) {
        self.signal
            .update(cx, |_, cx| cx.emit(TerminalMetaChanged { file_id, patch }));
    }

    /// The title poll: each started terminal reports its foreground
    /// process, and a change renames the tab to the process or back to the
    /// folder.
    fn poll_status(&self, cx: &mut App) {
        let due: Vec<(String, String)> = {
            let mut state = self.state.borrow_mut();
            if state.hidden {
                return;
            }
            state
                .live
                .iter_mut()
                .filter(|(_, live)| live.spawned.get() && !live.exited && !live.polling)
                .map(|(id, live)| {
                    live.polling = true;
                    (id.clone(), live.cwd.clone())
                })
                .collect()
        };
        for (id, cwd) in due {
            let backend = self.backend.clone();
            let lookup_id = id.clone();
            let status = cx
                .background_executor()
                .spawn(async move { backend.status(&lookup_id) });
            cx.spawn(async move |cx| {
                let result = status.await;
                cx.update(|cx| {
                    if let Some(terminals) = Terminals::try_global(cx) {
                        terminals.finish_poll(&id, &cwd, result, cx);
                    }
                });
            })
            .detach();
        }
    }

    fn finish_poll(
        &self,
        id: &str,
        cwd: &str,
        result: Result<Option<String>, String>,
        cx: &mut App,
    ) {
        let patch = {
            let mut state = self.state.borrow_mut();
            let Some(live) = state.live.get_mut(id) else {
                return;
            };
            live.polling = false;
            let Ok(foreground) = result else {
                return;
            };
            let foreground = foreground
                .map(|value| monocode_core::js::trim(&value).to_string())
                .filter(|value| !value.is_empty());
            live.running_process = foreground.clone();
            if foreground == live.last_foreground {
                return;
            }
            live.last_foreground = foreground.clone();
            match foreground {
                Some(process) => TerminalMetaPatch {
                    title: Some(process.clone()),
                    cwd: None,
                    foreground: Some(Some(process)),
                },
                None => TerminalMetaPatch {
                    title: Some(default_terminal_title(cwd)),
                    cwd: None,
                    foreground: Some(None),
                },
            }
        };
        self.emit_meta(id.to_string(), patch, cx);
    }
}

/// `monocode_terminal_view::Pty` over an engine terminal. Writes and
/// resizes queue behind the spawn and run in order off the UI thread.
pub struct EnginePty {
    commands: async_channel::Sender<Command>,
    events: Option<async_channel::Receiver<PtyEvent>>,
}

impl Pty for EnginePty {
    fn take_events(&mut self) -> Option<async_channel::Receiver<PtyEvent>> {
        self.events.take()
    }

    fn write(&mut self, bytes: &[u8]) {
        let _ = self.commands.try_send(Command::Write(bytes.to_vec()));
    }

    fn resize(&mut self, size: PtySize) {
        let _ = self.commands.try_send(Command::Resize {
            cols: size.cols,
            rows: size.rows,
        });
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A `PtyBackend` that records calls and lets a test emit output.

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Call {
        Spawn(String, String, u16, u16),
        Write(String, Vec<u8>),
        Resize(String, u16, u16),
        Kill(String),
        KillAll,
    }

    #[derive(Default)]
    pub struct FakePty {
        pub calls: Mutex<Vec<Call>>,
        pub foreground: Mutex<HashMap<String, String>>,
        pub fail_spawn: Mutex<Option<String>>,
        pub events: Mutex<Option<Arc<dyn PtyEvents>>>,
    }

    impl FakePty {
        pub fn calls(&self) -> Vec<Call> {
            self.calls.lock().clone()
        }

        pub fn set_foreground(&self, id: &str, process: Option<&str>) {
            let mut foreground = self.foreground.lock();
            match process {
                Some(process) => foreground.insert(id.into(), process.into()),
                None => foreground.remove(id),
            };
        }

        /// Output from the child, as a reader thread would deliver it.
        pub fn output(&self, id: &str, bytes: &[u8]) {
            if let Some(events) = self.events.lock().clone() {
                events.data(id, bytes);
            }
        }

        pub fn exit(&self, id: &str, code: Option<i32>) {
            if let Some(events) = self.events.lock().clone() {
                events.exit(id, code);
            }
        }
    }

    impl PtyBackend for FakePty {
        fn spawn(&self, id: &str, cwd: &str, cols: u16, rows: u16) -> Result<(), String> {
            self.calls
                .lock()
                .push(Call::Spawn(id.into(), cwd.into(), cols, rows));
            match self.fail_spawn.lock().clone() {
                Some(message) => Err(message),
                None => Ok(()),
            }
        }

        fn write(&self, id: &str, data: &[u8]) -> Result<(), String> {
            self.calls
                .lock()
                .push(Call::Write(id.into(), data.to_vec()));
            Ok(())
        }

        fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
            self.calls.lock().push(Call::Resize(id.into(), cols, rows));
            Ok(())
        }

        fn status(&self, id: &str) -> Result<Option<String>, String> {
            Ok(self.foreground.lock().get(id).cloned())
        }

        fn kill(&self, id: &str) -> Result<(), String> {
            self.calls.lock().push(Call::Kill(id.into()));
            Ok(())
        }

        fn kill_all(&self) -> Result<(), String> {
            self.calls.lock().push(Call::KillAll);
            Ok(())
        }
    }

    /// Install `Terminals` over a fresh fake.
    pub fn init_fake_terminals(cx: &mut App) -> Arc<FakePty> {
        let fake = Arc::new(FakePty::default());
        let backend = fake.clone();
        Terminals::init_with(
            move |events| {
                *backend.events.lock() = Some(events);
                backend
            },
            cx,
        );
        fake
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{Call, init_fake_terminals};
    use super::*;
    use gpui::TestAppContext;

    fn drain(events: &async_channel::Receiver<PtyEvent>) -> Vec<PtyEvent> {
        let mut out = Vec::new();
        while let Ok(event) = events.try_recv() {
            out.push(event);
        }
        out
    }

    #[test]
    fn trims_the_replay_buffer_by_bytes_and_count() {
        assert_eq!(trim_replay(&[10, 20], 30), (0, 30));
        let big = MAX_BUFFERED_BYTES / 2 + 1;
        assert_eq!(trim_replay(&[big, big, 5], big * 2 + 5), (1, big + 5));
        // The newest chunk stays even when it alone is over budget.
        assert_eq!(
            trim_replay(&[1, MAX_BUFFERED_BYTES + 1], MAX_BUFFERED_BYTES + 2),
            (1, MAX_BUFFERED_BYTES + 1)
        );
        let many = vec![1; MAX_BUFFERED + 3];
        assert_eq!(trim_replay(&many, many.len()), (3, MAX_BUFFERED));
    }

    #[gpui::test]
    fn spawns_on_attach_and_queues_input_behind_the_spawn(cx: &mut TestAppContext) {
        let fake = cx.update(init_fake_terminals);
        let mut pty = cx.update(|cx| Terminals::global(cx).attach("t1", "/repo", cx));
        pty.write(b"ls\r");
        pty.resize(PtySize {
            cols: 100,
            rows: 30,
            pixel_width: 0,
            pixel_height: 0,
        });
        cx.run_until_parked();
        assert_eq!(
            fake.calls(),
            vec![
                Call::Spawn("t1".into(), "/repo".into(), 80, 24),
                Call::Write("t1".into(), b"ls\r".to_vec()),
                Call::Resize("t1".into(), 100, 30),
            ]
        );
        // A second attach reuses the running shell.
        let _again = cx.update(|cx| Terminals::global(cx).attach("t1", "/repo", cx));
        cx.run_until_parked();
        assert_eq!(fake.calls().len(), 3);
    }

    #[gpui::test]
    fn routes_output_to_the_view_and_replays_it_to_the_next_one(cx: &mut TestAppContext) {
        let fake = cx.update(init_fake_terminals);
        let mut first = cx.update(|cx| Terminals::global(cx).attach("t1", "/repo", cx));
        let events = first.take_events().unwrap();
        fake.output("t1", b"hello");
        fake.output("stranger", b"ignored");
        assert_eq!(drain(&events), vec![PtyEvent::Output(b"hello".to_vec())]);

        drop(events);
        drop(first);
        fake.output("t1", b"while away");
        let mut second = cx.update(|cx| Terminals::global(cx).attach("t1", "/repo", cx));
        let events = second.take_events().unwrap();
        assert_eq!(
            drain(&events),
            vec![PtyEvent::Output(b"while away".to_vec())]
        );
        fake.exit("t1", Some(0));
        assert_eq!(drain(&events), vec![PtyEvent::Exited(Some(0))]);
    }

    #[gpui::test]
    fn shows_a_spawn_error_and_drops_later_input(cx: &mut TestAppContext) {
        let fake = cx.update(init_fake_terminals);
        *fake.fail_spawn.lock() = Some("No shell".into());
        let mut pty = cx.update(|cx| Terminals::global(cx).attach("t1", "/repo", cx));
        let events = pty.take_events().unwrap();
        pty.write(b"x");
        cx.run_until_parked();
        assert_eq!(
            drain(&events),
            vec![PtyEvent::Output(b"\x1b[31mNo shell\x1b[0m\r\n".to_vec())]
        );
        assert_eq!(
            fake.calls(),
            vec![Call::Spawn("t1".into(), "/repo".into(), 80, 24)]
        );
    }

    #[gpui::test]
    fn kills_after_queued_input(cx: &mut TestAppContext) {
        let fake = cx.update(init_fake_terminals);
        let mut pty = cx.update(|cx| Terminals::global(cx).attach("t1", "/repo", cx));
        pty.write(b"exit\r");
        cx.update(|cx| Terminals::global(cx).kill("t1"));
        cx.run_until_parked();
        assert_eq!(
            fake.calls(),
            vec![
                Call::Spawn("t1".into(), "/repo".into(), 80, 24),
                Call::Write("t1".into(), b"exit\r".to_vec()),
                Call::Kill("t1".into()),
            ]
        );
        assert!(!cx.update(|cx| Terminals::global(cx).is_open("t1")));
    }

    #[gpui::test]
    fn reports_the_osc_cwd_and_the_foreground_process(cx: &mut TestAppContext) {
        let fake = cx.update(init_fake_terminals);
        let terminals = cx.update(|cx| Terminals::global(cx));
        let seen: Rc<RefCell<Vec<TerminalMetaChanged>>> = Rc::default();
        let log = seen.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(
                &terminals.signal,
                move |_, event: &TerminalMetaChanged, _| log.borrow_mut().push(event.clone()),
            )
        });
        let _pty = cx.update(|cx| terminals.attach("t1", "/repo", cx));
        cx.run_until_parked();

        fake.output("t1", b"\x1b]7;file://host/repo/app\x07$ ");
        cx.run_until_parked();
        assert_eq!(
            seen.borrow_mut().drain(..).collect::<Vec<_>>(),
            vec![TerminalMetaChanged {
                file_id: "t1".into(),
                patch: TerminalMetaPatch {
                    title: Some("app".into()),
                    cwd: Some("/repo/app".into()),
                    foreground: None,
                },
            }]
        );

        fake.set_foreground("t1", Some("vite"));
        cx.executor().advance_clock(STATUS_POLL);
        cx.run_until_parked();
        assert_eq!(
            seen.borrow_mut().drain(..).collect::<Vec<_>>(),
            vec![TerminalMetaChanged {
                file_id: "t1".into(),
                patch: TerminalMetaPatch {
                    title: Some("vite".into()),
                    cwd: None,
                    foreground: Some(Some("vite".into())),
                },
            }]
        );

        // Unchanged: no event. Back to the shell: the folder name returns.
        cx.executor().advance_clock(STATUS_POLL);
        cx.run_until_parked();
        assert!(seen.borrow().is_empty());
        fake.set_foreground("t1", None);
        cx.executor().advance_clock(STATUS_POLL);
        cx.run_until_parked();
        assert_eq!(
            seen.borrow_mut().drain(..).collect::<Vec<_>>(),
            vec![TerminalMetaChanged {
                file_id: "t1".into(),
                patch: TerminalMetaPatch {
                    title: Some("app".into()),
                    cwd: None,
                    foreground: Some(None),
                },
            }]
        );
    }

    #[gpui::test]
    fn reads_foregrounds_for_the_close_prompt(cx: &mut TestAppContext) {
        let fake = cx.update(init_fake_terminals);
        let _pty = cx.update(|cx| Terminals::global(cx).attach("t1", "/repo", cx));
        cx.run_until_parked();
        fake.set_foreground("t1", Some("npm"));
        fake.set_foreground("gone", Some("x"));
        let task =
            cx.update(|cx| Terminals::global(cx).foregrounds(vec!["t1".into(), "gone".into()], cx));
        cx.run_until_parked();
        let map = futures::FutureExt::now_or_never(task).unwrap();
        assert_eq!(map.get("t1"), Some(&Some("npm".to_string())));
        assert_eq!(map.get("gone"), Some(&None));
    }

    /// Spawns the user's real shell through `monocode_terminal`.
    #[gpui::test]
    #[ignore = "spawns a real shell"]
    fn runs_a_real_shell(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(Terminals::init);
        let mut pty = cx.update(|cx| Terminals::global(cx).attach("live", "/tmp", cx));
        let events = pty.take_events().unwrap();
        cx.run_until_parked();
        pty.write(b"echo monocode-$((40+2))\r");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        while std::time::Instant::now() < deadline {
            cx.run_until_parked();
            while let Ok(PtyEvent::Output(bytes)) = events.try_recv() {
                seen.extend(bytes);
            }
            if String::from_utf8_lossy(&seen).contains("monocode-42") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(String::from_utf8_lossy(&seen).contains("monocode-42"));
        cx.update(|cx| Terminals::global(cx).kill("live"));
        cx.run_until_parked();
    }
}
