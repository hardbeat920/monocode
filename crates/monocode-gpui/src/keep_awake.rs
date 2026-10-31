//! The footer's keep-awake pill and popover. It holds `caffeinate` (and, with
//! the lid option, `pmset disablesleep`) while `monocode_core::keep_awake`
//! says the Mac should stay up.
use crate::{
    popover::{Align, Anchor, AnchorSlot, Side, popover},
    settings::KeepAwakePrefs,
    theme::*,
};
use gpui::{
    AnyElement, Context, DispatchPhase, EventEmitter, FocusHandle, FontWeight, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Role, SharedString, Task, Window, canvas,
    div, prelude::*, px,
};
use monocode_core::keep_awake::{
    self as policy, Hold, KeepAwake as Policy, LidScreen, Machine, Mode, Settings, Status,
    hotspot::{self, Hotspot, Step},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// The Mac is on a hotspot this moved it to; background refreshes hold off.
static ON_HOTSPOT: AtomicBool = AtomicBool::new(false);
pub(crate) fn on_hotspot() -> bool {
    ON_HOTSPOT.load(Ordering::Relaxed)
}
/// The saved "continue replies cut off by a drop" choice, for conversations.
static CONTINUE_AFTER_DROP: AtomicBool = AtomicBool::new(true);
pub(crate) fn continue_after_drop() -> bool {
    CONTINUE_AFTER_DROP.load(Ordering::Relaxed)
}

const TICK: Duration = Duration::from_secs(1);
/// The lid sensor and battery, sampled every this many ticks while held.
const SAMPLE_TICKS: u32 = 2;
const WIDTH: f32 = 300.;

/// The popover changed a setting; the shell saves it like Settings would.
pub(crate) struct PrefsChanged(pub(crate) KeepAwakePrefs);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ruler {
    Grace,
    Timer,
}
impl Ruler {
    /// (min, max, step, px per step, major, minor) in minutes.
    fn spec(self) -> (u32, u32, u32, f32, u32, u32) {
        match self {
            Self::Grace => (0, 180, 5, 8., 30, 15),
            Self::Timer => (15, 720, 5, 7., 60, 30),
        }
    }
    fn height(self) -> f32 {
        match self {
            Self::Grace => 34.,
            Self::Timer => 40.,
        }
    }
    fn presets(self) -> &'static [u32] {
        match self {
            Self::Grace => &[0, 5, 15, 30, 60],
            Self::Timer => &[30, 60, 120, 240, 480],
        }
    }
    fn get(self, p: &KeepAwakePrefs) -> u32 {
        match self {
            Self::Grace => p.grace_minutes,
            Self::Timer => p.timer_minutes,
        }
    }
    fn set(self, p: &mut KeepAwakePrefs, minutes: u32) {
        match self {
            Self::Grace => p.grace_minutes = minutes,
            Self::Timer => p.timer_minutes = minutes,
        }
    }
    fn tick_label(self, minutes: u32) -> String {
        match (minutes / 60, minutes % 60) {
            (0, m) => format!("{m}m"),
            (h, 0) => format!("{h}h"),
            (h, m) => format!("{h}h{m}"),
        }
    }
}

struct Drag {
    ruler: Ruler,
    x: Pixels,
    minutes: u32,
}

