//! Port of src/features/sessions/ui/QuestionForm.tsx: the agent's
//! clarifying questions, one at a time, answered by picking options or
//! typing into "Other". Arrow keys, Home, End, Enter, Space, and the digits
//! 1 to 9 work the options; Skip leaves a question out; the last Continue
//! sends the reply.
//!
//! The form answers a [`UserQuestionPrompt`] from core and reports the
//! reply as a [`QuestionFormEvent`]. When the harness set a deadline, any
//! interaction is reported too, so the host can keep the question open.

use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
    px,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_core::user_question::{
    CUSTOM_OPTION_ID, QuestionAnswers, QuestionCustom, UserQuestion, UserQuestionOption,
    UserQuestionPrompt, UserQuestionReply, build_question_reply, is_other_option,
    question_is_complete,
};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::{IconName, Theme, icon, u};

/// What the reader did.
#[derive(Debug, Clone, PartialEq)]
pub enum QuestionFormEvent {
    /// `onReply(requestId, reply)`.
    Reply {
        request_id: i64,
        reply: UserQuestionReply,
    },
    /// `onInteraction(requestId)`: the reader touched a question with a
    /// deadline, which keeps it open.
    Interaction { request_id: i64 },
}

/// `displayOptions`: the question's options, plus "Other" when it takes
/// free text and has no other option of its own.
pub fn display_options(question: &UserQuestion) -> Vec<UserQuestionOption> {
    if question.options.is_empty() {
        return Vec::new();
    }
    if question.options.iter().any(is_other_option) || !question.allow_custom {
        return question.options.clone();
    }
    let mut options = question.options.clone();
    options.push(UserQuestionOption {
        id: CUSTOM_OPTION_ID.into(),
        label: "Other".into(),
        description: None,
    });
    options
}

/// `customOptionId`.
pub fn custom_option_id(question: &UserQuestion) -> String {
    question
        .options
        .iter()
        .find(|option| is_other_option(option))
        .map(|option| option.id.clone())
        .unwrap_or_else(|| CUSTOM_OPTION_ID.into())
}

/// `isCustomId`.
pub fn is_custom_id(question: &UserQuestion, option_id: &str) -> bool {
    option_id == CUSTOM_OPTION_ID || option_id == custom_option_id(question)
}

/// `nextSelection`.
pub fn next_selection(question: &UserQuestion, current: &[String], option_id: &str) -> Vec<String> {
    if !question.multi_select {
        return vec![option_id.to_string()];
    }
    if current.iter().any(|id| id == option_id) {
        return current
            .iter()
            .filter(|id| *id != option_id)
            .cloned()
            .collect();
    }
    if is_custom_id(question, option_id) {
        let mut next: Vec<String> = current
            .iter()
            .filter(|id| !is_custom_id(question, id))
            .cloned()
            .collect();
        next.push(option_id.to_string());
        return next;
    }
    let mut next = current.to_vec();
    next.push(option_id.to_string());
    next
}

/// The countdown under a question with a deadline.
pub fn deadline_label(auto_resolve_at: i64, now: i64) -> String {
    let left = auto_resolve_at - now;
    if left > 60_000 {
        "Optional question".into()
    } else {
        let seconds = ((left as f64) / 1000.).ceil().max(0.) as i64;
        format!("Continues without an answer in {seconds}s")
    }
}

/// `<QuestionForm prompt onReply onInteraction />`.
pub struct QuestionForm {
    prompt: UserQuestionPrompt,
    step: usize,
    answers: QuestionAnswers,
    custom: QuestionCustom,
    /// `QuestionFields`' highlighted option, reset per question.
    highlighted: usize,
    /// The question the field state belongs to.
    field_question: Option<String>,
    now: i64,
    options_focus: FocusHandle,
    custom_input: Entity<InputState>,
    scroll: ScrollHandle,
    _clock: Option<Task<()>>,
    _input: Subscription,
}

impl EventEmitter<QuestionFormEvent> for QuestionForm {}

use super::util::now_ms;

