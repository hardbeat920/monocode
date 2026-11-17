//! The AppKit side of a draft: a `WKWebView` placed over the window's content
//! view where the card's canvas is, and the calls that read it.
use gpui::{Context, Window, canvas, prelude::*};
use image::ImageEncoder as _;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject,
};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSBitmapFormat, NSBitmapImageFileType, NSBitmapImageRep, NSEvent, NSEventMask,
    NSEventType, NSImage, NSView, NSWindowOrderingMode,
};
use objc2_core_graphics::{
    CGBitmapInfo, CGDataProvider, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo, CGMutablePath,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, NSURL, NSURLRequest,
};
use objc2_quartz_core::{CALayer, CAShapeLayer, CATransaction, kCAFillRuleEvenOdd};
use objc2_web_kit::{
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKNavigationType,
    WKScriptMessage, WKScriptMessageHandler, WKSnapshotConfiguration, WKUserContentController,
    WKUserScript, WKUserScriptInjectionTime, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

/// A draft's page, for the tasks that read and drive it.
pub(super) type Page = Retained<WKWebView>;

/// When AppKit last delivered the person's own click or key press to a
/// draft's page. The page reports input too, but anything it runs can forge
/// that report, so the report only counts shortly after one of these.
pub(super) type Input = Rc<std::cell::Cell<Option<std::time::Instant>>>;

pub(super) struct Clip {
    /// Whether the page scrolls inside its card, and so keeps the wheel.
    scrolls: std::cell::Cell<bool>,
    input: Input,
    /// Kept running out of sight for the agent; see `Lease`.
    parked: std::cell::Cell<bool>,
    /// Where GPUI's controls float over the page, in this view's coordinates.
    holes: RefCell<Vec<Hole>>,
}

/// A rounded rectangle cut out of the page.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hole {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
}

impl Hole {
    /// In a window's points, offset to the clip view at `origin`. The radius
    /// never exceeds half a side, which Core Graphics would abort on.
    fn new(bounds: gpui::Bounds<gpui::Pixels>, radius: gpui::Pixels, origin: (f64, f64)) -> Self {
        let (width, height) = (f64::from(bounds.size.width), f64::from(bounds.size.height));
        Self {
            x: f64::from(bounds.left()) - origin.0,
            y: f64::from(bounds.top()) - origin.1,
            width,
            height,
            radius: f64::from(radius).min(width / 2.).min(height / 2.),
        }
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        let (left, top) = (x - self.x, y - self.y);
        if left < 0. || top < 0. || left > self.width || top > self.height {
            return false;
        }
        // Past a corner's center, the point has to be within its arc.
        let dx = (self.radius - left)
            .max(left - self.width + self.radius)
            .max(0.);
        let dy = (self.radius - top)
            .max(top - self.height + self.radius)
            .max(0.);
        dx * dx + dy * dy <= self.radius * self.radius
    }

    fn rect(&self) -> NSRect {
        NSRect::new(
            NSPoint::new(self.x, self.y),
            NSSize::new(self.width, self.height),
        )
    }
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "MonocodeDraftClip"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Clip]
    struct DraftClip;
    unsafe impl NSObjectProtocol for DraftClip {}
    impl DraftClip {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool {
            true
        }

        /// A page that cannot scroll lets the wheel through to the thread
        /// behind it; everything else still lands on the page. A mouse
        /// button pressed over the page is the person's own input.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            let kind = NSApplication::sharedApplication(self.mtm())
                .currentEvent()
                .map(|event| event.r#type());
            let hit: Option<Retained<NSView>> = if self.ivars().parked.get()
                || kind == Some(NSEventType::ScrollWheel) && !self.ivars().scrolls.get()
                || self.in_hole(point)
            {
                None
            } else {
                unsafe { msg_send![super(self), hitTest: point] }
            };
            if hit.is_some()
                && matches!(
                    kind,
                    Some(
                        NSEventType::LeftMouseDown
                            | NSEventType::RightMouseDown
                            | NSEventType::OtherMouseDown
                    )
                )
            {
                self.ivars().input.set(Some(std::time::Instant::now()));
            }
            hit
        }
    }
);

impl DraftClip {
    /// Whether `point`, in the superview's coordinates, is on a control GPUI
    /// floats over the page: the press belongs to the control.
    fn in_hole(&self, point: NSPoint) -> bool {
        let holes = self.ivars().holes.borrow();
        if holes.is_empty() {
            return false;
        }
        let parent = unsafe { self.superview() };
        let here = self.convertPoint_fromView(point, parent.as_deref());
        holes.iter().any(|hole| hole.contains(here.x, here.y))
    }
}

/// Credits a key press to the draft whose page has the keyboard. Installed
/// once; it only reads events and passes every one on.
fn watch_keys(mtm: MainThreadMarker) {
    thread_local! {
        static MONITOR: RefCell<Option<Retained<objc2::runtime::AnyObject>>> =
            const { RefCell::new(None) };
    }
    if MONITOR.with(|monitor| monitor.borrow().is_some()) {
        return;
    }
    let block = block2::RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| {
        let focused = unsafe { event.as_ref() }
            .window(mtm)
            .and_then(|window| window.firstResponder());
        if let Some(focused) = focused
            && let Some(focused) = focused.downcast_ref::<NSView>()
        {
            HOSTS.with(|hosts| {
                for Tracked { view, .. } in hosts.borrow().iter() {
                    if let Some(view) = view.upgrade()
                        && let Some(hosted) = view.borrow().as_ref()
                        && focused.isDescendantOf(&hosted.web)
                    {
                        hosted
                            .clip
                            .ivars()
                            .input
                            .set(Some(std::time::Instant::now()));
                    }
                }
            });
        }
        event.as_ptr()
    });
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
    };
    MONITOR.with(|slot| *slot.borrow_mut() = monitor);
}

/// The page tells the app it has something new to read, so the app reads it
/// then rather than on a timer. A wake carries nothing and grants nothing: it
/// only makes the app read the page sooner.
#[derive(Clone)]
pub(super) struct Wakes {
    send: async_channel::Sender<()>,
    receive: async_channel::Receiver<()>,
    /// How many times the page itself has pinged: it may look different since.
    pings: Pings,
    /// How many times the app asked for a read, as when the phone changed
    /// the record.
    asks: Pings,
}
type Pings = Rc<std::cell::Cell<u64>>;
impl Default for Wakes {
    fn default() -> Self {
        // One pending wake covers any number of reports.
        let (send, receive) = async_channel::bounded(1);
        Self {
            send,
            receive,
            pings: Pings::default(),
            asks: Pings::default(),
        }
    }
}

/// The name the wrapper posts to; see `WRAPPER_SCRIPT`.
const WAKE: &str = "mcWake";

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MonocodeDraftWake"]
    #[thread_kind = MainThreadOnly]
    #[ivars = (async_channel::Sender<()>, Pings)]
    struct Wake;
    unsafe impl NSObjectProtocol for Wake {}
    unsafe impl WKScriptMessageHandler for Wake {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn received(&self, _: &WKUserContentController, message: &WKScriptMessage) {
            // The wrapper is the top frame; the agent's page is a sandboxed
            // frame inside it and does not get to wake the app itself.
            if unsafe { message.frameInfo().isMainFrame() } {
                let (send, pings) = self.ivars();
                pings.set(pings.get() + 1);
                let _ = send.try_send(());
            }
        }
    }
);

/// What a draft's web view may navigate to, and how it reports that its page
/// process died.
pub(super) struct Guard {
    policy: Policy,
    died: Rc<std::cell::Cell<bool>>,
    wake: async_channel::Sender<()>,
}

