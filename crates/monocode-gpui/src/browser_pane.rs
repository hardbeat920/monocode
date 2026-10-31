//! "Show in MonoCode": one of a thread's browser tabs, live beside the
//! thread. The pictures the browser makes of the tab are drawn as images,
//! with no web view of our own; clicks, scrolls and keys go back to the tab.
use crate::{
    input::{self, Input},
    popover::AnchorSlot,
    theme::*,
};
use gpui::{
    AnyElement, App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, ImageSource,
    KeyDownEvent, KeyUpEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, RenderImage, Role, ScrollDelta, ScrollWheelEvent, SharedString, Subscription,
    Task, Window, div, img, prelude::*, px,
};
use monocode_core::browser::{
    Ended, LivePane,
    live::{KeyEvent, LiveInput, LiveNav, MouseButton as LiveButton, MouseEvent, PanelSize},
    protocol::TabId,
};
use std::sync::Arc;

/// Ctrl+T (Cmd+T) in the pane: the user wants a new tab of the thread.
pub(crate) struct NewTab;

/// A picture on screen: its image and the page viewport it shows, in CSS
/// pixels.
struct Shown {
    image: Arc<RenderImage>,
    width: f64,
    height: f64,
    /// Drawn at least once (counted for a development build's notes).
    drawn: bool,
}

pub(crate) struct BrowserPane {
    pub(crate) session: i64,
    pub(crate) tab: TabId,
    focus: FocusHandle,
    address: Entity<Input>,
    live: Option<Arc<LivePane>>,
    picture: Option<Shown>,
    ended: Option<Ended>,
    error: Option<String>,
    stage: AnchorSlot,
    size: Option<PanelSize>,
    /// Buttons held down over the picture, CDP's bit mask.
    buttons: u32,
    /// Where the button went down, until the pointer leaves the click's
    /// slop ([`CLICK_SLOP`]).
    press: Option<Press>,
    title: String,
    url: String,
    /// A blank tab the user just opened: the address field takes the keys.
    focus_address: bool,
    _pull: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<NewTab> for BrowserPane {}
impl Focusable for BrowserPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// CDP's modifier bits: Alt 1, Ctrl 2, Meta 4, Shift 8.
fn modifier_bits(modifiers: &Modifiers) -> u32 {
    u32::from(modifiers.alt)
        | (u32::from(modifiers.control) << 1)
        | (u32::from(modifiers.platform) << 2)
        | (u32::from(modifiers.shift) << 3)
}

fn button_of(button: MouseButton) -> Option<(LiveButton, u32)> {
    match button {
        MouseButton::Left => Some((LiveButton::Left, 1)),
        MouseButton::Right => Some((LiveButton::Right, 2)),
        MouseButton::Middle => Some((LiveButton::Middle, 4)),
        MouseButton::Navigate(_) => None,
    }
}

/// Keys that edit or move rather than type: the DOM's key and code, and
/// the virtual key code.
fn raw_key(key: &str) -> Option<(&'static str, &'static str, u32)> {
    Some(match key {
        "backspace" => ("Backspace", "Backspace", 8),
        "tab" => ("Tab", "Tab", 9),
        "enter" => ("Enter", "Enter", 13),
        "escape" => ("Escape", "Escape", 27),
        "pageup" => ("PageUp", "PageUp", 33),
        "pagedown" => ("PageDown", "PageDown", 34),
        "end" => ("End", "End", 35),
        "home" => ("Home", "Home", 36),
        "left" => ("ArrowLeft", "ArrowLeft", 37),
        "up" => ("ArrowUp", "ArrowUp", 38),
        "right" => ("ArrowRight", "ArrowRight", 39),
        "down" => ("ArrowDown", "ArrowDown", 40),
        "delete" => ("Delete", "Delete", 46),
        _ => return None,
    })
}

/// The tab's input for a key the user pressed in the pane: editing keys,
/// shortcuts such as select all, and the text the key typed.
pub(crate) fn key_input(keystroke: &gpui::Keystroke, down: bool) -> Option<LiveInput> {
    let modifiers = modifier_bits(&keystroke.modifiers);
    if let Some((key, code, key_code)) = raw_key(&keystroke.key) {
        return Some(LiveInput::Key {
            event: match (down, key_code) {
                (false, _) => KeyEvent::KeyUp,
                (true, 13) => KeyEvent::KeyDown,
                (true, _) => KeyEvent::RawKeyDown,
            },
            key: key.into(),
            code: code.into(),
            text: (down && key_code == 13).then(|| "\r".into()),
            key_code,
            modifiers,
        });
    }
    let command = keystroke.modifiers.control || keystroke.modifiers.platform;
    let mut chars = keystroke.key.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && command
        && !keystroke.modifiers.alt
        && c.is_ascii_alphanumeric()
    {
        let upper = c.to_ascii_uppercase();
        return Some(LiveInput::Key {
            event: if down {
                KeyEvent::RawKeyDown
            } else {
                KeyEvent::KeyUp
            },
            key: keystroke.key.clone(),
            code: if c.is_ascii_digit() {
                format!("Digit{c}")
            } else {
                format!("Key{upper}")
            },
            text: None,
            key_code: u32::from(upper),
            modifiers,
        });
    }
    if !down || command {
        return None;
    }
    if keystroke.key == "space" {
        return Some(LiveInput::Text { text: " ".into() });
    }
    keystroke
        .key_char
        .clone()
        .filter(|text| !text.is_empty())
        .map(|text| LiveInput::Text { text })
}

/// How far the pointer may move while pressed and still click. Chromium
/// starts dragging a link from 4 pixels, which a hand easily moves while
/// clicking in the pane (Edge on Windows then sends the link no click); a
/// drag that goes further is a drag.
const CLICK_SLOP: f32 = 8.;

/// A button that went down over the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Press {
    /// In the window.
    at: Point<Pixels>,
    /// In the page.
    page: (f64, f64),
}