impl QuestionForm {
    pub fn new(prompt: UserQuestionPrompt, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let custom_input = cx.new(|cx| InputState::new(window, cx).placeholder("Type your answer"));
        let input =
            cx.subscribe_in(
                &custom_input,
                window,
                |this, input, event, window, cx| match event {
                    InputEvent::Change => {
                        this.interact(cx);
                        let value = input.read(cx).value().to_string();
                        this.set_custom(&value, cx);
                    }
                    InputEvent::Focus => {
                        this.interact(cx);
                        this.focus_custom(cx);
                    }
                    InputEvent::PressEnter { .. } => {
                        this.interact(cx);
                        this.continue_current(window, cx);
                    }
                    InputEvent::Blur => {}
                },
            );
        let mut this = Self {
            prompt: prompt.clone(),
            step: 0,
            answers: QuestionAnswers::new(),
            custom: QuestionCustom::new(),
            highlighted: 0,
            field_question: None,
            now: now_ms(),
            options_focus: cx.focus_handle(),
            custom_input,
            scroll: ScrollHandle::new(),
            _clock: None,
            _input: input,
        };
        this.reset_prompt(prompt, window, cx);
        this
    }

    /// Show a new prompt. A new request id starts over at its first question.
    pub fn set_prompt(
        &mut self,
        prompt: UserQuestionPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if prompt == self.prompt {
            return;
        }
        if prompt.request_id != self.prompt.request_id {
            self.reset_prompt(prompt, window, cx);
        } else {
            let deadline_changed = prompt.auto_resolve_at != self.prompt.auto_resolve_at;
            self.prompt = prompt;
            if deadline_changed {
                self.sync_clock(cx);
            }
            self.sync_fields(window, cx);
        }
        cx.notify();
    }

    fn reset_prompt(
        &mut self,
        prompt: UserQuestionPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prompt = prompt;
        self.step = 0;
        self.answers.clear();
        self.custom.clear();
        self.field_question = None;
        self.sync_clock(cx);
        self.sync_fields(window, cx);
    }