pub(super) enum Policy {
    /// The agent's own page: nothing loads but the page itself.
    Page(gpui::AsyncApp),
    /// A dev server on this machine.
    Local(gpui::AsyncApp),
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MonocodeDraftNavigation"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Guard]
    struct Navigation;
    unsafe impl NSObjectProtocol for Navigation {}
    unsafe impl WKNavigationDelegate for Navigation {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        unsafe fn decide(
            &self,
            _: &WKWebView,
            action: &WKNavigationAction,
            reply: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let url = unsafe { action.request().URL() }
                .and_then(|url| url.absoluteString())
                .map(|url| url.to_string());
            let policy = &self.ivars().policy;
            let (Policy::Page(app) | Policy::Local(app)) = policy;
            let web = url
                .as_deref()
                .is_some_and(|url| url.starts_with("http://") || url.starts_with("https://"));
            let local = url.as_deref().is_some_and(is_local);
            let link = unsafe { action.navigationType() } == WKNavigationType::LinkActivated;
            // Links to anywhere but the draft itself open in the user's browser.
            if link && web && !(matches!(policy, Policy::Local(_)) && local) {
                let url = url.clone().unwrap_or_default();
                app.clone().update(|cx| cx.open_url(&url));
            }
            let allowed = match url.as_deref() {
                None => true,
                Some(url) if url.starts_with("about:") => true,
                Some(_) => matches!(policy, Policy::Local(_)) && local,
            };
            reply.call((if allowed {
                WKNavigationActionPolicy::Allow
            } else {
                WKNavigationActionPolicy::Cancel
            },));
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn terminated(&self, _: &WKWebView) {
            self.ivars().died.set(true);
            let _ = self.ivars().wake.try_send(());
        }
    }
);

pub(super) fn is_local(url: &str) -> bool {
    monocode_core::visual_bridge::local_url(url).is_ok()
}

/// What the web view shows.
#[derive(Clone)]
pub(super) enum Load {
    Html(Arc<str>),
    Url(Arc<str>),
}

struct Hosted {
    clip: Retained<DraftClip>,
    web: Retained<WKWebView>,
    _guard: Retained<Navigation>,
    /// The content last handed to the web view.
    loaded: Option<(u64, Load)>,
    /// Clip frame, web frame and zoom as last applied.
    placed: [f64; 9],
    /// The clip view's top left, in the window's points from its top left.
    origin: (f64, f64),
    /// The clip view's size when its holes were last cut.
    cut: (f64, f64),
    /// The page's opacity as last applied: 0 until it is ready to be seen.
    alpha: f64,
    /// Whether the page had drawn anything when it was last hidden.
    drew: bool,
    /// When a page that had drawn came back into view, until it has drawn again.
    returned: Option<std::time::Instant>,
    /// Its page process is gone: WebKit leaves the view empty for good.
    died: Rc<std::cell::Cell<bool>>,
    wake: async_channel::Sender<()>,
    /// How often the open page was restyled for a new theme.
    restyles: u64,
    glide: Glide,
    /// The latest picture of the page, drawn in its place while the card moves.
    still: Option<Still>,
    /// What the page last looked like it was, and since when; a still is
    /// taken once that has held for `QUIET`.
    seen: Option<(StillKey, std::time::Instant)>,
    taking: Rc<std::cell::Cell<bool>>,
    /// When a paint last asked for a still it had none of.
    asked: Option<std::time::Instant>,
}

/// A native view moves with the thread a frame late, since AppKit commits
/// its frame apart from GPUI's Metal drawing, and moving it makes WebKit
/// work every frame. So while the card moves, as the thread scrolls, a still
/// of the page is drawn in GPUI's own frame instead and the page steps aside
/// until the card has stayed put for `SETTLE`.
#[derive(Default)]
struct Glide {
    /// The card's page bounds at its last paint, in the window's pixels.
    at: Option<gpui::Bounds<gpui::Pixels>>,
    /// When those bounds last changed.
    moved: Option<std::time::Instant>,
    /// A still is drawn in the page's place, and the page is off its card.
    on: bool,
    /// A repaint is due once the card may have settled.
    settling: Rc<std::cell::Cell<bool>>,
}

/// How long a card stays put before its page comes back.
const SETTLE: Duration = Duration::from_millis(140);
/// How long the page has to look the same, and its card stay put, before a
/// still is taken of it.
const QUIET: Duration = Duration::from_millis(400);

impl Glide {
    /// Records the card at `bounds`, and says whether it moved within `SETTLE`.
    fn track(&mut self, bounds: gpui::Bounds<gpui::Pixels>, now: std::time::Instant) -> bool {
        if self.at.is_some_and(|at| at != bounds) {
            self.moved = Some(now);
        }
        self.at = Some(bounds);
        !self.still_for(SETTLE, now)
    }
    /// Whether the card has stayed put for `long`.
    fn still_for(&self, long: Duration, now: std::time::Instant) -> bool {
        self.moved
            .is_none_or(|moved| now.saturating_duration_since(moved) >= long)
    }
}

/// What a still shows: a page that differs in any of these looks different.
#[derive(Clone, Copy, PartialEq, Debug)]
struct StillKey {
    generation: u64,
    pings: u64,
    input: Option<std::time::Instant>,
    restyles: u64,
    /// The web view's size in points.
    size: (u64, u64),
}

impl StillKey {
    /// Whether a still taken at `self` can stand in for the page at `now`.
    /// Pings alone do not rule it out: a page may report as the pointer
    /// passes over it, mid scroll.
    fn stands_for(&self, now: &StillKey) -> bool {
        self.generation == now.generation
            && self.input == now.input
            && self.restyles == now.restyles
            && self.size == now.size
    }
}

struct Still {
    image: Arc<gpui::RenderImage>,
    key: StillKey,
    /// Taken at once because the card had none to draw, perhaps while the
    /// page was still arriving: a calmer one replaces it.
    early: bool,
}

/// Measuring only: the stills as they were before, taken only after `QUIET`.
fn early_stills() -> bool {
    static LATE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    !*LATE.get_or_init(|| std::env::var_os("MONOCODE_DRAFT_STILL_LATE").is_some())
}

thread_local! {
    /// Stills no longer drawn, for the next paint to free from the atlas.
    static RETIRED: RefCell<Vec<Arc<gpui::RenderImage>>> = const { RefCell::new(Vec::new()) };
}

fn retire(image: Arc<gpui::RenderImage>) {
    RETIRED.with(|retired| retired.borrow_mut().push(image));
}

fn free_retired(window: &mut Window, cx: &mut gpui::App) {
    let images = RETIRED.with(|retired| std::mem::take(&mut *retired.borrow_mut()));
    for image in images {
        cx.drop_image(image, Some(window));
    }
}

/// How moving cards were drawn and what their stills cost, for the measuring
/// self-test.
#[derive(Default, Clone, Copy)]
pub(super) struct Stats {
    /// Frames a moving card drew its still.
    pub still: u64,
    /// Frames a moving card had its live page moved instead.
    pub dragged: u64,
    /// Snapshots turned into pictures, and the main-thread time that took.
    pub taken: u64,
    pub convert_ms: f64,
}

thread_local! {
    static STATS: std::cell::Cell<Stats> = const {
        std::cell::Cell::new(Stats { still: 0, dragged: 0, taken: 0, convert_ms: 0. })
    };
}

pub(super) fn stats() -> Stats {
    STATS.with(std::cell::Cell::get)
}

fn count(change: impl FnOnce(&mut Stats)) {
    STATS.with(|stats| {
        let mut now = stats.get();
        change(&mut now);
        stats.set(now);
    });
}

fn web_size(rect: NSRect) -> (u64, u64) {
    (
        rect.size.width.round() as u64,
        rect.size.height.round() as u64,
    )
}

/// Whether a page in view has anything on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Blank {
    /// It has, or it is out of sight and nothing is expected of it.
    No,
    /// It came back with nothing drawn and may yet draw.
    Waiting,
    /// It will not draw: open the page again.
    Gone,
}

/// How long a page that came back empty gets to draw on its own.
const BLANK_LIMIT: Duration = Duration::from_secs(3);
/// How long a page that came back keeps being checked: WebKit may let go of
/// its layers just after it is shown.
const RETURN_WATCH: Duration = Duration::from_millis(1500);

impl Blank {
    /// What a page that came back `away` ago amounts to, drawn or not.
    fn of(painted: bool, away: Duration) -> Option<Self> {
        match (painted, away >= RETURN_WATCH, away >= BLANK_LIMIT) {
            (true, true, _) => None,
            (true, false, _) => Some(Self::No),
            (false, _, false) => Some(Self::Waiting),
            (false, _, true) => Some(Self::Gone),
        }
    }
}

/// Whether any layer under `layer` holds drawn content. WebKit keeps a page's
/// tiles in the view's own layer tree, and after a long time hidden it empties
/// that tree until the page process commits a new one. The page is clear over
/// the thread, so until then nothing at all shows where it is.
fn holds_content(layer: &CALayer) -> bool {
    let contents: Option<Retained<objc2::runtime::AnyObject>> =
        unsafe { msg_send![layer, contents] };
    if contents.is_some() {
        return true;
    }
    let sublayers: Option<Retained<NSArray<CALayer>>> = unsafe { msg_send![layer, sublayers] };
    sublayers.is_some_and(|layers| layers.iter().any(|layer| holds_content(&layer)))
}

fn painted(web: &WKWebView) -> bool {
    let layer: Option<Retained<CALayer>> = unsafe { msg_send![web, layer] };
    layer.is_some_and(|layer| holds_content(&layer))
}

/// While a popover covers the panel, a still of the page is painted in its
/// place and the native view steps aside.
#[derive(Default)]
pub(super) struct Cover {
    snapshot: Option<Arc<gpui::RenderImage>>,
    pending: bool,
    /// Set once nothing covers the page, so the next render frees the still.
    retired: bool,
}

/// How a draft is laid out in the space the thread gives it.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(crate) struct Fit {
    /// Display scale: the whole width fits, never larger than life.
    pub scale: f32,
}

/// A draft is as wide as its column allows and as tall as that makes it: the
/// thread scrolls, so height never limits the scale.
pub(crate) fn fit(width: u32, area: (f32, f32)) -> Fit {
    Fit {
        scale: (area.0 / width.max(1) as f32).clamp(0.05, 1.),
    }
}

/// A draft's native view and what the sentinel needs to keep it right.
struct Tracked {
    view: std::rc::Weak<RefCell<Option<Hosted>>>,
    painted: Painted,
    leases: Leases,
    shown: Shown,
    cover: Rc<RefCell<Cover>>,
    replay: Rc<RefCell<Option<Replay>>>,
}
type Leases = Rc<std::cell::Cell<u32>>;
/// The window whose latest frame holds the draft's card, drawn anew or
/// replayed from cache; set by `presence::Presence`.
type Shown = Rc<std::cell::Cell<Option<gpui::WindowId>>>;