/// What the pane sends for a mouse event while a button may be held.
#[derive(Debug, PartialEq)]
enum Pointer {
    /// Send it at this point of the page.
    Send((f64, f64)),
    /// Hold it back: a hand that shakes while clicking.
    Hold,
}

/// Keep a shaking click a click: moves within [`CLICK_SLOP`] of the press
/// stay with MonoCode and the release lands where the button went down, so
/// the page sees a click and never the start of a drag. `press` is updated
/// for the next event.
fn through_slop(
    press: &mut Option<Press>,
    event: MouseEvent,
    at: Point<Pixels>,
    page: (f64, f64),
) -> Pointer {
    if event == MouseEvent::Pressed {
        *press = Some(Press { at, page });
        return Pointer::Send(page);
    }
    let Some(down) = *press else {
        return Pointer::Send(page);
    };
    let moved = (at.x - down.at.x)
        .as_f32()
        .hypot((at.y - down.at.y).as_f32());
    let within = moved < CLICK_SLOP;
    match event {
        MouseEvent::Moved if within => Pointer::Hold,
        MouseEvent::Moved => {
            *press = None;
            Pointer::Send(page)
        }
        _ => {
            *press = None;
            Pointer::Send(if within { down.page } else { page })
        }
    }
}

/// A point on the shown picture, in the page's CSS pixels.
fn page_point(at: Point<Pixels>, rect: Bounds<Pixels>, width: f64, height: f64) -> (f64, f64) {
    let fraction = |offset: Pixels, side: Pixels| {
        (f64::from(offset.as_f32()) / f64::from(side.as_f32().max(1.))).clamp(0., 1.)
    };
    let round = |v: f64| (v * 10.).round() / 10.;
    (
        round(fraction(at.x - rect.left(), rect.size.width) * width),
        round(fraction(at.y - rect.top(), rect.size.height) * height),
    )
}

