//! Port of the controls in SettingsView.tsx (`Toggle`, `Segmented`,
//! `Slider`), SecondaryButton.tsx, and the bordered text fields the settings
//! rows use.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, ElementId, Entity, Focusable as _, Global,
    HitboxBehavior, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, canvas, div,
    prelude::FluentBuilder as _, relative,
};
use gpui_base::input::InputEditorStyle;
use gpui_component::input::InputState;
use monocode_settings::{Kv, Subscription};
use monocode_ui::widgets::switch;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::host::SettingsHosts;

/// The hosts, readable from any section the way React read context.
pub struct HostsGlobal(pub SettingsHosts);

impl Global for HostsGlobal {}

/// The installed hosts, or the defaults when no page set any.
pub fn hosts(cx: &App) -> SettingsHosts {
    cx.try_global::<HostsGlobal>()
        .map(|hosts| hosts.0.clone())
        .unwrap_or_default()
}

type BoolHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// `Toggle`: the settings switch. Flipping it plays the switch cue.
#[derive(IntoElement)]
pub struct Toggle {
    label: SharedString,
    on: bool,
    disabled: bool,
    on_change: Option<BoolHandler>,
}

/// A switch named `label` (its `aria-label`, and its test selector
/// `switch:<label>`).
pub fn toggle(label: impl Into<SharedString>, on: bool) -> Toggle {
    Toggle {
        label: label.into(),
        on,
        disabled: false,
        on_change: None,
    }
}

impl Toggle {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let label = self.label.clone();
        let mut control =
            switch(ElementId::from(self.label.clone()), self.on).disabled(self.disabled);
        if let Some(handler) = self.on_change {
            control = control.on_change(move |next, window, cx| {
                handler(&next, window, cx);
                hosts(cx).general.play_switch_cue(cx);
            });
        }
        div()
            .flex_none()
            .debug_selector(move || format!("switch:{label}"))
            .child(control)
    }
}

type PickHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// `Segmented`: equal options in a bordered strip.
#[derive(IntoElement)]
pub struct Segmented {
    label: SharedString,
    value: SharedString,
    options: Vec<(SharedString, SharedString)>,
    option_id_prefix: Option<SharedString>,
    compact: bool,
    width: Option<f32>,
    on_change: Option<PickHandler>,
}

/// A radio group named `label`. Each option's selector is
/// `radio:<label>:<value>`, or `<prefix>-<value>` with an id prefix.
pub fn segmented(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    options: impl IntoIterator<Item = (impl Into<SharedString>, impl Into<SharedString>)>,
) -> Segmented {
    Segmented {
        label: label.into(),
        value: value.into(),
        options: options
            .into_iter()
            .map(|(value, label)| (value.into(), label.into()))
            .collect(),
        option_id_prefix: None,
        compact: false,
        width: None,
        on_change: None,
    }
}

impl Segmented {
    pub fn option_id_prefix(mut self, prefix: impl Into<SharedString>) -> Self {
        self.option_id_prefix = Some(prefix.into());
        self
    }

    /// `px-1.5` options in a fixed width, as the project dialog draws them.
    pub fn compact(mut self, width: f32) -> Self {
        self.compact = true;
        self.width = Some(width);
        self
    }