/// What a frame that replays the card from cache needs to keep the page
/// clear of popovers: the card's hitbox, which a replay keeps, the card's
/// view, and a way to redraw it once WebKit hands over a still.
struct Replay {
    hitbox: gpui::Hitbox,
    card: gpui::EntityId,
    notify: Rc<dyn Fn()>,
}
type Painted = Rc<std::cell::Cell<(gpui::WindowId, u64)>>;

thread_local! {
    /// Bumped before each paint of a window, so a draft can tell whether it
    /// was drawn in the paint that just happened.
    static EPOCH: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static HOSTS: RefCell<Vec<Tracked>> = const { RefCell::new(Vec::new()) };
}

/// Put last in a window's root: a native view stays where it was last placed
/// and draws over everything GPUI paints, so any draft of this window that was
/// not painted this time steps aside. That covers a row scrolled far away or
/// dropped from the list, a conversation in the background, and a surface
/// such as Settings drawn in the thread's place. Anything that floats over a
/// thread has to be `deferred`, so the page can see it is covered, or hold a
/// `float`, so the page leaves a hole for it.
pub(crate) fn sentinel() -> gpui::AnyElement {
    canvas(
        |_, _, _| EPOCH.with(|epoch| epoch.set(epoch.get() + 1)),
        |_, _, window, cx| {
            free_retired(window, cx);
            let now = (
                Window::window_handle(window).window_id(),
                EPOCH.with(std::cell::Cell::get),
            );
            HOSTS.with(|hosts| {
                hosts.borrow_mut().retain(|tracked| {
                    let Some(view) = tracked.view.upgrade() else {
                        return false;
                    };
                    // Another window's drafts are its own sentinel's to hide.
                    if tracked.painted.get().0 != now.0 || tracked.painted.get() == now {
                        return true;
                    }
                    let mut view = view.borrow_mut();
                    let Some(hosted) = view.as_mut() else {
                        return true;
                    };
                    // The thread was replayed from cache with the card in it:
                    // the page stays where it is, but a popover may have
                    // opened over it since.
                    let replay = tracked.replay.borrow();
                    match replay
                        .as_ref()
                        .filter(|_| tracked.shown.get() == Some(now.0))
                    {
                        // The frame replays the card's still. The page was
                        // last placed where the card used to be, so it stays
                        // off until the settle repaint places it.
                        Some(_) if hosted.glide.on => {}
                        Some(replay) => {
                            let covered = window.is_hitbox_covered(&replay.hitbox);
                            keep_clear(
                                hosted,
                                &tracked.cover,
                                covered,
                                &tracked.leases,
                                replay,
                                window,
                            );
                        }
                        None => hosted.step_aside(tracked.leases.get() > 0),
                    }
                    true
                });
            });
        },
    )
    .absolute()
    .size_0()
    .into_any_element()
}

/// The web view of one draft, shared with the tasks that read it.
#[derive(Clone, Default)]
pub(super) struct Host {
    view: Rc<RefCell<Option<Hosted>>>,
    /// The window and epoch of the last paint that drew the draft.
    painted: Painted,
    cover: Rc<RefCell<Cover>>,
    /// The panel area the draft was last painted in, and the window zoom.
    area: Rc<std::cell::Cell<Option<(f32, f32, f32)>>>,
    /// A viewport the page is laid out at for a moment, out of sight, to
    /// measure it at another width. Paints keep it until it is cleared.
    measuring: Rc<std::cell::Cell<Option<(u32, u32)>>>,
    input: Input,
    wakes: Wakes,
    leases: Leases,
    shown: Shown,
    /// The frame that holds the card; see `presence::Presence`.
    pub(super) claim: Rc<std::cell::Cell<Option<super::presence::Claim>>>,
    replay: Rc<RefCell<Option<Replay>>>,
}

/// While held, the agent is working with the page: when its card is not on
/// screen, because the conversation is in the background or the card
/// scrolled away, the page is parked instead of hidden. A parked page stays
/// in the window, clear and out of the pointer's way, so WebKit keeps running
/// and drawing it. Without a lease an unseen page is hidden, and paused.
pub(crate) struct Lease(Host);

impl Drop for Lease {
    fn drop(&mut self) {
        let host = &self.0;
        host.leases.set(host.leases.get().saturating_sub(1));
        if host.leases.get() == 0
            && let Some(hosted) = host.view.borrow_mut().as_mut()
            && hosted.clip.ivars().parked.get()
        {
            hosted.step_aside(false);
        }
    }
}