impl BrowserPane {
    /// `blank`: the user opened the tab here; they type its address.
    pub(crate) fn new(session: i64, tab: TabId, blank: bool, cx: &mut Context<Self>) -> Self {
        let address = cx.new(|cx| {
            let mut input = Input::field("Address", "Enter an address", content(0.35), cx);
            input.set_font_size(12.);
            input
        });
        let mut subscriptions =
            vec![
                cx.subscribe(&address, |this, input, _: &input::Submitted, cx| {
                    let url = input.read(cx).content.trim().to_owned();
                    if !url.is_empty() {
                        this.navigate(LiveNav::Go { url }, cx);
                    }
                }),
            ];
        if let Some(browser) = crate::browser::get(cx) {
            subscriptions.push(cx.observe(&browser, |this, _, cx| this.follow_tab(cx)));
        }
        cx.on_release(|this, cx| {
            if let Some(shown) = this.picture.take() {
                cx.drop_image(shown.image, None);
            }
        })
        .detach();
        let mut this = Self {
            session,
            tab,
            focus: cx.focus_handle(),
            address,
            live: None,
            picture: None,
            ended: None,
            error: None,
            stage: AnchorSlot::default(),
            size: None,
            buttons: 0,
            press: None,
            title: String::new(),
            url: String::new(),
            focus_address: blank,
            _pull: None,
            _subscriptions: subscriptions,
        };
        this.follow_tab(cx);
        this
    }

    /// The tab's title and address from the module's state.
    fn follow_tab(&mut self, cx: &mut Context<Self>) {
        let Some(state) = crate::browser::state(cx) else {
            return;
        };
        let Some(tab) = state.tabs.iter().find(|t| t.tab.tab == self.tab) else {
            return;
        };
        let (title, url) = (tab.tab.title.clone(), tab.tab.url.clone());
        if url != self.url {
            // An address the user is typing stays until they send it.
            if self.address.read(cx).content.as_ref() == self.url {
                self.address
                    .update(cx, |input, cx| input.set_text(url.clone(), cx));
            }
            self.url = url;
            cx.notify();
        }
        if title != self.title {
            self.title = title;
            cx.notify();
        }
    }

    pub(crate) fn title(&self) -> String {
        if self.title.is_empty() {
            monocode_core::browser::view::tab_host(&self.url)
        } else {
            self.title.clone()
        }
    }

    /// Start the view once the pane has a size; follow later sizes.
    fn sync_size(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.stage.get() else {
            return;
        };
        let size = PanelSize::new(
            bounds.size.width.as_f32().round() as u32,
            bounds.size.height.as_f32().round() as u32,
            f64::from(window.scale_factor()),
        );
        if self.size == Some(size) {
            return;
        }
        self.size = Some(size);
        match &self.live {
            Some(live) => {
                let live = live.clone();
                cx.background_executor()
                    .spawn(async move { live.resize(size) })
                    .detach();
            }
            None if self.ended.is_none() => self.start(size, cx),
            None => {}
        }
    }

