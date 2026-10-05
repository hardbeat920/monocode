//! Port of src/features/skills/ui/SkillPromptField.tsx: the automation
//! instructions field with the composer's slash-skill behavior. Typing `/`
//! opens the compact skill list under the slash; Enter or Tab inserts the
//! highlighted skill, and known `/skill` tokens show in the skill color.
//!
//! The slash rules and ranking live in monocode-engine
//! (`submit::skills`). The owner passes them in through
//! [`SkillCompletions`], built over its skill catalog.
//!
//! The textarea draws its text transparent over a mirror that carries the
//! highlights, as the React field layers a highlight div under its
//! textarea. The mirror wraps at the textarea's wrap width, which keeps a
//! 10px right margin inside the field.

use std::rc::Rc;

use gpui::{
    Anchor, App, AppContext as _, Context, Entity, HighlightStyle, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, StyledText, Subscription,
    Window, anchored, deferred, div, point, px,
};
use gpui_component::input::{
    Enter, Escape, IndentInline, InputEvent, MoveDown, MoveUp, TextareaState,
};
use monocode_ui::{Theme, UiStyled as _, u};

use super::anchor::popover_surface;
use super::field::plain_input;
use super::skill_picker::{PickerSkill, skill_picker};

/// `SlashToken`: byte offsets into the field's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashToken {
    pub start: usize,
    pub end: usize,
    pub query: String,
}

/// `SkillTextPart`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillTextPart {
    pub text: String,
    pub skill: bool,
}

/// What the field needs from the skill model.
pub trait SkillCompletions {
    /// `slashTokenAt(text, cursor, hasNativeCommands(harness))`.
    fn slash_token_at(&self, text: &str, cursor: usize) -> Option<SlashToken>;
    /// `rankSkills(skills, query, limit)` over the owner's catalog.
    fn rank(&self, query: &str) -> Vec<PickerSkill>;
    /// `replaceSlashToken`.
    fn replace_slash_token(&self, text: &str, token: &SlashToken, invocation: &str) -> String;
    /// `skillTextParts(text, names)` with the catalog's invocations.
    fn text_parts(&self, text: &str) -> Vec<SkillTextPart>;
}

/// `px-3 py-3`.
const PADDING: f32 = 12.0;
/// The input element's `RIGHT_MARGIN` inside the wrap width, in pixels.
const WRAP_MARGIN: f32 = 10.0;

type ChangeFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

pub struct SkillPromptField {
    completions: Rc<dyn SkillCompletions>,
    input: Entity<TextareaState>,
    value: String,
    slash: Option<SlashToken>,
    ranked: Vec<PickerSkill>,
    active: usize,
    last_sync: Option<(String, usize)>,
    on_change: Option<ChangeFn>,
    _subscriptions: Vec<Subscription>,
}