/// What is actually held on the machine right now.
#[derive(Default)]
struct Held {
    caffeinate: Option<(std::process::Child, Hold)>,
    lid_disabled: bool,
    /// A `pmset disablesleep` change is in flight.
    lid_pending: bool,
    lid_was_closed: bool,
    /// The panel's brightness before a closed lid dimmed it.
    dimmed_from: Option<f32>,
    /// Closing the lid muted the sound; opening it unmutes.
    muted_by_us: bool,
    low_power: bool,
    low_power_pending: bool,
}
impl Held {
    fn kill(&mut self) {
        if let Some((mut child, _)) = self.caffeinate.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(crate) struct KeepAwake {
    prefs: KeepAwakePrefs,
    policy: Policy,
    /// Titles of the agents working now, across projects and Monos.
    agents: Vec<SharedString>,
    machine: Machine,
    /// The sudoers rule for `pmset disablesleep`: `None` until checked.
    lid_ready: Option<bool>,
    /// The same rule covers the battery's power mode (newer rules do).
    power_ready: Option<bool>,
    held: Held,
    open: bool,
    classic: bool,
    asking_lid: bool,
    installing: bool,
    error: Option<SharedString>,
    drag: Option<Drag>,
    anchor: AnchorSlot,
    rulers: [AnchorSlot; 2],
    trigger: FocusHandle,
    details: FocusHandle,
    previous_focus: Option<FocusHandle>,
    marker: PathBuf,
    power_marker: PathBuf,
    clock: Option<Task<()>>,
    sampling: bool,
    hotspot: Hotspot,
    last_probe: Option<Instant>,
    /// A reachability check, join or leave is in flight.
    networking: bool,
    /// Why the last join failed.
    network_error: Option<SharedString>,
}
impl EventEmitter<PrefsChanged> for KeepAwake {}

impl KeepAwake {
    pub(crate) fn supported() -> bool {
        cfg!(target_os = "macos")
    }
    pub(crate) fn new(prefs: KeepAwakePrefs, data_dir: PathBuf, cx: &mut Context<Self>) -> Self {
        let marker = data_dir.join("keep-awake-disablesleep");
        let power_marker = data_dir.join("keep-awake-powermode");
        cx.on_app_quit(|this, cx| {
            this.held.kill();
            let lid = std::mem::take(&mut this.held.lid_disabled);
            let low_power = std::mem::take(&mut this.held.low_power);
            let unmute = std::mem::take(&mut this.held.muted_by_us);
            let brightness = this.held.dimmed_from.take();
            let marker = this.marker.clone();
            let power_marker = this.power_marker.clone();
            cx.background_executor().spawn(async move {
                #[cfg(target_os = "macos")]
                {
                    if lid {
                        let _ = policy::mac::set_lid_sleep_disabled(false, &marker);
                    }
                    if low_power {
                        let _ = policy::mac::set_low_power(false, &power_marker);
                    }
                    if unmute {
                        policy::mac::set_muted(false);
                    }
                    if let Some(value) = brightness {
                        policy::mac::set_brightness(value);
                    }
                }
                let _ = (lid, low_power, unmute, brightness, marker, power_marker);
            })
        })
        .detach();
        CONTINUE_AFTER_DROP.store(prefs.continue_after_drop, Ordering::Relaxed);
        let mut this = Self {
            prefs,
            policy: Policy::default(),
            agents: Vec::new(),
            machine: Machine::default(),
            lid_ready: None,
            power_ready: None,
            held: Held::default(),
            open: false,
            classic: false,
            asking_lid: false,
            installing: false,
            error: None,
            drag: None,
            anchor: AnchorSlot::default(),
            rulers: Default::default(),
            trigger: cx.focus_handle(),
            details: cx.focus_handle(),
            previous_focus: None,
            marker,
            power_marker,
            clock: None,
            sampling: false,
            hotspot: Hotspot::default(),
            last_probe: None,
            networking: false,
            network_error: None,
        };
        // Know the power source before holding anything that depends on it.
        this.sample(cx);
        // Also undoes a lid hold or Low Power Mode a crashed run left behind.
        if this.prefs.lid || this.marker.exists() || this.power_marker.exists() {
            this.check_lid_rule(cx);
        }
        this
    }
    fn settings(&self) -> Settings {
        self.prefs.settings()
    }
    pub(crate) fn set_classic(&mut self, classic: bool, cx: &mut Context<Self>) {
        if self.classic != classic {
            self.classic = classic;
            cx.notify();
        }
    }
    pub(crate) fn set_prefs(&mut self, prefs: KeepAwakePrefs, cx: &mut Context<Self>) {
        if self.prefs == prefs {
            return;
        }
        let retime = self.prefs.timer_minutes != prefs.timer_minutes;
        CONTINUE_AFTER_DROP.store(prefs.continue_after_drop, Ordering::Relaxed);
        self.prefs = prefs;
        if retime {
            self.policy.retime(&self.settings(), Instant::now());
        }
        if (self.prefs.lid || self.prefs.low_power) && self.lid_ready.is_none() {
            self.check_lid_rule(cx);
        }
        if !self.prefs.hotspot_ready() {
            self.network_error = None;
        }
        self.sync(cx);
    }
    /// The agents working now; an agent starting may turn this on.
    pub(crate) fn set_agents(&mut self, agents: Vec<SharedString>, cx: &mut Context<Self>) {
        if self.agents != agents {
            self.agents = agents;
            self.sync(cx);
        }
    }
    fn change(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut KeepAwakePrefs)) {
        let mut prefs = self.prefs.clone();
        f(&mut prefs);
        if prefs != self.prefs {
            self.set_prefs(prefs.clone(), cx);
            cx.emit(PrefsChanged(prefs));
        }
    }

    fn status(&self) -> Status {
        self.policy.status(
            self.agents.len(),
            &self.settings(),
            self.machine,
            Instant::now(),
        )
    }

    /// Advance the policy, then make the machine match it.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let settings = self.settings();
        self.policy
            .tick(self.agents.len(), &settings, Instant::now());
        let status = self.status();
        let hold = policy::hold(
            status,
            &settings,
            self.machine,
            self.lid_ready == Some(true),
        );
        self.hold_caffeinate(hold);
        self.hold_lid(hold.is_some_and(|h| h.lid), cx);
        self.follow_lid(hold.is_some_and(|h| h.lid), cx);
        self.hold_low_power(policy::low_power(hold, &settings, self.machine), cx);
        self.follow_network(cx);
        self.ensure_clock(cx);
        cx.notify();
    }
    fn hold_caffeinate(&mut self, hold: Option<Hold>) {
        // Something else may have killed it; the next sync starts a new one.
        if let Some((child, _)) = &mut self.held.caffeinate
            && !matches!(child.try_wait(), Ok(None))
        {
            self.held.caffeinate = None;
        }
        if self.held.caffeinate.as_ref().map(|(_, h)| *h) == hold {
            return;
        }
        self.held.kill();
        #[cfg(target_os = "macos")]
        if let Some(hold) = hold
            && !cfg!(test)
        {
            match policy::mac::caffeinate(hold, std::process::id()) {
                Ok(child) => {
                    self.held.caffeinate = Some((child, hold));
                    self.error = None;
                }
                Err(_) => self.error = Some("Couldn't start caffeinate.".into()),
            }
        }
    }
    fn hold_lid(&mut self, want: bool, cx: &mut Context<Self>) {
        if want == self.held.lid_disabled || self.held.lid_pending || cfg!(test) {
            return;
        }
        self.held.lid_pending = true;
        let marker = self.marker.clone();
        let work = cx.background_executor().spawn(async move {
            #[cfg(target_os = "macos")]
            return policy::mac::set_lid_sleep_disabled(want, &marker);
            #[cfg(not(target_os = "macos"))]
            {
                let _ = marker;
                Err::<(), String>("Only on macOS.".into())
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.held.lid_pending = false;
                match result {
                    Ok(()) => this.held.lid_disabled = want,
                    Err(error) => {
                        this.error = Some(error.into());
                        // The rule went away; ask again rather than retrying forever.
                        if want {
                            this.lid_ready = Some(false);
                        }
                    }
                }
                this.sync(cx);
            });
        })
        .detach();
    }
    /// A held lid closing locks, quiets and darkens the Mac; opening it
    /// brings back the sound and brightness, and the usual Wi-Fi.
    fn follow_lid(&mut self, lid_held: bool, cx: &mut Context<Self>) {
        let closed = self.machine.lid_closed;
        let was = std::mem::replace(&mut self.held.lid_was_closed, closed);
        if cfg!(test) || closed == was {
            return;
        }
        let settings = self.settings();
        let actions = policy::lid_actions(&settings, closed, lid_held, self.held.muted_by_us);
        if actions.unmute {
            self.held.muted_by_us = false;
        }
        if !closed && !self.networking && self.hotspot.lid_opened() == Step::Leave {
            self.network_step(Step::Leave, cx);
        }
        let screen = settings.lid_screen;
        let restore = (!closed).then(|| self.held.dimmed_from.take()).flatten();
        let dim = closed && lid_held && screen == LidScreen::Dim;
        let off = closed && lid_held && screen == LidScreen::Off;
        let work = cx.background_executor().spawn(async move {
            let mut before = None;
            let mut muted = false;
            #[cfg(target_os = "macos")]
            {
                // Locked before the screen goes dark, so it's never unlocked
                // and unattended.
                if actions.lock {
                    policy::mac::lock_screen();
                }
                if actions.pause {
                    policy::mac::pause_media();
                }
                if actions.mute && policy::mac::muted() == Some(false) {
                    muted = policy::mac::set_muted(true);
                }
                if actions.unmute {
                    policy::mac::set_muted(false);
                }
                if let Some(value) = restore {
                    policy::mac::set_brightness(value);
                }
                if off {
                    policy::mac::display_sleep_now();
                }
                if dim {
                    before = policy::mac::brightness();
                    if before.is_some() {
                        policy::mac::set_brightness(0.);
                    }
                }
            }
            let _ = (actions, restore, off, dim);
            (before, muted)
        });
        cx.spawn(async move |this, cx| {
            let (before, muted) = work.await;
            let _ = this.update(cx, |this, _| {
                if before.is_some() {
                    this.held.dimmed_from = before;
                }
                if muted {
                    this.held.muted_by_us = true;
                }
            });
        })
        .detach();
    }
    /// Low Power Mode while the lid is held closed on battery, then back to
    /// what it was.
    fn hold_low_power(&mut self, want: bool, cx: &mut Context<Self>) {
        let want = want && self.power_ready == Some(true);
        if want == self.held.low_power || self.held.low_power_pending || cfg!(test) {
            return;
        }
        self.held.low_power_pending = true;
        let marker = self.power_marker.clone();
        let work = cx.background_executor().spawn(async move {
            #[cfg(target_os = "macos")]
            return policy::mac::set_low_power(want, &marker);
            #[cfg(not(target_os = "macos"))]
            {
                let _ = marker;
                Err::<(), String>("Only on macOS.".into())
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.held.low_power_pending = false;
                match result {
                    Ok(()) => this.held.low_power = want,
                    Err(error) => {
                        this.error = Some(error.into());
                        if want {
                            this.power_ready = Some(false);
                        }
                    }
                }
                this.sync(cx);
            });
        })
        .detach();
    }

    /// While agents work, checks the internet every few seconds and moves to
    /// the hotspot after a sustained drop; back once they finish.
    fn follow_network(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || self.networking {
            return;
        }
        let working = self.prefs.enabled && self.prefs.hotspot_ready() && !self.agents.is_empty();
        let now = Instant::now();
        if !working {
            if self.hotspot.on_hotspot() {
                let step = self.hotspot.step(true, false, now);
                self.network_step(step, cx);
            }
            return;
        }
        if self
            .last_probe
            .is_some_and(|at| now.saturating_duration_since(at) < hotspot::PROBE_EVERY)
        {
            return;
        }
        self.last_probe = Some(now);
        self.networking = true;
        let probe = cx
            .background_executor()
            .spawn(async { hotspot::online(Duration::from_secs(3)) });
        cx.spawn(async move |this, cx| {
            let online = probe.await;
            let _ = this.update(cx, |this, cx| {
                this.networking = false;
                let working =
                    this.prefs.enabled && this.prefs.hotspot_ready() && !this.agents.is_empty();
                if online || !working {
                    this.network_error = None;
                }
                let step = this.hotspot.step(online, working, Instant::now());
                this.network_step(step, cx);
            });
        })
        .detach();
    }
    fn network_step(&mut self, step: Step, cx: &mut Context<Self>) {
        if step == Step::Stay || cfg!(test) {
            return;
        }
        self.networking = true;
        let ssid = self.prefs.hotspot_ssid.trim().to_string();
        let work = cx.background_executor().spawn(async move {
            #[cfg(target_os = "macos")]
            return match step {
                Step::Join => hotspot::mac::join(&ssid),
                _ => hotspot::mac::leave(),
            };
            #[cfg(not(target_os = "macos"))]
            {
                let _ = ssid;
                Err::<(), String>("Only on macOS.".into())
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.networking = false;
                match (step, result) {
                    (Step::Join, Ok(())) => {
                        this.hotspot.joined();
                        this.network_error = None;
                    }
                    (Step::Join, Err(error)) => this.network_error = Some(error.into()),
                    // Even if Wi-Fi didn't cycle cleanly, macOS rejoins on its own.
                    (_, _) => this.hotspot.left(),
                }
                ON_HOTSPOT.store(this.hotspot.on_hotspot(), Ordering::Relaxed);
                cx.notify();
            });
        })
        .detach();
    }
    /// Ticks once a second while there is anything to count down or follow.
    /// Armed needs no clock: the shell reports agents as they start.
    fn ensure_clock(&mut self, cx: &mut Context<Self>) {
        let status = self.status();
        let needed = status.awake()
            || status.paused()
            || self.hotspot.on_hotspot()
            || self.held.lid_disabled
            || self.open;
        if !needed {
            self.clock = None;
            return;
        }
        if self.clock.is_some() || cfg!(test) {
            return;
        }
        // The power source may have changed while nothing was held.
        self.sample(cx);
        self.clock = Some(cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(TICK).await;
                ticks += 1;
                if ticks.is_multiple_of(SAMPLE_TICKS) {
                    let sample = this.update(cx, |this, cx| this.sample(cx));
                    if sample.is_err() {
                        break;
                    }
                }
                let keep = this.update(cx, |this, cx| {
                    this.sync(cx);
                    this.clock.is_some()
                });
                if !keep.unwrap_or(false) {
                    break;
                }
            }
        }));
    }
    fn sample(&mut self, cx: &mut Context<Self>) {
        if self.sampling || cfg!(test) {
            return;
        }
        self.sampling = true;
        let work = cx.background_executor().spawn(async {
            #[cfg(target_os = "macos")]
            return policy::mac::machine();
            #[cfg(not(target_os = "macos"))]
            Machine::default()
        });
        cx.spawn(async move |this, cx| {
            let machine = work.await;
            let _ = this.update(cx, |this, cx| {
                this.sampling = false;
                if this.machine != machine {
                    this.machine = machine;
                    this.sync(cx);
                }
            });
        })
        .detach();
    }
    fn check_lid_rule(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) {
            return;
        }
        let marker = self.marker.clone();
        let power_marker = self.power_marker.clone();
        // The restore runs first, so a hold can only start after it.
        let work = cx.background_executor().spawn(async move {
            #[cfg(target_os = "macos")]
            {
                policy::mac::restore_after_crash(&marker, &power_marker);
                (
                    policy::mac::lid_rule_ready(),
                    policy::mac::power_rule_ready(),
                )
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (marker, power_marker);
                (false, false)
            }
        });
        cx.spawn(async move |this, cx| {
            let (lid, power) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.lid_ready = Some(lid);
                this.power_ready = Some(power);
                this.sync(cx);
            });
        })
        .detach();
    }
    fn install_lid_rule(&mut self, cx: &mut Context<Self>) {
        self.install_rule(cx, |p| p.lid = true);
    }
    /// One rule covers lid-close sleep and Low Power Mode; `then` turns on
    /// whichever asked for it.
    fn install_rule(
        &mut self,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut KeepAwakePrefs) + 'static,
    ) {
        if self.installing {
            return;
        }
        self.installing = true;
        self.error = None;
        let work = cx.background_executor().spawn(async {
            #[cfg(target_os = "macos")]
            return policy::mac::install_lid_rule();
            #[cfg(not(target_os = "macos"))]
            Err::<(), String>("Only on macOS.".into())
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.installing = false;
                match result {
                    Ok(()) => {
                        this.lid_ready = Some(true);
                        this.power_ready = Some(true);
                        this.asking_lid = false;
                        this.change(cx, then);
                    }
                    Err(error) => this.error = Some(error.into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    /// Lid mode needs the rule first; asking shows the consent card.
    fn want_lid(&mut self, on: bool, cx: &mut Context<Self>) {
        if !on {
            self.asking_lid = false;
            self.change(cx, |p| p.lid = false);
        } else if self.lid_ready == Some(true) {
            self.change(cx, |p| p.lid = true);
        } else {
            self.asking_lid = true;
            self.error = None;
            cx.notify();
        }
    }
    /// Settings' toggle. Its row already explains the password, and macOS's
    /// own prompt names why, so it installs the rule without the card.
    pub(crate) fn request_lid(&mut self, on: bool, cx: &mut Context<Self>) {
        if on && self.lid_ready != Some(true) {
            self.install_lid_rule(cx);
        } else {
            self.want_lid(on, cx);
        }
    }

    /// Settings' Low Power Mode toggle; older rules need replacing first.
    pub(crate) fn request_low_power(&mut self, on: bool, cx: &mut Context<Self>) {
        if on && self.power_ready != Some(true) {
            self.install_rule(cx, |p| p.low_power = true);
        } else {
            self.change(cx, |p| p.low_power = on);
        }
    }

    /// Picking a mode also turns keep-awake on.
    fn start(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.change(cx, |p| p.enabled = true);
        self.policy.start(mode, &self.settings(), Instant::now());
        self.sync(cx);
    }
    /// The popover's switch, saved like Settings' toggle.
    fn set_on(&mut self, on: bool, cx: &mut Context<Self>) {
        self.change(cx, |p| p.enabled = on);
    }
    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.dismiss(window, cx);
            return;
        }
        self.previous_focus = window.focused(cx).filter(|f| *f != self.trigger);
        self.open = true;
        self.error = None;
        window.focus(&self.details, cx);
        self.sample(cx);
        if self.lid_ready.is_none() {
            self.check_lid_rule(cx);
        }
        self.sync(cx);
    }
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.asking_lid = false;
        self.drag = None;
        window.focus(self.previous_focus.as_ref().unwrap_or(&self.trigger), cx);
        self.sync(cx);
    }

    fn drag_to(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(drag) = &self.drag else {
            return;
        };
        let (min, max, step, px_per, ..) = drag.ruler.spec();
        let steps = ((drag.x - x).as_f32() / px_per).round() as i64;
        let minutes = (i64::from(drag.minutes) + steps * i64::from(step))
            .clamp(i64::from(min), i64::from(max)) as u32;
        let ruler = drag.ruler;
        if ruler.get(&self.prefs) != minutes {
            self.change(cx, |p| ruler.set(p, minutes));
        }
    }

    /// The pill's text: how long, or why, it holds.
    fn label(status: Status, on_hotspot: bool) -> Option<String> {
        let label = match status {
            // Idle is the usual state; the icon alone says it's there.
            Status::Off | Status::Armed => return None,
            Status::Working { agents: 1 } => "1 agent".into(),
            Status::Working { agents } => format!("{agents} agents"),
            Status::Grace { left, .. } | Status::Timer { left, .. } => policy::clock(left),
            Status::UntilOff { .. } => "On".into(),
            Status::BatteryLow { .. } | Status::Hot => "Paused".into(),
        };
        Some(if on_hotspot {
            format!("{label} · Hotspot")
        } else {
            label
        })
    }
    fn headline(&self, status: Status) -> (String, String, f32) {
        let s = |n: usize| if n == 1 { "" } else { "s" };
        let grace = self.prefs.grace_minutes;
        match status {
            Status::Off => (
                "Keep-awake is off".into(),
                "Your Mac follows its usual sleep settings, even while agents work.".into(),
                0.,
            ),
            Status::Armed => (
                "Waiting for an agent".into(),
                "The next one you start keeps the Mac awake.".into(),
                0.,
            ),
            Status::Working { agents } => (
                format!("Awake while {agents} agent{} work", s(agents)),
                if grace == 0 {
                    "Lets go when the last one finishes.".into()
                } else {
                    format!(
                        "Then waits {}.",
                        policy::length(Duration::from_secs(u64::from(grace) * 60))
                    )
                },
                1.,
            ),
            Status::Grace { left, total } => (
                format!("Letting go in {}", policy::clock(left)),
                "Agents finished. Room for a follow-up.".into(),
                fraction(left, total),
            ),
            Status::Timer { left, total } => (
                format!("{} left", policy::clock(left)),
                "Then awake only while agents work.".into(),
                fraction(left, total),
            ),
            Status::UntilOff { since } => (
                "Awake until you turn it off".into(),
                format!(
                    "On for {}. Ends if MonoCode quits.",
                    policy::length(since.max(Duration::from_secs(60)))
                ),
                1.,
            ),
            Status::BatteryLow { percent } => (
                "Paused on low battery".into(),
                format!("At {percent}%, the Mac may sleep. Plug in to keep going."),
                0.,
            ),
            Status::Hot => (
                "Paused: your Mac is hot".into(),
                "It got hot with the lid closed, so it may sleep until it cools down.".into(),
                0.,
            ),
        }
    }
}

fn fraction(left: Duration, total: Duration) -> f32 {
    if total.is_zero() {
        0.
    } else {
        (left.as_secs_f32() / total.as_secs_f32()).clamp(0., 1.)
    }
}

impl KeepAwake {
    fn switch(
        &self,
        id: &'static str,
        label: &'static str,
        on: bool,
        cx: &mut Context<Self>,
        change: impl Fn(&mut Self, bool, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .role(Role::Switch)
            .aria_label(label)
            .aria_toggled(if on {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
            .relative()
            .h(px(18.))
            .w(px(30.))
            .flex_shrink_0()
            .rounded_full()
            .cursor_pointer()
            .bg(if on { accent_color() } else { content(0.2) })
            .child(
                div()
                    .absolute()
                    .top(px(2.))
                    .left(px(if on { 14. } else { 2. }))
                    .size(px(14.))
                    .rounded_full()
                    .bg(gpui::white()),
            )
            .on_click(cx.listener(move |this, _, _, cx| change(this, !on, cx)))
            .into_any_element()
    }
    fn check(
        &self,
        id: &'static str,
        label: &'static str,
        hint: Option<SharedString>,
        on: bool,
        cx: &mut Context<Self>,
        change: impl Fn(&mut Self, bool, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .role(Role::CheckBox)
            .aria_label(label)
            .aria_toggled(if on {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
            .flex()
            .items_start()
            .gap_2()
            .cursor_pointer()
            .child(
                div()
                    .mt(px(1.))
                    .size(px(14.))
                    .flex_shrink_0()
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .map(|d| {
                        if on {
                            d.bg(accent_color())
                        } else {
                            d.border_1().border_color(content(0.3))
                        }
                    })
                    .when(on, |d| {
                        d.child(icon("check", 10.).text_color(gpui::white()))
                    }),
            )
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .child(div().text_color(content(0.9)).child(label))
                    .when_some(hint, |d, hint| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(content(0.5))
                                .child(hint),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| change(this, !on, cx)))
            .into_any_element()
    }
    fn chip(
        &self,
        id: SharedString,
        label: String,
        selected: bool,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label.clone())
            .aria_selected(selected)
            .flex_1()
            .h(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .border_1()
            .text_size(px(12.))
            .cursor_pointer()
            .map(|d| {
                if selected {
                    let mut tint = accent_color();
                    tint.a = 0.16;
                    d.border_color(accent_color())
                        .bg(tint)
                        .text_color(accent_color())
                } else {
                    d.border_color(content(0.12))
                        .text_color(content(0.8))
                        .hover(|s| s.bg(content(0.06)))
                }
            })
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| f(this, cx)))
            .into_any_element()
    }
    fn modes(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.policy.mode();
        div()
            .id("keep-awake-modes")
            .role(Role::RadioGroup)
            .aria_label("Keep awake mode")
            .grid()
            .grid_cols(3)
            .gap(px(2.))
            .p(px(2.))
            .rounded_lg()
            .bg(content(0.06))
            .children(
                [
                    (Mode::Working, "While working"),
                    (Mode::Timer, "For a time"),
                    (Mode::UntilOff, "Until off"),
                ]
                .into_iter()
                .map(|(mode, text)| {
                    let selected = current == mode;
                    div()
                        .id(SharedString::from(format!(
                            "keep-awake-mode-{}",
                            mode.key()
                        )))
                        .role(Role::RadioButton)
                        .aria_label(text)
                        .aria_selected(selected)
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .whitespace_nowrap()
                        .rounded_md()
                        .text_size(px(12.))
                        .cursor_pointer()
                        .map(|d| {
                            if selected {
                                d.bg(content(0.12)).text_color(content(1.))
                            } else {
                                d.text_color(content(0.65))
                                    .hover(|s| s.text_color(content(1.)))
                            }
                        })
                        .child(text)
                        .on_click(cx.listener(move |this, _, _, cx| this.start(mode, cx)))
                }),
            )
            .into_any_element()
    }
    /// A tape of minutes under a fixed needle; drag or scroll it.
    fn ruler(&self, ruler: Ruler, cx: &mut Context<Self>) -> AnyElement {
        let (min, max, step, px_per, major, minor) = ruler.spec();
        let height = ruler.height();
        let value = ruler.get(&self.prefs).clamp(min, max);
        let slot = self.rulers[ruler as usize].clone();
        let width = slot
            .get()
            .map(|b| b.size.width.as_f32())
            .unwrap_or(WIDTH - 24.);
        let center = width / 2.;
        let half = center.max(1.);
        let mut ticks = Vec::new();
        let mut v = min;
        while v <= max {
            let x = center + (v as f32 - value as f32) / step as f32 * px_per;
            if (-px_per..=width + px_per).contains(&x) {
                let (major_tick, minor_tick) = (v % major == 0, v % minor == 0);
                // Fades toward the edges instead of a mask.
                let edge = (1. - ((x - center).abs() / half).powi(2)).clamp(0., 1.);
                let alpha = if major_tick {
                    0.55
                } else if minor_tick {
                    0.3
                } else {
                    0.16
                } * edge;
                let (top, bottom) = if major_tick {
                    (6., 11.)
                } else if minor_tick {
                    (9., 13.)
                } else {
                    (12., 15.)
                };
                ticks.push(
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(top))
                        .bottom(px(bottom))
                        .w(px(1.))
                        .bg(content(alpha))
                        .into_any_element(),
                );
                if major_tick {
                    ticks.push(
                        div()
                            .absolute()
                            .left(px(x + 3.))
                            .bottom(px(0.))
                            .text_size(px(9.))
                            .line_height(px(11.))
                            .text_color(content(0.45 * edge))
                            .child(ruler.tick_label(v))
                            .into_any_element(),
                    );
                }
            }
            v += step;
        }
        let name = match ruler {
            Ruler::Grace => "Wait after agents finish",
            Ruler::Timer => "How long to stay awake",
        };
        div()
            .id(SharedString::from(format!("keep-awake-ruler-{ruler:?}")))
            .role(Role::Slider)
            .aria_label(name)
            .aria_description(format!("{value} minutes"))
            .relative()
            .w_full()
            .h(px(height))
            .rounded_lg()
            .bg(content(0.04))
            .overflow_hidden()
            .cursor(gpui::CursorStyle::ResizeLeftRight)
            .child(slot.layout_probe())
            .children(ticks)
            .child(
                div()
                    .absolute()
                    .left(px(center - 1.))
                    .top(px(5.))
                    .bottom(px(5.))
                    .w(px(2.))
                    .rounded_full()
                    .bg(accent_color()),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.drag = Some(Drag {
                        ruler,
                        x: event.position.x,
                        minutes: ruler.get(&this.prefs),
                    });
                    cx.stop_propagation();
                }),
            )
            .on_scroll_wheel(cx.listener(
                move |this, event: &gpui::ScrollWheelEvent, window, cx| {
                    let delta = event.delta.pixel_delta(window.line_height());
                    let d = if delta.x.abs() > delta.y.abs() {
                        -delta.x
                    } else {
                        delta.y
                    };
                    if d.as_f32().abs() < 0.5 {
                        return;
                    }
                    let current = ruler.get(&this.prefs);
                    let next = if d.as_f32() < 0. {
                        current.saturating_add(step)
                    } else {
                        current.saturating_sub(step)
                    }
                    .clamp(min, max);
                    this.change(cx, |p| ruler.set(p, next));
                    cx.stop_propagation();
                },
            ))
            .into_any_element()
    }
    fn presets(&self, ruler: Ruler, cx: &mut Context<Self>) -> AnyElement {
        let current = ruler.get(&self.prefs);
        div()
            .flex()
            .gap_1()
            .children(ruler.presets().iter().map(|&m| {
                let label = if m == 0 {
                    "None".into()
                } else {
                    policy::length(Duration::from_secs(u64::from(m) * 60))
                };
                self.chip(
                    format!("keep-awake-{ruler:?}-{m}").into(),
                    label,
                    current == m,
                    cx,
                    move |this, cx| this.change(cx, |p| ruler.set(p, m)),
                )
            }))
            .into_any_element()
    }
    /// Moves and releases anywhere in the window keep driving a ruler drag.
    fn drag_watch(&self, cx: &Context<Self>) -> AnyElement {
        let entity = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moves = entity.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = moves.update(cx, |this, cx| this.drag_to(event.position.x, cx));
                    }
                });
                let release = entity.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        let _ = release.update(cx, |this, _| this.drag = None);
                    }
                });
            },
        )
        .absolute()
        .size_0()
        .into_any_element()
    }

    fn details(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let status = self.status();
        let (title, detail, filled) = self.headline(status);
        let mode = self.policy.mode();
        let on = self.prefs.enabled;
        let mut body = div()
            .id("keep-awake-details")
            .track_focus(&self.details)
            .key_context("KeepAwake")
            .flex()
            .flex_col()
            .gap_3()
            .text_color(content(1.))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.dismiss(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Keep Mac awake"),
                    )
                    .child(self.switch(
                        "keep-awake-on",
                        "Keep Mac awake",
                        on,
                        cx,
                        |this, on, cx| this.set_on(on, cx),
                    )),
            )
            .child(
                div()
                    .id("keep-awake-status")
                    .role(Role::Status)
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(10.))
                    .rounded_lg()
                    .border_1()
                    .border_color(content(0.08))
                    .bg(content(0.03))
                    .child(progress_ring(filled, status.awake()))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(13.)).child(title))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(content(0.55))
                                    .child(detail),
                            ),
                    ),
            )
            .when(self.hotspot.on_hotspot(), |d| {
                d.child(
                    div()
                        .id("keep-awake-hotspot")
                        .role(Role::Status)
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .text_color(content(0.7))
                        .child(format!(
                            "Wi-Fi dropped, so it joined {}. Back to your usual Wi-Fi when agents finish.",
                            self.prefs.hotspot_ssid.trim()
                        )),
                )
            })
            .when_some(self.network_error.clone(), |d, error| {
                d.child(
                    div()
                        .id("keep-awake-hotspot-error")
                        .role(Role::Alert)
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .text_color(gpui::rgb(0xf59e0b))
                        .child(format!("Wi-Fi dropped. {error} Trying again every minute.")),
                )
            })
            .child(self.modes(cx));
        body = match mode {
            Mode::Working => {
                let grace = self.prefs.grace_minutes;
                body.when(!self.agents.is_empty(), |d| {
                    d.child(
                        div().flex().flex_col().children(self.agents.iter().take(4).map(|title| {
                            div()
                                .h(px(22.))
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_1()
                                .text_size(px(12.))
                                .text_color(content(0.8))
                                .child(div().size(px(6.)).flex_shrink_0().rounded_full().bg(gpui::rgb(0x10b981)))
                                .child(div().min_w_0().truncate().child(title.clone()))
                        })),
                    )
                })
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(if grace == 0 {
                                    "Then sleep right away".to_string()
                                } else {
                                    format!("Then wait {}", policy::length(Duration::from_secs(u64::from(grace) * 60)))
                                }),
                        )
                        .child(div().text_size(px(11.)).text_color(content(0.5)).child("after the last agent")),
                )
                .child(self.ruler(Ruler::Grace, cx))
                .child(self.presets(Ruler::Grace, cx))
            }
            Mode::Timer => {
                let minutes = self.prefs.timer_minutes;
                body.child(
                    div()
                        .flex()
                        .items_baseline()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(22.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(policy::length(Duration::from_secs(u64::from(minutes) * 60))),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(content(0.5))
                                .child(format!("until {}", clock_at(Duration::from_secs(u64::from(minutes) * 60)))),
                        ),
                )
                .child(self.ruler(Ruler::Timer, cx))
                .child(self.presets(Ruler::Timer, cx))
            }
            Mode::UntilOff => body.child(
                div()
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(content(0.55))
                    .child("Stays awake until you turn it off or quit MonoCode. It also ends if MonoCode crashes."),
            ),
        };
        let power = if self.prefs.lid_on_battery {
            "Plugged in or on battery."
        } else {
            "Plugged in only."
        };
        let lid_hint: SharedString =
            if self.prefs.lid && !self.machine.on_ac && !self.prefs.lid_on_battery {
                "Paused on battery. Plug in to stay awake with the lid closed.".into()
            } else if self.lid_ready == Some(true) {
                power.into()
            } else {
                format!("Asks for your admin password once. {power}").into()
            };
        body = body
            .child(div().h(px(1.)).mx(px(-12.)).bg(content(0.08)))
            .child(self.check(
                "keep-awake-display",
                "Keep the display on",
                None,
                self.prefs.display,
                cx,
                |this, on, cx| this.change(cx, |p| p.display = on),
            ))
            .child(self.check(
                "keep-awake-lid",
                "Even with the lid closed",
                Some(lid_hint),
                self.prefs.lid || self.asking_lid,
                cx,
                |this, on, cx| this.want_lid(on, cx),
            ));
        if self.asking_lid {
            body = body.child(self.consent(cx));
        }
        if self.prefs.lid {
            let screen = self.prefs.lid_screen.clone();
            let sound = self.prefs.lid_sound.clone();
            let caption = |text: &'static str| {
                div()
                    .text_size(px(11.))
                    .text_color(content(0.5))
                    .child(text)
            };
            body = body.child(
                div()
                    .ml(px(22.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(self.check(
                        "keep-awake-lid-battery",
                        "On battery too",
                        None,
                        self.prefs.lid_on_battery,
                        cx,
                        |this, on, cx| this.change(cx, |p| p.lid_on_battery = on),
                    ))
                    .child(self.check(
                        "keep-awake-lock",
                        "Lock the Mac",
                        Some("Staying awake skips the lock that sleep would do.".into()),
                        self.prefs.lock,
                        cx,
                        |this, on, cx| this.change(cx, |p| p.lock = on),
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(caption("Screen"))
                            .child(div().flex().gap_1().children(
                                [("off", "Off"), ("dim", "Dim"), ("leave", "Leave on")].map(
                                    |(key, text)| {
                                        self.chip(
                                            format!("keep-awake-screen-{key}").into(),
                                            text.into(),
                                            screen == key,
                                            cx,
                                            move |this, cx| {
                                                this.change(cx, |p| p.lid_screen = key.into())
                                            },
                                        )
                                    },
                                ),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(caption("Sound"))
                            .child(
                                div().flex().gap_1().children(
                                    [
                                        ("leave", "Leave"),
                                        ("mute", "Mute"),
                                        ("pause", "Pause media"),
                                    ]
                                    .map(|(key, text)| {
                                        self.chip(
                                            format!("keep-awake-sound-{key}").into(),
                                            text.into(),
                                            sound == key,
                                            cx,
                                            move |this, cx| {
                                                this.change(cx, |p| p.lid_sound = key.into())
                                            },
                                        )
                                    }),
                                ),
                            ),
                    ),
            );
        }
        if let Some(error) = self.error.clone() {
            body = body.child(
                div()
                    .id("keep-awake-error")
                    .role(Role::Alert)
                    .text_size(px(11.))
                    .text_color(gpui::rgb(0xf87171))
                    .child(error),
            );
        }
        body.child(self.drag_watch(cx)).into_any_element()
    }
    fn consent(&self, cx: &mut Context<Self>) -> AnyElement {
        let amber = gpui::rgb(0xf59e0b);
        let mut tint: gpui::Hsla = amber.into();
        tint.a = 0.08;
        let mut edge: gpui::Hsla = amber.into();
        edge.a = 0.35;
        let installing = self.installing;
        div()
            .id("keep-awake-consent")
            .flex()
            .flex_col()
            .gap_2()
            .p(px(10.))
            .rounded_lg()
            .border_1()
            .border_color(edge)
            .bg(tint)
            .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).child("MonoCode needs admin access once"))
            .child(
                div()
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(content(0.6))
                    .child("It adds one rule so it can switch lid-close sleep off and back on without asking again. Nothing else gets admin rights."),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        crate::settings::widgets::secondary_button("keep-awake-consent-cancel", "Cancel", false, installing)
                            .child("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.asking_lid = false;
                                this.error = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        crate::settings::widgets::primary_button(
                            "keep-awake-consent-continue",
                            if installing { "Waiting…" } else { "Continue" },
                            installing,
                        )
                        .child(if installing { "Waiting…" } else { "Continue" })
                        .on_click(cx.listener(|this, _, _, cx| this.install_lid_rule(cx))),
                    ),
            )
            .into_any_element()
    }
}

