//! Provider-neutral question card: one question per page, sliding between
//! pages, laid over the composer it temporarily replaces.
use crate::{input, theme::*};
use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, Entity, EntityId, EventEmitter, FocusHandle,
    Focusable, Hsla, MouseButton, Role, Subscription, Window, canvas, div, prelude::*, px,
};
use monocode_core::agent::Question;
use std::time::{Duration, Instant};

/// Crossfade between the composer and the card.
const FADE: Duration = Duration::from_millis(200);
/// One page sliding in while the previous one slides out.
const SLIDE: Duration = Duration::from_millis(260);
const SLIDE_DISTANCE: f32 = 28.;
const RADIUS: f32 = 12.;

pub(crate) struct Answered {
    pub id: String,
    pub answers: Option<Vec<Vec<String>>>,
}
pub(crate) struct KeptOpen {
    pub id: String,
}
struct Turn {
    from: usize,
    forward: bool,
    at: Instant,
}
pub(crate) struct QuestionCard {
    request: Question,
    selected: Vec<Vec<String>>,
    fields: Vec<Entity<input::Input>>,
    focus: FocusHandle,
    focused: bool,
    step: usize,
    reached: usize,
    highlighted: Option<usize>,
    turns: u64,
    turn: Option<Turn>,
    _events: Vec<Subscription>,
    _clock: Option<gpui::Task<()>>,
}
impl EventEmitter<Answered> for QuestionCard {}
impl EventEmitter<KeptOpen> for QuestionCard {}
impl Focusable for QuestionCard {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl QuestionCard {
    pub(crate) fn new(request: Question, cx: &mut Context<Self>) -> Self {
        let fields: Vec<_> = request
            .items
            .iter()
            .map(|item| {
                let placeholder = if item.options.is_empty() {
                    "Type your answer"
                } else {
                    "Something else"
                };
                cx.new(|cx| input::Input::field(&item.prompt, placeholder, content(0.35), cx))
            })
            .collect();
        let mut events: Vec<_> = fields
            .iter()
            .enumerate()
            .map(|(ix, field)| {
                cx.subscribe(field, move |this, field, _: &input::Changed, cx| {
                    this.keep_open(cx);
                    // A typed answer replaces a single choice rather than joining it.
                    if !this.request.items[ix].multiple && !field.read(cx).content.trim().is_empty()
                    {
                        this.selected[ix].clear();
                    }
                    cx.notify();
                })
            })
            .collect();
        events.extend(fields.iter().enumerate().map(|(ix, field)| {
            cx.subscribe(field, move |this, _, _: &input::Submitted, cx| {
                if this.step == ix && this.answered(ix, cx) {
                    this.advance(cx);
                }
            })
        }));
        let selected = vec![Vec::new(); request.items.len()];
        let clock = request.auto_resolve_at.map(|_| {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    if !this
                        .update(cx, |this, cx| {
                            cx.notify();
                            this.request.auto_resolve_at.is_some()
                        })
                        .unwrap_or(false)
                    {
                        break;
                    }
                }
            })
        });
        Self {
            request,
            selected,
            fields,
            focus: cx.focus_handle(),
            focused: false,
            step: 0,
            reached: 0,
            highlighted: None,
            turns: 0,
            turn: None,
            _events: events,
            _clock: clock,
        }
    }
    pub(crate) fn id(&self) -> &str {
        &self.request.id
    }
    pub(crate) fn blocks_steering(&self) -> bool {
        self.request.auto_resolve_at.is_none()
    }
    fn keep_open(&mut self, cx: &mut Context<Self>) {
        if self.request.auto_resolve_at.take().is_some() {
            cx.emit(KeptOpen {
                id: self.request.id.clone(),
            });
            cx.notify();
        }
    }
    pub(crate) fn notification_label(&self) -> &str {
        if !self.request.title.is_empty() {
            &self.request.title
        } else {
            self.request
                .items
                .first()
                .map(|item| item.prompt.as_str())
                .unwrap_or("Question")
        }
    }
    fn answers(&self, cx: &App) -> Vec<Vec<String>> {
        self.request
            .items
            .iter()
            .enumerate()
            .map(|(ix, item)| {
                let mut answers = self.selected[ix].clone();
                let text = self.fields[ix].read(cx).content.trim();
                if item.allow_custom && !text.is_empty() {
                    if !item.multiple {
                        answers.clear();
                    }
                    answers.push(text.to_owned());
                }
                answers
            })
            .collect()
    }
    fn answered(&self, ix: usize, cx: &App) -> bool {
        !self.selected[ix].is_empty()
            || (self.request.items[ix].allow_custom
                && !self.fields[ix].read(cx).content.trim().is_empty())
    }
    fn go(&mut self, step: usize, cx: &mut Context<Self>) {
        if step == self.step || step >= self.request.items.len() {
            return;
        }
        self.turn = Some(Turn {
            from: self.step,
            forward: step > self.step,
            at: Instant::now(),
        });
        self.turns += 1;
        self.step = step;
        self.reached = self.reached.max(step);
        self.highlighted = None;
        cx.notify();
    }
    /// Moves to the next question, or replies once the last one is settled.
    fn advance(&mut self, cx: &mut Context<Self>) {
        if self.step + 1 < self.request.items.len() {
            self.go(self.step + 1, cx);
        } else {
            self.finish(cx);
        }
    }
    fn skip(&mut self, cx: &mut Context<Self>) {
        self.keep_open(cx);
        let ix = self.step;
        self.selected[ix].clear();
        self.fields[ix].update(cx, |field, cx| field.set_text(String::new(), cx));
        self.advance(cx);
    }
    fn finish(&mut self, cx: &mut Context<Self>) {
        let answers = self.answers(cx);
        cx.emit(Answered {
            id: self.request.id.clone(),
            answers: answers.iter().any(|a| !a.is_empty()).then_some(answers),
        });
    }
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(Answered {
            id: self.request.id.clone(),
            answers: None,
        });
    }
    fn choose(&mut self, ix: usize, oi: usize, cx: &mut Context<Self>) {
        self.keep_open(cx);
        let item = &self.request.items[ix];
        let label = item.options[oi].label.clone();
        self.highlighted = Some(oi);
        if item.multiple {
            if self.selected[ix].contains(&label) {
                self.selected[ix].retain(|s| s != &label);
            } else {
                self.selected[ix].push(label);
            }
            cx.notify();
            return;
        }
        self.selected[ix] = vec![label];
        self.fields[ix].update(cx, |field, cx| field.set_text(String::new(), cx));
        self.advance(cx);
        cx.notify();
    }
    fn key(&mut self, event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ix = self.step;
        if self.fields[ix].focus_handle(cx).is_focused(window) {
            return;
        }
        let modifiers = &event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }
        let count = self.request.items[ix].options.len();
        let key = event.keystroke.key.as_str();
        match key {
            "down" | "up" if count > 0 => {
                let next = match (self.highlighted, key) {
                    (None, "down") => 0,
                    (None, _) => count - 1,
                    (Some(h), "down") => (h + 1) % count,
                    (Some(h), _) => (h + count - 1) % count,
                };
                self.highlighted = Some(next);
            }
            "enter" | "space" => match self.highlighted {
                Some(h) if h < count => self.choose(ix, h, cx),
                _ if self.answered(ix, cx) => self.advance(cx),
                _ => return,
            },
            "left" if ix > 0 => self.go(ix - 1, cx),
            "right" if ix < self.reached => self.go(ix + 1, cx),
            "tab" if self.request.items[ix].allow_custom => {
                window.focus(&self.fields[ix].focus_handle(cx), cx)
            }
            _ => match key.parse::<usize>() {
                Ok(n) if (1..=count.min(9)).contains(&n) => self.choose(ix, n - 1, cx),
                _ => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Question `ix` as a page; the outgoing page keeps painting without listeners.
    fn page(&self, ix: usize, live: bool, cx: &mut Context<Self>) -> AnyElement {
        let item = &self.request.items[ix];
        let multiple = item.multiple;
        let options = item.options.iter().enumerate().map(|(oi, option)| {
            let label = option.label.clone();
            let selected = self.selected[ix].contains(&label);
            let highlighted = live && self.highlighted == Some(oi);
            div()
                .when(oi > 0, |d| {
                    d.child(div().mx(px(12.)).h(px(1.)).bg(content(0.06)))
                })
                .child(
                    div()
                        .id(("question-option", ix * 1024 + oi))
                        .debug_selector(move || format!("question-option-{ix}-{oi}"))
                        .role(if multiple {
                            Role::CheckBox
                        } else {
                            Role::RadioButton
                        })
                        .aria_label(label.clone())
                        .aria_selected(selected)
                        .group("question-option")
                        .flex()
                        .items_start()
                        .gap(px(12.))
                        .px(px(10.))
                        .py(px(9.))
                        .rounded(px(8.))
                        .when(selected, |d| d.bg(content(0.05)))
                        .when(highlighted && !selected, |d| d.bg(content(0.04)))
                        .hover(|s| s.bg(content(0.05)))
                        .child(badge(
                            div().child(if selected && multiple {
                                icon("check", 12.)
                                    .text_color(inverse_ink())
                                    .into_any_element()
                            } else {
                                div().child((oi + 1).to_string()).into_any_element()
                            }),
                            selected,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .pt(px(2.))
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(content(if selected { 1. } else { 0.85 }))
                                        .child(label.clone()),
                                )
                                .when(!option.description.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .text_size(px(11.5))
                                            .line_height(px(16.))
                                            .text_color(content(0.5))
                                            .child(option.description.clone()),
                                    )
                                }),
                        )
                        .when(selected && !multiple, |d| {
                            d.child(
                                div()
                                    .pt(px(4.))
                                    .child(icon("check", 14.).text_color(content(0.7))),
                            )
                        })
                        .when(live, |d| {
                            d.on_click(cx.listener(move |this, _, _, cx| this.choose(ix, oi, cx)))
                        }),
                )
        });
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .pl(px(16.))
                    .pr(px(if self.request.items.len() > 1 {
                        128.
                    } else {
                        48.
                    }))
                    .pt(px(14.))
                    .pb(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(content(0.95))
                            .child(item.prompt.clone()),
                    )
                    .when(multiple, |d| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(content(0.45))
                                .child("Select all that apply"),
                        )
                    }),
            )
            .when(!item.options.is_empty(), |d| {
                d.child(
                    div()
                        .id(("question-options", ix))
                        .px(px(6.))
                        .max_h(px(280.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .children(options),
                )
            })
            .into_any_element()
    }
    fn header_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let total = self.request.items.len();
        let step = self.step;
        let countdown = self.request.auto_resolve_at.map(|deadline| {
            let remaining = deadline.saturating_sub(monocode_core::time::now_ms());
            if remaining > 60_000 {
                "Optional".to_owned()
            } else {
                format!("Continues in {}s", remaining.div_ceil(1000))
            }
        });
        div()
            .absolute()
            .top(px(10.))
            .right(px(10.))
            .flex()
            .items_center()
            .gap(px(2.))
            .text_size(px(12.))
            .text_color(content(0.45))
            .when_some(countdown, |d, text| {
                d.child(div().mr(px(6.)).text_size(px(11.)).child(text))
            })
            .when(total > 1, |d| {
                d.child(
                    icon_button_enabled(
                        "question-prev",
                        "Previous question",
                        "chevron-left",
                        step > 0,
                    )
                    .debug_selector(|| "question-prev".into())
                    .when(step > 0, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| this.go(this.step - 1, cx)))
                    }),
                )
                .child(div().min_w(px(40.)).flex().justify_center().child(format!(
                    "{} of {}",
                    step + 1,
                    total
                )))
                .child(
                    icon_button_enabled(
                        "question-next",
                        "Next question",
                        "chevron-right",
                        step < self.reached,
                    )
                    .debug_selector(|| "question-next".into())
                    .when(step < self.reached, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| this.go(this.step + 1, cx)))
                    }),
                )
            })
            .child(
                icon_button("question-dismiss", "Dismiss question", "close")
                    .ml(px(4.))
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx))),
            )
    }
    fn footer(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ix = self.step;
        let item = &self.request.items[ix];
        let field = &self.fields[ix];
        let typing = field.focus_handle(cx).is_focused(window);
        let ready = self.answered(ix, cx);
        let last = ix + 1 == self.request.items.len();
        // Single choices advance on click; a typed or multiple answer needs a
        // button. A revisited page offers the way forward too.
        let proceed = ready && (item.multiple || typing || ix < self.reached || !item.allow_custom);
        div()
            .m(px(6.))
            .mt(px(4.))
            .pl(px(10.))
            .pr(px(6.))
            .py(px(6.))
            .rounded(px(9.))
            .bg(content(if typing { 0.06 } else { 0.04 }))
            .flex()
            .items_center()
            .gap(px(12.))
            .when(item.allow_custom, |d| {
                d.child(badge(
                    icon("pencil-line", 12.).text_color(content(0.6)),
                    false,
                ))
                .child(div().flex_1().min_w_0().child(field.clone()))
            })
            .when(!item.allow_custom, |d| {
                d.child(
                    div()
                        .flex_1()
                        .pl(px(2.))
                        .text_size(px(12.))
                        .text_color(content(0.4))
                        .child(if item.options.len() > 1 {
                            format!("Press 1–{} to choose", item.options.len().min(9))
                        } else {
                            "Choose an option".to_owned()
                        }),
                )
            })
            .child(
                footer_button("question-skip", "Skip question", false)
                    .debug_selector(|| "question-skip".into())
                    .child("Skip")
                    .on_click(cx.listener(|this, _, _, cx| this.skip(cx))),
            )
            .when(proceed, |d| {
                d.child(
                    footer_button(
                        "question-continue",
                        if last {
                            "Submit answers"
                        } else {
                            "Next question"
                        },
                        true,
                    )
                    .debug_selector(|| "question-continue".into())
                    .child(if last { "Submit" } else { "Next" })
                    .on_click(cx.listener(|this, _, _, cx| this.advance(cx))),
                )
            })
    }
}

