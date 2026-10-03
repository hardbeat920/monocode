//! Port of src/shared/ui/ColorPickerPopover.tsx (`ColorSwatchRow` and
//! `ColorPickerPopover`), the HSV helpers it uses from
//! src/shared/lib/colorUtils.ts, and `AccentColorPicker` from
//! SettingsView.tsx.
//!
//! GPUI draws two-stop linear gradients only, so the hue bar is six
//! segments and the custom swatch's conic gradient becomes a horizontal hue
//! band under the pipette.

use std::rc::Rc;

use gpui::{
    Anchor, App, AppContext as _, Bounds, Context, ElementId, Entity, FocusHandle, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement as _, Pixels, Render, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, anchored, canvas, deferred,
    div, linear_color_stop, linear_gradient, px, relative,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_ui::color::{is_hex_color, parse_hex};
use monocode_ui::widgets::{POPOVER_GAP, POPOVER_PADDING, popover_frame};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::controls::plain_input;

/// `Hsv`: hue in degrees, saturation and value in percent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsv {
    pub h: f64,
    pub s: f64,
    pub v: f64,
}

/// `normalizeHex`. The webview asked a canvas to parse CSS colors; only
/// `#rrggbb` reaches this picker, and anything else reads as the canvas
/// fallback.
pub fn normalize_hex(color: &str) -> String {
    if is_hex_color(color) {
        color.to_lowercase()
    } else {
        "#808080".into()
    }
}

/// `hexToHsv`.
pub fn hex_to_hsv(hex: &str) -> Hsv {
    let value = normalize_hex(hex);
    let channel = |i: usize| u8::from_str_radix(&value[i..i + 2], 16).unwrap_or(0) as f64 / 255.0;
    let (r, g, b) = (channel(1), channel(3), channel(5));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let mut h = 0.0;
    if delta != 0.0 {
        h = if max == r {
            ((g - b) / delta) % 6.0
        } else if max == g {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }
    }
    let s = if max == 0.0 { 0.0 } else { delta / max };
    Hsv {
        h,
        s: s * 100.0,
        v: max * 100.0,
    }
}

/// `hsvToHex`.
pub fn hsv_to_hex(h: f64, s: f64, v: f64) -> String {
    let sat = s.clamp(0.0, 100.0) / 100.0;
    let val = v.clamp(0.0, 100.0) / 100.0;
    let hue = ((h % 360.0) + 360.0) % 360.0;
    let c = val * sat;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = val - c;
    let (r, g, b) = if hue < 60.0 {
        (c, x, 0.0)
    } else if hue < 120.0 {
        (x, c, 0.0)
    } else if hue < 180.0 {
        (0.0, c, x)
    } else if hue < 240.0 {
        (0.0, x, c)
    } else if hue < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let byte =
        |channel: f64| monocode_core::js::round(((channel + m) * 255.0).clamp(0.0, 255.0)) as u8;
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
}

/// The hex input's rule: six hex digits with an optional `#`.
pub fn parse_hex_input(raw: &str) -> Option<String> {
    let trimmed = monocode_core::js::trim(raw);
    let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(normalize_hex(&format!("#{digits}")))
    } else {
        None
    }
}

/// A swatch's paint: a theme color or a hex value.
#[derive(Clone)]
pub enum Swatch {
    Color(Hsla),
    Hex(SharedString),
}

type IndexHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;
type ToggleHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// `ColorSwatchRow`: preset swatches and the custom color button.
#[derive(IntoElement)]
pub struct ColorSwatchRow {
    colors: Vec<Swatch>,
    labels: Vec<SharedString>,
    color_index: Option<usize>,
    custom_color: Option<SharedString>,
    custom_picker_open: bool,
    on_pick_index: Option<IndexHandler>,
    on_toggle_custom: Option<ToggleHandler>,
}

pub fn color_swatch_row(colors: Vec<Swatch>, labels: Vec<SharedString>) -> ColorSwatchRow {
    ColorSwatchRow {
        colors,
        labels,
        color_index: None,
        custom_color: None,
        custom_picker_open: false,
        on_pick_index: None,
        on_toggle_custom: None,
    }
}

impl ColorSwatchRow {
    pub fn color_index(mut self, index: Option<usize>) -> Self {
        self.color_index = index;
        self
    }

    pub fn custom_color(mut self, color: Option<SharedString>) -> Self {
        self.custom_color = color;
        self
    }

    pub fn custom_picker_open(mut self, open: bool) -> Self {
        self.custom_picker_open = open;
        self
    }

