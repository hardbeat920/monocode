//! Port of src/features/automations/ui/AutomationsView.tsx: the title bar,
//! the filterable list of automation cards with their enable switches, the
//! error banner, and the template picker or the editor.

use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, ProviderLogo, Theme, UiStyled as _, icon, provider_logo, u};

use super::data::{AutomationsData, AutomationsSnapshot, card_trigger_kind};
use super::editor::{AutomationEditor, EditorStatus};
use super::model::{
    Automation, AutomationDraft, automation_matches_query, draft_from_automation,
    draft_from_template, new_automation_draft,
};
use super::templates::{
    AUTOMATION_TEMPLATE_CATEGORIES, AutomationTemplate, TemplateCategory, templates_for_category,
};
use super::trigger_mark;
use super::triggers::trigger_label;
use crate::data::ProjectsData;
use crate::format::{format_relative_time, now_ms};
use crate::widgets::{
    PageChrome, page_title_bar, plain_input, project_mark, spinner_icon, toggle_switch,
};

/// `rememberedAutomationId`: the selection outlives the view. The engine
/// keeps it in its entity; this global covers hosts without one.
#[derive(Default)]
struct RememberedCategory(Option<TemplateCategory>);

impl gpui::Global for RememberedCategory {}

/// Which editor is showing: a new draft or a saved automation, rebuilt when
/// the page replaces the draft (reset, save).
#[derive(Clone, Debug, PartialEq, Eq)]
struct EditorKey {
    id: Option<String>,
    revision: u64,
}