    fn start(&mut self, size: PanelSize, cx: &mut Context<Self>) {
        let live = match LivePane::start(self.session, self.tab.clone(), size) {
            Ok(live) => Arc::new(live),
            Err(error) => {
                self.ended = Some(Ended::Failed(error));
                cx.notify();
                return;
            }
        };
        self.live = Some(live.clone());
        self.ended = None;
        self._pull = Some(cx.spawn(async move |this, cx| {
            loop {
                let pane = live.clone();
                let taken = cx
                    .background_executor()
                    .spawn(async move { (pane.take(), pane.ended(), pane.take_input_error()) })
                    .await;
                let (picture, ended, input_error) = taken;
                let keep = this.update(cx, |this, cx| {
                    if let Some(error) = input_error {
                        this.error = Some(error);
                        cx.notify();
                    }
                    match picture {
                        Some(Ok(picture)) => {
                            let image =
                                Arc::new(RenderImage::new(vec![image::Frame::new(picture.pixels)]));
                            let old = this.picture.replace(Shown {
                                image,
                                width: picture.width,
                                height: picture.height,
                                drawn: false,
                            });
                            if let Some(old) = old {
                                cx.drop_image(old.image, None);
                            }
                            cx.notify();
                            true
                        }
                        Some(Err(error)) => {
                            this.error = Some(error);
                            cx.notify();
                            true
                        }
                        None => match ended {
                            Some(ended) => {
                                this.ended = Some(ended);
                                this.live = None;
                                cx.notify();
                                false
                            }
                            None => true,
                        },
                    }
                });
                if !keep.unwrap_or(false) {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.ended = None;
        self.error = None;
        if let Some(size) = self.size {
            self.start(size, cx);
        }
    }

    fn send(&mut self, input: LiveInput) {
        if let Some(live) = &self.live {
            live.input(input);
        }
    }

    fn navigate(&mut self, nav: LiveNav, cx: &mut Context<Self>) {
        let Some(live) = self.live.clone() else {
            return;
        };
        let task = cx
            .background_executor()
            .spawn(async move { live.navigate(&nav) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.error = result.err();
                cx.notify();
            });
        })
        .detach();
    }

    /// Where the picture is drawn in the pane: the page's shape, as large as
    /// fits.
    fn picture_rect(&self) -> Option<(Bounds<Pixels>, f64, f64)> {
        let stage = self.stage.get()?;
        let shown = self.picture.as_ref()?;
        let (w, h) = (shown.width.max(1.), shown.height.max(1.));
        let scale = (f64::from(stage.size.width.as_f32()) / w)
            .min(f64::from(stage.size.height.as_f32()) / h);
        let size = gpui::size(px((w * scale) as f32), px((h * scale) as f32));
        let origin = gpui::point(
            stage.left() + (stage.size.width - size.width) / 2.,
            stage.top() + (stage.size.height - size.height) / 2.,
        );
        Some((Bounds { origin, size }, w, h))
    }

    fn mouse(
        &mut self,
        event: MouseEvent,
        at: Point<Pixels>,
        button: LiveButton,
        click_count: usize,
        modifiers: &Modifiers,
    ) {
        let Some((rect, width, height)) = self.picture_rect() else {
            return;
        };
        let page = page_point(at, rect, width, height);
        let Pointer::Send((x, y)) = through_slop(&mut self.press, event, at, page) else {
            return;
        };
        self.send(LiveInput::Mouse {
            event,
            x,
            y,
            button,
            buttons: self.buttons,
            click_count: click_count as u32,
            modifiers: modifier_bits(modifiers),
        });
    }

    fn toolbar_button(
        id: &'static str,
        label: &'static str,
        path: &'static str,
        cx: &mut Context<Self>,
        click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .size(px(24.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .hover(|s| s.bg(content(0.1)))
            .child(icon(path, 14.).text_color(content(0.6)))
            .on_click(cx.listener(move |this, _, _, cx| click(this, cx)))
            .into_any_element()
    }

    fn body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(ended) = &self.ended {
            let (text, retry) = match ended {
                Ended::TabGone => ("This tab closed or left the thread.".to_owned(), false),
                Ended::Stopped => ("The tab is shown somewhere else now.".to_owned(), true),
                Ended::Failed(error) => (error.clone(), true),
            };
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .p_6()
                .text_center()
                .text_size(px(13.))
                .text_color(content(0.5))
                .child(text)
                .when(retry, |d| {
                    d.child(
                        crate::settings::widgets::secondary_button(
                            "browser-pane-retry",
                            "Show it here",
                            false,
                            false,
                        )
                        .child("Show it here")
                        .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                    )
                })
                .into_any_element();
        }
        if let (Some(shown), Some(live)) = (&mut self.picture, &self.live)
            && !shown.drawn
        {
            shown.drawn = true;
            live.drawn();
        }
        let Some(shown) = &self.picture else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(crate::right_panel::spinner(
                    "browser-pane-loading",
                    "loader",
                    16.,
                ))
                .into_any_element();
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                img(ImageSource::Render(shown.image.clone()))
                    .max_w_full()
                    .max_h_full()
                    .w_full()
                    .h_full()
                    .object_fit(gpui::ObjectFit::Contain),
            )
            .into_any_element()
    }
}

impl Render for BrowserPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_size(window, cx);
        if std::mem::take(&mut self.focus_address) {
            window.focus(&self.address.focus_handle(cx), cx);
        }
        let toolbar = div()
            .h(px(36.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(content(0.08))
            .child(Self::toolbar_button(
                "browser-pane-back",
                "Back",
                "arrow-left",
                cx,
                |this, cx| this.navigate(LiveNav::Back, cx),
            ))
            .child(Self::toolbar_button(
                "browser-pane-forward",
                "Forward",
                "chevron-right",
                cx,
                |this, cx| this.navigate(LiveNav::Forward, cx),
            ))
            .child(Self::toolbar_button(
                "browser-pane-reload",
                "Reload",
                "rotate-ccw",
                cx,
                |this, cx| this.navigate(LiveNav::Reload, cx),
            ))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .rounded_md()
                    .bg(content(0.05))
                    .px_2()
                    .child(self.address.clone()),
            );
        let error = self.error.clone();
        div()
            .id("browser-pane")
            .debug_selector(|| "browser-pane".into())
            .role(Role::Group)
            .aria_label(SharedString::from(format!("Browser tab {}", self.title())))
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(toolbar)
            .when_some(error, |d, error| {
                d.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_size(px(12.))
                        .text_color(gpui::rgb(0xf87171))
                        .child(error),
                )
            })
            .child(
                div()
                    .id("browser-pane-stage")
                    .track_focus(&self.focus)
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.stage.layout_probe())
                    .child(self.body(cx))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            window.focus(&this.focus, cx);
                            this.press(event, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| this.press(event, cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| this.press(event, cx)),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseUpEvent, _, cx| this.release(event, cx)),
                    )
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseUpEvent, _, cx| this.release(event, cx)),
                    )
                    .on_mouse_up(
                        MouseButton::Middle,
                        cx.listener(|this, event: &MouseUpEvent, _, cx| this.release(event, cx)),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, _| {
                        this.mouse(
                            MouseEvent::Moved,
                            event.position,
                            LiveButton::None,
                            0,
                            &event.modifiers,
                        );
                    }))
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                        this.wheel(event);
                        cx.stop_propagation();
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if this.key(&event.keystroke, true, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| {
                        if this.key(&event.keystroke, false, cx) {
                            cx.stop_propagation();
                        }
                    })),
            )
    }
}