fn progress_ring(filled: f32, awake: bool) -> AnyElement {
    let accent = accent_color();
    let track = content(0.12);
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let center = bounds.center();
            let radius = 11.;
            let stroke = 2.5;
            let ring = |from: f32, to: f32, color: gpui::Hsla, window: &mut Window| {
                if to <= from {
                    return;
                }
                let mut path = gpui::PathBuilder::stroke(px(stroke));
                let steps = ((to - from) * 64.).ceil().max(2.) as usize;
                for i in 0..=steps {
                    let t = from + (to - from) * i as f32 / steps as f32;
                    let a = t * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                    let p = gpui::point(
                        center.x + px(radius * a.cos()),
                        center.y + px(radius * a.sin()),
                    );
                    if i == 0 {
                        path.move_to(p);
                    } else {
                        path.line_to(p);
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            };
            ring(0., 1., track, window);
            if awake {
                ring(0., filled.max(0.001), accent, window);
            }
        },
    )
    .size(px(28.))
    .flex_shrink_0()
    .into_any_element()
}

/// The wall-clock time `after` from now, as `4:42 PM`.
fn clock_at(after: Duration) -> String {
    let at = chrono::Local::now() + chrono::Duration::seconds(after.as_secs() as i64);
    at.format("%-I:%M %p").to_string()
}