pub struct AutomationsView {
    data: Rc<dyn AutomationsData>,
    projects: Rc<dyn ProjectsData>,
    chrome: PageChrome,
    cwd: Option<String>,
    snapshot: AutomationsSnapshot,
    focus: FocusHandle,
    query: Entity<InputState>,
    query_text: String,
    /// The template picker shows instead of the editor.
    picker_open: bool,
    category: TemplateCategory,
    /// Unsaved edits (`draft`).
    draft: Option<AutomationDraft>,
    /// Errors from the page's own calls (open session).
    local_error: Option<String>,
    revision: u64,
    editor: Option<(EditorKey, Entity<AutomationEditor>)>,
    list_scroll: ScrollHandle,
    picker_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl AutomationsView {
    pub fn new(
        data: Rc<dyn AutomationsData>,
        projects: Rc<dyn ProjectsData>,
        cwd: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Filter automations"));
        let query_events = cx.subscribe_in(&query, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.query_text = input.read(cx).value().to_string();
                cx.notify();
            }
        });
        let weak = cx.weak_entity();
        let changes = data.subscribe(
            Box::new(move |cx| {
                weak.update(cx, |this, cx| {
                    this.snapshot = this.data.snapshot(cx);
                    cx.notify();
                })
                .ok();
            }),
            cx,
        );
        data.refresh(cx);
        let snapshot = data.snapshot(cx);
        let category = cx
            .try_global::<RememberedCategory>()
            .and_then(|remembered| remembered.0)
            .unwrap_or(TemplateCategory::Popular);
        Self {
            data,
            projects,
            chrome: PageChrome::default(),
            cwd: cwd.map(str::to_string),
            snapshot,
            focus: cx.focus_handle(),
            query,
            query_text: String::new(),
            picker_open: true,
            category,
            draft: None,
            local_error: None,
            revision: 0,
            editor: None,
            list_scroll: ScrollHandle::new(),
            picker_scroll: ScrollHandle::new(),
            _subscriptions: vec![query_events, changes],
        }
    }

    pub fn chrome(mut self, chrome: PageChrome) -> Self {
        self.chrome = chrome;
        self
    }

    pub fn snapshot(&self) -> &AutomationsSnapshot {
        &self.snapshot
    }

    pub fn is_picker_open(&self) -> bool {
        self.picker_open
    }

    pub fn draft(&self) -> Option<&AutomationDraft> {
        self.draft.as_ref()
    }

    pub fn editor(&self) -> Option<&Entity<AutomationEditor>> {
        self.editor.as_ref().map(|(_, editor)| editor)
    }

    pub fn query_input(&self) -> &Entity<InputState> {
        &self.query
    }

    pub fn category(&self) -> TemplateCategory {
        self.category
    }

    /// The cards the filter shows.
    pub fn visible(&self, cx: &App) -> Vec<Automation> {
        self.snapshot
            .automations
            .iter()
            .filter(|automation| {
                let label = self.projects.mark(&automation.cwd, cx).label;
                automation_matches_query(automation, &self.query_text, &label)
            })
            .cloned()
            .collect()
    }

    /// `editorDraft`: none while the picker shows, else the unsaved draft or
    /// the selected automation.
    pub fn editor_draft(&self) -> Option<AutomationDraft> {
        if self.picker_open {
            return None;
        }
        self.draft
            .clone()
            .or_else(|| self.snapshot.selected().map(draft_from_automation))
    }

    pub fn set_category(&mut self, category: TemplateCategory, cx: &mut Context<Self>) {
        self.category = category;
        cx.set_global(RememberedCategory(Some(category)));
        cx.notify();
    }

    /// `beginCreate`: the + button.
    pub fn begin_create(&mut self, cx: &mut Context<Self>) {
        self.picker_open = true;
        self.draft = None;
        cx.notify();
    }

    fn default_target(&self, cx: &App) -> super::data::DraftTarget {
        self.data
            .default_target(self.cwd.as_deref(), self.snapshot.selected(), cx)
    }

    /// New drafts use the window's active project after the page reopens.
    pub fn set_cwd(&mut self, cwd: Option<&str>, cx: &mut Context<Self>) {
        if self.cwd.as_deref() != cwd {
            self.cwd = cwd.map(str::to_owned);
            cx.notify();
        }
    }

    /// `beginBlank`.
    pub fn begin_blank(&mut self, cx: &mut Context<Self>) {
        let target = self.default_target(cx);
        self.picker_open = false;
        self.draft = Some(new_automation_draft(
            &target.project,
            target.harness,
            &target.model,
        ));
        self.revision += 1;
        cx.notify();
    }

    /// `beginFromTemplate`.
    pub fn begin_from_template(&mut self, template: &AutomationTemplate, cx: &mut Context<Self>) {
        let target = self.default_target(cx);
        self.picker_open = false;
        self.draft = Some(draft_from_template(
            &target.project,
            target.harness,
            &target.model,
            &template.name,
            &template.prompt,
            &template.trigger,
        ));
        self.revision += 1;
        cx.notify();
    }

    /// A card was clicked.
    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.picker_open = false;
        self.draft = None;
        self.revision += 1;
        self.data.select(Some(id.to_string()), cx);
        cx.notify();
    }

    /// `onSave`.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.editor_draft() else {
            return;
        };
        if self.snapshot.saving {
            return;
        }
        let task = self.data.save(draft, cx);
        cx.spawn(async move |this, cx| {
            if task.await.is_ok() {
                this.update(cx, |this, cx| {
                    this.picker_open = false;
                    this.draft = None;
                    this.revision += 1;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// `onRun`.
    pub fn run_now(&mut self, cx: &mut Context<Self>) {
        let Some(selected) = self.snapshot.selected().cloned() else {
            return;
        };
        self.data.run_now(&selected.id, cx).detach();
    }

    /// `onDelete`: confirm, then delete with its run history.
    pub fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.snapshot.selected().cloned() else {
            return;
        };
        let confirm = self.data.confirm(
            &format!(
                "Delete \u{201c}{}\u{201d} and its run history?",
                selected.name
            ),
            window,
            cx,
        );
        cx.spawn(async move |this, cx| {
            if !confirm.await {
                return;
            }
            let Ok(task) = this.update(cx, |this, cx| this.data.delete(&selected.id, cx)) else {
                return;
            };
            if task.await.is_ok() {
                this.update(cx, |this, cx| {
                    this.draft = None;
                    this.revision += 1;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// `onToggle`.
    pub fn toggle(&mut self, automation: &Automation, enabled: bool, cx: &mut Context<Self>) {
        self.data.set_enabled(automation, enabled, cx).detach();
    }

    fn open_session(&mut self, session_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let task = self.data.open_session(session_id, window, cx);
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                this.update(cx, |this, cx| {
                    this.local_error = Some(error);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// The editor's Cancel or Reset.
    fn close_editor(&mut self, cx: &mut Context<Self>) {
        let new = self.editor_draft().is_some_and(|draft| draft.id.is_none());
        self.draft = None;
        if new {
            self.picker_open = true;
        }
        self.revision += 1;
        cx.notify();
    }

    /// Keep the editor in step: rebuild it when the draft is replaced, and
    /// pass the page state down.
    fn sync_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.editor_draft() else {
            self.editor = None;
            return;
        };
        let key = EditorKey {
            id: draft.id.clone(),
            revision: self.revision,
        };
        if self.editor.as_ref().map(|(current, _)| current) != Some(&key) {
            let saved = draft.id.is_some();
            let change = cx.weak_entity();
            let close = cx.weak_entity();
            let submit = cx.weak_entity();
            let run = cx.weak_entity();
            let delete = cx.weak_entity();
            let open = cx.weak_entity();
            let data = self.data.clone();
            let projects = self.projects.clone();
            let editor = cx.new(|cx| {
                let mut editor = AutomationEditor::new(draft, data, projects, window, cx)
                    .on_change(move |draft, _, cx| {
                        let draft = draft.clone();
                        change
                            .update(cx, |this, cx| {
                                this.draft = Some(draft);
                                cx.notify();
                            })
                            .ok();
                    })
                    .on_close(move |_, cx| {
                        close.update(cx, |this, cx| this.close_editor(cx)).ok();
                    })
                    .on_submit(move |_, cx| {
                        submit.update(cx, |this, cx| this.save(cx)).ok();
                    })
                    .on_open_session(move |session, window, cx| {
                        let session = session.to_string();
                        open.update(cx, |this, cx| this.open_session(&session, window, cx))
                            .ok();
                    });
                if saved {
                    editor = editor
                        .on_run(move |_, cx| {
                            run.update(cx, |this, cx| this.run_now(cx)).ok();
                        })
                        .on_delete(move |window, cx| {
                            delete.update(cx, |this, cx| this.delete(window, cx)).ok();
                        });
                }
                editor
            });
            self.editor = Some((key, editor));
        }
        let status = EditorStatus {
            runs: if self.editor_draft().and_then(|draft| draft.id).is_some() {
                self.snapshot.runs.clone()
            } else {
                Vec::new()
            },
            saving: self.snapshot.saving,
            running: self.editor_draft().and_then(|draft| draft.id) == self.snapshot.running
                && self.snapshot.running.is_some(),
            dirty: self.draft.is_some(),
        };
        if let Some((_, editor)) = &self.editor {
            editor.update(cx, |editor, cx| editor.set_status(status, cx));
        }
    }

    fn render_card(
        &self,
        automation: Automation,
        active: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mark = self.projects.mark(&automation.cwd, cx);
        let last_run = automation
            .last_run_at
            .map(|at| format_relative_time(at, now_ms()));
        let model = self
            .data
            .model_name(automation.harness, &automation.model, cx);
        let ink = theme.colors.content;
        let hover = theme.content(0.05);
        let id = automation.id.clone();
        let toggle_target = automation.clone();
        let enabled = automation.enabled;
        let name = automation.name.clone();
        let selector = automation.name.clone();
        let mut meta = div()
            .mt(u(4.))
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .text_px(theme.text.caption)
            .text_color(theme.content(0.45))
            .child(project_mark(&mark, 14., 12., theme.content(0.45)))
            .child(div().min_w_0().truncate().child(mark.label.clone()));
        if let Some(last_run) = last_run {
            meta = meta
                .child(div().flex_none().child("·"))
                .child(div().flex_none().child(last_run));
        }
        let logo = ProviderLogo::from_id(automation.harness.as_str());
        meta = meta.child(
            div()
                .id(ElementId::Name(format!("automation-model-{id}").into()))
                .ml_auto()
                .flex()
                .min_w_0()
                .max_w(u(112.))
                .items_center()
                .gap(u(4.))
                .text_color(theme.content(0.50))
                .tooltip(tooltip(model.clone()))
                .children(logo.map(|logo| provider_logo(logo).size(14.)))
                .child(div().min_w_0().truncate().child(model)),
        );
        div()
            .id(ElementId::Name(format!("automation-card-{id}").into()))
            .debug_selector(move || format!("automation-card {selector}"))
            .relative()
            .rounded(u(theme.radius.md))
            .map(|card| {
                if active {
                    card.bg(theme.colors.selection)
                } else {
                    card.hover(move |s| s.bg(hover))
                }
            })
            .on_click(cx.listener(move |this, _, _, cx| this.select(&id, cx)))
            .child(
                div()
                    .px(u(10.))
                    .py(u(8.))
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .items_center()
                            .gap(u(6.))
                            .pr(u(32.))
                            .text_px(theme.text.micro)
                            .text_color(theme.content(0.45))
                            .child(trigger_mark(
                                card_trigger_kind(&automation),
                                12.,
                                theme.content(0.45),
                            ))
                            .child(div().min_w_0().truncate().child(trigger_label(&automation))),
                    )
                    .child(
                        div()
                            .mt(u(4.))
                            .truncate()
                            .text_px(theme.text.body)
                            .semibold()
                            .text_color(ink)
                            .child(automation.name.clone()),
                    )
                    .child(meta),
            )
            .child(
                div().absolute().right(u(10.)).top(u(8.)).flex().child(
                    toggle_switch(
                        ElementId::Name(format!("automation-toggle-{}", automation.id).into()),
                        enabled,
                        true,
                        format!("{} {name}", if enabled { "Pause" } else { "Enable" }),
                        theme,
                    )
                    .debug_selector({
                        let name = automation.name.clone();
                        move || format!("automation-toggle {name}")
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.toggle(&toggle_target, !enabled, cx)
                    })),
                ),
            )
            .into_any_element()
    }

    fn render_list(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let hover = theme.content(0.10);
        let ink = theme.colors.content;
        let toolbar = div()
            .flex()
            .flex_none()
            .h(u(theme.metrics.toolbar_height))
            .items_center()
            .gap(u(4.))
            .px(u(8.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h(u(28.))
                    .items_center()
                    .child(
                        div().absolute().left(u(8.)).child(
                            icon(IconName::Search)
                                .size(u(12.))
                                .text_color(theme.content(0.40)),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .size_full()
                            .items_center()
                            .pl(u(28.))
                            .pr(u(8.))
                            .text_px(theme.text.label)
                            .child(plain_input(&self.query, None, cx)),
                    ),
            )
            .child(
                div()
                    .id("new-automation")
                    .debug_selector(|| "New automation".into())
                    .flex()
                    .flex_none()
                    .size(u(24.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.md))
                    .group("new-automation")
                    .hover(move |s| s.bg(hover))
                    .tooltip(tooltip("New automation"))
                    .on_click(cx.listener(|this, _, _, cx| this.begin_create(cx)))
                    .child(
                        icon(IconName::Plus)
                            .size(u(14.))
                            .text_color(theme.content(0.45))
                            .group_hover("new-automation", move |s| s.text_color(ink)),
                    ),
            );
        let mut list = div()
            .id("automations-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .p(u(6.));
        let visible = self.visible(cx);
        if self.snapshot.loading {
            list = list.child(div().flex().justify_center().py(u(48.)).child(spinner_icon(
                "automations-loading",
                16.,
                theme.content(0.35),
            )));
        } else if visible.is_empty() {
            list = list.child(
                div()
                    .px(u(12.))
                    .py(u(32.))
                    .text_center()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child(if monocode_core::js::trim(&self.query_text).is_empty() {
                        "No automations yet"
                    } else {
                        "No matching automations"
                    }),
            );
        } else {
            let selected = self.snapshot.selected_id.clone();
            let draft_id = self.draft.as_ref().map(|draft| draft.id.clone());
            let mut cards = div().flex().flex_col().gap(u(2.));
            for automation in visible {
                let active = !self.picker_open
                    && selected.as_deref() == Some(automation.id.as_str())
                    && draft_id
                        .as_ref()
                        .is_none_or(|id| id.as_deref() == Some(automation.id.as_str()));
                cards = cards.child(self.render_card(automation, active, theme, cx));
            }
            list = list.child(cards);
        }
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(u(280.))
            .min_h_0()
            .border_r_1()
            .border_color(theme.colors.stroke)
            .child(toolbar)
            .child(list)
            .into_any_element()
    }

    fn render_picker(&self, theme: &Theme, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let content = theme.colors.content;
        let mut categories = div().mt(u(16.)).flex().flex_wrap().gap(u(6.));
        for category in AUTOMATION_TEMPLATE_CATEGORIES {
            let selected = category == self.category;
            let hover = theme.content(0.08);
            let label = category.label();
            categories = categories.child(
                div()
                    .id(ElementId::Name(
                        format!("template-category-{}", category.id()).into(),
                    ))
                    .debug_selector(move || format!("template-category {label}"))
                    .flex()
                    .h(u(28.))
                    .items_center()
                    .px(u(12.))
                    .rounded_full()
                    .text_px(theme.text.label)
                    .medium()
                    .map(|chip| {
                        if selected {
                            chip.bg(content).text_color(theme.colors.background_base)
                        } else {
                            chip.text_color(theme.content(0.55))
                                .hover(move |s| s.bg(hover).text_color(content))
                        }
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_category(category, cx)))
                    .child(label),
            );
        }
        let card = |id: ElementId, dashed: bool, theme: &Theme| {
            let border_hover = theme.content(if dashed { 0.25 } else { 0.16 });
            let fill = theme.content(0.05);
            let card = div()
                .id(id)
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .min_h(u(148.))
                .p(u(16.))
                .rounded(u(theme.radius.xl))
                .border_1()
                .border_color(theme.content(if dashed { 0.15 } else { 0.10 }))
                .hover(move |s| s.border_color(border_hover).bg(fill));
            if dashed { card.border_dashed() } else { card }
        };
        let tile = |glyph: IconName, theme: &Theme| {
            div()
                .flex()
                .flex_none()
                .size(u(36.))
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(theme.content(0.08))
                .child(icon(glyph).size(u(16.)).text_color(theme.content(0.70)))
        };
        let text = |name: String, description: String, theme: &Theme| {
            div()
                .min_w_0()
                .child(
                    div()
                        .text_px(theme.text.body)
                        .medium()
                        .text_color(theme.colors.content)
                        .child(name),
                )
                .child(
                    div()
                        .mt(u(4.))
                        .text_px(theme.text.label)
                        .leading(theme.leading.snug)
                        .text_color(theme.content(0.50))
                        .child(description),
                )
        };
        let mut cards: Vec<AnyElement> = vec![
            card("template-blank".into(), true, theme)
                .debug_selector(|| "template Start from scratch".into())
                .on_click(cx.listener(|this, _, _, cx| this.begin_blank(cx)))
                .child(
                    div()
                        .flex()
                        .gap(u(12.))
                        .child(tile(IconName::Plus, theme))
                        .child(text(
                            "Start from scratch".into(),
                            "Write your own instructions and choose a trigger.".into(),
                            theme,
                        )),
                )
                .into_any_element(),
        ];
        let templates = templates_for_category(&self.data.templates(cx), self.category);
        for template in templates {
            let name = template.name.clone();
            let picked = template.clone();
            cards.push(
                card(
                    ElementId::Name(format!("template-{}", template.id).into()),
                    false,
                    theme,
                )
                .debug_selector(move || format!("template {name}"))
                .on_click(cx.listener(move |this, _, _, cx| this.begin_from_template(&picked, cx)))
                .child(
                    div()
                        .flex()
                        .gap(u(12.))
                        .child(tile(template.icon.icon(), theme))
                        .child(text(
                            template.name.clone(),
                            template.description.clone(),
                            theme,
                        )),
                )
                .child(
                    div()
                        .mt_auto()
                        .pt(u(12.))
                        .flex()
                        .min_w_0()
                        .items_center()
                        .gap(u(6.))
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.45))
                        .child(trigger_mark(
                            template.trigger.kind,
                            12.,
                            theme.content(0.45),
                        ))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .child(template.trigger_label.clone()),
                        ),
                )
                .into_any_element(),
            );
        }
        // `grid-cols-1 min-[780px]:grid-cols-2`, against the window width.
        let scale = f32::from(window.rem_size()) / 16.0;
        let columns = if f32::from(window.viewport_size().width) / scale >= 780. {
            2
        } else {
            1
        };
        let mut grid = div().mt(u(16.)).flex().flex_col().gap(u(12.));
        let mut row: Vec<AnyElement> = Vec::new();
        for card in cards {
            row.push(card);
            if row.len() == columns {
                grid = grid.child(
                    div()
                        .flex()
                        .items_stretch()
                        .gap(u(12.))
                        .children(row.drain(..)),
                );
            }
        }
        if !row.is_empty() {
            // An empty cell with the cards' padding and border, so the last
            // card keeps half the width.
            row.push(
                div()
                    .flex_1()
                    .min_w_0()
                    .p(u(16.))
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .into_any_element(),
            );
            grid = grid.child(div().flex().items_stretch().gap(u(12.)).children(row));
        }
        div()
            .id("automation-picker")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            .track_scroll(&self.picker_scroll)
            .child(
                div()
                    .mx_auto()
                    .w_full()
                    .max_w(u(1024.))
                    .px(u(32.))
                    .pt(u(20.))
                    .pb(u(40.))
                    .child(
                        div()
                            .text_px(theme.text.title)
                            .semibold()
                            .leading(theme.leading.tight)
                            .text_color(content)
                            .child("New automation"),
                    )
                    .child(
                        div()
                            .mt(u(6.))
                            .text_px(theme.text.body)
                            .text_color(theme.content(0.50))
                            .child("Pick an example or start from scratch."),
                    )
                    .child(categories)
                    .child(grid),
            )
            .into_any_element()
    }
}

impl Focusable for AutomationsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AutomationsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_editor(window, cx);
        let theme = Theme::of(cx).clone();
        let error = self
            .snapshot
            .error
            .clone()
            .or_else(|| self.local_error.clone());
        let mut main = div().flex().flex_col().flex_1().min_h_0().min_w_0();
        if let Some(error) = error {
            main = main.child(
                div()
                    .m(u(16.))
                    .flex()
                    .flex_none()
                    .items_start()
                    .gap(u(8.))
                    .rounded(u(theme.radius.lg))
                    .border_1()
                    .border_color(theme.colors.danger.opacity(0.2))
                    .bg(theme.colors.danger.opacity(0.08))
                    .px(u(12.))
                    .py(u(8.))
                    .text_px(theme.text.label)
                    .text_color(theme.colors.danger_soft)
                    .debug_selector(|| "automations-error".into())
                    .child(
                        icon(IconName::AlertCircle)
                            .mt(u(2.))
                            .size(u(14.))
                            .text_color(theme.colors.danger_soft),
                    )
                    .child(div().child(error)),
            );
        }
        let editor = self.editor.as_ref().map(|(_, editor)| editor.clone());
        main =
            if self.snapshot.loading && editor.is_none() {
                main.child(div().flex().flex_1().items_center().justify_center().child(
                    spinner_icon("automations-main-loading", 16., theme.content(0.35)),
                ))
            } else if let Some(editor) = editor {
                main.child(editor)
            } else {
                main.child(self.render_picker(&theme, window, cx))
            };
        let title = div()
            .min_w_0()
            .truncate()
            .text_color(theme.colors.content)
            .child("Automations");
        div()
            .id("automations-view")
            .key_context("AutomationsView")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .min_h_0()
            .min_w_0()
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal))
            .child(page_title_bar(&self.chrome, IconName::Zap, title, &theme))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.render_list(&theme, cx))
                    .child(main),
            )
    }
}