impl BrowserPane {
    fn press(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some((button, bit)) = button_of(event.button) else {
            return;
        };
        self.buttons |= bit;
        self.mouse(
            MouseEvent::Pressed,
            event.position,
            button,
            event.click_count.max(1),
            &event.modifiers,
        );
        cx.stop_propagation();
    }

    fn release(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some((button, bit)) = button_of(event.button) else {
            return;
        };
        self.buttons &= !bit;
        self.mouse(
            MouseEvent::Released,
            event.position,
            button,
            event.click_count.max(1),
            &event.modifiers,
        );
        cx.stop_propagation();
    }

    fn wheel(&mut self, event: &ScrollWheelEvent) {
        let Some((rect, width, height)) = self.picture_rect() else {
            return;
        };
        let (x, y) = page_point(event.position, rect, width, height);
        // GPUI's deltas move the content; the page's wheel moves the view.
        let (dx, dy) = match event.delta {
            ScrollDelta::Pixels(delta) => (delta.x.as_f32() as f64, delta.y.as_f32() as f64),
            ScrollDelta::Lines(delta) => (f64::from(delta.x) * 40., f64::from(delta.y) * 40.),
        };
        self.send(LiveInput::Wheel {
            x,
            y,
            delta_x: -dx,
            delta_y: -dy,
            modifiers: modifier_bits(&event.modifiers),
        });
    }