/// The number or icon square leading each row.
fn badge(child: impl IntoElement, filled: bool) -> impl IntoElement {
    div()
        .size(px(22.))
        .flex_shrink_0()
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .bg(content(if filled { 0.9 } else { 0.07 }))
        .text_color(if filled { inverse_ink() } else { content(0.6) })
        .child(child)
}
fn footer_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .flex_shrink_0()
        .h(px(28.))
        .px(px(12.))
        .rounded(px(7.))
        .flex()
        .items_center()
        .text_size(px(12.))
        .when(primary, |d| {
            d.bg(content(0.9))
                .text_color(inverse_ink())
                .hover(|s| s.bg(content(0.75)))
        })
        .when(!primary, |d| {
            d.border_1()
                .border_color(content(0.12))
                .text_color(content(0.75))
                .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
        })
}
/// Ink for text on a `content(0.9)` fill.
fn inverse_ink() -> Hsla {
    let mut ink = shell_fill();
    ink.a = 1.;
    ink
}
fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

impl Render for QuestionCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused {
            self.focused = true;
            let focus = self.focus.clone();
            window.defer(cx, move |window, cx| window.focus(&focus, cx));
        }
        let reduced = cx.reduce_motion();
        let outgoing = self
            .turn
            .as_ref()
            .filter(|turn| !reduced && turn.at.elapsed() < SLIDE)
            .map(|turn| (turn.from, turn.forward));
        if outgoing.is_none() {
            self.turn = None;
        }
        let forward = outgoing.is_none_or(|(_, forward)| forward);
        let shift = if forward {
            SLIDE_DISTANCE
        } else {
            -SLIDE_DISTANCE
        };
        let page = div().relative().child(self.page(self.step, true, cx));
        // The first page arrives with the card's own fade.
        let page = if self.turns == 0 {
            page.into_any_element()
        } else {
            page.with_animation(
                ("question-page", self.turns),
                Animation::new(SLIDE).with_easing(ease_out),
                move |d, t| d.left(px(shift * (1. - t))).opacity(t),
            )
            .into_any_element()
        };
        let leaving = outgoing.map(|(from, _)| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .child(self.page(from, false, cx))
                .with_animation(
                    ("question-page-out", self.turns),
                    Animation::new(SLIDE.mul_f32(0.7)).with_easing(ease_out),
                    move |d, t| d.left(px(-shift * t)).opacity(1. - t),
                )
        });
        let (fill, blur) = (composer_fill(), composer_blur());
        div()
            .id("provider-question")
            .key_context("QuestionCard")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .role(Role::Group)
            .aria_label(self.notification_label().to_owned())
            .relative()
            .w_full()
            .rounded(px(RADIUS))
            .border_1()
            .border_color(composer_border(true))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.18),
                offset: gpui::point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(-6.),
                inset: false,
            }])
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.keep_open(cx)),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let surface = gpui::fill(bounds, fill).corner_radii(px(RADIUS - 1.));
                        #[cfg(any(target_os = "macos", target_os = "windows"))]
                        let surface = if blur > 0. {
                            surface.border_color(gpui::backdrop_blur_marker(
                                blur * window.scale_factor(),
                            ))
                        } else {
                            surface
                        };
                        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                        let _ = blur;
                        window.paint_quad(surface);
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(
                div()
                    .relative()
                    .overflow_hidden()
                    .rounded_t(px(RADIUS))
                    .children(leaving)
                    .child(page),
            )
            .child(self.header_controls(cx))
            .child(self.footer(window, cx))
    }
}