impl Host {
    /// Keeps the page running for the agent until the lease drops, creating
    /// it in `window` if its card has never been painted.
    pub(super) fn lease(&self, placement: &Placement, window: &Window) -> Lease {
        self.leases.set(self.leases.get() + 1);
        self.track();
        let mut view = self.view.borrow_mut();
        if view.is_none()
            && let Some(mtm) = MainThreadMarker::new()
            && let Ok(handle) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::AppKit(handle) = handle.as_raw()
        {
            // GPUI keeps its AppKit content view alive with the window.
            let parent = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
            let size = NSSize::new(f64::from(placement.width), f64::from(placement.height));
            let rect = NSRect::new(NSPoint::new(0., 0.), size);
            let mut hosted = unsafe {
                create(
                    mtm,
                    rect,
                    rect,
                    placement,
                    self.input.clone(),
                    self.wakes.send.clone(),
                    self.wakes.pings.clone(),
                )
            };
            unsafe { load(&hosted.web, &placement.load) };
            hosted.loaded = Some((placement.generation, placement.load.clone()));
            parent.addSubview_positioned_relativeTo(
                &hosted.clip,
                NSWindowOrderingMode::Above,
                None,
            );
            hosted.step_aside(true);
            // Laid out at life size until the card gives it a column.
            if self.area.get().is_none() {
                self.area
                    .set(Some((placement.width as f32, placement.height as f32, 1.)));
            }
            *view = Some(hosted);
        } else if let Some(hosted) = view.as_mut()
            && hosted.clip.isHidden()
        {
            hosted.step_aside(true);
        }
        Lease(self.clone())
    }
    /// Gives an open page a newer version of the draft now, rather than at
    /// the card's next paint.
    pub(super) fn refresh(&self, placement: &Placement) {
        if let Some(hosted) = self.view.borrow_mut().as_mut()
            && hosted
                .loaded
                .as_ref()
                .is_none_or(|(generation, _)| *generation != placement.generation)
        {
            unsafe { load(&hosted.web, &placement.load) };
            hosted.loaded = Some((placement.generation, placement.load.clone()));
        }
    }
    /// Lets the sentinel step this page aside when its card is not painted.
    fn track(&self) {
        HOSTS.with(|hosts| {
            let mut hosts = hosts.borrow_mut();
            if !hosts
                .iter()
                .any(|tracked| std::ptr::eq(tracked.view.as_ptr(), Rc::as_ptr(&self.view)))
            {
                hosts.push(Tracked {
                    view: Rc::downgrade(&self.view),
                    painted: self.painted.clone(),
                    leases: self.leases.clone(),
                    shown: self.shown.clone(),
                    cover: self.cover.clone(),
                    replay: self.replay.clone(),
                });
            }
        });
    }
    /// Resolves when the page or the shared record has something new.
    pub(super) fn woken(&self) -> async_channel::Receiver<()> {
        self.wakes.receive.clone()
    }
    /// Reads the page again soon, as when the phone changed the record.
    pub(super) fn wake(&self) {
        self.wakes.asks.set(self.wakes.asks.get() + 1);
        let _ = self.wakes.send.try_send(());
    }
    /// Runs the reader's checks again without asking it to read the page.
    pub(super) fn nudge(&self) {
        let _ = self.wakes.send.try_send(());
    }
    /// Grows whenever there may be something new to read from the page. A
    /// wake without it, such as the tick's, has nothing to read.
    pub(super) fn demand(&self) -> u64 {
        self.wakes.pings.get() + self.wakes.asks.get()
    }
    /// Wakes the reader from another thread.
    pub(super) fn waker(&self) -> impl Fn() + Send + use<> {
        let send = self.wakes.send.clone();
        move || {
            let _ = send.try_send(());
        }
    }
    /// When the person last clicked or typed in the page, as AppKit saw it.
    pub(super) fn input(&self) -> Option<std::time::Instant> {
        self.input.get()
    }
    /// Stands in for the person's own click, for the self-test.
    #[cfg(debug_assertions)]
    pub(super) fn simulate_input(&self) {
        self.input.set(Some(std::time::Instant::now()));
    }
    pub(super) fn web(&self) -> Option<Retained<WKWebView>> {
        self.view.borrow().as_ref().map(|view| view.web.clone())
    }
    /// Where the native view sits in the window, in points: x, y, width,
    /// height, and whether it is hidden.
    #[cfg(debug_assertions)]
    pub(super) fn placement(&self) -> Option<[f64; 5]> {
        self.view.borrow().as_ref().map(|view| {
            let frame = view.clip.frame();
            [
                frame.origin.x,
                frame.origin.y,
                frame.size.width,
                frame.size.height,
                f64::from(u8::from(view.clip.isHidden())),
            ]
        })
    }
    /// Each hole as x, y, width and height in the clip view, its radius, and
    /// whether a press at its center still reaches the page.
    #[cfg(debug_assertions)]
    pub(super) fn holes(&self) -> Vec<[f64; 6]> {
        let view = self.view.borrow();
        let Some(view) = view.as_ref() else {
            return Vec::new();
        };
        let holes = view.clip.ivars().holes.borrow().clone();
        let parent = unsafe { view.clip.superview() };
        holes
            .iter()
            .map(|hole| {
                let center = NSPoint::new(hole.x + hole.width / 2., hole.y + hole.height / 2.);
                let outside = view.clip.convertPoint_toView(center, parent.as_deref());
                let hit: Option<Retained<NSView>> =
                    unsafe { msg_send![&*view.clip, hitTest: outside] };
                [
                    hole.x,
                    hole.y,
                    hole.width,
                    hole.height,
                    hole.radius,
                    f64::from(u8::from(hit.is_some())),
                ]
            })
            .collect()
    }
    /// Whether the latest frame of `window` holds the card.
    #[cfg(test)]
    pub(super) fn painted_in(&self, window: gpui::WindowId) -> bool {
        self.claim.get().is_some_and(|claim| claim.window == window)
    }
    /// Whether an agent's lease is keeping the page running.
    #[cfg(test)]
    pub(super) fn leased(&self) -> bool {
        self.leases.get() > 0
    }
    /// The width the thread gave the draft when it was last painted.
    pub(super) fn column(&self) -> Option<f32> {
        self.area.get().map(|(width, _, _)| width)
    }
    /// The page is running: on screen, or parked for the agent.
    pub(super) fn visible(&self) -> bool {
        self.view
            .borrow()
            .as_ref()
            .is_some_and(|view| !view.clip.isHidden())
    }
    pub(super) fn hide(&self) {
        if let Some(view) = self.view.borrow_mut().as_mut() {
            view.step_aside(false);
        }
    }
    /// The page's process and whether it has anything drawn, for the self-test.
    #[cfg(debug_assertions)]
    pub(super) fn page_process(&self) -> Option<(i32, bool)> {
        let view = self.view.borrow();
        let view = view.as_ref()?;
        let pid: i32 = unsafe { msg_send![&*view.web, _webProcessIdentifier] };
        Some((pid, painted(&view.web)))
    }
    /// Whether the page has anything on screen; see `Blank`.
    pub(super) fn blank(&self) -> Blank {
        let mut view = self.view.borrow_mut();
        let Some(view) = view.as_mut() else {
            return Blank::No;
        };
        if view.died.get() {
            return Blank::Gone;
        }
        let Some(returned) = view.returned else {
            return Blank::No;
        };
        if view.clip.isHidden() {
            view.returned = None;
            return Blank::No;
        }
        match Blank::of(painted(&view.web), returned.elapsed()) {
            Some(blank) => blank,
            None => {
                view.returned = None;
                Blank::No
            }
        }
    }
    /// Says which window's frame holds the card, if any; see `Shown`.
    pub(super) fn set_shown(&self, window: Option<gpui::WindowId>) {
        self.shown.set(window);
    }
    /// The card left the frame: the page steps aside, or parks for the agent.
    pub(super) fn off_screen(&self) {
        if let Some(view) = self.view.borrow_mut().as_mut() {
            view.step_aside(self.leases.get() > 0);
        }
    }
    /// Lays the page out for a new viewport now rather than at the next
    /// frame, so a picture taken straight after a resize is of the new size.
    pub(super) fn resize(&self, width: u32, height: u32) {
        let Some((w, h, zoom)) = self.area.get() else {
            return;
        };
        let mut view = self.view.borrow_mut();
        let Some(view) = view.as_mut() else {
            return;
        };
        // Measuring lays the page out at life size: scaled down, WebKit's
        // smallest font size would make the text taller than it is.
        let scale = if self.measuring.get().is_some() {
            1.
        } else {
            fit(width, (w, h)).scale
        };
        let size = NSSize::new(
            f64::from(width as f32 * scale * zoom),
            f64::from(height as f32 * scale * zoom),
        );
        let (clip, web) = (view.clip.frame().origin, view.web.frame().origin);
        view.clip.setFrame(NSRect::new(clip, size));
        view.web.setFrame(NSRect::new(web, size));
        unsafe { view.web.setPageZoom(f64::from(scale * zoom)) };
        // The next paint places it again.
        view.placed = [f64::NAN; 9];
    }
    /// Lays the hidden page out at another viewport to measure it, or back at
    /// its own with `None`.
    pub(super) fn measure_at(&self, viewport: Option<(u32, u32)>, own: (u32, u32)) {
        self.measuring.set(viewport);
        let (width, height) = viewport.unwrap_or(own);
        self.resize(width, height);
    }
    /// The page is laid out at another width for measuring, so its height is
    /// not its own.
    pub(super) fn measuring(&self) -> bool {
        self.measuring.get().is_some()
    }
    /// Whether the page scrolls inside its card, and so keeps the wheel.
    pub(super) fn set_scrolls(&self, scrolls: bool) {
        if let Some(view) = self.view.borrow().as_ref() {
            view.clip.ivars().scrolls.set(scrolls);
        }
    }
    /// False while there is no web view yet to take it.
    pub(super) fn set_appearance(&self, light: Option<bool>) -> bool {
        let Some(view) = self.view.borrow().as_ref().map(|view| view.web.clone()) else {
            return false;
        };
        {
            let name = light.map(|light| unsafe {
                if light {
                    NSAppearanceNameAqua
                } else {
                    NSAppearanceNameDarkAqua
                }
            });
            view.setAppearance(name.and_then(NSAppearance::appearanceNamed).as_deref());
        }
        true
    }
    /// Takes a new still of the page for its card to draw while it moves.
    /// A card with none to draw gets one at once, even mid scroll: one
    /// picture costs less than moving the page every frame. After that a new
    /// one is taken once the page in view has looked the same for `QUIET`,
    /// so that work never lands in a scroll. Returns how soon to ask again.
    pub(super) fn freshen_still(&self, generation: u64) -> Option<Duration> {
        let mut view = self.view.borrow_mut();
        let hosted = view.as_mut()?;
        let now = std::time::Instant::now();
        let in_view = !hosted.clip.isHidden()
            && !hosted.clip.ivars().parked.get()
            && hosted.alpha >= 1.
            && self.measuring.get().is_none()
            && self.cover.borrow().snapshot.is_none();
        if !in_view
            || !hosted
                .loaded
                .as_ref()
                .is_some_and(|(g, _)| *g == generation)
        {
            return None;
        }
        let key = hosted.still_key(generation, self.wakes.pings.get(), self.input.get());
        let (same, stands, early) = hosted
            .still
            .as_ref()
            .map_or((false, false, false), |still| {
                (still.key == key, still.key.stands_for(&key), still.early)
            });
        if same && !early {
            return None;
        }
        if hosted.taking.get() || hosted.returned.is_some() {
            return Some(SETTLE);
        }
        // A click or a key press changes the page over the next moments;
        // picture what it settles into.
        let pressed = key
            .input
            .is_some_and(|at| now.saturating_duration_since(at) < QUIET);
        let early = early_stills() && !stands && !pressed;
        if !early {
            if !hosted.glide.still_for(QUIET, now) {
                return Some(QUIET);
            }
            match hosted.seen {
                Some((seen, since)) if seen == key => {
                    let waited = now.saturating_duration_since(since);
                    if waited < QUIET {
                        return Some(QUIET - waited);
                    }
                }
                _ => {
                    hosted.seen = Some((key, now));
                    return Some(QUIET);
                }
            }
        }
        if !painted(&hosted.web) {
            return Some(SETTLE);
        }
        hosted.taking.set(true);
        let taking = hosted.taking.clone();
        let view = Rc::downgrade(&self.view);
        let done = block2::RcBlock::new(move |image: *mut NSImage, _: *mut NSError| {
            taking.set(false);
            // SAFETY: WebKit passes a live image or null for the duration of the call.
            let Some(image) = unsafe { image.as_ref() }.and_then(pixels).map(Arc::new) else {
                return;
            };
            let Some(view) = view.upgrade() else {
                retire(image);
                return;
            };
            let Ok(mut view) = view.try_borrow_mut() else {
                retire(image);
                return;
            };
            // The page may have been replaced while WebKit drew it.
            match view.as_mut() {
                Some(hosted) if web_size(hosted.web.frame()) == key.size => {
                    if let Some(old) = hosted.still.replace(Still { image, key, early }) {
                        retire(old.image);
                    }
                }
                _ => retire(image),
            }
        });
        unsafe {
            hosted
                .web
                .takeSnapshotWithConfiguration_completionHandler(None, &done)
        };
        // The calmer one that follows an early still.
        early.then_some(QUIET)
    }
    /// Whether the web view has been given `generation` of the draft's content.
    pub(super) fn has_loaded(&self, generation: u64) -> bool {
        self.view
            .borrow()
            .as_ref()
            .and_then(|view| view.loaded.as_ref())
            .is_some_and(|(loaded, _)| *loaded == generation)
    }
}