    pub fn on_pick_index(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_pick_index = Some(Rc::new(f));
        self
    }

    pub fn on_toggle_custom(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_toggle_custom = Some(Rc::new(f));
        self
    }
}

/// A 14px swatch in a 20px button, ringed when selected (`ring-2
/// ring-content/80 ring-offset-1`).
fn swatch_button(
    id: ElementId,
    selector: String,
    selected: bool,
    ring: Hsla,
    inner: gpui::Div,
) -> gpui::Stateful<gpui::Div> {
    let mut button = div()
        .id(id)
        .flex()
        .flex_none()
        .size(u(20.))
        .items_center()
        .justify_center()
        .rounded_full()
        .debug_selector(move || selector);
    if selected {
        button = button.border_2().border_color(ring);
    }
    button.child(inner.size(u(14.)).rounded_full())
}

impl RenderOnce for ColorSwatchRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let ring = theme.content(0.80);
        let mut row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(u(4.))
            .px(u(2.));
        for (index, color) in self.colors.iter().enumerate() {
            let label = self
                .labels
                .get(index)
                .cloned()
                .unwrap_or_else(|| format!("Color {}", index + 1).into());
            let selected = self.custom_color.is_none()
                && (self.color_index == Some(index) || (self.color_index.is_none() && index == 0));
            let fill = match color {
                Swatch::Color(color) => *color,
                Swatch::Hex(hex) => parse_hex(hex).unwrap_or(theme.colors.content),
            };
            let mut button = swatch_button(
                ElementId::from(SharedString::from(format!("swatch-{index}"))),
                format!("swatch:{label}"),
                selected,
                ring,
                div().bg(fill),
            );
            if let Some(handler) = self.on_pick_index.clone() {
                button = button.on_click(move |_, window, cx| handler(index, window, cx));
            }
            row = row.child(button);
        }
        let pipette_active = self.custom_color.is_some() || self.custom_picker_open;
        let inner = match &self.custom_color {
            Some(hex) => div().bg(parse_hex(hex).unwrap_or(theme.colors.content)),
            None => div()
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(gpui::red(), 0.),
                    linear_color_stop(gpui::blue(), 1.),
                ))
                .child(
                    icon(IconName::Pipette)
                        .size(u(8.))
                        .text_color(gpui::white()),
                ),
        };
        let mut custom = swatch_button(
            ElementId::from("swatch-custom"),
            "swatch:Custom color".into(),
            pipette_active,
            ring,
            inner,
        );
        if let Some(handler) = self.on_toggle_custom.clone() {
            custom = custom.on_click(move |_, window, cx| handler(window, cx));
        }
        row.child(custom)
    }
}

type HexHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// Which surface a drag is moving.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dragging {
    None,
    Square,
    Hue,
}

/// `ColorPickerPopover`: a saturation and brightness square, a hue bar, and
/// a hex field.
pub struct ColorPicker {
    hsv: Hsv,
    dragging: Dragging,
    hex: Entity<InputState>,
    on_change: Option<HexHandler>,
    _hex_events: Subscription,
}