/// Which card (if any) covers the composer, remembering the one leaving so
/// both directions crossfade.
#[derive(Default)]
pub(crate) struct Overlay {
    shown: Option<EntityId>,
    /// Bumped whenever the composer hides or returns, restarting its fade.
    flips: u64,
    leaving: Option<(Entity<QuestionCard>, Instant)>,
}
impl Overlay {
    /// Lays `card` over `composer`, which keeps its height so the transcript
    /// above does not move while the card is open.
    pub(crate) fn wrap(
        &mut self,
        composer: AnyElement,
        card: Option<&Entity<QuestionCard>>,
        composer_focus: &FocusHandle,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let current = card.map(|card| card.entity_id());
        if current != self.shown {
            if current.is_some() != self.shown.is_some() {
                self.flips += 1;
                // Answering hands the keyboard back to the composer, unless
                // focus already moved somewhere else.
                let answered = self
                    .leaving
                    .as_ref()
                    .is_some_and(|(card, _)| card.read(cx).focus.contains_focused(window, cx));
                if current.is_none() && (answered || window.focused(cx).is_none()) {
                    let focus = composer_focus.clone();
                    window.defer(cx, move |window, cx| window.focus(&focus, cx));
                }
            }
            self.shown = current;
        }
        if self
            .leaving
            .as_ref()
            .is_some_and(|(card, at)| Some(card.entity_id()) == current || at.elapsed() > FADE)
        {
            self.leaving = None;
        }
        let covered = card.is_some();
        let composer = div().child(composer);
        let composer = if self.flips == 0 {
            composer.into_any_element()
        } else {
            composer
                .with_animation(
                    ("composer-under-question", self.flips),
                    Animation::new(FADE).with_easing(ease_out),
                    move |d, t| {
                        let opacity = if covered { 1. - t } else { t };
                        if covered && t >= 1. {
                            d.invisible()
                        } else {
                            d.opacity(opacity)
                        }
                    },
                )
                .into_any_element()
        };
        let layer = || div().absolute().left_0().right_0().bottom_0();
        div()
            .relative()
            .w_full()
            .child(composer)
            .children(self.leaving.as_ref().map(|(card, _)| {
                layer()
                    .child(card.clone())
                    .with_animation(
                        ("question-card-out", card.entity_id()),
                        Animation::new(FADE).with_easing(ease_out),
                        |d, t| d.opacity(1. - t).bottom(px(-6. * t)),
                    )
                    .into_any_element()
            }))
            .children(card.map(|card| {
                layer()
                    .child(card.clone())
                    .with_animation(
                        ("question-card-in", card.entity_id()),
                        Animation::new(FADE).with_easing(ease_out),
                        |d, t| d.opacity(t).bottom(px(-8. * (1. - t))),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }
    /// Keeps `card` painting while it fades out after being answered, then
    /// repaints `owner` so the faded card stops covering the composer.
    pub(crate) fn release<T: 'static>(&mut self, card: Entity<QuestionCard>, cx: &mut Context<T>) {
        self.leaving = Some((card, Instant::now()));
        cx.spawn(async move |owner, cx| {
            cx.background_executor()
                .timer(FADE + Duration::from_millis(30))
                .await;
            let _ = owner.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};
    use monocode_core::agent::{QuestionItem, QuestionOption};
    use std::{cell::RefCell, rc::Rc};

    type Replies = Rc<RefCell<Vec<Option<Vec<Vec<String>>>>>>;

    fn option(label: &str) -> QuestionOption {
        QuestionOption {
            id: label.to_lowercase(),
            label: label.into(),
            description: String::new(),
        }
    }
    fn card(cx: &mut TestAppContext) -> (gpui::WindowHandle<QuestionCard>, Replies, Subscription) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let request = Question {
            id: "r".into(),
            title: "A question".into(),
            auto_resolve_at: None,
            items: vec![
                QuestionItem {
                    id: "one".into(),
                    prompt: "Choose one".into(),
                    multiple: false,
                    allow_custom: true,
                    options: vec![option("First"), option("Second")],
                },
                QuestionItem {
                    id: "many".into(),
                    prompt: "Choose several".into(),
                    multiple: true,
                    allow_custom: true,
                    options: vec![option("A"), option("B")],
                },
            ],
        };
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| QuestionCard::new(request, cx))
            })
            .unwrap()
        });
        let replies = Rc::new(RefCell::new(Vec::new()));
        let output = replies.clone();
        let events = window
            .update(cx, |_, _, cx| {
                cx.subscribe(&cx.entity(), move |_, _, reply: &Answered, _| {
                    output.borrow_mut().push(reply.answers.clone())
                })
            })
            .unwrap();
        (window, replies, events)
    }
    fn click(visual: &mut VisualTestContext, selector: &'static str) {
        visual.refresh().unwrap();
        let bounds = visual.debug_bounds(selector).unwrap();
        visual.simulate_click(bounds.center(), Default::default());
        visual.refresh().unwrap();
    }
    #[gpui::test]
    fn single_choices_advance_and_multiple_choices_submit_together(cx: &mut TestAppContext) {
        let (window, replies, _events) = card(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        click(visual, "question-option-0-1");
        window
            .update(visual, |card, _, _| assert_eq!(card.step, 1))
            .unwrap();
        assert!(visual.debug_bounds("question-continue").is_none());
        click(visual, "question-option-1-0");
        click(visual, "question-option-1-1");
        click(visual, "question-prev");
        window
            .update(visual, |card, _, cx| {
                assert_eq!(card.step, 0);
                assert_eq!(card.answers(cx), vec![vec!["Second"], vec!["A", "B"]]);
            })
            .unwrap();
        click(visual, "question-next");
        assert!(replies.borrow().is_empty());
        click(visual, "question-continue");
        assert_eq!(
            *replies.borrow(),
            vec![Some(vec![
                vec!["Second".to_owned()],
                vec!["A".to_owned(), "B".to_owned()]
            ])]
        );
    }
    #[gpui::test]
    fn typed_answers_replace_single_choices_and_skipping_everything_declines(
        cx: &mut TestAppContext,
    ) {
        let (window, replies, _events) = card(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        click(visual, "question-option-0-0");
        click(visual, "question-prev");
        window
            .update(visual, |card, _, cx| {
                card.fields[0].update(cx, |field, cx| field.set_text("Custom one".into(), cx));
                card.fields[1].update(cx, |field, cx| field.set_text("Custom extra".into(), cx));
                card.selected[1] = vec!["A".into()];
                assert_eq!(
                    card.answers(cx),
                    vec![vec!["Custom one"], vec!["A", "Custom extra"]]
                );
                card.selected = vec![Vec::new(); 2];
                card.fields[0].update(cx, |field, cx| field.set_text(String::new(), cx));
                card.fields[1].update(cx, |field, cx| field.set_text(String::new(), cx));
            })
            .unwrap();
        click(visual, "question-skip");
        click(visual, "question-skip");
        assert_eq!(*replies.borrow(), vec![None]);
    }
}