impl Hosted {
    /// Out of the way while its card is not drawn: parked if the agent holds
    /// a lease on it, hidden otherwise.
    fn step_aside(&mut self, leased: bool) {
        let parked = &self.clip.ivars().parked;
        if leased {
            if !parked.get() {
                parked.set(true);
                self.clip.setAlphaValue(0.);
                self.alpha = 0.;
            }
            if self.clip.isHidden() {
                self.clip.setHidden(false);
            }
        } else {
            parked.set(false);
            if !self.clip.isHidden() {
                self.drew = painted(&self.web);
                self.returned = None;
                self.clip.setHidden(true);
            }
        }
    }
    /// Cuts the page away under the controls floating over it. A native view
    /// draws above all of GPUI, so a hole is the only way for them to show.
    fn cut_out(&mut self, floats: &[(gpui::Bounds<gpui::Pixels>, gpui::Pixels)], zoom: f32) {
        let holes: Vec<Hole> = floats
            .iter()
            .map(|(bounds, radius)| {
                Hole::new(bounds.map(|v| v * zoom), *radius * zoom, self.origin)
            })
            .collect();
        let frame = self.clip.frame().size;
        let size = (frame.width, frame.height);
        let same = *self.clip.ivars().holes.borrow() == holes;
        if same && (holes.is_empty() || size == self.cut) {
            return;
        }
        let layer: Option<Retained<CALayer>> = unsafe { msg_send![&*self.clip, layer] };
        if let Some(layer) = layer {
            // A shape layer's path otherwise eases to its new value.
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            let mask = mask_for(&holes, frame);
            unsafe { layer.setMask(mask.as_deref()) };
            CATransaction::commit();
        }
        self.cut = size;
        *self.clip.ivars().holes.borrow_mut() = holes;
    }

    /// Off its card while a still stands in for it: clear, so it keeps
    /// running without being drawn, and out of the pointer's way. A hidden
    /// page stays hidden.
    fn hold_still(&mut self) {
        self.glide.on = true;
        if self.clip.isHidden() {
            return;
        }
        self.clip.ivars().parked.set(true);
        if self.alpha != 0. {
            self.clip.setAlphaValue(0.);
            self.alpha = 0.;
        }
    }

    /// What the page looks like it is now, for a still of it.
    fn still_key(
        &self,
        generation: u64,
        pings: u64,
        input: Option<std::time::Instant>,
    ) -> StillKey {
        StillKey {
            generation,
            pings,
            input,
            restyles: self.restyles,
            size: web_size(self.web.frame()),
        }
    }

    /// Back in its card.
    fn step_in(&mut self) {
        self.glide.on = false;
        self.clip.ivars().parked.set(false);
        if self.clip.isHidden() {
            self.clip.setHidden(false);
            if self.drew {
                self.returned = Some(std::time::Instant::now());
                let _ = self.wake.try_send(());
            }
        }
    }
}

impl Drop for Hosted {
    fn drop(&mut self) {
        // The content controller holds the handler; let both go with the page.
        unsafe {
            self.web
                .configuration()
                .userContentController()
                .removeScriptMessageHandlerForName(&NSString::from_str(WAKE));
        }
        if let Some(still) = self.still.take() {
            retire(still.image);
        }
        self.clip.removeFromSuperview();
    }
}

/// Everything the paint callback needs about the draft.
#[derive(Clone)]
pub(super) struct Placement {
    pub load: Load,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    /// A script that restyles the open page in place, when the theme changed
    /// but the content did not.
    pub retheme: Option<String>,
    /// When the page was ready to be seen; until then it stays transparent
    /// over the card's placeholder, then fades in.
    pub revealed: Option<std::time::Instant>,
    pub app: gpui::AsyncApp,
}

/// How long a ready page takes to fade in over its placeholder.
const FADE: Duration = Duration::from_millis(180);

/// Paints the draft: places the web view where this canvas is and keeps a still
/// of it for when a popover covers it. Returns the element and, via `Host`,
/// the handle other tasks use.
pub(super) fn surface<T: 'static>(
    host: &Host,
    placement: Placement,
    window: &mut Window,
    cx: &mut Context<T>,
) -> gpui::AnyElement {
    let this = cx.entity().downgrade();
    let view = host.view.clone();
    let cover = host.cover.clone();
    let area = host.area.clone();
    let painted = host.painted.clone();
    let measuring = host.measuring.clone();
    let input = host.input.clone();
    let wakes = host.wakes.send.clone();
    let pings = host.wakes.pings.clone();
    let leases = host.leases.clone();
    let replay = host.replay.clone();
    host.track();
    let snapshot = {
        let mut state = cover.borrow_mut();
        if std::mem::take(&mut state.retired)
            && let Some(image) = state.snapshot.take()
        {
            cx.drop_image(image, Some(window));
        }
        state.snapshot.clone()
    };
    let paint = canvas(
        |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
        move |bounds, hitbox, window, cx| {
            painted.set((
                Window::window_handle(window).window_id(),
                EPOCH.with(std::cell::Cell::get),
            ));
            let covered = window.is_hitbox_covered(&hitbox);
            let Ok(handle) = HasWindowHandle::window_handle(window) else {
                return;
            };
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                return;
            };
            // GPUI keeps its AppKit content view alive throughout paint.
            let parent = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
            let zoom = window.content_zoom();
            let available = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            area.set(Some((available.0, available.1, zoom)));
            let (width, height, scale) = match measuring.get() {
                Some((width, height)) => (width, height, 1.),
                None => (
                    placement.width,
                    placement.height,
                    fit(placement.width, available).scale,
                ),
            };
            let shown_size = gpui::size(
                gpui::px(width as f32 * scale),
                gpui::px(height as f32 * scale),
            );
            let draft = gpui::Bounds::new(
                gpui::point(
                    bounds.left() + (bounds.size.width - shown_size.width) / 2.,
                    bounds.top(),
                ),
                shown_size,
            );
            let logical = draft;
            let clipped = draft
                .intersect(&window.content_mask().bounds)
                .map(|v| v * zoom);
            let draft = draft.map(|v| v * zoom);
            let mut view = view.borrow_mut();
            if clipped.size.width <= gpui::px(0.) || clipped.size.height <= gpui::px(0.) {
                if let Some(view) = view.as_mut() {
                    view.step_aside(leases.get() > 0);
                }
                return;
            }
            let (x, y) = (f64::from(clipped.origin.x), f64::from(clipped.origin.y));
            let origin = (x, y);
            let (clip_width, clip_height) = (
                f64::from(clipped.size.width),
                f64::from(clipped.size.height),
            );
            let y = if parent.isFlipped() {
                y
            } else {
                parent.bounds().size.height - y - clip_height
            };
            let rect = NSRect::new(NSPoint::new(x, y), NSSize::new(clip_width, clip_height));
            let web_rect = NSRect::new(
                NSPoint::new(
                    f64::from(draft.left() - clipped.left()),
                    f64::from(draft.top() - clipped.top()),
                ),
                NSSize::new(f64::from(draft.size.width), f64::from(draft.size.height)),
            );
            if view.is_none() {
                let Some(mtm) = MainThreadMarker::new() else {
                    return;
                };
                *view = Some(unsafe {
                    create(
                        mtm,
                        rect,
                        web_rect,
                        &placement,
                        input.clone(),
                        wakes.clone(),
                        pings.clone(),
                    )
                });
            }
            let Some(hosted) = view.as_mut() else {
                return;
            };
            if hosted
                .loaded
                .as_ref()
                .is_none_or(|(generation, _)| *generation != placement.generation)
            {
                unsafe { load(&hosted.web, &placement.load) };
                hosted.loaded = Some((placement.generation, placement.load.clone()));
            } else if let Some(script) = &placement.retheme {
                unsafe {
                    hosted
                        .web
                        .evaluateJavaScript_completionHandler(&NSString::from_str(script), None);
                }
                hosted.restyles += 1;
            }
            // Re-adding a subview pulls it out of the hierarchy, which blanks
            // WebKit; only attach when it is not already there.
            let attached =
                unsafe { hosted.clip.superview() }.is_some_and(|sup| std::ptr::eq(&*sup, parent));
            if !attached {
                parent.addSubview_positioned_relativeTo(
                    &hosted.clip,
                    NSWindowOrderingMode::Above,
                    None,
                );
            }
            let notify: Rc<dyn Fn()> = {
                let app = placement.app.clone();
                let this = this.clone();
                Rc::new(move || {
                    let _ = app.update(|cx| this.update(cx, |_, cx| cx.notify()));
                })
            };
            let shown = Replay {
                hitbox,
                card: this.entity_id(),
                notify,
            };
            free_retired(window, cx);
            let moving = hosted.glide.track(logical, std::time::Instant::now());
            let now_key = StillKey {
                generation: placement.generation,
                pings: 0,
                input: input.get(),
                restyles: hosted.restyles,
                size: web_size(web_rect),
            };
            let still = hosted
                .still
                .as_ref()
                .filter(|still| still.key.stands_for(&now_key))
                .map(|still| still.image.clone());
            let ready = placement.revealed.is_some_and(|at| at.elapsed() >= FADE);
            let still_missing = still.is_none();
            if moving
                && ready
                && measuring.get().is_none()
                && let Some(still) = still
            {
                let _ =
                    window.paint_image(logical, logical, gpui::Corners::default(), still, 0, false);
                hosted.hold_still();
                count(|stats| stats.still += 1);
                *replay.borrow_mut() = Some(shown);
                // Bring the page back once the card has stayed put.
                if !hosted.glide.settling.replace(true) {
                    let settling = hosted.glide.settling.clone();
                    let card = this.entity_id();
                    cx.spawn(async move |cx| {
                        cx.background_executor().timer(SETTLE).await;
                        settling.set(false);
                        cx.update(|cx| cx.notify(card));
                    })
                    .detach();
                }
                return;
            }
            let placed = [
                rect.origin.x,
                rect.origin.y,
                rect.size.width,
                rect.size.height,
                web_rect.origin.x,
                web_rect.origin.y,
                web_rect.size.width,
                web_rect.size.height,
                f64::from(scale * zoom),
            ];
            // Nothing to draw should the card move: have a still taken now.
            if ready
                && still_missing
                && early_stills()
                && hosted
                    .asked
                    .is_none_or(|at| at.elapsed() >= Duration::from_millis(250))
            {
                hosted.asked = Some(std::time::Instant::now());
                let _ = wakes.try_send(());
            }
            if hosted.placed != placed {
                if moving {
                    count(|stats| stats.dragged += 1);
                }
                hosted.clip.setFrame(rect);
                hosted.web.setFrame(web_rect);
                // The page lays out at its logical width and is scaled to fit.
                unsafe { hosted.web.setPageZoom(f64::from(scale * zoom)) };
                hosted.placed = placed;
            }
            hosted.origin = origin;
            let clear = keep_clear(hosted, &cover, covered, &leases, &shown, window);
            *replay.borrow_mut() = Some(shown);
            if !clear {
                return;
            }
            let alpha = placement.revealed.map_or(0., |at| {
                let t = (at.elapsed().as_secs_f64() / FADE.as_secs_f64()).min(1.);
                1. - (1. - t).powi(3)
            });
            if hosted.alpha != alpha {
                hosted.clip.setAlphaValue(alpha);
                hosted.alpha = alpha;
            }
            if alpha < 1. && placement.revealed.is_some() {
                window.request_animation_frame();
            }
        },
    )
    .absolute()
    .size_full();
    gpui::div()
        .relative()
        .size_full()
        .children(snapshot.map(|image| gpui::img(image).absolute().size_full()))
        .child(paint)
        .into_any_element()
}