impl ColorPicker {
    pub fn new(value: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let hsv = hex_to_hsv(value);
        let preview = hsv_to_hex(hsv.h, hsv.s, hsv.v);
        let hex = cx.new(|cx| InputState::new(window, cx).default_value(preview));
        let hex_events = cx.subscribe_in(&hex, window, |this, hex, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = hex.read(cx).value().to_string();
                this.on_hex_input(&raw, window, cx);
            }
        });
        Self {
            hsv,
            dragging: Dragging::None,
            hex,
            on_change: None,
            _hex_events: hex_events,
        }
    }

    pub fn on_change(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn hsv(&self) -> Hsv {
        self.hsv
    }

    pub fn hex_input(&self) -> &Entity<InputState> {
        &self.hex
    }

    /// The `value` prop changed: keep the current HSV when it already maps
    /// to that hex, so dragging through gray keeps its hue.
    pub fn set_value(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let hex = normalize_hex(value);
        if hsv_to_hex(self.hsv.h, self.hsv.s, self.hsv.v) != hex {
            self.hsv = hex_to_hsv(&hex);
            self.sync_hex_field(window, cx);
            cx.notify();
        }
    }

    fn sync_hex_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preview = hsv_to_hex(self.hsv.h, self.hsv.s, self.hsv.v);
        self.hex.update(cx, |hex, cx| {
            if hex.value() != preview.as_str() {
                hex.set_value(preview, window, cx);
            }
        });
    }

    /// `applyHsv`.
    pub fn apply_hsv(&mut self, next: Hsv, window: &mut Window, cx: &mut Context<Self>) {
        self.hsv = next;
        let hex = hsv_to_hex(next.h, next.s, next.v);
        self.sync_hex_field(window, cx);
        if let Some(on_change) = self.on_change.clone() {
            window.defer(cx, move |window, cx| on_change(&hex, window, cx));
        }
        cx.notify();
    }

    /// `onHexInput`.
    pub fn on_hex_input(&mut self, raw: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hex) = parse_hex_input(raw) else {
            return;
        };
        self.hsv = hex_to_hsv(&hex);
        if let Some(on_change) = self.on_change.clone() {
            window.defer(cx, move |window, cx| on_change(&hex, window, cx));
        }
        cx.notify();
    }

    fn square_at(
        &mut self,
        position: gpui::Point<Pixels>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let x = (f32::from(position.x - bounds.origin.x) / f32::from(bounds.size.width))
            .clamp(0.0, 1.0);
        let y = (f32::from(position.y - bounds.origin.y) / f32::from(bounds.size.height))
            .clamp(0.0, 1.0);
        let next = Hsv {
            s: x as f64 * 100.0,
            v: (1.0 - y as f64) * 100.0,
            ..self.hsv
        };
        self.apply_hsv(next, window, cx);
    }

    fn hue_at(
        &mut self,
        position: gpui::Point<Pixels>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let x = (f32::from(position.x - bounds.origin.x) / f32::from(bounds.size.width))
            .clamp(0.0, 1.0);
        let next = Hsv {
            h: x as f64 * 360.0,
            ..self.hsv
        };
        self.apply_hsv(next, window, cx);
    }

    /// A transparent layer that turns presses and drags inside `bounds` into
    /// picks on `surface`.
    fn drag_layer(&self, surface: Dragging, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        canvas(
            |bounds, window, _| {
                (
                    bounds,
                    window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
                )
            },
            move |_, (bounds, hitbox), window, _| {
                let down = this.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if !phase.bubble()
                        || event.button != MouseButton::Left
                        || !hitbox.is_hovered(window)
                    {
                        return;
                    }
                    down.update(cx, |this, cx| {
                        this.dragging = surface;
                        match surface {
                            Dragging::Square => this.square_at(event.position, bounds, window, cx),
                            _ => this.hue_at(event.position, bounds, window, cx),
                        }
                    })
                    .ok();
                });
                let moving = this.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if !phase.bubble() {
                        return;
                    }
                    moving
                        .update(cx, |this, cx| {
                            if this.dragging != surface {
                                return;
                            }
                            if event.pressed_button != Some(MouseButton::Left) {
                                this.dragging = Dragging::None;
                                return;
                            }
                            match surface {
                                Dragging::Square => {
                                    this.square_at(event.position, bounds, window, cx)
                                }
                                _ => this.hue_at(event.position, bounds, window, cx),
                            }
                        })
                        .ok();
                });
                let up = this.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, _, _, cx| {
                    up.update(cx, |this, _| this.dragging = Dragging::None).ok();
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

/// The hue bar's six two-stop segments.
const HUE_STOPS: [u32; 7] = [
    0xff0000, 0xffff00, 0x00ff00, 0x00ffff, 0x0000ff, 0xff00ff, 0xff0000,
];

impl Render for ColorPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let preview = hsv_to_hex(self.hsv.h, self.hsv.s, self.hsv.v);
        let hue_color = hsv_to_hex(self.hsv.h, 100.0, 100.0);
        let preview_color = parse_hex(&preview).unwrap_or(theme.colors.content);
        let hue_fill = parse_hex(&hue_color).unwrap_or(theme.colors.content);
        let white = gpui::white();
        let black = gpui::black();
        let thumb = |fill: Hsla| {
            div()
                .absolute()
                .size(u(14.))
                .ml(u(-7.))
                .mt(u(-7.))
                .rounded_full()
                .border_2()
                .border_color(white)
                .shadow_md()
                .bg(fill)
        };
        let square = div()
            .relative()
            .h(u(112.))
            .w_full()
            .rounded(u(theme.radius.md))
            .bg(linear_gradient(
                90.,
                linear_color_stop(white, 0.),
                linear_color_stop(hue_fill, 1.),
            ))
            .debug_selector(|| "color-square".into())
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(u(theme.radius.md))
                    .bg(linear_gradient(
                        0.,
                        linear_color_stop(black, 0.),
                        linear_color_stop(gpui::transparent_black(), 1.),
                    )),
            )
            .child(
                thumb(preview_color)
                    .left(relative((self.hsv.s / 100.0) as f32))
                    .top(relative((1.0 - self.hsv.v / 100.0) as f32)),
            )
            .child(self.drag_layer(Dragging::Square, cx));
        let mut hue_bar = div()
            .relative()
            .mt(u(8.))
            .h(u(12.))
            .w_full()
            .flex()
            .debug_selector(|| "color-hue".into());
        for (index, pair) in HUE_STOPS.windows(2).enumerate() {
            let from = monocode_ui::color::hex(pair[0]);
            let to = monocode_ui::color::hex(pair[1]);
            let mut segment = div().flex_1().h_full().bg(linear_gradient(
                90.,
                linear_color_stop(from, 0.),
                linear_color_stop(to, 1.),
            ));
            if index == 0 {
                segment = segment.rounded_l_full();
            }
            if index == HUE_STOPS.len() - 2 {
                segment = segment.rounded_r_full();
            }
            hue_bar = hue_bar.child(segment);
        }
        let hue_bar = hue_bar
            .child(
                thumb(hue_fill)
                    .left(relative((self.hsv.h / 360.0) as f32))
                    .top(relative(0.5)),
            )
            .child(self.drag_layer(Dragging::Hue, cx));
        let field = plain_input(&self.hex, cx);
        let _ = window;
        div()
            .mt(u(8.))
            .p(u(8.))
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.05))
            .child(square)
            .child(hue_bar)
            .child(
                div()
                    .mt(u(8.))
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .child(
                        div()
                            .size(u(28.))
                            .flex_none()
                            .rounded(u(theme.radius.md))
                            .border_1()
                            .border_color(theme.content(0.10))
                            .bg(preview_color),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(u(8.))
                            .py(u(4.))
                            .rounded(u(theme.radius.md))
                            .border_1()
                            .border_color(theme.content(0.10))
                            .bg(theme.content(0.05))
                            .font_family(theme.fonts.mono.clone())
                            .text_px(theme.text.label)
                            .debug_selector(|| "color-hex".into())
                            .child(field),
                    ),
            )
    }
}

