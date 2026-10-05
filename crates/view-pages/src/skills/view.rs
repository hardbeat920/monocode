//! Port of src/features/skills/ui/SkillsPage.tsx: inspect and manage file
//! skills (filter, rescan, hide from the catalog, copy path, reveal, add a
//! starter skill) with an inline preview of the SKILL.md. The settings page
//! embeds it.
//!
//! React switched to a side-by-side layout with a container query at 48rem.
//! The view measures its own width the same way: a probe records the last
//! width, and a change across the threshold redraws.

use std::rc::Rc;

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable, HighlightStyle,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, StyledText, Subscription, Window,
    canvas, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use monocode_view_composer::pickers::{CreateScope, CreateSkillForm, SkillDocumentPreview};

use super::data::{DiscoveredSkill, SkillsData};
use crate::format::is_local_project;
use crate::notes::source::heading_ranges;
use crate::widgets::{
    MarkdownMode, MarkdownModes, markdown_mode_toggle, plain_input, switch_track,
};

/// `@3xl/skills`: 48rem.
const WIDE: f32 = 768.;

pub struct SkillsPage {
    data: Rc<dyn SkillsData>,
    cwd: String,
    header: Option<AnyView>,
    focus: FocusHandle,
    filter: Entity<InputState>,
    query: String,
    skills: Option<Vec<DiscoveredSkill>>,
    error: Option<String>,
    disabled: Vec<String>,
    action_error: Option<String>,
    form: Option<Entity<CreateSkillForm>>,
    busy: bool,
    list_generation: u64,
    preview: Option<DiscoveredSkill>,
    preview_text: Option<String>,
    preview_error: Option<String>,
    preview_generation: u64,
    document: Option<Entity<SkillDocumentPreview>>,
    width: Rc<std::cell::Cell<f32>>,
    list_scroll: ScrollHandle,
    preview_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl SkillsPage {
    pub fn new(
        data: Rc<dyn SkillsData>,
        cwd: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter"));
        let filter_events = cx.subscribe_in(&filter, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = input.read(cx).value().to_string();
                cx.notify();
            }
        });
        let weak = cx.weak_entity();
        let changes = data.subscribe(
            Box::new(move |cx| {
                weak.update(cx, |this, cx| {
                    this.disabled = this.data.disabled_paths(cx);
                    cx.notify();
                })
                .ok();
            }),
            cx,
        );
        let disabled = data.disabled_paths(cx);
        let mut page = Self {
            data,
            cwd: cwd.to_string(),
            header: None,
            focus: cx.focus_handle(),
            filter,
            query: String::new(),
            skills: None,
            error: None,
            disabled,
            action_error: None,
            form: None,
            busy: false,
            list_generation: 0,
            preview: None,
            preview_text: None,
            preview_error: None,
            preview_generation: 0,
            document: None,
            width: Rc::new(std::cell::Cell::new(WIDE)),
            list_scroll: ScrollHandle::new(),
            preview_scroll: ScrollHandle::new(),
            _subscriptions: vec![filter_events, changes],
        };
        page.reload(cx);
        page
    }

    /// The settings page's section header, drawn above the toolbar.
    pub fn header(mut self, header: impl Into<AnyView>) -> Self {
        self.header = Some(header.into());
        self
    }

    pub fn set_cwd(&mut self, cwd: &str, cx: &mut Context<Self>) {
        if self.cwd != cwd {
            self.cwd = cwd.to_string();
            self.form = None;
            self.reload(cx);
        }
    }

    pub fn skills(&self) -> Option<&[DiscoveredSkill]> {
        self.skills.as_deref()
    }

    pub fn preview(&self) -> Option<&DiscoveredSkill> {
        self.preview.as_ref()
    }

    pub fn preview_text(&self) -> Option<&str> {
        self.preview_text.as_deref()
    }

    pub fn preview_error(&self) -> Option<&str> {
        self.preview_error.as_deref()
    }

    pub fn is_adding(&self) -> bool {
        self.form.is_some()
    }

    pub fn filter_input(&self) -> &Entity<InputState> {
        &self.filter
    }

    pub fn form(&self) -> Option<&Entity<CreateSkillForm>> {
        self.form.as_ref()
    }

    /// The list effect: scan the project again, dropping older answers.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.list_generation += 1;
        let generation = self.list_generation;
        self.skills = None;
        self.error = None;
        let task = self.data.list_skills(&self.cwd, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                if generation != this.list_generation {
                    return;
                }
                match result {
                    Ok(skills) => {
                        this.skills = Some(skills);
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The Refresh button: invalidate the catalog and rescan.
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        self.data.invalidate(cx);
        self.reload(cx);
    }

    /// The skills the filter shows: name, description, source, or path.
    pub fn filtered(&self) -> Vec<DiscoveredSkill> {
        let needle = monocode_core::js::trim(&self.query).to_lowercase();
        self.skills
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|skill| {
                needle.is_empty()
                    || skill.name.to_lowercase().contains(&needle)
                    || skill.description.to_lowercase().contains(&needle)
                    || skill.source.to_lowercase().contains(&needle)
                    || skill.path.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    }

    pub fn is_disabled(&self, path: &str) -> bool {
        self.disabled.iter().any(|item| item == path)
    }

    /// `onToggle`: include or hide a skill in MonoCode's catalog.
    pub fn toggle(&mut self, path: &str, enabled: bool, cx: &mut Context<Self>) {
        let next: Vec<String> = if enabled {
            self.disabled
                .iter()
                .filter(|item| *item != path)
                .cloned()
                .collect()
        } else {
            let mut next = self.disabled.clone();
            next.push(path.to_string());
            next
        };
        match self.data.save_disabled_paths(next.clone(), cx) {
            Ok(()) => {
                self.disabled = next;
                self.action_error = None;
            }
            Err(_) => {
                self.action_error = Some("Could not save the skill preference. Try again.".into())
            }
        }
        cx.notify();
    }

    fn reveal(&mut self, path: &str, cx: &mut Context<Self>) {
        self.action_error = None;
        let task = self.data.reveal(path, cx);
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                this.update(cx, |this, cx| {
                    this.action_error = Some(format!("Could not open the folder: {error}"));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        cx.notify();
    }

    fn copy_path(&mut self, path: &str, cx: &mut Context<Self>) {
        self.action_error = None;
        let task = self.data.copy_text(path, cx);
        cx.spawn(async move |this, cx| {
            if task.await.is_err() {
                this.update(cx, |this, cx| {
                    this.action_error = Some("Could not copy the path to the clipboard.".into());
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        cx.notify();
    }

    /// Add skill and Close.
    pub fn toggle_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.form.take().is_some() {
            self.focus.focus(window, cx);
            cx.notify();
            return;
        }
        let project = is_local_project(&self.cwd);
        let query = self.query.clone();
        let cancel = cx.weak_entity();
        let create = cx.weak_entity();
        self.form = Some(cx.new(|cx| {
            CreateSkillForm::new(&query, project, window, cx)
                .monospace(false)
                .on_cancel(move |window, cx| {
                    cancel
                        .update(cx, |this, cx| {
                            this.form = None;
                            this.focus.focus(window, cx);
                            cx.notify();
                        })
                        .ok();
                })
                .on_create(move |name, scope, _, cx| {
                    let name = name.to_string();
                    create
                        .update(cx, |this, cx| this.create(&name, scope, cx))
                        .ok();
                })
        }));
        cx.notify();
    }

    /// `onCreate`.
    fn create(&mut self, name: &str, scope: CreateScope, cx: &mut Context<Self>) {
        self.busy = true;
        if let Some(form) = self.form.clone() {
            form.update(cx, |form, cx| {
                form.set_busy(true, cx);
                form.set_error(None, cx);
            });
        }
        let task = self
            .data
            .create_blank_skill(&self.cwd, name, scope == CreateScope::Project, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        this.data.invalidate(cx);
                        this.form = None;
                        this.reload(cx);
                    }
                    Err(error) => {
                        if let Some(form) = this.form.clone() {
                            form.update(cx, |form, cx| {
                                form.set_busy(false, cx);
                                form.set_error(Some(error.into()), cx);
                            });
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// `onPreview`: open the panel for a skill and read its file. A read
    /// that lands after another skill opened is dropped.
    pub fn open_preview(
        &mut self,
        skill: &DiscoveredSkill,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preview = Some(skill.clone());
        self.preview_text = None;
        self.preview_error = None;
        self.preview_generation += 1;
        let generation = self.preview_generation;
        let task = self.data.read_text_file(&skill.path, cx);
        self.focus.focus(window, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                if generation != this.preview_generation {
                    return;
                }
                match result {
                    Ok(text) => {
                        match this.document.clone() {
                            Some(document) => {
                                document.update(cx, |document, cx| document.set_text(&text, cx))
                            }
                            None => {
                                this.document =
                                    Some(cx.new(|cx| SkillDocumentPreview::new(&text, cx)))
                            }
                        }
                        this.preview_text = Some(text);
                    }
                    Err(error) => {
                        this.preview_error = Some(format!("Could not read SKILL.md. {error}"))
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn close_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview = None;
        self.preview_text = None;
        self.preview_error = None;
        self.preview_generation += 1;
        self.filter.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// The preview's remembered mode (`skill:<path>`).
    pub fn preview_mode(&self, cx: &App) -> MarkdownMode {
        let key = format!(
            "skill:{}",
            self.preview
                .as_ref()
                .map_or("", |skill| skill.path.as_str())
        );
        MarkdownModes::get(&key, cx)
    }

    pub fn set_preview_mode(&mut self, mode: MarkdownMode, cx: &mut Context<Self>) {
        let key = format!(
            "skill:{}",
            self.preview
                .as_ref()
                .map_or("", |skill| skill.path.as_str())
        );
        MarkdownModes::set(&key, mode, cx);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape"
            && !event.keystroke.modifiers.modified()
            && self.preview.is_some()
        {
            cx.stop_propagation();
            self.close_preview(window, cx);
        }
    }

    fn icon_action(
        id: SharedString,
        glyph: IconName,
        label: &'static str,
        selector: String,
        theme: &Theme,
    ) -> gpui::Stateful<gpui::Div> {
        let hover = theme.content(0.10);
        let ink = theme.colors.content;
        let group: SharedString = format!("{id}-group").into();
        div()
            .id(gpui::ElementId::Name(id))
            .debug_selector(move || selector.clone())
            .flex()
            .flex_none()
            .size(u(20.))
            .items_center()
            .justify_center()
            .rounded(u(theme.radius.sm))
            .group(group.clone())
            .hover(move |s| s.bg(hover))
            .tooltip(tooltip(label))
            .child(
                icon(glyph)
                    .size(u(12.))
                    .text_color(theme.content(0.40))
                    .group_hover(group, move |s| s.text_color(ink)),
            )
    }

    fn render_row(
        &self,
        skill: DiscoveredSkill,
        last: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let disabled = self.is_disabled(&skill.path);
        let previewing = self
            .preview
            .as_ref()
            .is_some_and(|preview| preview.path == skill.path);
        let ink = theme.colors.content;
        let name_target = skill.clone();
        let eye_target = skill.clone();
        let toggle_path = skill.path.clone();
        let copy_path = skill.path.clone();
        let reveal_path = skill.path.clone();
        let name = skill.name.clone();
        let key = skill.path.clone();
        let mut row = div()
            .px(u(12.))
            .py(u(8.))
            .when(!last, |row| {
                row.border_b_1().border_color(theme.content(0.05))
            })
            .when(previewing, |row| row.bg(theme.content(0.05)))
            .when(disabled, |row| row.opacity(0.5))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .child(
                        div()
                            .id(gpui::ElementId::Name(format!("skill-name-{key}").into()))
                            .debug_selector({
                                let name = name.clone();
                                move || format!("skill-name {name}")
                            })
                            .mr_auto()
                            .min_w_0()
                            .truncate()
                            .text_px(theme.text.label)
                            .text_color(ink)
                            .hover(|s| s.underline())
                            .tooltip(tooltip(format!("Preview {name}")))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_preview(&name_target, window, cx)
                            }))
                            .child(skill.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .rounded_full()
                            .bg(theme.content(0.10))
                            .px(u(6.))
                            .py(u(2.))
                            .text_px(theme.text.micro)
                            .medium()
                            .text_color(theme.content(0.60))
                            .child(skill.scope_label().to_uppercase()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(u(80.))
                            .truncate()
                            .text_right()
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.40))
                            .child(skill.source.clone()),
                    )
                    .child(
                        switch_track(
                            gpui::ElementId::Name(format!("skill-switch-{key}").into()),
                            !disabled,
                            theme,
                        )
                        .debug_selector({
                            let name = skill.name.clone();
                            move || format!("Include {name} in MonoCode catalog")
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let enabled = this.is_disabled(&toggle_path);
                            this.toggle(&toggle_path, enabled, cx)
                        })),
                    ),
            );
        if !skill.description.is_empty() {
            row = row.child(
                div()
                    .id(gpui::ElementId::Name(
                        format!("skill-description-{key}").into(),
                    ))
                    .mt(u(2.))
                    .truncate()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.55))
                    .tooltip(tooltip(skill.description.clone()))
                    .child(skill.description.clone()),
            );
        }
        row.child(
            div()
                .mt(u(2.))
                .flex()
                .items_center()
                .gap(u(4.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.35))
                        .child(skill.path.clone()),
                )
                .child(
                    Self::icon_action(
                        format!("skill-eye-{key}").into(),
                        IconName::Eye,
                        "Preview skill",
                        format!("Preview skill {}", skill.name),
                        theme,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_preview(&eye_target, window, cx)
                    })),
                )
                .child(
                    Self::icon_action(
                        format!("skill-copy-{key}").into(),
                        IconName::Copy,
                        "Copy path",
                        format!("Copy path of {}", skill.name),
                        theme,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.copy_path(&copy_path, cx))),
                )
                .child(
                    Self::icon_action(
                        format!("skill-reveal-{key}").into(),
                        IconName::FolderOpen,
                        "Reveal in file manager",
                        format!("Reveal {} in file explorer", skill.name),
                        theme,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.reveal(&reveal_path, cx))),
                ),
        )
    }

    fn render_list(
        &self,
        theme: &Theme,
        wide: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let filtered = self.filtered();
        let count = match &self.skills {
            None => "…".to_string(),
            Some(_) => format!(
                "{} {}",
                filtered.len(),
                if filtered.len() == 1 {
                    "skill"
                } else {
                    "skills"
                }
            ),
        };
        let refresh_disabled = self.skills.is_none() && self.error.is_none();
        let hover = theme.content(0.10);
        let ink = theme.colors.content;
        let toolbar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(u(12.))
            .pb(u(12.))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(u(12.))
                    .child(
                        div()
                            .flex_none()
                            .text_px(theme.text.label)
                            .tabular()
                            .text_color(theme.content(0.40))
                            .debug_selector(|| "skills-count".into())
                            .child(count),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .h(u(28.))
                            .items_center()
                            .gap(u(8.))
                            .px(u(8.))
                            .rounded(u(theme.radius.md))
                            .border_1()
                            .border_color(theme.content(0.10))
                            .child(
                                icon(IconName::Search)
                                    .size(u(14.))
                                    .text_color(theme.content(0.45)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_1()
                                    .min_w_0()
                                    .items_center()
                                    .text_px(theme.text.label)
                                    .debug_selector(|| "Filter skills".into())
                                    .child(plain_input(
                                        &self.filter,
                                        Some(theme.content(0.35)),
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .id("skills-refresh")
                            .debug_selector(|| "Refresh skills".into())
                            .flex()
                            .flex_none()
                            .size(u(24.))
                            .items_center()
                            .justify_center()
                            .rounded(u(theme.radius.md))
                            .group("skills-refresh")
                            .tooltip(tooltip("Rescan skill folders"))
                            .map(|button| {
                                if refresh_disabled {
                                    button.opacity(0.4)
                                } else {
                                    button
                                        .hover(move |s| s.bg(hover))
                                        .on_click(cx.listener(|this, _, _, cx| this.rescan(cx)))
                                }
                            })
                            .child(
                                icon(IconName::RefreshCw)
                                    .size(u(14.))
                                    .text_color(theme.content(0.45))
                                    .group_hover("skills-refresh", move |s| s.text_color(ink)),
                            ),
                    ),
            )
            .child(
                div()
                    .id("skills-add")
                    .debug_selector(|| "Add skill".into())
                    .flex_none()
                    .px(u(10.))
                    .py(u(4.))
                    .rounded(u(theme.radius.md))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.70))
                    .tooltip(tooltip("Create a starter SKILL.md you can edit"))
                    .map(|button| {
                        if self.busy {
                            button.opacity(0.4)
                        } else {
                            button.hover(move |s| s.bg(hover)).on_click(
                                cx.listener(|this, _, window, cx| this.toggle_form(window, cx)),
                            )
                        }
                    })
                    .child(if self.form.is_some() {
                        "Close"
                    } else {
                        "Add skill"
                    }),
            );
        let mut column = div()
            .mx_auto()
            .w_full()
            .max_w(u(1024.))
            .py(u(32.))
            .px(u(if self.preview.is_some() { 16. } else { 32. }))
            .children(self.header.clone())
            .child(toolbar);
        if let Some(form) = self.form.clone() {
            column = column.child(
                div()
                    .mb(u(16.))
                    .overflow_hidden()
                    .rounded(u(theme.radius.lg))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .bg(theme.content(0.03))
                    .child(form),
            );
        }
        if let Some(error) = self.action_error.clone() {
            column = column.child(
                div()
                    .pb(u(12.))
                    .text_px(theme.text.label)
                    .text_color(theme.colors.danger)
                    .child(error),
            );
        }
        if let Some(error) = self.error.clone() {
            column = column.child(
                div()
                    .text_px(theme.text.label)
                    .text_color(theme.colors.danger)
                    .child(error),
            );
        } else if let Some(skills) = &self.skills {
            let mut list = div()
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(u(theme.radius.lg))
                .border_1()
                .border_color(theme.content(0.10));
            if filtered.is_empty() {
                list = list.child(
                    div()
                        .px(u(12.))
                        .py(u(12.))
                        .text_px(theme.text.label)
                        .text_color(theme.content(0.45))
                        .child(if skills.is_empty() {
                            "No skills yet. Add skill creates a starter SKILL.md."
                        } else {
                            "No matching skills"
                        }),
                );
            } else {
                let count = filtered.len();
                for (index, skill) in filtered.into_iter().enumerate() {
                    list = list.child(self.render_row(skill, index + 1 == count, theme, cx));
                }
            }
            column = column.child(list);
        } else {
            column = column.child(
                div()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child("Loading skills…"),
            );
        }
        let mono = theme.fonts.sans.clone();
        column = column.child(
            div()
                .pt(u(12.))
                .text_px(theme.text.label)
                .text_color(theme.content(0.40))
                .font_family(mono)
                .child(
                    "Hidden skills stay on disk and are excluded from MonoCode's file-skill catalog. Provider-managed skills and native commands are unaffected. Skills live in .agents/skills for this project and ~/.agents/skills for you personally; harness folders are also picked up.",
                ),
        );
        let _ = wide;
        div()
            .id("skills-list")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .child(column)
    }

    fn render_preview(
        &self,
        skill: &DiscoveredSkill,
        theme: &Theme,
        wide: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let mode = self.preview_mode(cx);
        let (toggle, [preview_tab, source_tab]) = markdown_mode_toggle("skill-mode", mode, theme);
        let toggle = toggle
            .child(preview_tab.on_click(
                cx.listener(|this, _, _, cx| this.set_preview_mode(MarkdownMode::Preview, cx)),
            ))
            .child(source_tab.on_click(
                cx.listener(|this, _, _, cx| this.set_preview_mode(MarkdownMode::Source, cx)),
            ));
        let hover = theme.content(0.10);
        let ink = theme.colors.content;
        let header = div()
            .flex()
            .flex_none()
            .items_start()
            .gap(u(8.))
            .px(u(16.))
            .pt(u(16.))
            .pb(u(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_px(16.)
                    .semibold()
                    .text_color(ink)
                    .child(skill.name.clone()),
            )
            .child(
                div()
                    .id("skill-preview-close")
                    .debug_selector(|| "Close skill preview".into())
                    .flex()
                    .flex_none()
                    .size(u(24.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.md))
                    .group("skill-preview-close")
                    .hover(move |s| s.bg(hover))
                    .tooltip(tooltip("Close preview (Escape)"))
                    .on_click(cx.listener(|this, _, window, cx| this.close_preview(window, cx)))
                    .child(
                        icon(IconName::X)
                            .size(u(14.))
                            .text_color(theme.content(0.45))
                            .group_hover("skill-preview-close", move |s| s.text_color(ink)),
                    ),
            );
        let sub = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(u(12.))
            .px(u(16.))
            .pt(u(4.))
            .pb(u(12.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .child(
                div()
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.50))
                    .debug_selector(|| "skill-preview-path".into())
                    .child(skill.path.clone()),
            )
            .child(div().flex().justify_end().child(toggle));
        let body: gpui::AnyElement = if let Some(error) = self.preview_error.clone() {
            div()
                .px(u(16.))
                .py(u(20.))
                .text_px(theme.text.label)
                .text_color(theme.colors.danger)
                .debug_selector(|| "skill-preview-error".into())
                .child(error)
                .into_any_element()
        } else if let Some(text) = self.preview_text.clone() {
            match (mode, self.document.clone()) {
                (MarkdownMode::Preview, Some(document)) => div()
                    .px(u(16.))
                    .py(u(12.))
                    .debug_selector(|| "skill-preview-document".into())
                    .child(document)
                    .into_any_element(),
                _ => markdown_source(text, theme).into_any_element(),
            }
        } else {
            div()
                .px(u(16.))
                .py(u(20.))
                .text_px(theme.text.label)
                .text_color(theme.content(0.50))
                .debug_selector(|| "skill-preview-loading".into())
                .child("Loading skill…")
                .into_any_element()
        };
        let mut aside = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .debug_selector(|| "skill-preview".into())
            .child(header)
            .child(sub)
            .child(
                div()
                    .id("skill-preview-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.preview_scroll)
                    .child(body),
            );
        aside = if wide {
            aside
                .max_w(u(720.))
                .border_l_1()
                .border_color(theme.colors.stroke)
        } else {
            aside.border_t_1().border_color(theme.colors.stroke)
        };
        aside
    }
}

/// `MarkdownSource`: the raw text in 13px monospace with heading lines in
/// the heading color.
pub fn markdown_source(text: String, theme: &Theme) -> impl IntoElement + use<> {
    let highlights = heading_ranges(&text)
        .into_iter()
        .map(|range| {
            (
                range,
                HighlightStyle {
                    color: Some(theme.colors.markdown_heading),
                    ..Default::default()
                },
            )
        })
        .collect::<Vec<_>>();
    div()
        .px(u(16.))
        .py(u(12.))
        .font_family(theme.fonts.mono.clone())
        .text_px(theme.text.body)
        .line_height(u(20.))
        .text_color(theme.content(0.85))
        .debug_selector(|| "markdown-source".into())
        .child(StyledText::new(text).with_highlights(highlights))
}

impl Focusable for SkillsPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SkillsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let scale = f32::from(window.rem_size()) / 16.0;
        let wide = self.width.get() / scale >= WIDE;
        let width = self.width.clone();
        let probe = canvas(
            move |bounds, window, _| {
                let measured = f32::from(bounds.size.width);
                let scale = f32::from(window.rem_size()) / 16.0;
                let was_wide = width.get() / scale >= WIDE;
                width.set(measured);
                if (measured / scale >= WIDE) != was_wide {
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let preview = self.preview.clone();
        let mut body = div().flex().flex_1().min_h_0().min_w_0();
        body = if wide {
            body.flex_row()
        } else {
            body.flex_col()
        };
        body = body.child(self.render_list(&theme, wide, cx));
        if let Some(skill) = preview {
            body = body.child(self.render_preview(&skill, &theme, wide, cx));
        }
        div()
            .id("skills-page")
            .key_context("SkillsPage")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .relative()
            .flex()
            .flex_1()
            .size_full()
            .min_h_0()
            .min_w_0()
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal))
            .child(probe)
            .child(body)
            .child(div().w(px(0.)))
    }
}