/// Keeps a popover clear of the page: while one covers it, a still of the
/// page is drawn in its place and the native view steps aside. Returns
/// whether the page is in view.
fn keep_clear(
    hosted: &mut Hosted,
    cover: &Rc<RefCell<Cover>>,
    covered: bool,
    leases: &Leases,
    card: &Replay,
    window: &Window,
) -> bool {
    hosted.cut_out(&window.floats_over(&card.hitbox), window.content_zoom());
    let mut state = cover.borrow_mut();
    let still = state.snapshot.is_some();
    if !covered {
        state.pending = false;
        if still {
            // The card's next render frees the still.
            state.retired = true;
            let card = card.card;
            window.on_next_frame(move |_, cx| cx.notify(card));
        }
    } else if !still && !state.pending {
        // The page stays up until its still arrives, a frame or two,
        // rather than leaving a hole.
        state.pending = true;
        let notify = card.notify.clone();
        still_of(&hosted.web, cover.clone(), move || notify());
    }
    drop(state);
    if covered && still {
        hosted.step_aside(leases.get() > 0);
        return false;
    }
    hosted.step_in();
    true
}

/// A mask for a view of `size` with `holes` cut out, or none without any. Each
/// hole is a layer masking the next: holes that overlap would cancel out in a
/// single even-odd path, and show the page again where they meet.
fn mask_for(holes: &[Hole], size: NSSize) -> Option<Retained<CALayer>> {
    let bounds = NSRect::new(NSPoint::new(0., 0.), size);
    let mut mask: Option<Retained<CALayer>> = None;
    for hole in holes.iter().rev() {
        let path = CGMutablePath::new();
        unsafe {
            CGMutablePath::add_rect(Some(&path), std::ptr::null(), bounds);
            CGMutablePath::add_rounded_rect(
                Some(&path),
                std::ptr::null(),
                hole.rect(),
                hole.radius,
                hole.radius,
            );
        }
        let layer = CAShapeLayer::new();
        layer.setFrame(bounds);
        layer.setFillRule(unsafe { kCAFillRuleEvenOdd });
        layer.setPath(Some(&path));
        unsafe { layer.setMask(mask.as_deref()) };
        mask = Some(layer.into_super());
    }
    mask
}

/// # Safety
/// Main thread only.
unsafe fn create(
    mtm: MainThreadMarker,
    rect: NSRect,
    web_rect: NSRect,
    placement: &Placement,
    input: Input,
    wakes: async_channel::Sender<()>,
    pings: Pings,
) -> Hosted {
    watch_keys(mtm);
    // A dev server's page may scroll itself; only the agent's own page is known.
    let clip = DraftClip::alloc(mtm).set_ivars(Clip {
        scrolls: std::cell::Cell::new(matches!(placement.load, Load::Url(_))),
        input,
        parked: std::cell::Cell::new(false),
        holes: RefCell::default(),
    });
    let clip: Retained<DraftClip> = unsafe { msg_send![super(clip), initWithFrame: rect] };
    clip.setWantsLayer(true);
    clip.setClipsToBounds(true);
    // No file base URL, app credentials or persistent website data, and only
    // the wake handler: a draft is a page, not part of the app.
    let config = unsafe { WKWebViewConfiguration::new(mtm) };
    unsafe {
        config
            .preferences()
            .setJavaScriptCanOpenWindowsAutomatically(false);
        config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm));
    }
    let policy = match placement.load {
        Load::Html(_) => Policy::Page(placement.app.clone()),
        Load::Url(_) => {
            // A dev server's page is not ours to wrap, so the probe goes in
            // beside it as a user script.
            let script = unsafe {
                WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                    WKUserScript::alloc(mtm),
                    &NSString::from_str(&super::page::browser_probe()),
                    WKUserScriptInjectionTime::AtDocumentStart,
                    true,
                )
            };
            let controller: Retained<WKUserContentController> =
                unsafe { config.userContentController() };
            unsafe { controller.addUserScript(&script) };
            Policy::Local(placement.app.clone())
        }
    };
    let died = Rc::new(std::cell::Cell::new(false));
    let wake = Wake::alloc(mtm).set_ivars((wakes.clone(), pings));
    let wake: Retained<Wake> = unsafe { msg_send![super(wake), init] };
    unsafe {
        config.userContentController().addScriptMessageHandler_name(
            ProtocolObject::from_ref(&*wake),
            &NSString::from_str(WAKE),
        );
    }
    let web =
        unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), web_rect, &config) };
    // The agent's page sits on the thread's own background, so it reads as
    // part of the conversation. A dev server paints its own.
    if matches!(placement.load, Load::Html(_)) {
        let _: () = unsafe {
            msg_send![&web, setValue: &*NSNumber::new_bool(false), forKey: &*NSString::from_str("drawsBackground")]
        };
    }
    let guard = Navigation::alloc(mtm).set_ivars(Guard {
        policy,
        died: died.clone(),
        wake: wakes.clone(),
    });
    let guard: Retained<Navigation> = unsafe { msg_send![super(guard), init] };
    unsafe {
        web.setNavigationDelegate(Some(ProtocolObject::from_ref(&*guard)));
    }
    clip.addSubview(&web);
    Hosted {
        clip,
        web,
        _guard: guard,
        loaded: None,
        placed: [f64::NAN; 9],
        origin: (0., 0.),
        cut: (0., 0.),
        alpha: f64::NAN,
        drew: false,
        returned: None,
        died,
        wake: wakes,
        restyles: 0,
        glide: Glide::default(),
        still: None,
        seen: None,
        taking: Rc::default(),
        asked: None,
    }
}

/// # Safety
/// Main thread only.
unsafe fn load(web: &WKWebView, load: &Load) {
    match load {
        Load::Html(html) => unsafe {
            web.loadHTMLString_baseURL(&NSString::from_str(html), None);
        },
        Load::Url(url) => {
            if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
                unsafe { web.loadRequest(&NSURLRequest::requestWithURL(&url)) };
            }
        }
    }
}

/// Runs `script` in a web view's top document and returns its string result.
pub(super) async fn evaluate(web: &WKWebView, script: &str, cx: &gpui::AsyncApp) -> Option<String> {
    // The completion handler or the deadline answers, whichever comes first.
    let (send, receive) = async_channel::bounded(2);
    let deadline = send.clone();
    let timer = cx.background_executor().timer(Duration::from_secs(3));
    cx.background_executor()
        .spawn(async move {
            timer.await;
            let _ = deadline.try_send(None);
        })
        .detach();
    let block = block2::RcBlock::new(
        move |value: *mut objc2::runtime::AnyObject, _: *mut NSError| {
            let text = unsafe { value.as_ref() }
                .and_then(|value| value.downcast_ref::<NSString>())
                .map(|text| text.to_string());
            let _ = send.try_send(text);
        },
    );
    unsafe {
        web.evaluateJavaScript_completionHandler(&NSString::from_str(script), Some(&block));
    }
    receive.recv().await.ok().flatten()
}

