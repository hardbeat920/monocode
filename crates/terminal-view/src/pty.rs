//! The PTY as the terminal view sees it.
//!
//! In the React app this was `spawnPty`, `writePty`, `resizePty`, and
//! `subscribePty` in src/platform/tauri/pty.ts, with output arriving as
//! base64 chunks over Tauri events. The native app hands the view raw bytes
//! instead. `monocode-terminal` will implement [`Pty`]; tests and the shell
//! example implement it over portable-pty directly.

/// Something the PTY reports to the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyEvent {
    /// Output from the child process.
    Output(Vec<u8>),
    /// The child exited, with its exit code when it has one.
    Exited(Option<i32>),
}

/// Grid size the view asks the PTY to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
    /// Grid width in pixels, for programs that ask (TIOCGWINSZ).
    pub pixel_width: u16,
    /// Grid height in pixels.
    pub pixel_height: u16,
}

/// A running PTY session.
pub trait Pty: 'static {
    /// The stream of output and exit events. The view calls this once, when
    /// it attaches. Return `None` if the host feeds output itself through
    /// [`crate::TerminalView::feed`].
    fn take_events(&mut self) -> Option<async_channel::Receiver<PtyEvent>>;

    /// Bytes for the child's input: keystrokes, pastes, and replies to
    /// terminal queries.
    fn write(&mut self, bytes: &[u8]);

    /// The view's grid changed size.
    fn resize(&mut self, size: PtySize);
}

/// A [`Pty`] that records what the view sends and lets a test push output.
#[derive(Clone)]
pub struct RecordingPty {
    written: std::rc::Rc<std::cell::RefCell<Vec<u8>>>,
    sizes: std::rc::Rc<std::cell::RefCell<Vec<PtySize>>>,
    events: Option<async_channel::Receiver<PtyEvent>>,
    sender: async_channel::Sender<PtyEvent>,
}

impl RecordingPty {
    pub fn new() -> Self {
        let (sender, events) = async_channel::unbounded();
        Self {
            written: Default::default(),
            sizes: Default::default(),
            events: Some(events),
            sender,
        }
    }

    /// Everything written so far, then cleared.
    pub fn take_written(&self) -> Vec<u8> {
        std::mem::take(&mut self.written.borrow_mut())
    }

    /// Every resize so far.
    pub fn sizes(&self) -> Vec<PtySize> {
        self.sizes.borrow().clone()
    }

    /// Send output or an exit to the view.
    pub fn sender(&self) -> async_channel::Sender<PtyEvent> {
        self.sender.clone()
    }
}

impl Default for RecordingPty {
    fn default() -> Self {
        Self::new()
    }
}

impl Pty for RecordingPty {
    fn take_events(&mut self) -> Option<async_channel::Receiver<PtyEvent>> {
        self.events.take()
    }

    fn write(&mut self, bytes: &[u8]) {
        self.written.borrow_mut().extend_from_slice(bytes);
    }

    fn resize(&mut self, size: PtySize) {
        self.sizes.borrow_mut().push(size);
    }
}
