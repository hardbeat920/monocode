//! Port of src/features/skills/ui/SkillPicker.tsx: the slash-command list
//! (MonoCode commands, provider commands, and skills) with its starter
//! skill form.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::{Enter, InputEvent, InputState};
use monocode_ui::styled::glass_backdrop;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::field::plain_input;
use super::file_mention_picker::follow_active;

/// `Skill.kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillKind {
    /// A SKILL.md on disk.
    File,
    /// A MonoCode command or the bundled create-skill skill.
    Builtin,
    /// A provider-owned command.
    Native,
}

impl SkillKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SkillKind::File => "file",
            SkillKind::Builtin => "builtin",
            SkillKind::Native => "native",
        }
    }
}

/// `Skill.scope`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillScope {
    Project,
    User,
    Builtin,
}

/// `NativeCommand.subcommands[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillSubcommand {
    pub name: SharedString,
    pub usage: Option<SharedString>,
}

/// One slash command or skill, as the picker shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerSkill {
    pub kind: SkillKind,
    pub name: SharedString,
    pub invocation: SharedString,
    pub description: SharedString,
    /// `agents`, `monocode`, or a harness id.
    pub source: SharedString,
    pub scope: SkillScope,
    /// Where a native command comes from (`custom`, `builtin`).
    pub origin: Option<SharedString>,
    pub input_hint: Option<SharedString>,
    pub subcommands: Vec<SkillSubcommand>,
}

impl PickerSkill {
    /// A MonoCode builtin, such as `/plan`.
    pub fn builtin(name: &str, description: &str) -> Self {
        Self {
            kind: SkillKind::Builtin,
            name: name.to_string().into(),
            invocation: name.to_string().into(),
            description: description.to_string().into(),
            source: "monocode".into(),
            scope: SkillScope::Builtin,
            origin: None,
            input_hint: None,
            subcommands: Vec::new(),
        }
    }

    /// A SKILL.md skill.
    pub fn file(name: &str, description: &str, scope: SkillScope, source: &str) -> Self {
        Self {
            kind: SkillKind::File,
            source: source.to_string().into(),
            scope,
            ..Self::builtin(name, description)
        }
    }

    /// The React list key.
    pub fn key(&self) -> String {
        format!("{}:{}:{}", self.kind.as_str(), self.source, self.invocation)
    }

    /// `scopeLabel`.
    pub fn scope_label(&self) -> String {
        match self.kind {
            SkillKind::Native => match &self.origin {
                Some(origin) => format!("{} · {}", self.source, origin),
                None => self.source.to_string(),
            },
            SkillKind::Builtin => "monocode".into(),
            SkillKind::File if self.scope == SkillScope::User => "personal".into(),
            SkillKind::File if self.source != "agents" && self.source != "monocode" => {
                self.source.to_string()
            }
            SkillKind::File => "project".into(),
        }
    }

