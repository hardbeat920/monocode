//! A discrete, catalog-backed effort control shared by model selectors.
use crate::{conversation::effort_name, pane_motion::PaneSlide, popover::AnchorSlot, theme::*};
use gpui::{
    App, Context, DispatchPhase, EventEmitter, FocusHandle, Focusable, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Role, SharedString, Window, canvas, div, prelude::*, px,
    relative, rgb,
};
use std::time::Instant;

gpui::actions!(model_effort, [Increase, Decrease, First, Last, Activate]);
struct SliderKeys;
impl gpui::Global for SliderKeys {}
fn install_keys(cx: &mut App) {
    if cx.has_global::<SliderKeys>() {
        return;
    }
    cx.set_global(SliderKeys);
    cx.bind_keys([
        gpui::KeyBinding::new("right", Increase, Some("EffortSlider")),
        gpui::KeyBinding::new("up", Increase, Some("EffortSlider")),
        gpui::KeyBinding::new("left", Decrease, Some("EffortSlider")),
        gpui::KeyBinding::new("down", Decrease, Some("EffortSlider")),
        gpui::KeyBinding::new("home", First, Some("EffortSlider")),
        gpui::KeyBinding::new("end", Last, Some("EffortSlider")),
        gpui::KeyBinding::new("enter", Activate, Some("ModelControlButton")),
        gpui::KeyBinding::new("space", Activate, Some("ModelControlButton")),
    ]);
}

pub(crate) struct EffortChanged(pub String);
pub(crate) enum ModelControlAction {
    ChooseModel,
    ToggleFast,
    ResetEffort,
}
const THUMB: f32 = 28.;
const INSET: f32 = THUMB / 2.;
const CENTER_Y: f32 = 20.;

/// Persistent presentation state. Targets change only on input, never on render.
struct SliderMotion {
    position: PaneSlide,
    labels: Vec<f32>,
    target: usize,
    started: Option<Instant>,
    rising: bool,
}
struct SliderFrame {
    position: f32,
    labels: Vec<f32>,
    emphasis: f32,
    moving: bool,
}
impl SliderMotion {
    fn new(index: usize, count: usize) -> Self {
        let position = index as f32 / count.saturating_sub(1).max(1) as f32;
        Self {
            position: PaneSlide::stationary(position, 240.),
            labels: (0..count)
                .map(|ix| if ix == index { 1. } else { 0. })
                .collect(),
            target: index,
            started: None,
            rising: false,
        }
    }
    fn retarget(&mut self, index: usize, now: Instant) {
        let frame = self.frame(now, false);
        let target = index as f32 / self.labels.len().saturating_sub(1).max(1) as f32;
        self.rising = target > frame.position;
        self.labels = frame.labels;
        self.position = self.position.retarget(target, now);
        self.position.at(now);
        self.target = index;
        self.started = Some(now);
    }
    fn frame(&mut self, now: Instant, reduced: bool) -> SliderFrame {
        if reduced {
            self.position = PaneSlide::stationary(self.position.to, 240.);
            self.started = None;
        }
        let (position, settled) = self.position.at(now);
        let progress = self.started.map_or(1., |start| {
            (now.saturating_duration_since(start).as_secs_f32() / 0.18).clamp(0., 1.)
        });
        let blend = 1. - (1. - progress).powi(3);
        let labels = self
            .labels
            .iter()
            .enumerate()
            .map(|(ix, from)| {
                let target = if ix == self.target { 1. } else { 0. };
                from * (1. - blend) + target * blend
            })
            .collect();
        let emphasis = if self.rising && !reduced {
            (std::f32::consts::PI * progress).sin().max(0.)
        } else {
            0.
        };
        if progress >= 1. {
            self.started = None;
        }
        SliderFrame {
            position,
            labels,
            emphasis,
            moving: !settled || progress < 1.,
        }
    }
}