/// `ACCENT_COLOR_PRESETS`.
pub const ACCENT_COLOR_PRESETS: [&str; 6] = [
    "#4da3f5", "#8b5cf6", "#ec4899", "#ef4444", "#f59e0b", "#10b981",
];

/// The swatch labels, the default first.
pub const ACCENT_COLOR_LABELS: [&str; 7] = [
    "Default", "Blue", "Violet", "Pink", "Red", "Orange", "Green",
];

/// The preset a value matches: 0 for no accent, 1 to 6 for a preset, and
/// `None` for a custom color.
pub fn accent_preset_index(value: Option<&str>) -> Option<usize> {
    match value {
        None => Some(0),
        Some(value) => ACCENT_COLOR_PRESETS
            .iter()
            .position(|preset| *preset == value)
            .map(|index| index + 1),
    }
}

type AccentHandler = Rc<dyn Fn(Option<String>, &mut Window, &mut App)>;

/// `AccentColorPicker`: the default, six presets, and a custom picker.
pub struct AccentColorPicker {
    value: Option<String>,
    open: bool,
    picker: Option<Entity<ColorPicker>>,
    focus: FocusHandle,
    on_change: AccentHandler,
}

impl AccentColorPicker {
    pub fn new(
        value: Option<String>,
        on_change: impl Fn(Option<String>, &mut Window, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            value,
            open: false,
            picker: None,
            focus: cx.focus_handle(),
            on_change: Rc::new(on_change),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The custom color picker, once opened.
    pub fn picker(&self) -> Option<&Entity<ColorPicker>> {
        self.picker.as_ref()
    }

    pub fn set_value(
        &mut self,
        value: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.value == value {
            return;
        }
        self.value = value.clone();
        if let Some(picker) = &self.picker {
            let hex = value.unwrap_or_else(|| ACCENT_COLOR_PRESETS[0].to_string());
            picker.update(cx, |picker, cx| picker.set_value(&hex, window, cx));
        }
        cx.notify();
    }

    /// `onPickIndex`.
    pub fn pick_index(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        let next = if index == 0 {
            None
        } else {
            Some(
                ACCENT_COLOR_PRESETS
                    .get(index - 1)
                    .copied()
                    .unwrap_or(ACCENT_COLOR_PRESETS[0])
                    .to_string(),
            )
        };
        self.value = next.clone();
        let on_change = self.on_change.clone();
        window.defer(cx, move |window, cx| on_change(next, window, cx));
        cx.notify();
    }

    /// `onToggleCustom`.
    pub fn toggle_custom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = !self.open;
        if self.open {
            let value = self
                .value
                .clone()
                .unwrap_or_else(|| ACCENT_COLOR_PRESETS[0].to_string());
            let on_change = self.on_change.clone();
            let this = cx.entity().downgrade();
            self.picker = Some(cx.new(|cx| {
                ColorPicker::new(&value, window, cx).on_change(move |hex, window, cx| {
                    let hex = hex.to_string();
                    this.update(cx, |this, _| this.value = Some(hex.clone()))
                        .ok();
                    on_change(Some(hex), window, cx);
                })
            }));
            self.focus.focus(window, cx);
        }
        cx.notify();
    }
}

impl Render for AccentColorPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let preset = accent_preset_index(self.value.as_deref());
        let mut colors = vec![Swatch::Color(theme.colors.content)];
        colors.extend(
            ACCENT_COLOR_PRESETS
                .iter()
                .map(|hex| Swatch::Hex((*hex).into())),
        );
        let this = cx.entity().downgrade();
        let toggle = this.clone();
        let row = color_swatch_row(
            colors,
            ACCENT_COLOR_LABELS
                .iter()
                .map(|label| (*label).into())
                .collect(),
        )
        .color_index(preset)
        .custom_color(if preset.is_none() {
            self.value.clone().map(Into::into)
        } else {
            None
        })
        .custom_picker_open(self.open)
        .on_pick_index(move |index, window, cx| {
            this.update(cx, |this, cx| this.pick_index(index, window, cx))
                .ok();
        })
        .on_toggle_custom(move |window, cx| {
            toggle
                .update(cx, |this, cx| this.toggle_custom(window, cx))
                .ok();
        });
        let popover = match (&self.picker, self.open) {
            (Some(picker), true) => {
                let gap = u(POPOVER_GAP).to_pixels(window.rem_size());
                Some(
                    div().absolute().top(relative(1.)).right_0().size_0().child(
                        deferred(
                            anchored()
                                .anchor(Anchor::TopRight)
                                .offset(gpui::point(px(0.), gap))
                                .snap_to_window_with_margin(px(POPOVER_PADDING))
                                .child(
                                    div()
                                        .occlude()
                                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                            this.open = false;
                                            cx.notify();
                                        }))
                                        .child(
                                            popover_frame("accent-color-popover")
                                                .width(248.)
                                                .child(
                                                    div().px(u(8.)).pb(u(8.)).child(picker.clone()),
                                                ),
                                        ),
                                ),
                        )
                        .with_priority(theme.layer.popover),
                    ),
                )
            }
            _ => None,
        };
        div()
            .id("accent-color-picker")
            .relative()
            .w(u(192.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if this.open && event.keystroke.key == "escape" {
                    this.open = false;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(row)
            .children(popover)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_hex_to_hsv_and_back() {
        let hsv = hex_to_hsv("#4da3f5");
        assert!((hsv.h - 209.29).abs() < 0.01, "{hsv:?}");
        assert_eq!(hsv_to_hex(hsv.h, hsv.s, hsv.v), "#4da3f5");
        assert_eq!(
            hex_to_hsv("#000000"),
            Hsv {
                h: 0.0,
                s: 0.0,
                v: 0.0
            }
        );
        assert_eq!(hsv_to_hex(0.0, 100.0, 100.0), "#ff0000");
        assert_eq!(hsv_to_hex(-120.0, 100.0, 100.0), "#0000ff");
        assert_eq!(hsv_to_hex(120.0, 150.0, 50.0), "#008000");
    }

    #[test]
    fn normalizes_hex_and_parses_the_input() {
        assert_eq!(normalize_hex("#ABCDEF"), "#abcdef");
        assert_eq!(normalize_hex("tomato"), "#808080");
        assert_eq!(parse_hex_input(" ABCDEF ").as_deref(), Some("#abcdef"));
        assert_eq!(parse_hex_input("#abc"), None);
        assert_eq!(parse_hex_input("#abcdeg"), None);
    }

    #[test]
    fn finds_the_accent_preset() {
        assert_eq!(accent_preset_index(None), Some(0));
        assert_eq!(accent_preset_index(Some("#8b5cf6")), Some(2));
        assert_eq!(accent_preset_index(Some("#123456")), None);
    }
}