    /// A native command's argument hint or subcommand usages.
    pub fn native_hint(&self) -> Option<String> {
        if self.kind != SkillKind::Native {
            return None;
        }
        if let Some(hint) = self.input_hint.as_ref().filter(|hint| !hint.is_empty()) {
            return Some(hint.to_string());
        }
        if self.subcommands.is_empty() {
            return None;
        }
        Some(
            self.subcommands
                .iter()
                .map(|sub| {
                    sub.usage
                        .as_ref()
                        .filter(|usage| !usage.is_empty())
                        .unwrap_or(&sub.name)
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(" · "),
        )
    }
}

/// The text a row renders: the command, its scope, and in the full list
/// the description and the native hint.
pub fn skill_row_texts(skill: &PickerSkill, compact: bool) -> Vec<String> {
    let mut texts = vec![format!("/{}", skill.invocation), skill.scope_label()];
    if !compact {
        if !skill.description.is_empty() {
            texts.push(skill.description.to_string());
        }
        if let Some(hint) = skill.native_hint() {
            texts.push(hint);
        }
    }
    texts
}

/// `slugSkillName`.
// TODO(port): monocode-engine's submit::skills has the same function and
// `is_valid_skill_name`. Move one copy to monocode-core.
pub fn slug_skill_name(raw: &str) -> String {
    let lower = monocode_core::js::trim(raw).to_lowercase();
    let mut slug = String::new();
    let mut dash = false;
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
            dash = false;
        } else if !dash {
            slug.push('-');
            dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    let capped: String = trimmed.chars().take(64).collect();
    capped.trim_end_matches('-').to_string()
}

/// `isValidSkillName`: `/^[a-z0-9]+(?:-[a-z0-9]+)*$/`, at most 64 characters.
pub fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

type IndexFn = Rc<dyn Fn(usize, &mut Window, &mut App)>;
type SkillFn = Rc<dyn Fn(&PickerSkill, &mut Window, &mut App)>;
type ActionFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// `<SkillPicker>`: a controlled list. Pass the form entity while creating.
#[derive(IntoElement)]
pub struct SkillPicker {
    id: ElementId,
    skills: Vec<PickerSkill>,
    query: SharedString,
    active: usize,
    creating: Option<Entity<CreateSkillForm>>,
    compact: bool,
    show_create: bool,
    on_active: Option<IndexFn>,
    on_pick: Option<SkillFn>,
    on_start_create: Option<ActionFn>,
}

pub fn skill_picker(
    id: impl Into<ElementId>,
    skills: Vec<PickerSkill>,
    query: impl Into<SharedString>,
    active: usize,
) -> SkillPicker {
    SkillPicker {
        id: id.into(),
        skills,
        query: query.into(),
        active,
        creating: None,
        compact: false,
        show_create: true,
        on_active: None,
        on_pick: None,
        on_start_create: None,
    }
}

impl SkillPicker {
    /// Show the starter skill form instead of the list.
    pub fn creating(mut self, form: Option<Entity<CreateSkillForm>>) -> Self {
        self.creating = form;
        self
    }

    /// One-line rows without a frame, for the instructions field.
    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    pub fn show_create(mut self, show: bool) -> Self {
        self.show_create = show;
        self
    }

    pub fn on_active(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_active = Some(Rc::new(f));
        self
    }

    pub fn on_pick(mut self, f: impl Fn(&PickerSkill, &mut Window, &mut App) + 'static) -> Self {
        self.on_pick = Some(Rc::new(f));
        self
    }

    pub fn on_start_create(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_start_create = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for SkillPicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut frame = div()
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family(theme.fonts.sans.clone())
            .debug_selector(|| "skill-picker".into());
        if !self.compact {
            frame = frame
                .rounded(u(theme.radius.lg))
                .border_1()
                .border_color(theme.content(0.10))
                .child(glass_backdrop(theme.radius.lg, 24., theme.content(0.05)));
        }
        if let Some(form) = self.creating {
            return frame.child(div().relative().child(form));
        }
        if self.skills.is_empty() {
            let text = if monocode_core::js::trim(&self.query).is_empty() {
                "No commands yet"
            } else {
                "No matching commands or skills"
            };
            frame = frame.child(
                div()
                    .relative()
                    .px(u(12.))
                    .py(u(10.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.50))
                    .child(text),
            );
        } else {
            let scroll = follow_active(&self.id, self.active, window, cx);
            let mut list = div()
                .id(self.id)
                .relative()
                .flex()
                .flex_col()
                .max_h(u(if self.compact { 192. } else { 240. }))
                .overflow_y_scroll()
                .track_scroll(&scroll)
                .px(u(4.))
                .py(u(4.));
            for (index, skill) in self.skills.into_iter().enumerate() {
                let highlighted = index == self.active;
                let mut row = div()
                    .id(("skill-row", index))
                    .debug_selector({
                        let invocation = skill.invocation.clone();
                        move || format!("skill-row-{invocation}")
                    })
                    .flex()
                    .flex_none()
                    .w_full()
                    .px(u(8.))
                    .rounded(u(theme.radius.md))
                    .text_color(theme.colors.content);
                row = if self.compact {
                    row.h(u(32.)).items_center()
                } else {
                    row.flex_col().gap(u(2.)).py(u(6.))
                };
                if highlighted {
                    row = row.bg(theme.content(0.10));
                }
                row = row.child(
                    div()
                        .flex()
                        .w_full()
                        .min_w_0()
                        .items_baseline()
                        .gap(u(8.))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_px(theme.text.body)
                                .leading(theme.leading.normal)
                                .child(format!("/{}", skill.invocation)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_px(theme.text.micro)
                                .text_color(theme.content(0.40))
                                .child(skill.scope_label().to_uppercase()),
                        ),
                );
                if !self.compact {
                    if !skill.description.is_empty() {
                        row = row.child(
                            div()
                                .line_clamp(2)
                                .text_px(theme.text.caption)
                                .line_height(u(16.))
                                .text_color(theme.content(0.50))
                                .child(skill.description.clone()),
                        );
                    }
                    if let Some(hint) = skill.native_hint() {
                        row = row.child(
                            div()
                                .line_clamp(2)
                                .text_px(theme.text.caption)
                                .leading(theme.leading.normal)
                                .text_color(theme.content(0.40))
                                .child(hint),
                        );
                    }
                }
                if let Some(on_active) = self.on_active.clone() {
                    row = row.on_hover(move |hovered, window, cx| {
                        if *hovered {
                            on_active(index, window, cx);
                        }
                    });
                }
                if let Some(on_pick) = self.on_pick.clone() {
                    row = row.on_click(move |_, window, cx| on_pick(&skill, window, cx));
                }
                list = list.child(row);
            }
            frame = frame.child(list);
        }
        if self.show_create {
            let hover = theme.content(0.10);
            let ink = theme.colors.content;
            let mut button = div()
                .id("skill-picker-new")
                .debug_selector(|| "skill-picker-new".into())
                .relative()
                .flex()
                .w_full()
                .items_center()
                .gap(u(8.))
                .px(u(10.))
                .py(u(8.))
                .border_t_1()
                .border_color(theme.colors.stroke)
                .text_px(theme.text.label)
                .text_color(theme.content(0.70))
                .group("skill-picker-new")
                .hover(move |s| s.bg(hover).text_color(ink))
                .child(
                    icon(IconName::Plus)
                        .size(u(14.))
                        .text_color(theme.content(0.70))
                        .group_hover("skill-picker-new", move |s| s.text_color(ink)),
                )
                .child("New skill");
            if let Some(on_start) = self.on_start_create {
                button = button.on_click(move |_, window, cx| on_start(window, cx));
            }
            frame = frame.child(button);
        }
        frame
    }
}

/// Where a new skill is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateScope {
    /// `.agents/skills` in the project.
    Project,
    /// `~/.agents/skills`.
    User,
}

type CreateFn = Rc<dyn Fn(&str, CreateScope, &mut Window, &mut App)>;

/// `CreateSkillForm`: the starter-skill form shared by the composer picker
/// and Settings. `project` is `isLocalProject(cwd)`: a remote or
/// non-project folder can only take a personal skill.
pub struct CreateSkillForm {
    project: bool,
    monospace: bool,
    scope: CreateScope,
    name: String,
    error: Option<SharedString>,
    busy: bool,
    input: Entity<InputState>,
    focus: FocusHandle,
    on_cancel: Option<ActionFn>,
    on_create: Option<CreateFn>,
    _input_events: Subscription,
}

impl CreateSkillForm {
    pub fn new(query: &str, project: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = slug_skill_name(query);
        let initial = name.clone();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("skill-name")
                .default_value(initial)
        });
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        let input_events = cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.name = input.read(cx).value().to_string();
                cx.notify();
            }
        });
        Self {
            project,
            monospace: true,
            scope: if project {
                CreateScope::Project
            } else {
                CreateScope::User
            },
            name,
            error: None,
            busy: false,
            input,
            focus: cx.focus_handle(),
            on_cancel: None,
            on_create: None,
            _input_events: input_events,
        }
    }

    pub fn monospace(mut self, monospace: bool) -> Self {
        self.monospace = monospace;
        self
    }

    pub fn on_cancel(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(f));
        self
    }

    /// Called with the slug and where to write it.
    pub fn on_create(
        mut self,
        f: impl Fn(&str, CreateScope, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_create = Some(Rc::new(f));
        self
    }

    pub fn set_error(&mut self, error: Option<SharedString>, cx: &mut Context<Self>) {
        self.error = error;
        cx.notify();
    }

    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        self.input
            .update(cx, |input, cx| input.set_disabled(busy, cx));
        cx.notify();
    }

    pub fn slug(&self) -> String {
        slug_skill_name(&self.name)
    }

    pub fn valid(&self) -> bool {
        is_valid_skill_name(&self.slug())
    }

    pub fn scope(&self) -> CreateScope {
        self.scope
    }

    pub fn set_scope(&mut self, scope: CreateScope, cx: &mut Context<Self>) {
        if self.busy || (scope == CreateScope::Project && !self.project) {
            return;
        }
        self.scope = scope;
        cx.notify();
    }

    /// `submit`.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.valid() || self.busy {
            return;
        }
        let slug = self.slug();
        let scope = if self.project {
            self.scope
        } else {
            CreateScope::User
        };
        if let Some(f) = self.on_create.clone() {
            window.defer(cx, move |window, cx| f(&slug, scope, window, cx));
        }
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.on_cancel.clone() {
            window.defer(cx, move |window, cx| f(window, cx));
        }
    }

    fn scope_button(
        &self,
        scope: CreateScope,
        label: &'static str,
        hint: &'static str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let selected = self.scope == scope;
        let disabled = self.busy || (scope == CreateScope::Project && !self.project);
        let font = if self.monospace {
            theme.fonts.mono.clone()
        } else {
            theme.fonts.sans.clone()
        };
        div()
            .id(label)
            .debug_selector(move || format!("skill-scope-{label}"))
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .px(u(8.))
            .py(u(6.))
            .rounded(u(theme.radius.md))
            .map(|el| {
                if selected {
                    el.bg(theme.colors.selection_emphasis)
                        .text_color(theme.colors.content)
                } else {
                    el.bg(theme.colors.selection)
                        .text_color(theme.content(0.70))
                }
            })
            .when(disabled, |el| el.opacity(0.4))
            .when(!disabled, |el| {
                el.on_click(cx.listener(move |this, _, _, cx| this.set_scope(scope, cx)))
            })
            .child(
                div()
                    .text_px(theme.text.label)
                    .leading(theme.leading.normal)
                    .child(label),
            )
            .child(
                div()
                    .truncate()
                    .text_px(theme.text.micro)
                    .leading(theme.leading.normal)
                    .text_color(theme.content(0.40))
                    .font_family(font)
                    .child(hint),
            )
    }
}