pub(crate) struct EffortSlider {
    levels: Vec<String>,
    active: usize,
    committed: bool,
    focus: FocusHandle,
    track: AnchorSlot,
    dragging: bool,
    motion: SliderMotion,
    model_label: Option<SharedString>,
    fast: Option<bool>,
    actions: [FocusHandle; 3],
}
impl EventEmitter<EffortChanged> for EffortSlider {}
impl EventEmitter<ModelControlAction> for EffortSlider {}
impl Focusable for EffortSlider {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        if self.levels.is_empty() {
            self.actions[0].clone()
        } else {
            self.focus.clone()
        }
    }
}
impl EffortSlider {
    /// The catalog's order and identifiers are authoritative; no synthetic stops.
    pub(crate) fn new(
        levels: Vec<String>,
        current: Option<&str>,
        default: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Self {
        install_keys(cx);
        let committed = current.is_some_and(|value| levels.iter().any(|v| v == value));
        let active = current
            .and_then(|value| levels.iter().position(|v| v == value))
            .or_else(|| default.and_then(|value| levels.iter().position(|v| v == value)))
            .unwrap_or(0);
        let motion = SliderMotion::new(active, levels.len());
        Self {
            levels,
            active,
            committed,
            focus: cx.focus_handle(),
            track: Default::default(),
            dragging: false,
            motion,
            model_label: None,
            fast: None,
            actions: [cx.focus_handle(), cx.focus_handle(), cx.focus_handle()],
        }
    }
    pub(crate) fn header(mut self, model: SharedString, fast: Option<bool>) -> Self {
        self.model_label = Some(model);
        self.fast = fast;
        self
    }
    pub(crate) fn set_fast(&mut self, fast: Option<bool>, cx: &mut Context<Self>) {
        self.fast = fast;
        cx.notify();
    }
    pub(crate) fn set_effort(&mut self, effort: &str, cx: &mut Context<Self>) {
        if let Some(index) = self.levels.iter().position(|value| value == effort) {
            self.select(index, cx);
        }
    }
    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.levels.is_empty() {
            return;
        }
        let index = index.min(self.levels.len() - 1);
        if self.motion.labels.len() != self.levels.len() {
            self.motion =
                SliderMotion::new(self.active.min(self.levels.len() - 1), self.levels.len());
        }
        if self.active != index || !self.committed {
            if self.active != index {
                if cx.reduce_motion() {
                    self.motion = SliderMotion::new(index, self.levels.len());
                } else {
                    self.motion.retarget(index, Instant::now());
                }
            }
            self.committed = true;
            self.active = index;
            cx.emit(EffortChanged(self.levels[index].clone()));
            cx.notify();
        }
    }
    fn point(&mut self, x: gpui::Pixels, cx: &mut Context<Self>) {
        if let Some(bounds) = self.track.get() {
            let width = (bounds.size.width - px(INSET * 2.)).max(px(1.));
            let fraction = ((x - bounds.left() - px(INSET)) / width).clamp(0., 1.);
            self.select(
                (fraction * self.levels.len().saturating_sub(1) as f32).round() as usize,
                cx,
            );
        }
    }
}
impl Render for EffortSlider {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Window listeners belong to paint, and keep a drag captured even
        // when the pointer leaves the track or the selector's bounds.
        let movement = cx.weak_entity();
        let release = movement.clone();
        let gestures = canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = movement.update(cx, |this, cx| {
                            if this.dragging {
                                if event.pressed_button == Some(MouseButton::Left) {
                                    this.point(event.position.x, cx);
                                } else {
                                    this.dragging = false;
                                }
                            }
                        });
                    }
                });
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = release.update(cx, |this, _| this.dragging = false);
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let frame = self.motion.frame(Instant::now(), cx.reduce_motion());
        if frame.moving {
            window.request_animation_frame();
        }
        let fraction = frame.position.clamp(0., 1.);
        let value = self
            .levels
            .get(self.active)
            .map(|v| effort_name(v))
            .unwrap_or_default();
        let focused = self.focus.is_focused(window);
        let color: gpui::Hsla = if self
            .levels
            .get(self.active)
            .is_some_and(|value| value == "ultra")
        {
            rgb(0xa78bfa).into()
        } else {
            rgb(0x509bfa).into()
        };
        let header_height = if self.model_label.is_some() && !self.levels.is_empty() {
            48.
        } else {
            32.
        };
        div()
            .id("effort-control")
            .debug_selector(|| "effort-control".into())
            .flex_shrink_0()
            .w_full()
            .flex()
            .flex_col()
            .px_2()
            .pt_1()
            .pb_2()
            .relative()
            .child(gestures)
            .child(
                div()
                    .debug_selector(|| "model-controls-header".into())
                    .relative()
                    .h(px(header_height))
                    .flex_shrink_0()
                    .w_full()
                    .child(
                        div()
                            .absolute()
                            .left(px(32.))
                            .right(px(32.))
                            .h_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .when(!self.levels.is_empty(), |d| {
                                d.child(
                                    div()
                                        .debug_selector(|| "effort-slider-value".into())
                                        .relative()
                                        .h(px(20.))
                                        .w_full()
                                        .overflow_hidden()
                                        .children(self.levels.iter().enumerate().filter_map(
                                            |(ix, level)| {
                                                let weight =
                                                    frame.labels.get(ix).copied().unwrap_or(0.);
                                                (weight > 0.001).then(|| {
                                                    div()
                                                        .absolute()
                                                        .left_0()
                                                        .right_0()
                                                        .top(px((1. - weight) * 4.))
                                                        .flex()
                                                        .justify_center()
                                                        .text_size(px(13.))
                                                        .line_height(px(20.))
                                                        .font_weight(gpui::FontWeight::MEDIUM)
                                                        .text_color(content(0.95))
                                                        .opacity(weight)
                                                        .child(effort_name(level))
                                                })
                                            },
                                        )),
                                )
                            })
                            .when_some(self.model_label.clone(), |d, model| {
                                d.child(
                                    div()
                                        .id("model-controls-choose-model")
                                        .debug_selector(|| "model-controls-choose-model".into())
                                        .role(Role::Button)
                                        .aria_label(format!("Choose model, {model}"))
                                        .track_focus(&self.actions[0])
                                        .tab_index(0)
                                        .cursor_pointer()
                                        .h(px(20.))
                                        .max_w_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap_1()
                                        .rounded(px(4.))
                                        .text_size(px(12.))
                                        .line_height(px(18.))
                                        .text_color(content(0.6))
                                        .hover(|d| d.text_color(content(1.)).bg(content(0.05)))
                                        .when(self.actions[0].is_focused(window), |d| {
                                            d.bg(content(0.08))
                                        })
                                        .child(div().min_w_0().truncate().child(model))
                                        .child(icon("chevron-down", 12.))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, window, cx| {
                                                window.focus(&this.actions[0], cx);
                                                cx.stop_propagation();
                                            }),
                                        )
                                        .on_click(cx.listener(|_, _, _, cx| {
                                            cx.emit(ModelControlAction::ChooseModel)
                                        }))
                                        .key_context("ModelControlButton")
                                        .on_action(cx.listener(|_, _: &Activate, _, cx| {
                                            cx.emit(ModelControlAction::ChooseModel);
                                            cx.stop_propagation();
                                        })),
                                )
                            }),
                    )
                    .when_some(self.fast, |d, enabled| {
                        d.child(
                            div()
                                .id("model-controls-fast")
                                .debug_selector(|| "model-controls-fast".into())
                                .role(Role::Button)
                                .aria_label("Fast mode")
                                .aria_value(if enabled { "On" } else { "Off" })
                                .aria_toggled(if enabled {
                                    gpui::Toggled::True
                                } else {
                                    gpui::Toggled::False
                                })
                                .track_focus(&self.actions[1])
                                .tab_index(0)
                                .absolute()
                                .left_0()
                                .top_0()
                                .size(px(32.))
                                .rounded(px(8.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(if enabled { color } else { content(0.5) })
                                .hover(|d| d.bg(content(0.06)))
                                .when(self.actions[1].is_focused(window), |d| d.bg(content(0.08)))
                                .child(icon("zap", 16.).text_color(if enabled {
                                    color
                                } else {
                                    content(0.5)
                                }))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, window, cx| {
                                        window.focus(&this.actions[1], cx);
                                        cx.stop_propagation();
                                    }),
                                )
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.emit(ModelControlAction::ToggleFast)
                                }))
                                .key_context("ModelControlButton")
                                .on_action(cx.listener(|_, _: &Activate, _, cx| {
                                    cx.emit(ModelControlAction::ToggleFast);
                                    cx.stop_propagation();
                                })),
                        )
                    })
                    .when(!self.levels.is_empty(), |d| {
                        d.child(
                            div()
                                .id("model-controls-reset")
                                .debug_selector(|| "model-controls-reset".into())
                                .role(Role::Button)
                                .aria_label("Reset effort to model default")
                                .track_focus(&self.actions[2])
                                .tab_index(0)
                                .absolute()
                                .right_0()
                                .top_0()
                                .size(px(32.))
                                .rounded(px(8.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(content(0.45))
                                .hover(|d| d.bg(content(0.06)).text_color(content(0.8)))
                                .when(self.actions[2].is_focused(window), |d| d.bg(content(0.08)))
                                .child(icon("rotate-ccw", 14.))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, window, cx| {
                                        window.focus(&this.actions[2], cx);
                                        cx.stop_propagation();
                                    }),
                                )
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.emit(ModelControlAction::ResetEffort)
                                }))
                                .key_context("ModelControlButton")
                                .on_action(cx.listener(|_, _: &Activate, _, cx| {
                                    cx.emit(ModelControlAction::ResetEffort);
                                    cx.stop_propagation();
                                })),
                        )
                    }),
            )
            .when(!self.levels.is_empty(), |d| {
                d.child(
                    div()
                        .id("effort-slider")
                        .debug_selector(|| "effort-slider".into())
                        .role(Role::Slider)
                        .aria_label("Reasoning effort")
                        .aria_value(value)
                        .aria_numeric_value(self.active as f64)
                        .aria_min_numeric_value(0.)
                        .aria_max_numeric_value(self.levels.len().saturating_sub(1) as f64)
                        .aria_numeric_value_step(1.)
                        .track_focus(&self.focus)
                        .tab_index(0)
                        .key_context("EffortSlider")
                        .relative()
                        .h(px(40.))
                        .flex_shrink_0()
                        .w_full()
                        .cursor_pointer()
                        .child(self.track.probe())
                        // One center line and one travel rail for track, fill, ticks and thumb.
                        // The thumb radius is included at both ends, so no part can clip.
                        .child(
                            div()
                                .debug_selector(|| "effort-slider-track".into())
                                .absolute()
                                .left_0()
                                .right_0()
                                .top(px(CENTER_Y - 12.))
                                .h(px(24.))
                                .rounded(px(12.))
                                .bg(content(0.1)),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(INSET))
                                .right(px(INSET))
                                .top(px(CENTER_Y - 12.))
                                .h(px(24.))
                                .child(
                                    div().relative().h_full().w(relative(fraction)).child(
                                        div()
                                            .debug_selector(|| "effort-slider-fill".into())
                                            .absolute()
                                            .left(px(-INSET))
                                            .right_0()
                                            .h_full()
                                            .rounded_l(px(12.))
                                            .bg(color),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(INSET))
                                .right(px(INSET))
                                .top(px(CENTER_Y))
                                .children(self.levels.iter().enumerate().map(|(ix, _)| {
                                    let stop = ix as f32
                                        / self.levels.len().saturating_sub(1).max(1) as f32;
                                    div()
                                        .absolute()
                                        .left(relative(stop))
                                        .ml(px(-2.))
                                        .mt(px(-2.))
                                        .size(px(4.))
                                        .rounded_full()
                                        .bg(if stop <= fraction {
                                            white(0.35)
                                        } else {
                                            content(0.28)
                                        })
                                })),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(INSET))
                                .right(px(INSET))
                                .top(px(CENTER_Y - THUMB / 2.))
                                .child(
                                    div()
                                        .debug_selector(|| "effort-slider-thumb".into())
                                        .absolute()
                                        .left(relative(fraction))
                                        .ml(px(-THUMB / 2.))
                                        .size(px(THUMB))
                                        .rounded_full()
                                        .bg(white(1.))
                                        .border_1()
                                        .border_color(black(0.16))
                                        .shadow(vec![
                                            gpui::BoxShadow {
                                                color: black(0.1),
                                                offset: gpui::point(px(0.), px(0.)),
                                                blur_radius: px(2. + frame.emphasis * 2.),
                                                spread_radius: px(0.),
                                                inset: false,
                                            },
                                            gpui::BoxShadow {
                                                color: if focused { color } else { content(0.) },
                                                offset: gpui::point(px(0.), px(0.)),
                                                blur_radius: px(0.),
                                                spread_radius: px(if focused { 2. } else { 0. }),
                                                inset: false,
                                            },
                                        ]),
                                ),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                window.focus(&this.focus, cx);
                                this.dragging = true;
                                this.point(event.position.x, cx);
                                cx.stop_propagation();
                            }),
                        )
                        .on_action(cx.listener(|this, _: &Increase, _, cx| {
                            this.select(this.active + 1, cx);
                            cx.stop_propagation();
                        }))
                        .on_action(cx.listener(|this, _: &Decrease, _, cx| {
                            this.select(this.active.saturating_sub(1), cx);
                            cx.stop_propagation();
                        }))
                        .on_action(cx.listener(|this, _: &First, _, cx| {
                            this.select(0, cx);
                            cx.stop_propagation();
                        }))
                        .on_action(cx.listener(|this, _: &Last, _, cx| {
                            this.select(this.levels.len().saturating_sub(1), cx);
                            cx.stop_propagation();
                        })),
                )
            })
            .when(!self.levels.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .justify_between()
                        .px(px(INSET))
                        .text_size(px(10.))
                        .line_height(px(16.))
                        .text_color(content(0.4))
                        .child(
                            self.levels
                                .first()
                                .map(|v| effort_name(v))
                                .unwrap_or_default(),
                        )
                        .child(
                            self.levels
                                .last()
                                .filter(|_| self.levels.len() > 1)
                                .map(|v| effort_name(v))
                                .unwrap_or_default(),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn animation_is_continuous_when_retargeted_and_does_not_restart_on_frames() {
        use std::time::Duration;
        let now = Instant::now();
        let mut motion = SliderMotion::new(0, 5);
        assert!(!motion.frame(now, false).moving);
        motion.retarget(4, now);
        let start = motion.frame(now, false);
        assert_eq!(start.position, 0.);
        assert_eq!(start.labels, [1., 0., 0., 0., 0.]);
        let midway = now + Duration::from_millis(70);
        let before = motion.frame(midway, false);
        assert!(before.position > 0. && before.position < 1.);
        assert!(before.labels[0] > 0. && before.labels[4] > 0.);
        assert!((before.labels.iter().sum::<f32>() - 1.).abs() < 0.001);
        assert!(before.emphasis > 0.);
        motion.retarget(1, midway);
        let after = motion.frame(midway, false);
        assert!((before.position - after.position).abs() < 0.001);
        assert_eq!(before.labels, after.labels);
        let end = motion.frame(now + Duration::from_secs(1), false);
        assert_eq!(end.position, 0.25);
        assert_eq!(end.labels, [0., 1., 0., 0., 0.]);
        assert!(!end.moving);
        assert_eq!(end.emphasis, 0.);
        assert!(!motion.frame(now + Duration::from_secs(2), false).moving);
    }

    #[test]
    fn reduced_motion_cancels_an_inflight_transition_without_a_later_replay() {
        use std::time::Duration;
        let now = Instant::now();
        let mut motion = SliderMotion::new(0, 3);
        motion.retarget(2, now);
        let frame = motion.frame(now + Duration::from_millis(40), true);
        assert_eq!(frame.position, 1.);
        assert_eq!(frame.labels, [0., 0., 1.]);
        assert_eq!(frame.emphasis, 0.);
        assert!(!frame.moving);
        assert!(!motion.frame(now + Duration::from_secs(1), false).moving);
    }

    #[gpui::test]
    fn thumb_is_centered_and_contained_at_minimum_middle_and_maximum(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    EffortSlider::new(
                        vec!["low".into(), "high".into(), "xhigh".into()],
                        Some("low"),
                        Some("high"),
                        cx,
                    )
                    .header("Example model".into(), Some(false))
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        for index in 0..3 {
            window
                .update(visual, |slider, _, cx| slider.select(index, cx))
                .unwrap();
            visual.refresh().unwrap();
            let track = visual.debug_bounds("effort-slider-track").unwrap();
            let thumb = visual.debug_bounds("effort-slider-thumb").unwrap();
            let hit = visual.debug_bounds("effort-slider").unwrap();
            assert!((thumb.center().y - track.center().y).abs() < px(0.01));
            assert!(thumb.left() >= hit.left() && thumb.right() <= hit.right());
            assert!(thumb.top() >= hit.top() && thumb.bottom() <= hit.bottom());
            let expected =
                hit.left() + px(INSET) + (hit.size.width - px(INSET * 2.)) * (index as f32 / 2.);
            assert!((thumb.center().x - expected).abs() < px(0.01));
            let value = visual.debug_bounds("effort-slider-value").unwrap();
            let header = visual.debug_bounds("model-controls-header").unwrap();
            assert!((value.center().x - header.center().x).abs() < px(0.01));
        }
    }

    #[gpui::test]
    fn discrete_slider_supports_mouse_drag_keyboard_and_catalog_fallback(
        cx: &mut gpui::TestAppContext,
    ) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    EffortSlider::new(
                        vec!["low".into(), "high".into(), "xhigh".into()],
                        Some("unsupported"),
                        Some("high"),
                        cx,
                    )
                })
            })
            .unwrap()
        });
        let values = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let output = values.clone();
        let _events = window
            .update(cx, |_, _, cx| {
                cx.subscribe(&cx.entity(), move |_, _, event: &EffortChanged, _| {
                    output.borrow_mut().push(event.0.clone())
                })
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        window
            .update(visual, |slider, _, _| assert_eq!(slider.active, 1))
            .unwrap();
        let track = visual.debug_bounds("effort-slider").unwrap();
        window
            .update(visual, |slider, _, _| {
                assert_eq!(
                    slider.track.get(),
                    Some(track),
                    "pointer probe uses the slider's actual bounds"
                )
            })
            .unwrap();
        visual.simulate_click(
            gpui::point(track.right() - px(10.), track.center().y),
            Default::default(),
        );
        visual.simulate_keystrokes("home right up end left");
        window
            .update(visual, |slider, _, _| assert_eq!(slider.active, 1))
            .unwrap();
        visual.update(|window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::MouseDown(MouseDownEvent {
                    position: track.center(),
                    button: MouseButton::Left,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
        });
        window
            .update(visual, |slider, _, _| {
                assert!(slider.dragging, "mouse down starts a captured slider drag")
            })
            .unwrap();
        visual.update(|window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::MouseMove(MouseMoveEvent {
                    position: gpui::point(track.left() - px(100.), track.top() - px(100.)),
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Default::default(),
                }),
                cx,
            );
        });
        visual.run_until_parked();
        window
            .update(visual, |slider, _, _| assert_eq!(slider.active, 0))
            .unwrap();
        assert!(
            values
                .borrow()
                .iter()
                .all(|v| ["low", "high", "xhigh"].contains(&v.as_str()))
        );
        assert!(values.borrow().contains(&"xhigh".to_owned()));
        window
            .update(visual, |slider, _, cx| {
                slider.levels = vec!["high".into()];
                slider.active = 0;
                slider.select(usize::MAX, cx);
                assert_eq!(slider.active, 0);
            })
            .unwrap();
    }
}