    /// A key in the pane; `true` when the tab took it.
    fn key(&mut self, keystroke: &gpui::Keystroke, down: bool, cx: &mut Context<Self>) -> bool {
        let command = keystroke.modifiers.control || keystroke.modifiers.platform;
        if command && !keystroke.modifiers.alt && !keystroke.modifiers.shift && keystroke.key == "t"
        {
            if down {
                cx.emit(NewTab);
            }
            return true;
        }
        if command && !keystroke.modifiers.alt && keystroke.key == "v" {
            if down
                && let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
                && !text.is_empty()
            {
                self.send(LiveInput::Text { text });
            }
            return true;
        }
        match key_input(keystroke, down) {
            Some(input) => {
                self.send(input);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(key: &str, key_char: Option<&str>, control: bool) -> gpui::Keystroke {
        gpui::Keystroke {
            modifiers: Modifiers {
                control,
                ..Default::default()
            },
            key: key.into(),
            key_char: key_char.map(Into::into),
        }
    }

    #[test]
    fn keys_reach_the_tab_as_the_page_expects_them() {
        assert_eq!(
            key_input(&stroke("enter", None, false), true),
            Some(LiveInput::Key {
                event: KeyEvent::KeyDown,
                key: "Enter".into(),
                code: "Enter".into(),
                text: Some("\r".into()),
                key_code: 13,
                modifiers: 0,
            })
        );
        assert_eq!(
            key_input(&stroke("a", None, true), true),
            Some(LiveInput::Key {
                event: KeyEvent::RawKeyDown,
                key: "a".into(),
                code: "KeyA".into(),
                text: None,
                key_code: 65,
                modifiers: 2,
            })
        );
        assert_eq!(
            key_input(&stroke("a", Some("ä"), false), true),
            Some(LiveInput::Text { text: "ä".into() })
        );
        assert_eq!(key_input(&stroke("a", Some("a"), false), false), None);
    }

    #[test]
    fn a_point_on_the_picture_is_a_point_on_the_page() {
        let rect = Bounds {
            origin: gpui::point(px(10.), px(20.)),
            size: gpui::size(px(200.), px(100.)),
        };
        assert_eq!(
            page_point(gpui::point(px(110.), px(70.)), rect, 1000., 500.),
            (500., 250.)
        );
        assert_eq!(
            page_point(gpui::point(px(0.), px(500.)), rect, 1000., 500.),
            (0., 500.)
        );
    }

    #[test]
    fn a_shaking_click_stays_a_click_and_a_longer_move_drags() {
        let at = |x: f32, y: f32| gpui::point(px(x), px(y));
        let mut press = None;
        let down = through_slop(&mut press, MouseEvent::Pressed, at(100., 100.), (50., 50.));
        assert_eq!(down, Pointer::Send((50., 50.)));
        // Chromium drags a link from 4 px: a hand moves that much clicking.
        let shake = through_slop(&mut press, MouseEvent::Moved, at(104., 103.), (52., 51.));
        assert_eq!(shake, Pointer::Hold);
        let up = through_slop(&mut press, MouseEvent::Released, at(105., 103.), (52., 51.));
        assert_eq!(up, Pointer::Send((50., 50.)), "released where it pressed");
        assert!(press.is_none());

        through_slop(&mut press, MouseEvent::Pressed, at(100., 100.), (50., 50.));
        let drag = through_slop(
            &mut press,
            MouseEvent::Moved,
            at(100. + CLICK_SLOP, 100.),
            (54., 50.),
        );
        assert_eq!(drag, Pointer::Send((54., 50.)));
        // Once a drag, the rest of it goes as it is, back near the start too.
        let back = through_slop(&mut press, MouseEvent::Moved, at(101., 100.), (50.5, 50.));
        assert_eq!(back, Pointer::Send((50.5, 50.)));
        let up = through_slop(
            &mut press,
            MouseEvent::Released,
            at(101., 100.),
            (50.5, 50.),
        );
        assert_eq!(up, Pointer::Send((50.5, 50.)));
        // A hover with no button is never held.
        let hover = through_slop(&mut press, MouseEvent::Moved, at(1., 1.), (0.5, 0.5));
        assert_eq!(hover, Pointer::Send((0.5, 0.5)));
    }
}
