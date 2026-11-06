//! The Device tab: iOS Simulators and Android Emulators in the right panel,
//! streamed from the device hub (`monocode_core::device::hub`).
//!
//! The app holds its full frame rate while a device streams because this
//! view does no work per video frame: frames go from `stream`'s threads to
//! `screen`'s video layer, and this entity re-renders only when something it
//! shows changes (the device, its state, the picture's size).
mod http;
pub(crate) mod overlay;
#[cfg(target_os = "macos")]
mod screen;
mod shelf;
pub(crate) mod sim;
mod stream;
mod tools;

use crate::{
    right_panel::{icon_action, pane_header, spinner, stroke},
    theme::*,
};
use gpui::{
    AnyElement, Context, EventEmitter, FocusHandle, Focusable, SharedString, Task, Window, div,
    prelude::*, px,
};
use monocode_core::device::{
    self, Device, DeviceList, Platform, hub,
    protocol::Button,
    toolchain::{self, Tool},
};
use stream::{Event, Input};

pub(crate) enum DeviceEvent {
    OpenSettings,
    /// Screenshots or a recording to add to the message being written.
    Attach(Vec<std::path::PathBuf>),
}

/// What the hub is doing, as far as this panel knows.
#[derive(Clone, Debug, PartialEq)]
enum HubState {
    Idle,
    /// Installing or starting; the label says which.
    Starting(SharedString),
    Ready,
    Failed(String),
}

/// A device being streamed.
struct Live {
    stream: stream::Stream,
    size: Option<(u32, u32)>,
    streaming: bool,
    /// Why the picture is not up, while reconnecting.
    connecting: Option<String>,
    input: Option<String>,
    _events: Task<()>,
}

pub(crate) struct DevicePanel {
    focus: FocusHandle,
    shown: bool,
    hub: HubState,
    list: Option<DeviceList>,
    listing: bool,
    selected: Option<Device>,
    /// Showing the device list in place of the screen.
    picking: bool,
    /// A boot or shutdown in progress.
    busy: Option<SharedString>,
    error: Option<String>,
    live: Option<Live>,
    #[cfg(target_os = "macos")]
    screen: Option<std::rc::Rc<screen::Screen>>,
    /// Bumped by each connect, so a late result for an older one is dropped.
    epoch: u64,
    /// What new devices can be made, once asked.
    options: Option<hub::NewDeviceOptions>,
    loading_options: bool,
    sheet: Option<shelf::NewDevice>,
    /// Devices being made, by platform and name.
    creating: Vec<(Platform, String)>,
    /// Ids of devices being deleted.
    deleting: Vec<String>,
    menu: Option<shelf::TileMenu>,
    menu_anchor: crate::popover::AnchorSlot,
    /// A failed create, delete or shutdown, shown above the shelf.
    notice: Option<String>,
    /// Closes the stream a while after the panel hides; dropped when it
    /// shows again first.
    hide: Option<Task<()>>,
    /// The tool rail beside an iOS Simulator.
    tools: tools::Tools,
}

/// How long a hidden panel keeps its stream, so switching tabs and back
/// shows the device at once instead of reconnecting (about a second and a
/// half). Hidden, its view is off screen and GPUI does no work for it.
const KEEP_HIDDEN: std::time::Duration = std::time::Duration::from_secs(60);

impl EventEmitter<DeviceEvent> for DevicePanel {}
impl Focusable for DevicePanel {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

/// An iPhone's shape, until the first frame gives the real one.
const DEFAULT_ASPECT: f32 = 1206. / 2622.;

/// `device` as `list` has it. An emulator's id changes from its AVD name to a
/// serial on boot.
fn latest<'a>(list: &'a DeviceList, device: &Device) -> Option<&'a Device> {
    list.find(&device.id).or_else(|| {
        list.devices
            .iter()
            .find(|d| d.platform == device.platform && d.name == device.name)
    })
}