/// The page as a PNG, `width` points wide.
pub(super) async fn png(
    web: &Retained<WKWebView>,
    width: f64,
    cx: &gpui::AsyncApp,
) -> Result<(Vec<u8>, (u32, u32)), String> {
    let mtm = MainThreadMarker::new().ok_or("Drafts render on the main thread")?;
    let (send, receive) = async_channel::bounded(1);
    let block = block2::RcBlock::new(move |image: *mut NSImage, _: *mut NSError| {
        // SAFETY: WebKit passes a live image or null for the duration of the call.
        let _ = send.try_send(unsafe { image.as_ref() }.and_then(Shot::of));
    });
    unsafe {
        let config = WKSnapshotConfiguration::new(mtm);
        config.setSnapshotWidth(Some(&NSNumber::new_f64(width)));
        web.takeSnapshotWithConfiguration_completionHandler(Some(&config), &block);
    }
    let started = std::time::Instant::now();
    loop {
        if let Ok(shot) = receive.try_recv() {
            let shot = shot.ok_or("The page could not be captured")?;
            // Compressing a full page takes long enough to drop frames.
            return cx
                .background_executor()
                .spawn(async move { shot.png() })
                .await
                .ok_or_else(|| "The page could not be captured".to_string());
        }
        if started.elapsed() > Duration::from_secs(8) {
            return Err("The page could not be captured".into());
        }
        cx.background_executor()
            .timer(Duration::from_millis(30))
            .await;
    }
}

fn still_of(web: &WKWebView, cover: Rc<RefCell<Cover>>, done_notify: impl Fn() + 'static) {
    let done = block2::RcBlock::new(move |image: *mut NSImage, _: *mut NSError| {
        let mut state = cover.borrow_mut();
        if !std::mem::take(&mut state.pending) {
            return;
        }
        // SAFETY: WebKit passes a live image or null for the duration of the call.
        state.snapshot = unsafe { image.as_ref() }.and_then(pixels).map(Arc::new);
        drop(state);
        done_notify();
    });
    unsafe { web.takeSnapshotWithConfiguration_completionHandler(None, &done) };
}

/// A picture of the page as it is now, for the card to show while the next
/// version loads.
pub(super) async fn picture(web: &WKWebView) -> Option<Arc<gpui::RenderImage>> {
    let (send, receive) = async_channel::bounded(1);
    let done = block2::RcBlock::new(move |image: *mut NSImage, _: *mut NSError| {
        // SAFETY: WebKit passes a live image or null for the duration of the call.
        let _ = send.try_send(unsafe { image.as_ref() }.and_then(pixels).map(Arc::new));
    });
    unsafe { web.takeSnapshotWithConfiguration_completionHandler(None, &done) };
    receive.recv().await.ok().flatten()
}

/// Copies an 8-bit RGB(A) snapshot into the BGRA layout GPUI's sprites use.
fn pixels(image: &NSImage) -> Option<gpui::RenderImage> {
    let started = std::time::Instant::now();
    let picture = pixels_of(image);
    count(|stats| {
        stats.taken += 1;
        stats.convert_ms += started.elapsed().as_secs_f64() * 1000.;
    });
    picture
}

fn pixels_of(image: &NSImage) -> Option<gpui::RenderImage> {
    let picture = match Raw::of(image) {
        Some(raw) => raw.sprite(),
        None => sprite_of_tiff(image)?,
    };
    Some(gpui::RenderImage::new(vec![image::Frame::new(picture)]))
}

/// A snapshot's pixels as WebKit made them: four bytes each, read without
/// encoding the picture first.
struct Raw {
    bytes: Vec<u8>,
    width: usize,
    height: usize,
    stride: usize,
    order: Order,
}

/// Where a pixel's channels sit in its four bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Order {
    red: usize,
    green: usize,
    blue: usize,
    alpha: Option<usize>,
    /// The colours are already multiplied by the alpha.
    premultiplied: bool,
}

impl Order {
    /// The layout GPUI's sprites use.
    const SPRITE: [usize; 4] = [2, 1, 0, 3];

    fn of(info: u32) -> Option<Self> {
        let little = match info & CGBitmapInfo::ByteOrderInfoMask.0 {
            order if order == CGImageByteOrderInfo::Order32Little.0 => true,
            order
                if order == CGImageByteOrderInfo::OrderDefault.0
                    || order == CGImageByteOrderInfo::Order32Big.0 =>
            {
                false
            }
            _ => return None,
        };
        let alpha = CGImageAlphaInfo(info & CGBitmapInfo::AlphaInfoMask.0);
        let first = match alpha {
            CGImageAlphaInfo::PremultipliedFirst
            | CGImageAlphaInfo::First
            | CGImageAlphaInfo::NoneSkipFirst => true,
            CGImageAlphaInfo::PremultipliedLast
            | CGImageAlphaInfo::Last
            | CGImageAlphaInfo::NoneSkipLast => false,
            _ => return None,
        };
        // Alpha, red, green, blue as the bytes come in memory.
        let [a, red, green, blue] = match (first, little) {
            (true, false) => [0, 1, 2, 3],
            (true, true) => [3, 2, 1, 0],
            (false, false) => [3, 0, 1, 2],
            (false, true) => [0, 3, 2, 1],
        };
        let skipped = matches!(
            alpha,
            CGImageAlphaInfo::NoneSkipFirst | CGImageAlphaInfo::NoneSkipLast
        );
        Some(Self {
            red,
            green,
            blue,
            alpha: (!skipped).then_some(a),
            premultiplied: matches!(
                alpha,
                CGImageAlphaInfo::PremultipliedFirst | CGImageAlphaInfo::PremultipliedLast
            ),
        })
    }
}

impl Raw {
    fn of(image: &NSImage) -> Option<Self> {
        // SAFETY: a null rectangle asks for the image at its own size.
        let image = unsafe {
            image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None)
        }?;
        let image = Some(&*image);
        let info = CGImage::bitmap_info(image).0;
        if CGImage::bits_per_component(image) != 8
            || CGImage::bits_per_pixel(image) != 32
            || info & CGBitmapInfo::ComponentInfoMask.0 != 0
        {
            return None;
        }
        let order = Order::of(info)?;
        let (width, height) = (CGImage::width(image), CGImage::height(image));
        let stride = CGImage::bytes_per_row(image);
        let bytes = CGDataProvider::data(CGImage::data_provider(image).as_deref())?.to_vec();
        if width == 0 || height == 0 || stride < width * 4 {
            return None;
        }
        // The last row may stop at its last pixel.
        if bytes.len() < stride * (height - 1) + width * 4 {
            return None;
        }
        Some(Self {
            bytes,
            width,
            height,
            stride,
            order,
        })
    }

    /// The picture for GPUI to draw. A snapshot usually has the sprite's
    /// layout already, and then its bytes are used as they are.
    fn sprite(self) -> image::RgbaImage {
        let Order {
            red,
            green,
            blue,
            alpha,
            ..
        } = self.order;
        let (width, height) = (self.width as u32, self.height as u32);
        if [red, green, blue] == Order::SPRITE[..3]
            && alpha == Some(Order::SPRITE[3])
            && self.stride == self.width * 4
        {
            let mut bytes = self.bytes;
            bytes.truncate(self.width * self.height * 4);
            if let Some(picture) = image::RgbaImage::from_raw(width, height, bytes) {
                return picture;
            }
            return image::RgbaImage::new(width, height);
        }
        let mut out = image::RgbaImage::new(width, height);
        for (row, source) in out.rows_mut().zip(self.bytes.chunks(self.stride)) {
            for (pixel, p) in row.zip(source.as_chunks::<4>().0) {
                pixel.0 = [
                    p[blue],
                    p[green],
                    p[red],
                    alpha.map_or(255, |alpha| p[alpha]),
                ];
            }
        }
        out
    }

    /// The picture as plain red, green, blue and alpha.
    fn rgba(self) -> image::RgbaImage {
        let Order {
            red,
            green,
            blue,
            alpha,
            premultiplied,
        } = self.order;
        let mut out = image::RgbaImage::new(self.width as u32, self.height as u32);
        for (row, source) in out.rows_mut().zip(self.bytes.chunks(self.stride)) {
            for (pixel, p) in row.zip(source.as_chunks::<4>().0) {
                let a = alpha.map_or(255, |alpha| p[alpha]);
                let plain = |value: u8| match a {
                    255 => value,
                    0 => 0,
                    _ if premultiplied => {
                        ((u32::from(value) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
                    }
                    _ => value,
                };
                pixel.0 = [plain(p[red]), plain(p[green]), plain(p[blue]), a];
            }
        }
        out
    }
}

/// A snapshot on its way to a PNG. Reading the pixels has to happen on the
/// main thread; compressing them does not.
enum Shot {
    Raw(Raw),
    /// A snapshot in a layout `Raw` does not read, encoded by AppKit.
    Png(Vec<u8>, (u32, u32)),
}

impl Shot {
    fn of(image: &NSImage) -> Option<Self> {
        if let Some(raw) = Raw::of(image) {
            return Some(Self::Raw(raw));
        }
        let rep = NSBitmapImageRep::imageRepWithData(&*image.TIFFRepresentation()?)?;
        let size = (rep.pixelsWide() as u32, rep.pixelsHigh() as u32);
        let data = unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        }?;
        Some(Self::Png(data.to_vec(), size))
    }

    fn png(self) -> Option<(Vec<u8>, (u32, u32))> {
        match self {
            Self::Png(png, size) => Some((png, size)),
            Self::Raw(raw) => {
                let picture = raw.rgba();
                let size = picture.dimensions();
                let mut out = std::io::Cursor::new(Vec::new());
                image::codecs::png::PngEncoder::new_with_quality(
                    &mut out,
                    image::codecs::png::CompressionType::Fast,
                    image::codecs::png::FilterType::Adaptive,
                )
                .write_image(
                    picture.as_raw(),
                    size.0,
                    size.1,
                    image::ExtendedColorType::Rgba8,
                )
                .ok()?;
                Some((out.into_inner(), size))
            }
        }
    }
}