    /// Tick once a second while the prompt has a deadline.
    fn sync_clock(&mut self, cx: &mut Context<Self>) {
        self.now = now_ms();
        if self.prompt.auto_resolve_at.is_none() {
            self._clock = None;
            return;
        }
        self._clock = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let alive = this
                    .update(cx, |this, cx| {
                        this.now = now_ms();
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        }));
    }

    /// `QuestionFields` remounts per question id: the highlight starts on the
    /// first selected option and the free text field shows that question's
    /// text.
    fn sync_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(question) = self.question().cloned() else {
            return;
        };
        if self.field_question.as_deref() == Some(question.id.as_str()) {
            return;
        }
        self.field_question = Some(question.id.clone());
        let selected = self.selected(&question.id);
        self.highlighted = display_options(&question)
            .iter()
            .position(|option| selected.contains(&option.id))
            .unwrap_or(0);
        let text = self.custom.get(&question.id).cloned().unwrap_or_default();
        self.custom_input
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }

    pub fn prompt(&self) -> &UserQuestionPrompt {
        &self.prompt
    }

    /// The index of the question on screen.
    pub fn index(&self) -> usize {
        let total = self.prompt.questions.len();
        self.step.min(total.saturating_sub(1))
    }

    pub fn question(&self) -> Option<&UserQuestion> {
        self.prompt.questions.get(self.index())
    }

    fn last(&self) -> bool {
        self.index() + 1 >= self.prompt.questions.len()
    }

    pub fn selected(&self, question_id: &str) -> Vec<String> {
        self.answers.get(question_id).cloned().unwrap_or_default()
    }

    pub fn highlighted(&self) -> usize {
        self.highlighted
    }

    /// `ready`: the question on screen can continue.
    pub fn ready(&self) -> bool {
        self.question()
            .is_some_and(|question| question_is_complete(question, &self.answers, &self.custom))
    }

    /// `interact`.
    fn interact(&mut self, cx: &mut Context<Self>) {
        if self.prompt.auto_resolve_at.is_some() {
            cx.emit(QuestionFormEvent::Interaction {
                request_id: self.prompt.request_id,
            });
        }
    }

    /// `finish`.
    fn finish(&mut self, cx: &mut Context<Self>) {
        let reply = build_question_reply(&self.prompt.questions, &self.answers, &self.custom);
        cx.emit(QuestionFormEvent::Reply {
            request_id: self.prompt.request_id,
            reply,
        });
    }

    /// `skipCurrent`.
    pub fn skip_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(question) = self.question().cloned() else {
            self.finish(cx);
            return;
        };
        self.answers.remove(&question.id);
        self.custom.remove(&question.id);
        if self.last() {
            self.finish(cx);
            return;
        }
        self.step = self.index() + 1;
        self.sync_fields(window, cx);
        cx.notify();
    }

    /// `continueCurrent`.
    pub fn continue_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.question().is_none() || !self.ready() {
            return;
        }
        if self.last() {
            self.finish(cx);
            return;
        }
        self.step = self.index() + 1;
        self.sync_fields(window, cx);
        cx.notify();
    }

    /// `onSelect(optionId)` for the question on screen.
    pub fn select(&mut self, option_id: &str, cx: &mut Context<Self>) {
        let Some(question) = self.question().cloned() else {
            return;
        };
        let current = self.selected(&question.id);
        self.answers.insert(
            question.id.clone(),
            next_selection(&question, &current, option_id),
        );
        cx.notify();
    }

    /// `onCustom(value)`: typed text selects the custom option.
    pub fn set_custom(&mut self, value: &str, cx: &mut Context<Self>) {
        let Some(question) = self.question().cloned() else {
            return;
        };
        self.custom.insert(question.id.clone(), value.to_string());
        let custom_id = custom_option_id(&question);
        let next = if question.multi_select {
            let mut without: Vec<String> = self
                .selected(&question.id)
                .into_iter()
                .filter(|id| !is_custom_id(&question, id))
                .collect();
            without.push(custom_id);
            without
        } else {
            vec![custom_id]
        };
        self.answers.insert(question.id.clone(), next);
        cx.notify();
    }

    /// Focusing the free text field selects the custom option.
    fn focus_custom(&mut self, cx: &mut Context<Self>) {
        let Some(question) = self.question().cloned() else {
            return;
        };
        let custom_selected = self
            .selected(&question.id)
            .iter()
            .any(|id| is_custom_id(&question, id));
        if !custom_selected {
            self.select(&custom_option_id(&question), cx);
        }
    }

    /// `highlight(index)`: move the highlight and keyboard focus.
    pub fn highlight(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.highlighted = index;
        self.options_focus.focus(window, cx);
        cx.notify();
    }

    /// `onOptionKeyDown`: returns whether the key was used.
    pub fn handle_option_key(
        &mut self,
        key: &str,
        modifiers: gpui::Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if modifiers.alt || modifiers.control || modifiers.platform {
            return false;
        }
        let Some(question) = self.question().cloned() else {
            return false;
        };
        let options = display_options(&question);
        if options.is_empty() {
            return false;
        }
        let index = self.highlighted.min(options.len() - 1);
        match key {
            "down" | "up" => {
                let offset = if key == "down" { 1 } else { options.len() - 1 };
                self.highlight((index + offset) % options.len(), window, cx);
                true
            }
            "home" => {
                self.highlight(0, window, cx);
                true
            }
            "end" => {
                self.highlight(options.len() - 1, window, cx);
                true
            }
            "enter" | "space" => {
                self.select(&options[index].id, cx);
                true
            }
            _ => {
                let Ok(shortcut) = key.parse::<usize>() else {
                    return false;
                };
                if shortcut >= 1 && shortcut <= options.len().min(9) {
                    let target = shortcut - 1;
                    self.highlight(target, window, cx);
                    self.select(&options[target].id, cx);
                    return true;
                }
                false
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_option(
        &self,
        question: &UserQuestion,
        option: &UserQuestionOption,
        index: usize,
        selected: &[String],
        focused: bool,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = selected.contains(&option.id);
        let highlighted = self.highlighted == index;
        let is_custom = is_other_option(option) || option.id == CUSTOM_OPTION_ID;
        let custom_selected = selected.iter().any(|id| is_custom_id(question, id));
        let multi = question.multi_select;
        let option_id = option.id.clone();
        let mark = div()
            .mt(px(2.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(14.))
            .border_1()
            .map(|el| {
                if multi {
                    el.rounded(px(3.))
                } else {
                    el.rounded_full()
                }
            })
            .map(|el| {
                if active {
                    el.border_color(theme.colors.content)
                        .bg(theme.colors.content)
                } else {
                    el.border_color(theme.content(0.3))
                }
            })
            .when(active, |el| {
                el.child(
                    icon(IconName::Check)
                        .size(u(10.))
                        .text_color(theme.colors.background_base),
                )
            });
        let button = div()
            .id(ElementId::Name(
                format!("question-option:{}", option.id).into(),
            ))
            .flex()
            .w_full()
            .items_start()
            .gap(u(8.))
            .rounded(u(6.))
            .border_1()
            .px(u(8.))
            .py(u(6.))
            .cursor_pointer()
            .map(|el| {
                if active {
                    el.border_color(theme.content(0.35))
                        .bg(theme.colors.selection)
                } else {
                    el.border_color(theme.content(0.1))
                        .hover(|s| s.bg(theme.content(0.05)))
                }
            })
            // `focus-visible:outline-2 outline-accent`.
            .when(focused && highlighted, |el| {
                el.border_color(theme.colors.accent)
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.interact(cx);
                this.highlight(index, window, cx);
                this.select(&option_id, cx);
            }))
            .child(mark)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_px(12.)
                            .leading(1.375)
                            .text_color(theme.colors.content)
                            .child(option.label.clone()),
                    )
                    .when_some(option.description.clone(), |el, description| {
                        el.child(
                            div()
                                .mt(px(2.))
                                .text_px(11.)
                                .leading(1.375)
                                .text_color(theme.content(0.5))
                                .child(description),
                        )
                    }),
            );
        div()
            .child(button)
            .when(is_custom && (active || custom_selected), |el| {
                el.child(self.render_custom_input(4., theme, window, cx))
            })
            .into_any_element()
    }

    fn render_custom_input(
        &self,
        top: f32,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let focused = self
            .custom_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        div()
            .mt(u(top))
            .w_full()
            .rounded(u(6.))
            .border_1()
            .border_color(if focused {
                theme.content(0.3)
            } else {
                theme.content(0.15)
            })
            .px(u(8.))
            .py(u(4.))
            .text_px(12.)
            .line_height(u(18.))
            .text_color(theme.colors.content)
            .child(super::util::bare_input(
                &self.custom_input,
                theme.content(0.35),
                theme,
                cx,
            ))
            .into_any_element()
    }
}