impl Render for KeepAwake {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.status();
        let awake = status.awake();
        let label = Self::label(status, self.hotspot.on_hotspot());
        let classic = self.classic;
        let weak = cx.weak_entity();
        let color = if awake {
            accent_color()
        } else if self.open {
            content(1.)
        } else {
            content(0.4)
        };
        div()
            .flex_shrink_0()
            .child(
                div()
                    .id("footer-keep-awake")
                    .debug_selector(|| "footer-keep-awake".into())
                    .role(Role::Button)
                    .aria_label("Keep Mac awake")
                    .aria_description(match (&label, status) {
                        (Some(label), _) => label.clone(),
                        (None, Status::Off) => "Off".into(),
                        (None, _) => "Waiting for an agent".into(),
                    })
                    .aria_expanded(self.open)
                    .track_focus(&self.trigger)
                    .tab_index(0)
                    .relative()
                    .h(px(20.))
                    .map(|d| {
                        if classic {
                            d.px(px(6.)).rounded_md()
                        } else {
                            d.px_2().rounded_full()
                        }
                    })
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .whitespace_nowrap()
                    .text_size(px(11.))
                    .text_color(color)
                    .when(awake, |d| {
                        let mut tint = accent_color();
                        tint.a = 0.14;
                        d.bg(tint)
                    })
                    .hover(|s| {
                        s.bg(content(0.1)).text_color(if awake {
                            accent_color()
                        } else {
                            content(1.)
                        })
                    })
                    .cursor_pointer()
                    .child(self.anchor.probe())
                    .child(icon("coffee", 14.).text_color(color))
                    .when_some(label, |d, label| d.child(label))
                    .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx))),
            )
            .when(self.open, |d| {
                d.child(
                    popover("keep-awake-details", Anchor::Slot(self.anchor.clone()))
                        .side(Side::Top)
                        .align(Align::End)
                        .gap(7.)
                        .width(WIDTH)
                        .max_height(700.)
                        .padding(12.)
                        .on_dismiss(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| this.dismiss(window, cx));
                        })
                        .child(self.details(cx)),
                )
            })
    }
}