/// The slow way to a sprite, for a snapshot `Raw` does not read.
fn sprite_of_tiff(image: &NSImage) -> Option<image::RgbaImage> {
    let tiff = image.TIFFRepresentation()?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
    let format = rep.bitmapFormat();
    let samples = rep.samplesPerPixel() as usize;
    let step = rep.bitsPerPixel() as usize / 8;
    if rep.isPlanar()
        || rep.bitsPerSample() != 8
        || !(samples == 3 || samples == 4)
        || step < samples
        || format.contains(NSBitmapFormat::FloatingPointSamples)
    {
        return None;
    }
    let (width, height) = (rep.pixelsWide() as usize, rep.pixelsHigh() as usize);
    let stride = rep.bytesPerRow() as usize;
    let data = rep.bitmapData();
    if data.is_null() || width == 0 || height == 0 || stride < width * step {
        return None;
    }
    // SAFETY: the rep owns `bytesPerRow * pixelsHigh` bytes while it lives.
    let bytes = unsafe { std::slice::from_raw_parts(data, stride * height) };
    let alpha_first = format.contains(NSBitmapFormat::AlphaFirst);
    let mut out = image::RgbaImage::new(width as u32, height as u32);
    for (y, row) in out.rows_mut().enumerate() {
        let source = &bytes[y * stride..];
        for (x, pixel) in row.enumerate() {
            let p = &source[x * step..];
            let [r, g, b, a] = match (samples, alpha_first) {
                (4, true) => [p[1], p[2], p[3], p[0]],
                (4, false) => [p[0], p[1], p[2], p[3]],
                _ => [p[0], p[1], p[2], 255],
            };
            pixel.0 = [b, g, r, a];
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_in_the_sprite_layout_is_used_as_it_is() {
        // What WebKit hands over: blue first, alpha last, premultiplied.
        let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
        let order = Order::of(info).unwrap();
        assert_eq!(
            [order.red, order.green, order.blue],
            Order::SPRITE[..3],
            "{order:?}"
        );
        assert_eq!(order.alpha, Some(3));
        let raw = |stride: usize| Raw {
            bytes: [[1u8, 2, 3, 255], [4, 5, 6, 255]]
                .iter()
                .flat_map(|pixel| {
                    pixel
                        .iter()
                        .copied()
                        .chain(std::iter::repeat_n(0, stride - 4))
                })
                .collect(),
            width: 1,
            height: 2,
            stride,
            order,
        };
        assert_eq!(raw(4).sprite().into_raw(), [1, 2, 3, 255, 4, 5, 6, 255]);
        // Rows with padding are copied without it.
        assert_eq!(raw(8).sprite().into_raw(), [1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!(raw(8).rgba().into_raw(), [3, 2, 1, 255, 6, 5, 4, 255]);
    }

    #[test]
    fn other_snapshot_layouts_are_reordered() {
        let pixel = |info: u32, bytes: [u8; 4]| {
            let raw = |order| Raw {
                bytes: bytes.to_vec(),
                width: 1,
                height: 1,
                stride: 4,
                order,
            };
            let order = Order::of(info).unwrap();
            (raw(order).sprite().into_raw(), raw(order).rgba().into_raw())
        };
        // Red first, alpha last.
        let (sprite, rgba) = pixel(CGImageAlphaInfo::Last.0, [10, 20, 30, 255]);
        assert_eq!(sprite, [30, 20, 10, 255]);
        assert_eq!(rgba, [10, 20, 30, 255]);
        // Alpha first, and a skipped alpha reads as opaque.
        let (sprite, rgba) = pixel(CGImageAlphaInfo::NoneSkipFirst.0, [7, 10, 20, 30]);
        assert_eq!(sprite, [30, 20, 10, 255]);
        assert_eq!(rgba, [10, 20, 30, 255]);
        // A PNG holds plain colours, so a premultiplied pixel is divided out.
        let info = CGImageAlphaInfo::PremultipliedLast.0;
        assert_eq!(pixel(info, [50, 100, 0, 128]).1, [100, 199, 0, 128]);
        assert_eq!(Order::of(CGImageAlphaInfo::Only.0), None);
        assert_eq!(Order::of(CGImageByteOrderInfo::Order16Little.0 | 1), None);
    }

    #[test]
    fn a_moving_card_draws_its_still_until_it_settles() {
        let at = |y: f32| {
            gpui::Bounds::new(
                gpui::point(gpui::px(0.), gpui::px(y)),
                gpui::size(gpui::px(400.), gpui::px(300.)),
            )
        };
        let start = std::time::Instant::now();
        let mut glide = Glide::default();
        // The first paint has nothing to compare with.
        assert!(!glide.track(at(100.), start));
        assert!(!glide.track(at(100.), start + Duration::from_millis(16)));
        // Scrolled: moving, and still moving just inside the settle time.
        assert!(glide.track(at(90.), start + Duration::from_millis(32)));
        assert!(glide.track(at(90.), start + Duration::from_millis(32) + SETTLE / 2));
        // Put for the settle time: the page comes back.
        assert!(!glide.track(at(90.), start + Duration::from_millis(32) + SETTLE));
        // A still waits for the card to stay put longer than that.
        assert!(!glide.still_for(QUIET, start + Duration::from_millis(32) + SETTLE));
        assert!(glide.still_for(QUIET, start + Duration::from_millis(32) + QUIET));
    }

    #[test]
    fn a_still_stands_in_until_the_page_could_look_different() {
        let key = StillKey {
            generation: 3,
            pings: 7,
            input: None,
            restyles: 0,
            size: (400, 300),
        };
        // The page reporting, as it may when the pointer passes mid scroll.
        assert!(key.stands_for(&StillKey { pings: 9, ..key }));
        assert!(!key.stands_for(&StillKey {
            generation: 4,
            ..key
        }));
        assert!(!key.stands_for(&StillKey {
            input: Some(std::time::Instant::now()),
            ..key
        }));
        assert!(!key.stands_for(&StillKey { restyles: 1, ..key }));
        assert!(!key.stands_for(&StillKey {
            size: (400, 320),
            ..key
        }));
    }

    #[test]
    fn a_draft_scales_to_its_column_but_never_past_life_size() {
        // A desktop mock in a narrow column shrinks by width.
        assert_eq!(fit(1280, (320., 900.)).scale, 0.25);
        // Height never limits it: the thread scrolls.
        assert!((fit(375, (300., 10.)).scale - 0.8).abs() < 1e-6);
        // Small drafts stay at life size.
        assert_eq!(fit(300, (600., 600.)).scale, 1.);
        // An absurdly small column stays drawable.
        assert!(fit(1600, (1., 1.)).scale > 0.);
    }

    #[test]
    fn a_page_that_comes_back_empty_gets_a_moment_then_is_opened_again() {
        let ms = Duration::from_millis;
        assert_eq!(Blank::of(false, ms(0)), Some(Blank::Waiting));
        assert_eq!(Blank::of(false, ms(2_999)), Some(Blank::Waiting));
        assert_eq!(Blank::of(false, BLANK_LIMIT), Some(Blank::Gone));
        // Drawn is not yet settled: WebKit can still let its layers go.
        assert_eq!(Blank::of(true, ms(0)), Some(Blank::No));
        assert_eq!(Blank::of(true, RETURN_WATCH), None);
    }

    #[test]
    fn a_hole_follows_its_rounded_corners() {
        let bounds = gpui::Bounds::new(
            gpui::point(gpui::px(110.), gpui::px(220.)),
            gpui::size(gpui::px(100.), gpui::px(30.)),
        );
        let pill = Hole::new(bounds, gpui::px(15.), (100., 200.));
        assert_eq!((pill.x, pill.y), (10., 20.));
        assert!(pill.contains(60., 35.), "the middle");
        assert!(pill.contains(10., 35.), "the end of the pill");
        assert!(!pill.contains(11., 21.), "a corner the pill rounds off");
        assert!(!pill.contains(109., 49.), "the far corner");
        assert!(!pill.contains(60., 19.), "just above it");
    }

    #[test]
    fn a_hole_radius_never_exceeds_half_its_shortest_side() {
        let sliver = gpui::Bounds::new(
            gpui::point(gpui::px(0.), gpui::px(0.)),
            gpui::size(gpui::px(40.), gpui::px(3.)),
        );
        // Core Graphics aborts the app on a radius past half a side, and a
        // content mask can leave a float that thin.
        assert_eq!(Hole::new(sliver, gpui::px(15.), (0., 0.)).radius, 1.5);
    }

    #[test]
    fn dev_server_links_are_local_only() {
        assert!(is_local("http://localhost:5173/x"));
        assert!(is_local("http://127.0.0.1:3000"));
        assert!(!is_local("https://example.com"));
        assert!(!is_local("http://localhost.evil.com"));
    }
}