impl SkillPromptField {
    pub fn new(
        value: &str,
        completions: Rc<dyn SkillCompletions>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial = value.to_string();
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 10_000)
                .placeholder("Tell the agent what to do when this automation runs…")
                .default_value(initial)
        });
        let events = cx.subscribe_in(
            &input,
            window,
            |this, input, event, window, cx| match event {
                InputEvent::Change => {
                    let value = input.read(cx).value().to_string();
                    this.value = value.clone();
                    if let Some(f) = this.on_change.clone() {
                        window.defer(cx, move |window, cx| f(&value, window, cx));
                    }
                    this.sync_slash(cx);
                }
                InputEvent::Blur => {
                    this.slash = None;
                    cx.notify();
                }
                _ => {}
            },
        );
        let observe = cx.observe(&input, |this, _, cx| this.sync_slash(cx));
        Self {
            completions,
            input,
            value: value.to_string(),
            slash: None,
            ranked: Vec::new(),
            active: 0,
            last_sync: None,
            on_change: None,
            _subscriptions: vec![events, observe],
        }
    }

    pub fn on_change(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// New catalog (cwd or harness changed).
    pub fn set_completions(
        &mut self,
        completions: Rc<dyn SkillCompletions>,
        cx: &mut Context<Self>,
    ) {
        self.completions = completions;
        self.active = 0;
        self.last_sync = None;
        self.sync_slash(cx);
    }

    pub fn input(&self) -> &Entity<TextareaState> {
        &self.input
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn slash(&self) -> Option<&SlashToken> {
        self.slash.as_ref()
    }

    pub fn ranked(&self) -> &[PickerSkill] {
        &self.ranked
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// `syncSlashToken`: read the token at the cursor, once per text and
    /// cursor change, so a dismissed list stays closed until either moves.
    fn sync_slash(&mut self, cx: &mut Context<Self>) {
        let state = self.input.read(cx);
        let value = state.value().to_string();
        let cursor = state.cursor();
        let key = (value, cursor);
        if self.last_sync.as_ref() == Some(&key) {
            return;
        }
        let (value, cursor) = key.clone();
        self.last_sync = Some(key);
        let next = self.completions.slash_token_at(&value, cursor);
        let query_changed =
            next.as_ref().map(|t| &t.query) != self.slash.as_ref().map(|t| &t.query);
        self.slash = next;
        if let Some(token) = &self.slash {
            self.ranked = self.completions.rank(&token.query);
        } else {
            self.ranked.clear();
        }
        if query_changed {
            self.active = 0;
        }
        self.active = if self.ranked.is_empty() {
            0
        } else {
            self.active.min(self.ranked.len() - 1)
        };
        cx.notify();
    }

    /// Arrows wrap through the ranked skills.
    pub fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let len = self.ranked.len();
        if len == 0 {
            return;
        }
        self.active = if down {
            (self.active + 1) % len
        } else {
            (self.active + len - 1) % len
        };
        cx.notify();
    }

    /// Escape closes the list and leaves the text alone.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.slash = None;
        cx.notify();
    }

    /// `pickSkill`: replace the token, put the cursor after the inserted
    /// command and its space, and keep focus in the field.
    pub fn pick_skill(&mut self, skill: &PickerSkill, window: &mut Window, cx: &mut Context<Self>) {
        let Some(token) = self.slash.clone() else {
            return;
        };
        let next = self
            .completions
            .replace_slash_token(&self.value, &token, &skill.invocation);
        let mut cursor = token.start + skill.invocation.len() + 1;
        if next.as_bytes().get(cursor) == Some(&b' ') {
            cursor += 1;
        }
        let cursor = cursor.min(next.len());
        self.input.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_selected_range(cursor..cursor, cx);
            input.focus(window, cx);
        });
        self.value = next.clone();
        self.slash = None;
        if let Some(f) = self.on_change.clone() {
            window.defer(cx, move |window, cx| f(&next, window, cx));
        }
        cx.notify();
    }

    fn pick_active(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.ranked.get(self.active).cloned() {
            Some(skill) if self.slash.is_some() => {
                self.pick_skill(&skill, window, cx);
                true
            }
            _ => false,
        }
    }

    fn render_picker(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let token = self.slash.as_ref()?;
        let bounds = self
            .input
            .read(cx)
            .range_to_bounds(&(token.start..token.start + 1))?;
        let entity = cx.entity();
        let pick_entity = entity.clone();
        let picker = skill_picker(
            "skill-prompt-suggestions",
            self.ranked.clone(),
            token.query.clone(),
            self.active,
        )
        .compact(true)
        .show_create(false)
        .on_active(move |index, _, cx| {
            entity.update(cx, |this, cx| {
                if this.active != index {
                    this.active = index;
                    cx.notify();
                }
            })
        })
        .on_pick(move |skill, window, cx| {
            let skill = skill.clone();
            pick_entity.update(cx, |this, cx| this.pick_skill(&skill, window, cx));
        });
        let outside = cx.listener(|this, _, _, cx| this.dismiss(cx));
        let surface = popover_surface(
            ("skill-prompt-popover", token.start),
            Some(300.),
            Some(194.),
            outside,
            div()
                .debug_selector(|| "skill-prompt-popover".into())
                .child(picker),
        );
        let gap = u(4.).to_pixels(window.rem_size());
        let layer = Theme::of(cx).layer.popover;
        Some(
            deferred(
                anchored()
                    .position(point(bounds.origin.x, bounds.bottom() + gap))
                    .anchor(Anchor::TopLeft)
                    .snap_to_window_with_margin(px(8.))
                    .child(surface),
            )
            .with_priority(layer),
        )
    }
}

impl Render for SkillPromptField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let parts = self.completions.text_parts(&self.value);
        let mut highlights = Vec::new();
        let mut offset = 0;
        for part in &parts {
            let end = offset + part.text.len();
            if part.skill {
                highlights.push((
                    offset..end,
                    HighlightStyle {
                        color: Some(theme.colors.skill),
                        ..Default::default()
                    },
                ));
            }
            offset = end;
        }
        let mut mirror_text = self.value.clone();
        if mirror_text.ends_with('\n') {
            // A trailing newline still takes a line, as `{"\n"}` keeps it in React.
            mirror_text.push(' ');
        }
        let pad = u(PADDING).to_pixels(window.rem_size());
        let mirror = div()
            .absolute()
            .top(pad)
            .left(pad)
            .right(pad + px(WRAP_MARGIN))
            .text_color(theme.colors.content)
            .child(StyledText::new(SharedString::from(mirror_text)).with_highlights(highlights));
        let mut root = div()
            .id("skill-prompt-field")
            .debug_selector(|| "skill-prompt-field".into())
            .relative()
            .min_h(u(112.))
            .text_px(theme.text.ui)
            .line_height(u(22.))
            .font_family(theme.fonts.sans.clone())
            .child(mirror)
            .child(
                div()
                    .relative()
                    .flex()
                    .p(pad)
                    .text_color(gpui::transparent_black())
                    .capture_action(cx.listener(|this: &mut Self, _: &MoveDown, _, cx| {
                        if this.slash.is_some() {
                            cx.stop_propagation();
                            this.step(true, cx);
                        }
                    }))
                    .capture_action(cx.listener(|this: &mut Self, _: &MoveUp, _, cx| {
                        if this.slash.is_some() {
                            cx.stop_propagation();
                            this.step(false, cx);
                        }
                    }))
                    .capture_action(cx.listener(|this: &mut Self, _: &Escape, _, cx| {
                        if this.slash.is_some() {
                            cx.stop_propagation();
                            this.dismiss(cx);
                        }
                    }))
                    .capture_action(
                        cx.listener(|this: &mut Self, _: &IndentInline, window, cx| {
                            if this.pick_active(window, cx) {
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .capture_action(cx.listener(|this: &mut Self, action: &Enter, window, cx| {
                        if !action.shift && this.pick_active(window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .child(plain_input(&self.input, cx)),
            );
        if let Some(picker) = self.render_picker(window, cx) {
            root = root.child(picker);
        }
        root
    }
}