impl DevicePanel {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            shown: false,
            hub: HubState::Idle,
            list: None,
            listing: false,
            selected: None,
            picking: false,
            busy: None,
            error: None,
            live: None,
            #[cfg(target_os = "macos")]
            screen: None,
            epoch: 0,
            options: None,
            loading_options: false,
            sheet: None,
            creating: Vec::new(),
            deleting: Vec::new(),
            menu: None,
            menu_anchor: Default::default(),
            notice: None,
            hide: None,
            tools: tools::Tools::default(),
        }
    }

    /// Streams only while on screen: hidden, the stream closes and the hub
    /// stops encoding for it.
    pub(crate) fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.shown == shown {
            return;
        }
        self.shown = shown;
        if shown {
            self.hide = None;
            self.activate(cx);
        } else if self.live.is_some() {
            self.hide = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(KEEP_HIDDEN).await;
                let _ = this.update(cx, |this, _| {
                    if !this.shown {
                        this.disconnect();
                    }
                });
            }));
        }
        cx.notify();
    }

    fn activate(&mut self, cx: &mut Context<Self>) {
        if !device::enabled() {
            return;
        }
        if self.list.is_none() && !self.listing {
            // The last list shows the shelf, and connects a booted device,
            // while a fresh one loads.
            if let Some(list) = hub::cached_list() {
                self.hub = HubState::Ready;
                self.apply_list(list, cx);
            }
            self.refresh(cx);
        } else if self.selected.as_ref().is_some_and(|d| d.booted) && self.live.is_none() {
            self.connect(cx);
        }
    }

    fn disconnect(&mut self) {
        self.live = None;
        self.epoch += 1;
        #[cfg(target_os = "macos")]
        if let Some(screen) = &self.screen {
            screen.set_input(None);
        }
    }

    fn starting_label() -> SharedString {
        if Tool::Hub.installed() {
            "Starting the Device Hub…".into()
        } else {
            "Installing the Device Hub…".into()
        }
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        if !device::enabled() || self.listing {
            return;
        }
        self.listing = true;
        if hub::running().is_none() {
            self.hub = HubState::Starting(Self::starting_label());
        }
        cx.notify();
        let work = cx.background_executor().spawn(async move { hub::list() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.listing = false;
                match result {
                    Ok(list) => {
                        this.hub = HubState::Ready;
                        this.apply_list(list, cx);
                    }
                    Err(error) => this.hub = HubState::Failed(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_list(&mut self, list: DeviceList, cx: &mut Context<Self>) {
        let current = self
            .selected
            .as_ref()
            .and_then(|selected| latest(&list, selected).cloned());
        let pick = current.or_else(|| {
            (self.selected.is_none())
                .then(|| list.devices.iter().find(|d| d.booted).cloned())
                .flatten()
        });
        self.list = Some(list);
        match pick {
            Some(device) => {
                let changed = self.selected.as_ref().map(|d| (&d.id, d.booted))
                    != Some((&device.id, device.booted));
                self.selected = Some(device);
                if changed || self.live.is_none() {
                    // A connect from a stale list may have failed for a
                    // device that has since stopped.
                    if changed {
                        self.error = None;
                    }
                    self.disconnect();
                    if self.selected.as_ref().is_some_and(|d| d.booted) {
                        self.connect(cx);
                    }
                }
            }
            None => {
                if self.selected.is_some() {
                    self.disconnect();
                }
                self.selected = None;
                self.picking = true;
            }
        }
    }

    /// Shows `device`, booting it if it is off.
    pub(crate) fn select(&mut self, device: Device, cx: &mut Context<Self>) {
        self.picking = false;
        self.error = None;
        let same = self.selected.as_ref().is_some_and(|d| d.id == device.id);
        if same && self.live.is_some() {
            cx.notify();
            return;
        }
        self.disconnect();
        #[cfg(target_os = "macos")]
        if !same && let Some(screen) = &self.screen {
            screen.clear();
        }
        let booted = device.booted;
        self.selected = Some(device);
        if booted {
            self.connect(cx);
        } else {
            self.boot(cx);
        }
        cx.notify();
    }

    pub(crate) fn selected(&self) -> Option<&Device> {
        self.selected.as_ref()
    }

    /// Goes back to the device a project last showed, leaving it off if it
    /// has stopped: switching projects shouldn't boot anything. Hidden, the
    /// stream waits for `activate`.
    pub(crate) fn recall(&mut self, device: &Device, cx: &mut Context<Self>) {
        let device = match &self.list {
            Some(list) => latest(list, device).cloned(),
            None => Some(device.clone()),
        };
        let Some(device) = device else {
            return;
        };
        if self.selected.as_ref().is_some_and(|d| d.id == device.id) {
            return;
        }
        self.disconnect();
        #[cfg(target_os = "macos")]
        if let Some(screen) = &self.screen {
            screen.clear();
        }
        self.picking = false;
        self.error = None;
        let booted = device.booted;
        self.selected = Some(device);
        if self.shown && booted {
            self.connect(cx);
        }
        cx.notify();
    }

    /// Shows a device an agent opened; it is booted already.
    pub(crate) fn show(&mut self, device: Device, cx: &mut Context<Self>) {
        if let Some(list) = self.list.as_mut() {
            match list.devices.iter_mut().find(|d| {
                d.id == device.id || (d.platform == device.platform && d.name == device.name)
            }) {
                Some(entry) => *entry = device.clone(),
                None => list.devices.insert(0, device.clone()),
            }
        }
        self.hub = HubState::Ready;
        self.select(device, cx);
    }

    fn boot(&mut self, cx: &mut Context<Self>) {
        let Some(device) = self.selected.clone() else {
            return;
        };
        self.busy = Some(format!("Starting {}…", device.name).into());
        cx.notify();
        let work = cx.background_executor().spawn({
            let device = device.clone();
            async move { hub::boot(&device) }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(id) => {
                        if let Some(selected) = this.selected.as_mut()
                            && selected.name == device.name
                        {
                            selected.id = id;
                            selected.booted = true;
                        }
                        this.connect(cx);
                        this.list = None;
                        this.refresh(cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        if !self.shown || !device::enabled() {
            return;
        }
        let Some(device) = self.selected.clone().filter(|d| d.booted) else {
            return;
        };
        self.epoch += 1;
        let epoch = self.epoch;
        let work = cx.background_executor().spawn({
            let device = device.clone();
            async move {
                let endpoint = hub::ensure()?;
                if device.platform == Platform::Ios {
                    hub::attach(&endpoint, &device.id)?;
                }
                Ok::<_, String>(endpoint)
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.epoch != epoch || !this.shown {
                    return;
                }
                match result {
                    Ok(endpoint) => {
                        this.hub = HubState::Ready;
                        this.start_stream(endpoint, device, cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    #[cfg(target_os = "macos")]
    fn start_stream(
        &mut self,
        endpoint: std::sync::Arc<hub::Endpoint>,
        device: Device,
        cx: &mut Context<Self>,
    ) {
        if self.screen.is_none() {
            self.screen = screen::Screen::new();
        }
        let Some(screen) = self.screen.clone() else {
            self.error = Some("The device screen could not be created.".into());
            return;
        };
        let stream =
            match stream::start(endpoint, device.platform, device.id.clone(), screen.sink()) {
                Ok(stream) => stream,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            };
        screen.set_input(Some(stream.sender()));
        let events = stream.events.clone();
        let epoch = self.epoch;
        let task = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let alive = this
                    .update(cx, |this, cx| {
                        if this.epoch == epoch {
                            this.apply_event(event, cx);
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        });
        self.error = None;
        self.live = Some(Live {
            stream,
            size: None,
            streaming: false,
            connecting: None,
            input: None,
            _events: task,
        });
    }

    #[cfg(not(target_os = "macos"))]
    fn start_stream(&mut self, _: std::sync::Arc<hub::Endpoint>, _: Device, _: &mut Context<Self>) {
        self.error = Some("Device screens need the macOS app.".into());
    }

    fn apply_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let mut attach = false;
        match event {
            Event::Connecting(reason) => {
                live.streaming = false;
                live.connecting = reason;
            }
            Event::Streaming => {
                let first = !live.streaming;
                live.streaming = true;
                live.connecting = None;
                attach = first;
            }
            Event::Size(width, height) => live.size = Some((width, height)),
            Event::Input(ready, reason) => {
                live.input = (!ready)
                    .then(|| reason.unwrap_or_default())
                    .filter(|r| !r.is_empty())
            }
        }
        if attach {
            self.tools_attach(cx);
        }
        cx.notify();
    }

    fn shutdown(&mut self, cx: &mut Context<Self>) {
        let Some(device) = self.selected.clone() else {
            return;
        };
        self.disconnect();
        self.busy = Some(format!("Shutting down {}…", device.name).into());
        cx.notify();
        let work = cx
            .background_executor()
            .spawn(async move { hub::shutdown(&device) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                if let Err(error) = result {
                    this.error = Some(error);
                }
                if let Some(selected) = this.selected.as_mut() {
                    selected.booted = false;
                }
                this.list = None;
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn send(&self, input: Input) {
        if let Some(live) = &self.live {
            live.stream.send(input);
        }
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        if matches!(self.hub, HubState::Failed(_)) {
            self.hub = HubState::Idle;
            self.list = None;
            self.refresh(cx);
        } else if self.selected.as_ref().is_some_and(|d| d.booted) {
            self.disconnect();
            self.connect(cx);
        } else {
            self.refresh(cx);
        }
        cx.notify();
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected.clone();
        let live = self.live.is_some();
        let platform = selected.as_ref().map(|d| d.platform);
        let title: SharedString = match &selected {
            Some(device) if !self.picking => device.name.clone().into(),
            _ => "Devices".into(),
        };
        let mut header = pane_header("device-header", "Device").child(
            div()
                .id("device-picker")
                .debug_selector(|| "device-picker".into())
                .role(gpui::Role::Button)
                .aria_label("Choose a device")
                .flex()
                .items_center()
                .gap_1()
                .min_w_0()
                .px_1()
                .h(px(24.))
                .rounded(px(4.))
                .hover(|s| s.bg(content(0.08)))
                .child(div().truncate().child(title))
                .child(icon("chevron-down", 12.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.picking = !this.picking;
                    if this.picking && !this.listing {
                        this.refresh(cx);
                    }
                    cx.notify();
                })),
        );
        header = header.child(div().flex_1());
        if let Some(elapsed) = self.recording_label().filter(|_| !self.picking) {
            let red: gpui::Hsla = gpui::rgb(0xf87171).into();
            header = header.child(
                div()
                    .id("device-recording")
                    .debug_selector(|| "device-recording".into())
                    .role(gpui::Role::Button)
                    .aria_label("Stop recording")
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .h(px(22.))
                    .px(px(6.))
                    .rounded(px(5.))
                    .text_size(px(11.5))
                    .text_color(red)
                    .cursor_pointer()
                    .hover(|s| s.bg(content(0.08)))
                    .child(div().size(px(7.)).rounded_full().bg(red))
                    .child(elapsed)
                    .child(div().text_color(content(0.7)).child("Stop"))
                    .on_click(cx.listener(|this, _, _, cx| this.stop_recording(cx))),
            );
        } else if self.recording_busy() {
            header = header.child(spinner("device-recording-saving", "loader", 12.));
        }
        if live && !self.picking {
            header = header
                .when(self.tools_available(), |h| {
                    h.child(
                        icon_action(
                            "device-screenshot",
                            "Add a screenshot to the message",
                            "camera",
                            !self.screenshot_busy(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.screenshot_busy() {
                                this.attach_screenshot(cx)
                            }
                        })),
                    )
                })
                .child(
                    icon_action("device-home", "Home", "home-01", true).on_click(
                        cx.listener(|this, _, _, _| this.send(Input::Button(Button::Home))),
                    ),
                )
                .when(platform == Some(Platform::Android), |h| {
                    h.child(
                        icon_action("device-back", "Back", "arrow-left", true).on_click(
                            cx.listener(|this, _, _, _| this.send(Input::Button(Button::Back))),
                        ),
                    )
                })
                .when(platform == Some(Platform::Ios), |h| {
                    h.child(
                        icon_action("device-rotate", "Rotate", "rotate-ccw", true)
                            .on_click(cx.listener(|this, _, _, _| this.send(Input::Rotate))),
                    )
                })
                .when(!self.selected.as_ref().is_some_and(|d| d.physical), |h| {
                    h.child(
                        icon_action("device-shutdown", "Shut down", "power", true)
                            .on_click(cx.listener(|this, _, _, cx| this.shutdown(cx))),
                    )
                });
        } else {
            header = header.child(
                icon_action("device-refresh", "Refresh", "refresh-cw", !self.listing)
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            );
        }
        header.into_any_element()
    }

    fn message(
        &self,
        title: impl Into<SharedString>,
        detail: Option<String>,
        action: Option<(&'static str, &'static str)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .p_6()
            .text_center()
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(content(0.85))
                    .child(title.into()),
            )
            .when_some(detail, |d, detail| {
                d.child(
                    div()
                        .max_w(px(320.))
                        .text_size(px(12.))
                        .text_color(content(0.55))
                        .child(detail),
                )
            })
            .when_some(action, |d, (id, label)| {
                d.child(
                    crate::settings::widgets::secondary_button(id, label, false, false)
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| match id {
                            "device-settings" => cx.emit(DeviceEvent::OpenSettings),
                            _ => this.retry(cx),
                        })),
                )
            })
            .into_any_element()
    }

    #[cfg(target_os = "macos")]
    fn screen_area(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.screen.is_none() {
            self.screen = screen::Screen::new();
        }
        let live = self.live.as_ref();
        let aspect = live
            .and_then(|live| live.size)
            .map(|(w, h)| w as f32 / h.max(1) as f32)
            .unwrap_or(DEFAULT_ASPECT);
        let status: Option<SharedString> = if let Some(busy) = &self.busy {
            Some(busy.clone())
        } else if let Some(live) = live {
            (!live.streaming).then(|| {
                live.connecting
                    .clone()
                    .map(|reason| format!("Reconnecting… {reason}"))
                    .unwrap_or_else(|| "Connecting…".into())
                    .into()
            })
        } else if self.selected.as_ref().is_some_and(|d| !d.booted) {
            Some("Off".into())
        } else {
            Some("Connecting…".into())
        };
        let streaming = live.is_some_and(|live| live.streaming);
        let note = live.and_then(|live| live.input.clone());
        let screen = self.screen.clone();
        let picture = div()
            .relative()
            .flex_1()
            .min_h_0()
            .p_4()
            .flex()
            .flex_col()
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .when_some(screen, |d, screen| {
                        d.child(screen.element(aspect, 22., content(0.06), self.live.is_some()))
                    })
                    .when_some(status.filter(|_| !streaming), |d, status| {
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .text_size(px(12.))
                                .text_color(content(0.55))
                                .when(self.busy.is_some() || self.live.is_some(), |d| {
                                    d.child(spinner("device-spinner", "loader", 16.))
                                })
                                .child(div().max_w(px(240.)).text_center().child(status)),
                        )
                    }),
            )
            .when_some(note, |d, note| {
                d.child(
                    div()
                        .pt_2()
                        .text_center()
                        .text_size(px(11.))
                        .text_color(content(0.45))
                        .child(note),
                )
            })
            .when(
                self.selected.as_ref().is_some_and(|d| !d.booted) && self.busy.is_none(),
                |d| {
                    d.child(
                        div().pt_2().flex().justify_center().child(
                            crate::settings::widgets::secondary_button(
                                "device-boot",
                                "Start",
                                false,
                                false,
                            )
                            .child("Start")
                            .on_click(cx.listener(|this, _, _, cx| this.boot(cx))),
                        ),
                    )
                },
            );
        if !self.tools_available() {
            return picture.into_any_element();
        }
        let dock = self.tool_dock(window, cx);
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(self.tool_rail(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(picture)
                    .children(dock)
                    .child(self.overrides_strip(cx)),
            )
            .into_any_element()
    }

    #[cfg(not(target_os = "macos"))]
    fn screen_area(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.message("Device screens need the macOS app.", None, None, cx)
    }
}

impl gpui::Render for DevicePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if !device::enabled() {
            self.message(
                "Devices are off",
                Some("Turn them on to see and use iOS Simulators and Android Emulators here. MonoCode installs the Device Hub and runs it with your Node.js.".into()),
                Some(("device-settings", "Open Settings")),
                cx,
            )
        } else if let HubState::Failed(error) = &self.hub {
            self.message(
                "The Device Hub could not start",
                Some(error.clone()),
                Some(("device-retry", "Try again")),
                cx,
            )
        } else if let Some(error) = self.error.clone().filter(|_| self.busy.is_none()) {
            self.message(
                "Something went wrong",
                Some(error),
                Some(("device-retry", "Try again")),
                cx,
            )
        } else if let HubState::Starting(label) = &self.hub
            && self.list.is_none()
        {
            let label = label.clone();
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .text_size(px(12.))
                .text_color(content(0.55))
                .child(spinner("device-hub-spinner", "loader", 16.))
                .child(label)
                .into_any_element()
        } else if self.picking || self.selected.is_none() {
            self.shelf(cx)
        } else {
            self.screen_area(window, cx)
        };
        let flyout = self.tool_flyout(window, cx);
        div()
            .id("device-panel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event, window, cx| this.tool_key(event, window, cx)))
            .relative()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.tools.region.probe())
            .child(self.header(cx))
            .child(body)
            .children(flyout)
            .child(div().h(px(1.)).bg(stroke()).flex_shrink_0().opacity(0.))
    }
}

/// What Settings › Devices shows: what this Mac has, and the tool install
/// or removal under way. Observed by Settings.
#[derive(Default)]
pub(crate) struct Setup {
    pub(crate) doctor: Option<toolchain::Doctor>,
    checking: bool,
    pub(crate) installing: Option<Tool>,
    pub(crate) error: Option<String>,
    _task: Option<Task<()>>,
}
impl gpui::Global for Setup {}

pub(crate) fn setup(cx: &gpui::App) -> Option<&Setup> {
    cx.try_global::<Setup>()
}

fn changed(cx: &mut gpui::App) {
    cx.update_global::<Setup, _>(|_, _| ());
}

/// Looks again at Node, Xcode and the Android SDK, in the background.
pub(crate) fn check(cx: &mut gpui::App) {
    // Read first: taking the global mutably tells Settings it changed, and a
    // check started from its render would otherwise re-render it forever.
    if setup(cx).is_some_and(|s| s.checking) {
        return;
    }
    cx.default_global::<Setup>().checking = true;
    let work = cx
        .background_executor()
        .spawn(async { toolchain::Doctor::check() });
    cx.spawn(async move |cx| {
        let doctor = work.await;
        cx.update(|cx| {
            let setup = cx.default_global::<Setup>();
            setup.doctor = Some(doctor);
            setup.checking = false;
            changed(cx);
        });
    })
    .detach();
}

/// Installs whichever of `tools` are missing, one after another.
pub(crate) fn install(tools: &[Tool], cx: &mut gpui::App) {
    let missing: Vec<Tool> = tools.iter().copied().filter(|t| !t.installed()).collect();
    if missing.is_empty() || setup(cx).is_some_and(|s| s.installing.is_some()) {
        return;
    }
    let setup = cx.default_global::<Setup>();
    setup.error = None;
    setup.installing = missing.first().copied();
    changed(cx);
    let task = cx.spawn(async move |cx| {
        for tool in missing {
            cx.update(|cx| {
                cx.default_global::<Setup>().installing = Some(tool);
                changed(cx);
            });
            let result = cx
                .background_executor()
                .spawn(async move { toolchain::install(tool) })
                .await;
            if let Err(error) = result {
                cx.update(|cx| {
                    let setup = cx.default_global::<Setup>();
                    setup.installing = None;
                    setup.error = Some(error);
                    changed(cx);
                });
                return;
            }
        }
        cx.update(|cx| {
            cx.default_global::<Setup>().installing = None;
            changed(cx);
        });
    });
    cx.default_global::<Setup>()._task = Some(task);
}

/// Stops the hub and deletes the installed tools.
pub(crate) fn remove(cx: &mut gpui::App) {
    if setup(cx).is_some_and(|s| s.installing.is_some()) {
        return;
    }
    let work = cx.background_executor().spawn(async {
        hub::stop();
        toolchain::remove_all()
    });
    cx.spawn(async move |cx| {
        let result = work.await;
        cx.update(|cx| {
            cx.default_global::<Setup>().error = result.err();
            changed(cx);
        });
    })
    .detach();
}