/// Screenshot states for `shell::keep_awake_shots`.
#[cfg(test)]
impl KeepAwake {
    pub(crate) fn stage(&mut self, step: &str, window: &mut Window, cx: &mut Context<Self>) {
        match step {
            "agents" => self.set_agents(
                vec![
                    "Refactor provider lifecycle".into(),
                    "Fix inbox sync".into(),
                ],
                cx,
            ),
            "open" => {
                if !self.open {
                    self.toggle(window, cx)
                }
            }
            "timer" => self.start(Mode::Timer, cx),
            "lid" => {
                self.lid_ready = Some(true);
                self.start(Mode::Working, cx);
                self.change(cx, |p| p.lid = true);
            }
            "consent" => {
                self.lid_ready = Some(false);
                self.change(cx, |p| p.lid = false);
                self.want_lid(true, cx);
            }
            "hotspot" => {
                self.asking_lid = false;
                self.lid_ready = Some(true);
                self.change(cx, |p| {
                    p.lid = true;
                    p.hotspot = true;
                    p.hotspot_ssid = "My iPhone".into();
                });
                self.hotspot.joined();
                self.sync(cx);
            }
            "close" => {
                self.dismiss(window, cx);
                self.machine = Machine {
                    lid_closed: false,
                    hot: false,
                    ..self.machine
                };
                self.sync(cx);
            }
            "hot" => {
                self.hotspot.left();
                self.machine = Machine {
                    lid_closed: true,
                    hot: true,
                    ..self.machine
                };
                self.sync(cx);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pill_names_why_it_holds() {
        assert_eq!(KeepAwake::label(Status::Off, false), None);
        assert_eq!(KeepAwake::label(Status::Armed, false), None);
        assert_eq!(
            KeepAwake::label(Status::Working { agents: 1 }, false).unwrap(),
            "1 agent"
        );
        assert_eq!(
            KeepAwake::label(Status::Working { agents: 3 }, false).unwrap(),
            "3 agents"
        );
        assert_eq!(
            KeepAwake::label(
                Status::Timer {
                    left: Duration::from_secs(3725),
                    total: Duration::ZERO
                },
                false
            )
            .unwrap(),
            "1:02:05"
        );
        assert_eq!(KeepAwake::label(Status::Hot, false).unwrap(), "Paused");
        assert_eq!(
            KeepAwake::label(Status::Working { agents: 2 }, true).unwrap(),
            "2 agents · Hotspot"
        );
    }

    #[test]
    fn rulers_step_in_five_minutes_and_label_their_hours() {
        assert_eq!(Ruler::Grace.spec().2, 5);
        assert_eq!(Ruler::Timer.spec().2, 5);
        assert_eq!(Ruler::Grace.tick_label(90), "1h30");
        assert_eq!(Ruler::Timer.tick_label(120), "2h");
        for ruler in [Ruler::Grace, Ruler::Timer] {
            let (min, max, step, ..) = ruler.spec();
            assert!(
                ruler
                    .presets()
                    .iter()
                    .all(|m| (min..=max).contains(m) && m % step == 0)
            );
        }
    }

    #[gpui::test]
    fn the_switch_is_a_saved_setting(cx: &mut gpui::TestAppContext) {
        let view =
            cx.new(|cx| KeepAwake::new(KeepAwakePrefs::default(), PathBuf::from("/tmp"), cx));
        let saved = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let log = saved.clone();
        cx.update(|cx| {
            cx.subscribe(&view, move |_, PrefsChanged(p), _| {
                log.borrow_mut().push(p.enabled)
            })
            .detach()
        });
        view.update(cx, |this, cx| {
            this.set_agents(vec!["Fix inbox sync".into()], cx);
            assert_eq!(this.status(), Status::Working { agents: 1 });
            this.set_on(false, cx);
            assert_eq!(this.status(), Status::Off);
            // Off stays off, however many agents start.
            this.set_agents(vec!["Fix inbox sync".into(), "Port settings".into()], cx);
            assert_eq!(this.status(), Status::Off);
            // Picking a mode turns it back on.
            this.start(Mode::UntilOff, cx);
            assert!(matches!(this.status(), Status::UntilOff { .. }));
        });
        assert_eq!(*saved.borrow(), vec![false, true]);
    }

    #[gpui::test]
    fn dragging_a_ruler_moves_in_whole_steps_and_saves(cx: &mut gpui::TestAppContext) {
        let view =
            cx.new(|cx| KeepAwake::new(KeepAwakePrefs::default(), PathBuf::from("/tmp"), cx));
        let saved = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let log = saved.clone();
        cx.update(|cx| {
            cx.subscribe(&view, move |_, PrefsChanged(p), _| {
                log.borrow_mut().push(p.grace_minutes)
            })
            .detach()
        });
        view.update(cx, |this, cx| {
            this.drag = Some(Drag {
                ruler: Ruler::Grace,
                x: px(100.),
                minutes: 5,
            });
            // Dragging left moves the tape left: more minutes under the needle.
            this.drag_to(px(100. - 8. * 3.), cx);
            assert_eq!(this.prefs.grace_minutes, 20);
            this.drag_to(px(1000.), cx);
            assert_eq!(this.prefs.grace_minutes, 0);
        });
        assert_eq!(*saved.borrow(), vec![20, 0]);
    }

    #[gpui::test]
    fn the_lid_option_asks_before_it_needs_admin(cx: &mut gpui::TestAppContext) {
        let view =
            cx.new(|cx| KeepAwake::new(KeepAwakePrefs::default(), PathBuf::from("/tmp"), cx));
        view.update(cx, |this, cx| {
            this.want_lid(true, cx);
            assert!(this.asking_lid);
            assert!(!this.prefs.lid);
            this.lid_ready = Some(true);
            this.want_lid(true, cx);
            assert!(this.prefs.lid);
        });
    }
}