    pub fn on_change(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let group_label = self.label.clone();
        let columns = self.options.len().max(1) as u16;
        let mut strip = div()
            .id(ElementId::from(self.label.clone()))
            .grid()
            .grid_cols(columns)
            .flex_none()
            .max_w_full()
            .gap(u(2.))
            .p(u(2.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .text_px(theme.text.label)
            .debug_selector(move || format!("radiogroup:{group_label}"));
        if let Some(width) = self.width {
            strip = strip.w(u(width));
        }
        for (value, label) in self.options {
            let picked = value == self.value;
            let selector = match &self.option_id_prefix {
                Some(prefix) => format!("{prefix}-{}", value.to_lowercase()),
                None => format!("radio:{}:{value}", self.label),
            };
            let mut option = div()
                .id(ElementId::from(value.clone()))
                .min_w_0()
                .flex()
                .justify_center()
                .px(u(if self.compact { 6. } else { 10. }))
                .py(u(4.))
                .rounded(u(5.))
                .whitespace_nowrap()
                .debug_selector(move || selector)
                .child(label);
            if picked {
                option = option.bg(c.selection).text_color(c.content);
            } else {
                let ink = c.content;
                option = option
                    .text_color(theme.content(0.50))
                    .hover(move |s| s.text_color(ink));
            }
            if let Some(handler) = self.on_change.clone() {
                option = option.on_click(move |_, window, cx| handler(&value, window, cx));
            }
            strip = strip.child(option);
        }
        strip
    }
}

type ValueHandler = Rc<dyn Fn(&f64, &mut Window, &mut App)>;

/// Whether a slider is being dragged, kept per slider across frames.
#[derive(Default)]
pub struct SliderDrag(bool);

/// `Slider`: a `.sidebar-opacity-slider` range input and its value.
#[derive(IntoElement)]
pub struct Slider {
    label: SharedString,
    value: f64,
    display: SharedString,
    min: f64,
    max: f64,
    disabled: bool,
    on_change: Option<ValueHandler>,
}

/// A range named `label` (selector `slider:<label>`), stepping by 1.
pub fn slider(
    label: impl Into<SharedString>,
    value: f64,
    display: impl Into<SharedString>,
    min: f64,
    max: f64,
) -> Slider {
    Slider {
        label: label.into(),
        value,
        display: display.into(),
        min,
        max,
        disabled: false,
        on_change: None,
    }
}

impl Slider {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, f: impl Fn(&f64, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

/// The value under `x` on a track: the thumb center runs from half a thumb
/// in from the left edge to half a thumb in from the right.
pub fn slider_value_at(x: f32, bounds: Bounds<Pixels>, thumb: f32, min: f64, max: f64) -> f64 {
    let left = f32::from(bounds.origin.x) + thumb / 2.0;
    let span = (f32::from(bounds.size.width) - thumb).max(1.0);
    let fraction = ((x - left) / span).clamp(0.0, 1.0) as f64;
    (min + fraction * (max - min)).round().clamp(min, max)
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let fraction = if self.max > self.min {
            ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let drag = window.use_keyed_state(
            ElementId::from(SharedString::from(format!("slider-drag-{}", self.label))),
            cx,
            |_, _| SliderDrag::default(),
        );
        let thumb = 12.0;
        let thumb_px = u(thumb).to_pixels(window.rem_size());
        let (min, max) = (self.min, self.max);
        let label = self.label.clone();
        let interactive = (!self.disabled).then_some(self.on_change).flatten();
        let track = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h(u(thumb))
            .debug_selector(move || format!("slider:{label}"))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(u((thumb - 4.) / 2.))
                    .h(u(4.))
                    .rounded_full()
                    .bg(theme.content(0.15)),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(u(thumb / 2.))
                    .right(u(thumb / 2.))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left(relative(fraction))
                            .ml(u(-thumb / 2.))
                            .size(u(thumb))
                            .rounded_full()
                            .bg(theme.colors.content),
                    ),
            )
            .when_some(interactive, |el, on_change| {
                el.child(
                    canvas(
                        |bounds, window, _| {
                            (bounds, window.insert_hitbox(bounds, HitboxBehavior::Normal))
                        },
                        move |_, (bounds, hitbox), window, _| {
                            let thumb = f32::from(thumb_px);
                            let (down_drag, down_change) = (drag.clone(), on_change.clone());
                            window.on_mouse_event(
                                move |event: &MouseDownEvent, phase, window, cx| {
                                    if !phase.bubble()
                                        || event.button != MouseButton::Left
                                        || !hitbox.is_hovered(window)
                                    {
                                        return;
                                    }
                                    down_drag.update(cx, |drag, _| drag.0 = true);
                                    let value = slider_value_at(
                                        f32::from(event.position.x),
                                        bounds,
                                        thumb,
                                        min,
                                        max,
                                    );
                                    down_change(&value, window, cx);
                                },
                            );
                            let (move_drag, move_change) = (drag.clone(), on_change.clone());
                            window.on_mouse_event(
                                move |event: &MouseMoveEvent, phase, window, cx| {
                                    if !phase.bubble() || !move_drag.read(cx).0 {
                                        return;
                                    }
                                    if event.pressed_button != Some(MouseButton::Left) {
                                        move_drag.update(cx, |drag, _| drag.0 = false);
                                        return;
                                    }
                                    let value = slider_value_at(
                                        f32::from(event.position.x),
                                        bounds,
                                        thumb,
                                        min,
                                        max,
                                    );
                                    move_change(&value, window, cx);
                                },
                            );
                            let up_drag = drag.clone();
                            window.on_mouse_event(move |_: &MouseUpEvent, _, _, cx| {
                                if up_drag.read(cx).0 {
                                    up_drag.update(cx, |drag, _| drag.0 = false);
                                }
                            });
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
            });
        div()
            .flex()
            .flex_none()
            .w(u(224.))
            .max_w_full()
            .items_center()
            .gap(u(12.))
            .when(self.disabled, |el| el.opacity(0.4))
            .child(track)
            .child(
                div()
                    .w(u(40.))
                    .flex_none()
                    .text_right()
                    .text_px(theme.text.label)
                    .tabular()
                    .text_color(theme.colors.content)
                    .child(self.display),
            )
    }
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// What leads a secondary button's label.
pub enum Leading {
    Icon(IconName),
    /// A spinning loader (`<Loader className="animate-spin">`).
    Spinner,
    /// An icon in the accent color, as `ArrowDownCircle` is drawn.
    AccentIcon(IconName),
}

/// `SecondaryButton`.
#[derive(IntoElement)]
pub struct SecondaryButton {
    id: SharedString,
    label: SharedString,
    leading: Option<Leading>,
    danger: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

/// A bordered button. `id` is its element id and test selector
/// (`button:<id>`).
pub fn secondary_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
) -> SecondaryButton {
    SecondaryButton {
        id: id.into(),
        label: label.into(),
        leading: None,
        danger: false,
        disabled: false,
        on_click: None,
    }
}

impl SecondaryButton {
    pub fn leading(mut self, leading: Leading) -> Self {
        self.leading = Some(leading);
        self
    }

    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

/// The spinning loader icon.
pub fn spinner_icon(id: impl Into<ElementId>, size: f32, color: gpui::Hsla) -> AnyElement {
    use gpui::{Animation, AnimationExt as _, Transformation, percentage};
    icon(IconName::Loader)
        .size(u(size))
        .text_color(color)
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_secs(1)).repeat(),
            |svg, t| svg.with_transformation(Transformation::rotate(percentage(t))),
        )
        .into_any_element()
}

impl RenderOnce for SecondaryButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let ink = if self.danger {
            c.danger
        } else {
            theme.content(0.70)
        };
        let id = self.id.clone();
        let mut el = div()
            .id(ElementId::from(self.id.clone()))
            .flex()
            .flex_none()
            .items_center()
            .gap(u(6.))
            .px(u(10.))
            .py(u(4.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .text_px(theme.text.label)
            .leading(theme.leading.label)
            .whitespace_nowrap()
            .text_color(ink)
            .debug_selector(move || format!("button:{id}"));
        if self.disabled {
            el = el.opacity(0.4);
        } else if self.danger {
            let fill = gpui::Hsla { a: 0.1, ..c.danger };
            let border = gpui::Hsla { a: 0.4, ..c.danger };
            el = el.hover(move |s| s.bg(fill).border_color(border));
        } else {
            let fill = theme.content(0.10);
            let hover_ink = c.content;
            el = el.hover(move |s| s.bg(fill).text_color(hover_ink));
        }
        match self.leading {
            Some(Leading::Icon(name)) => {
                el = el.child(icon(name).size(u(14.)).text_color(ink));
            }
            Some(Leading::AccentIcon(name)) => {
                el = el.child(icon(name).size(u(14.)).text_color(c.accent));
            }
            Some(Leading::Spinner) => {
                el = el.child(spinner_icon(
                    ElementId::from(SharedString::from(format!("{}-spinner", self.id))),
                    14.,
                    ink,
                ));
            }
            None => {}
        }
        el = el.child(self.label);
        if let (Some(handler), false) = (self.on_click, self.disabled) {
            el = el.on_click(move |event, window, cx| handler(event, window, cx));
        }
        el
    }
}

/// The input with settings colors: content text, `content/35` placeholder.
pub fn plain_input(state: &Entity<InputState>, cx: &mut App) -> AnyElement {
    let theme = Theme::of(cx).clone();
    let style = InputEditorStyle {
        foreground: theme.colors.content,
        muted_foreground: theme.content(0.35),
        background: gpui::transparent_black(),
        border: gpui::transparent_black(),
        selection: theme.accent(0.35),
        caret: theme.colors.content,
        ..Default::default()
    };
    state.update(cx, |state, _| state.set_editor_style(style));
    state.clone().into_any_element()
}

/// The bordered field settings rows use: `h-7 rounded-md border
/// border-content/10 px-2 text-[12px]`, `w-52` unless `width` says otherwise.
pub fn text_field(
    selector: &'static str,
    state: &Entity<InputState>,
    width: Option<f32>,
    window: &Window,
    cx: &mut App,
) -> gpui::Div {
    let theme = Theme::of(cx).clone();
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    let border = theme.content(if focused { 0.20 } else { 0.10 });
    let input = plain_input(state, cx);
    let mut field = div()
        .flex()
        .flex_none()
        .items_center()
        .h(u(28.))
        .max_w_full()
        .px(u(8.))
        .rounded(u(theme.radius.md))
        .border_1()
        .border_color(border)
        .text_px(theme.text.label)
        .debug_selector(move || selector.to_string())
        .child(div().flex_1().min_w_0().child(input));
    field = match width {
        Some(width) => field.w(u(width)),
        None => field.w(u(208.)),
    };
    field
}

/// A search field with a leading magnifier, as the header and the
/// keybindings filter draw it.
pub fn search_field(
    selector: &'static str,
    state: &Entity<InputState>,
    width: f32,
    trailing: Option<AnyElement>,
    window: &Window,
    cx: &mut App,
) -> gpui::Div {
    let theme = Theme::of(cx).clone();
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    let border = theme.content(if focused { 0.20 } else { 0.10 });
    let input = plain_input(state, cx);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(u(8.))
        .h(u(28.))
        .w(u(width))
        .px(u(8.))
        .rounded(u(theme.radius.md))
        .border_1()
        .border_color(border)
        .text_px(theme.text.label)
        .text_color(theme.content(0.45))
        .debug_selector(move || selector.to_string())
        .child(
            icon(IconName::Search)
                .size(u(14.))
                .text_color(theme.content(0.45)),
        )
        .child(div().flex_1().min_w_0().child(input))
        .children(trailing)
}

/// Follows changes to `keys` in `kv`: `on_change` runs on the entity after
/// each one, the way the TypeScript listened for its change events.
pub fn watch_keys<T: 'static>(
    kv: &Kv,
    keys: &[&str],
    on_change: fn(&mut T, &mut Context<T>),
    cx: &mut Context<T>,
) -> (Vec<Subscription>, Task<()>) {
    let (tx, rx) = async_channel::unbounded::<()>();
    let subscriptions = keys
        .iter()
        .map(|key| {
            let tx = tx.clone();
            kv.subscribe_key(key, move |_| {
                let _ = tx.try_send(());
            })
        })
        .collect();
    let task = cx.spawn(async move |this, cx| {
        while rx.recv().await.is_ok() {
            // Coalesce a burst of writes into one reload.
            while rx.try_recv().is_ok() {}
            if this.update(cx, on_change).is_err() {
                break;
            }
        }
    });
    (subscriptions, task)
}

/// A `px` value as CSS px at the current rem size.
pub fn css_px(value: Pixels, window: &Window) -> f32 {
    f32::from(value) / f32::from(window.rem_size()) * 16.0
}

/// A trigger's bounds from its last prepaint.
#[derive(Clone, Default)]
pub struct TriggerBounds(std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>);

impl TriggerBounds {
    pub fn get(&self) -> Option<Bounds<Pixels>> {
        self.0.get()
    }

    /// An absolute, full-size probe to put inside the trigger's wrapper.
    pub fn probe(&self) -> impl IntoElement + use<> {
        let cell = self.0.clone();
        canvas(move |bounds, _, _| cell.set(Some(bounds)), |_, _, _, _| {})
            .absolute()
            .top_0()
            .left_0()
            .size_full()
    }
}

/// Popover.tsx's flip: open above the trigger when `needed` CSS px do not
/// fit below it and there is more room above.
pub fn opens_above(trigger: Option<Bounds<Pixels>>, needed: f32, window: &Window) -> bool {
    use monocode_ui::widgets::{POPOVER_GAP, POPOVER_PADDING};
    let Some(trigger) = trigger else {
        return false;
    };
    let viewport = css_px(window.viewport_size().height, window);
    let below = viewport - css_px(trigger.bottom(), window) - POPOVER_GAP - POPOVER_PADDING;
    let above = css_px(trigger.top(), window) - POPOVER_GAP - POPOVER_PADDING;
    below < needed && above > below
}

/// A popover placed against its trigger's edge, in the popover layer. The
/// trigger's wrapper must be `relative()`. `align_end` lines up the right
/// edges; otherwise the left edges.
pub fn anchored_popover(
    above: bool,
    align_end: bool,
    layer: usize,
    window: &Window,
    content: impl IntoElement,
) -> AnyElement {
    use gpui::{Anchor, anchored, deferred, point, px};
    use monocode_ui::widgets::{POPOVER_GAP, POPOVER_PADDING};
    let gap = u(POPOVER_GAP).to_pixels(window.rem_size());
    let (corner, offset) = match (above, align_end) {
        (false, true) => (Anchor::TopRight, point(px(0.), gap)),
        (false, false) => (Anchor::TopLeft, point(px(0.), gap)),
        (true, true) => (Anchor::BottomRight, point(px(0.), -gap)),
        (true, false) => (Anchor::BottomLeft, point(px(0.), -gap)),
    };
    let mut slot = div().absolute().size_0();
    slot = if above {
        slot.top_0()
    } else {
        slot.top(relative(1.))
    };
    slot = if align_end {
        slot.right_0()
    } else {
        slot.left_0()
    };
    slot.child(
        deferred(
            anchored()
                .anchor(corner)
                .offset(offset)
                .snap_to_window_with_margin(px(POPOVER_PADDING))
                .child(content),
        )
        .with_priority(layer),
    )
    .into_any_element()
}