impl Focusable for CreateSkillForm {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CreateSkillForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let valid = self.valid();
        let font = if self.monospace {
            theme.fonts.mono.clone()
        } else {
            theme.fonts.sans.clone()
        };
        let note = match &self.error {
            Some(error) => Some((error.clone(), theme.content(0.70))),
            None if !monocode_core::js::trim(&self.name).is_empty() && !valid => Some((
                "Use lowercase letters, numbers, and hyphens.".into(),
                theme.content(0.50),
            )),
            None => None,
        };
        let cancel_hover = theme.content(0.10);
        let ink = theme.colors.content;
        let can_create = valid && !self.busy;
        div()
            .id("create-skill-form")
            .debug_selector(|| "create-skill-form".into())
            .flex()
            .flex_col()
            .px(u(10.))
            .py(u(8.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.cancel(window, cx);
                }
            }))
            .child(
                div()
                    .mb(u(8.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.50))
                    .child("Writes a starter SKILL.md you can edit."),
            )
            .child(
                div()
                    .mb(u(8.))
                    .w_full()
                    .h(u(31.))
                    .px(u(8.))
                    .rounded(u(theme.radius.md))
                    .bg(theme.content(0.10))
                    .text_px(theme.text.body)
                    .text_color(theme.colors.content)
                    .font_family(font)
                    .capture_action(cx.listener(|this: &mut Self, _: &Enter, window, cx| {
                        cx.stop_propagation();
                        this.submit(window, cx);
                    }))
                    .flex()
                    .items_center()
                    .child(plain_input(&self.input, cx)),
            )
            .child(
                div()
                    .mb(u(8.))
                    .flex()
                    .gap(u(4.))
                    .child(self.scope_button(
                        CreateScope::Project,
                        "Project",
                        ".agents/skills",
                        &theme,
                        cx,
                    ))
                    .child(self.scope_button(
                        CreateScope::User,
                        "Personal",
                        "~/.agents/skills",
                        &theme,
                        cx,
                    )),
            )
            .when_some(note, |el, (text, color)| {
                el.child(
                    div()
                        .mb(u(8.))
                        .text_px(theme.text.label)
                        .text_color(color)
                        .child(text),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(u(4.))
                    .child(
                        div()
                            .id("create-skill-cancel")
                            .debug_selector(|| "create-skill-cancel".into())
                            .px(u(8.))
                            .py(u(4.))
                            .rounded(u(theme.radius.md))
                            .text_px(theme.text.label)
                            .text_color(theme.content(0.50))
                            .hover(move |s| s.bg(cancel_hover).text_color(ink))
                            .when(!self.busy, |el| {
                                el.on_click(
                                    cx.listener(|this, _, window, cx| this.cancel(window, cx)),
                                )
                            })
                            .child("Cancel"),
                    )
                    .child(
                        div()
                            .id("create-skill-submit")
                            .debug_selector(|| "create-skill-submit".into())
                            .px(u(8.))
                            .py(u(4.))
                            .rounded(u(theme.radius.md))
                            .bg(theme.content(0.20))
                            .text_px(theme.text.label)
                            .text_color(theme.colors.content)
                            .when(!can_create, |el| el.opacity(0.4))
                            .when(can_create, |el| {
                                el.on_click(
                                    cx.listener(|this, _, window, cx| this.submit(window, cx)),
                                )
                            })
                            .child(if self.busy { "Creating…" } else { "Create" }),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native(name: &str, invocation: &str) -> PickerSkill {
        PickerSkill {
            kind: SkillKind::Native,
            source: "omp".into(),
            invocation: invocation.to_string().into(),
            ..PickerSkill::builtin(name, "")
        }
    }

    /// `native command picker` › renders native commands and argument hints
    /// alongside MonoCode shortcuts. The commands are what
    /// `ompCommandsFromRpcData` produces for the test's RPC data.
    #[test]
    fn renders_native_commands_and_argument_hints_alongside_monocode_shortcuts() {
        let plan = PickerSkill {
            description: "OMP planning".into(),
            origin: Some("builtin".into()),
            ..native("plan", "omp:plan")
        };
        let compact = PickerSkill {
            origin: Some("builtin".into()),
            input_hint: Some("[instructions]".into()),
            ..native("compact", "omp:compact")
        };
        let workflow = PickerSkill {
            description: "Choose planners and reviewers".into(),
            origin: Some("custom".into()),
            input_hint: Some("<reviewer> [path]".into()),
            ..native("workflow", "workflow")
        };
        let mcp = PickerSkill {
            origin: Some("builtin".into()),
            subcommands: vec![SkillSubcommand {
                name: "list".into(),
                usage: Some("list --all".into()),
            }],
            ..native("mcp", "mcp")
        };
        let skills = [
            PickerSkill::builtin(
                "add-to-folder",
                "Place this session in an existing or new sidebar folder.",
            ),
            PickerSkill::builtin(
                "plan",
                "Create a reviewable implementation plan before changing files.",
            ),
            PickerSkill::builtin(
                "compact",
                "Summarize older conversation context to free space.",
            ),
            plan,
            compact,
            workflow,
            mcp,
        ];
        let html: String = skills
            .iter()
            .flat_map(|skill| skill_row_texts(skill, false))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in [
            "/omp:plan",
            "/omp:compact",
            "/plan",
            "/compact",
            "/add-to-folder",
            "/workflow",
            "Choose planners and reviewers",
            "<reviewer> [path]",
            "omp · custom",
            "list --all",
        ] {
            assert!(html.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn scope_labels_follow_kind_scope_and_source() {
        assert_eq!(PickerSkill::builtin("plan", "").scope_label(), "monocode");
        let project = PickerSkill::file("deploy", "", SkillScope::Project, "agents");
        assert_eq!(project.scope_label(), "project");
        let personal = PickerSkill::file("deploy", "", SkillScope::User, "agents");
        assert_eq!(personal.scope_label(), "personal");
        let claude = PickerSkill::file("deploy", "", SkillScope::Project, "claude");
        assert_eq!(claude.scope_label(), "claude");
        assert_eq!(native("x", "x").scope_label(), "omp");
        assert_eq!(project.key(), "file:agents:deploy");
    }

    #[test]
    fn compact_rows_drop_descriptions_and_hints() {
        let skill = PickerSkill::file(
            "review",
            "Review the current changes",
            SkillScope::Project,
            "agents",
        );
        assert_eq!(skill_row_texts(&skill, true), vec!["/review", "project"]);
        assert_eq!(skill_row_texts(&skill, false).len(), 3);
    }

    #[test]
    fn slugs_and_validates_skill_names() {
        assert_eq!(slug_skill_name("  My New Skill! "), "my-new-skill");
        assert_eq!(slug_skill_name("--a__b--"), "a-b");
        assert_eq!(slug_skill_name(&"x".repeat(70)).len(), 64);
        assert!(is_valid_skill_name("review-pr"));
        assert!(!is_valid_skill_name("Review"));
        assert!(!is_valid_skill_name("a--b"));
        assert!(!is_valid_skill_name("-a"));
        assert!(!is_valid_skill_name(""));
    }
}