impl Focusable for QuestionForm {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.options_focus.clone()
    }
}

impl Render for QuestionForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(question) = self.question().cloned() else {
            return div().into_any_element();
        };
        let theme = Theme::of(cx).clone();
        let total = self.prompt.questions.len();
        let index = self.index();
        let title: SharedString = question
            .header
            .as_deref()
            .map(monocode_core::js::trim)
            .filter(|header| !header.is_empty())
            .or_else(|| {
                self.prompt
                    .title
                    .as_deref()
                    .map(monocode_core::js::trim)
                    .filter(|title| !title.is_empty())
            })
            .unwrap_or("Question")
            .to_string()
            .into();
        let ready = self.ready();
        let selected = self.selected(&question.id);
        let options = display_options(&question);
        let focused = self.options_focus.is_focused(window);

        let header = div()
            .flex()
            .items_center()
            .gap(u(6.))
            .child(
                icon(IconName::MessageSquare)
                    .size(u(14.))
                    .flex_none()
                    .text_color(theme.content(0.45)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_px(11.)
                    .text_color(theme.content(0.5))
                    .child(title),
            )
            .when(total > 1, |el| {
                el.child(
                    div()
                        .flex_none()
                        .text_px(11.)
                        .text_color(theme.content(0.4))
                        .child(format!("{} of {total}", index + 1)),
                )
            })
            .child(
                div()
                    .id("question-skip")
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(u(24.))
                    .px(u(6.))
                    .rounded(u(6.))
                    .text_px(11.)
                    .text_color(theme.content(0.55))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.content(0.1)).text_color(theme.colors.content))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.interact(cx);
                        this.skip_current(window, cx);
                    }))
                    .child("Skip"),
            );

        let mut fields = div()
            .mt(u(8.))
            .min_w_0()
            .child(
                div()
                    .text_px(13.)
                    .leading(1.375)
                    .medium()
                    .text_color(theme.colors.content)
                    .child(question.prompt.clone()),
            )
            .when(question.multi_select, |el| {
                el.child(
                    div()
                        .mt(px(2.))
                        .text_px(11.)
                        .text_color(theme.content(0.4))
                        .child("Select all that apply"),
                )
            });
        if options.is_empty() && question.allow_custom {
            fields = fields.child(self.render_custom_input(6., &theme, window, cx));
        } else {
            let mut group = div()
                .id("question-options")
                .track_focus(&self.options_focus)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    this.interact(cx);
                    let key = event.keystroke.key.clone();
                    if this.handle_option_key(&key, event.keystroke.modifiers, window, cx) {
                        cx.stop_propagation();
                    }
                }))
                .mt(u(6.))
                .flex()
                .flex_col()
                .gap(u(4.))
                .max_h(u(208.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll);
            for (option_index, option) in options.iter().enumerate() {
                group = group.child(self.render_option(
                    &question,
                    option,
                    option_index,
                    &selected,
                    focused,
                    &theme,
                    window,
                    cx,
                ));
            }
            fields = fields.child(group);
        }

        let deadline = self
            .prompt
            .auto_resolve_at
            .map(|at| deadline_label(at, self.now));
        let footer = div()
            .mt(u(10.))
            .flex()
            .items_center()
            .justify_end()
            .gap(u(8.))
            .when_some(deadline, |el, label| {
                el.child(
                    div()
                        .id("question-deadline")
                        // `mr-auto`: GPUI drops the next sibling after an
                        // auto right margin, so the label grows instead.
                        .flex_1()
                        .min_w_0()
                        .text_px(11.)
                        .text_color(theme.content(0.4))
                        .tooltip(monocode_ui::widgets::tooltip(
                            "Interact to keep this question open.",
                        ))
                        .child(label),
                )
            })
            .child(
                div()
                    .id("question-continue")
                    .flex()
                    .items_center()
                    .h(u(24.))
                    .px(u(10.))
                    .rounded(u(6.))
                    .bg(theme.colors.content)
                    .text_px(11.)
                    .medium()
                    .text_color(theme.colors.background_base)
                    .when(!ready, |el| el.opacity(0.4))
                    .when(ready, |el| {
                        el.cursor_pointer()
                            .hover(|s| s.bg(theme.content(0.8)))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.interact(cx);
                                this.continue_current(window, cx);
                            }))
                    })
                    .child("Continue"),
            );

        div()
            .id("question-form")
            .px(u(6.))
            .pb(u(6.))
            .font_family(theme.fonts.sans.clone())
            .on_any_mouse_down(cx.listener(|this, _, _, cx| this.interact(cx)))
            .child(
                div()
                    .rounded(u(8.))
                    .border_1()
                    .border_color(theme.content(0.1))
                    .bg(theme.content(0.03))
                    .px(u(12.))
                    .py(u(10.))
                    .child(header)
                    .child(fields)
                    .child(footer),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(multi_select: bool, allow_custom: bool) -> UserQuestion {
        UserQuestion {
            id: "colour".into(),
            header: None,
            prompt: "Pick a colour".into(),
            multi_select,
            allow_custom,
            options: ["red", "green", "blue"]
                .iter()
                .map(|id| UserQuestionOption {
                    id: id.to_string(),
                    label: id.to_string(),
                    description: None,
                })
                .collect(),
        }
    }

    #[test]
    fn adds_other_only_for_free_text_questions_without_one() {
        assert_eq!(display_options(&question(false, false)).len(), 3);
        let options = display_options(&question(false, true));
        assert_eq!(options.len(), 4);
        assert_eq!(options[3].id, CUSTOM_OPTION_ID);
        let mut own = question(false, true);
        own.options[2].label = "Other".into();
        assert_eq!(display_options(&own).len(), 3);
        assert_eq!(custom_option_id(&own), "blue");
        assert!(is_custom_id(&own, "blue"));
        assert!(is_custom_id(&own, CUSTOM_OPTION_ID));
    }

    #[test]
    fn single_select_replaces_and_multi_select_toggles() {
        let single = question(false, true);
        assert_eq!(next_selection(&single, &["red".into()], "blue"), ["blue"]);
        let multi = question(true, true);
        let picked = next_selection(&multi, &["red".into()], "blue");
        assert_eq!(picked, ["red", "blue"]);
        assert_eq!(next_selection(&multi, &picked, "red"), ["blue"]);
        let with_custom = next_selection(&multi, &picked, CUSTOM_OPTION_ID);
        assert_eq!(with_custom, ["red", "blue", CUSTOM_OPTION_ID]);
    }

    #[test]
    fn counts_down_the_last_minute() {
        assert_eq!(deadline_label(200_000, 100_000), "Optional question");
        assert_eq!(
            deadline_label(130_500, 100_000),
            "Continues without an answer in 31s"
        );
        assert_eq!(
            deadline_label(90_000, 100_000),
            "Continues without an answer in 0s"
        );
    }
}
