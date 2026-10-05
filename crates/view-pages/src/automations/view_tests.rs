//! GPUI tests for AutomationsView over [`LocalAutomations`]: the list and
//! filter, the template picker, the editor's save rules, triggers, the
//! actions menu, and the run history.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};

use super::model::{Automation, AutomationRun, AutomationTriggerKind, TemplateTrigger};
use super::{
    AutomationEditor, AutomationTemplate, AutomationsView, EditorTab, LocalAutomations,
    TemplateCategory, TemplateIcon,
};
use crate::data::StaticProjects;
use crate::test_support::{click, draw, exists, hover, mount, type_text};

fn automation(id: &str, name: &str, triggers: serde_json::Value, enabled: bool) -> Automation {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "name": name,
        "prompt": "Review the latest commits.",
        "harness": "claude",
        "model": "claude:sonnet",
        "cwd": "/work/app",
        "workspaceMode": "worktree",
        "reuseSession": false,
        "runtimeMode": "auto",
        "triggerKind": "time",
        "triggerEvent": "weekdays",
        "scheduleKind": "weekdays",
        "minute": 0,
        "time": "09:00",
        "dayOfWeek": 1,
        "triggers": triggers,
        "missedRunGraceMinutes": 720,
        "enabled": enabled,
        "nextRunAt": 0,
        "createdAt": 1,
        "updatedAt": 1,
    }))
    .unwrap()
}

fn weekdays() -> serde_json::Value {
    serde_json::json!([{
        "id": "t1", "kind": "time", "event": "weekdays", "scheduleKind": "weekdays",
        "minute": 0, "time": "09:00", "dayOfWeek": 1,
        "repos": [], "repo": "", "branch": "", "actor": "anyone",
    }])
}

fn issues() -> serde_json::Value {
    serde_json::json!([{
        "id": "t2", "kind": "github", "event": "issue_opened", "scheduleKind": "weekdays",
        "minute": 0, "time": "09:00", "dayOfWeek": 1,
        "repos": [], "repo": "", "branch": "", "actor": "anyone",
    }])
}

fn templates() -> Vec<AutomationTemplate> {
    vec![AutomationTemplate {
        id: "find-critical-bugs".into(),
        category: TemplateCategory::Review,
        popular: true,
        icon: TemplateIcon::Alert,
        name: "Find critical bugs".into(),
        description: "Analyze recent commits".into(),
        prompt: "Review recent git history.".into(),
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Time,
            event: "weekdays".into(),
            schedule_kind: None,
            time: Some("09:00".into()),
            day_of_week: None,
            minute: None,
        },
        trigger_label: "Weekdays at 09:00".into(),
    }]
}

struct Page<'a> {
    view: Entity<AutomationsView>,
    data: LocalAutomations,
    cx: &'a mut VisualTestContext,
}

fn render(cx: &mut TestAppContext, automations: Vec<Automation>) -> Page<'_> {
    let slot: Rc<RefCell<Option<LocalAutomations>>> = Rc::default();
    let built = slot.clone();
    let (view, cx) = mount(cx, move |window, cx| {
        let data = LocalAutomations::new(automations, templates(), vec!["/work/app".into()], cx);
        *built.borrow_mut() = Some(data.clone());
        let projects = Rc::new(StaticProjects::new(["/work/app"]));
        cx.new(|cx| AutomationsView::new(Rc::new(data), projects, Some("/work/app"), window, cx))
    });
    let data = slot.borrow().clone().unwrap();
    Page { view, data, cx }
}

fn editor(page: &mut Page) -> Entity<AutomationEditor> {
    page.view
        .read_with(page.cx, |view, _| view.editor().cloned())
        .expect("an editor")
}

#[gpui::test]
fn opens_on_the_template_picker_and_lists_the_cards(cx: &mut TestAppContext) {
    let page = render(
        cx,
        vec![
            automation("a1", "Nightly review", weekdays(), true),
            automation("a2", "Triage issues", issues(), false),
        ],
    );
    assert!(
        page.view
            .read_with(page.cx, |view, _| view.is_picker_open())
    );
    assert!(exists(page.cx, "template Start from scratch"));
    assert!(exists(page.cx, "template Find critical bugs"));
    assert!(exists(page.cx, "automation-card Nightly review"));
    assert!(exists(page.cx, "automation-card Triage issues"));
    click(page.cx, "template-category Security");
    assert!(!exists(page.cx, "template Find critical bugs"));
    assert_eq!(
        page.view.read_with(page.cx, |view, _| view.category()),
        TemplateCategory::Security
    );
}

#[gpui::test]
fn the_filter_matches_name_prompt_and_project(cx: &mut TestAppContext) {
    let page = render(
        cx,
        vec![
            automation("a1", "Nightly review", weekdays(), true),
            automation("a2", "Triage issues", issues(), false),
        ],
    );
    click(page.cx, "automation-card Nightly review");
    let query = page
        .view
        .read_with(page.cx, |view, _| view.query_input().clone());
    page.cx
        .update(|window, cx| query.update(cx, |input, cx| input.focus(window, cx)));
    type_text(page.cx, "triage");
    let visible: Vec<String> = page.cx.update(|_, cx| {
        page.view
            .read(cx)
            .visible(cx)
            .into_iter()
            .map(|automation| automation.name)
            .collect()
    });
    assert_eq!(visible, vec!["Triage issues"]);
}

#[gpui::test]
fn editing_an_automation_enables_save_and_saves_the_draft(cx: &mut TestAppContext) {
    let mut page = render(
        cx,
        vec![automation("a1", "Nightly review", weekdays(), true)],
    );
    click(page.cx, "automation-card Nightly review");
    let editor = editor(&mut page);
    assert!(!editor.read_with(page.cx, |editor, _| editor.can_submit()));
    assert!(!exists(page.cx, "automation-close"));
    click(page.cx, "Automation name");
    type_text(page.cx, " v2");
    assert!(editor.read_with(page.cx, |editor, _| editor.can_submit()));
    assert!(exists(page.cx, "automation-close"));
    click(page.cx, "automation-submit");
    let saved = page
        .cx
        .update(|_, cx| page.data.state().read(cx).saved.clone());
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].name, "Nightly review v2");
    assert_eq!(saved[0].id.as_deref(), Some("a1"));
    assert!(
        page.view
            .read_with(page.cx, |view, _| view.draft().is_none())
    );
}

#[gpui::test]
fn reset_drops_unsaved_edits(cx: &mut TestAppContext) {
    let mut page = render(
        cx,
        vec![automation("a1", "Nightly review", weekdays(), true)],
    );
    click(page.cx, "automation-card Nightly review");
    click(page.cx, "Automation name");
    type_text(page.cx, " changed");
    assert!(
        page.view
            .read_with(page.cx, |view, _| view.draft().is_some())
    );
    click(page.cx, "automation-close");
    assert!(
        page.view
            .read_with(page.cx, |view, _| view.draft().is_none())
    );
    let editor = editor(&mut page);
    let name = editor.read_with(page.cx, |editor, _| editor.draft().name.clone());
    assert_eq!(name, "Nightly review");
}

#[gpui::test]
fn a_template_starts_a_new_draft_and_cancel_returns_to_the_picker(cx: &mut TestAppContext) {
    let mut page = render(cx, Vec::new());
    click(page.cx, "template Find critical bugs");
    let editor = editor(&mut page);
    let draft = editor.read_with(page.cx, |editor, _| editor.draft().clone());
    assert_eq!(draft.name, "Find critical bugs");
    assert_eq!(draft.prompt, "Review recent git history.");
    assert_eq!(draft.cwd, "/work/app");
    assert_eq!(draft.triggers.len(), 1);
    assert!(draft.id.is_none());
    assert!(exists(page.cx, "automation-close"));
    click(page.cx, "automation-close");
    assert!(
        page.view
            .read_with(page.cx, |view, _| view.is_picker_open())
    );
}

#[gpui::test]
fn creating_from_scratch_needs_a_name_and_instructions(cx: &mut TestAppContext) {
    let mut page = render(cx, Vec::new());
    click(page.cx, "template Start from scratch");
    let editor = editor(&mut page);
    assert!(!editor.read_with(page.cx, |editor, _| editor.can_submit()));
    click(page.cx, "Automation name");
    type_text(page.cx, "Nightly");
    assert!(!editor.read_with(page.cx, |editor, _| editor.can_submit()));
    let prompt = editor.read_with(page.cx, |editor, _| editor.prompt_field().clone());
    let input = prompt.read_with(page.cx, |field, _| field.input().clone());
    page.cx
        .update(|window, cx| input.update(cx, |input, cx| input.focus(window, cx)));
    type_text(page.cx, "Run the tests");
    assert!(editor.read_with(page.cx, |editor, _| editor.can_submit()));
    click(page.cx, "automation-submit");
    let automations = page
        .cx
        .update(|_, cx| page.data.state().read(cx).automations.clone());
    assert_eq!(automations.len(), 1);
    assert_eq!(automations[0].name, "Nightly");
    assert_eq!(automations[0].prompt, "Run the tests");
}

#[gpui::test]
fn adds_an_event_trigger_from_the_menu(cx: &mut TestAppContext) {
    let mut page = render(
        cx,
        vec![automation("a1", "Nightly review", weekdays(), true)],
    );
    click(page.cx, "automation-card Nightly review");
    click(page.cx, "Add Trigger");
    let editor = editor(&mut page);
    assert!(editor.read_with(page.cx, |editor, _| editor.is_trigger_menu_open()));
    // Jira is not connected: hovering it opens nothing.
    hover(page.cx, "trigger-category Jira");
    assert_eq!(
        editor.read_with(page.cx, |editor, _| editor.trigger_category()),
        None
    );
    hover(page.cx, "trigger-category GitHub");
    assert_eq!(
        editor.read_with(page.cx, |editor, _| editor.trigger_category()),
        Some(AutomationTriggerKind::Github)
    );
    click(page.cx, "trigger-event Pull request opened");
    let triggers = editor.read_with(page.cx, |editor, _| editor.draft().triggers.clone());
    assert_eq!(triggers.len(), 2);
    assert_eq!(triggers[1].kind, AutomationTriggerKind::Github);
    assert_eq!(triggers[1].event, "pull_request_opened");
    assert!(!editor.read_with(page.cx, |editor, _| editor.is_trigger_menu_open()));
    // The first time trigger still drives the legacy fields.
    let kind = editor.read_with(page.cx, |editor, _| editor.draft().trigger_kind);
    assert_eq!(kind, AutomationTriggerKind::Time);
}

#[gpui::test]
fn removes_a_trigger(cx: &mut TestAppContext) {
    let mut page = render(
        cx,
        vec![automation("a1", "Nightly review", weekdays(), true)],
    );
    click(page.cx, "automation-card Nightly review");
    click(page.cx, "remove-trigger t1");
    let editor = editor(&mut page);
    assert!(editor.read_with(page.cx, |editor, _| editor.draft().triggers.is_empty()));
    assert!(
        page.view
            .read_with(page.cx, |view, _| view.draft().is_some())
    );
}

#[gpui::test]
fn the_card_switch_pauses_an_automation(cx: &mut TestAppContext) {
    let page = render(
        cx,
        vec![automation("a1", "Nightly review", weekdays(), true)],
    );
    click(page.cx, "automation-toggle Nightly review");
    let enabled = page
        .cx
        .update(|_, cx| page.data.state().read(cx).automations[0].enabled);
    assert!(!enabled);
    // The switch does not open the card.
    assert!(
        page.view
            .read_with(page.cx, |view, _| view.is_picker_open())
    );
}

#[gpui::test]
fn deletes_through_the_actions_menu_after_confirming(cx: &mut TestAppContext) {
    let page = render(
        cx,
        vec![automation("a1", "Nightly review", weekdays(), true)],
    );
    click(page.cx, "automation-card Nightly review");
    click(page.cx, "Automation actions");
    assert!(exists(page.cx, "Delete automation"));
    click(page.cx, "Delete automation");
    let deleted = page
        .cx
        .update(|_, cx| page.data.state().read(cx).deleted.clone());
    assert_eq!(deleted, vec!["a1".to_string()]);
}

#[gpui::test]
fn run_now_records_a_manual_run_and_history_opens_its_session(cx: &mut TestAppContext) {
    let mut page = render(
        cx,
        vec![automation("a1", "Nightly review", weekdays(), true)],
    );
    let run: AutomationRun = serde_json::from_value(serde_json::json!({
        "id": "r1", "automationId": "a1", "trigger": "scheduled", "scheduledFor": 5,
        "createdAt": 5, "startedAt": 5, "completedAt": 65_005, "status": "succeeded",
        "sessionId": "s1",
    }))
    .unwrap();
    let data = page.data.clone();
    page.cx.update(|_, cx| data.set_runs(vec![run], cx));
    click(page.cx, "automation-card Nightly review");
    click(page.cx, "automation-run");
    let ran = page
        .cx
        .update(|_, cx| page.data.state().read(cx).ran.clone());
    assert_eq!(ran, vec!["a1".to_string()]);
    click(page.cx, "automation-tab-history");
    let editor = editor(&mut page);
    assert_eq!(
        editor.read_with(page.cx, |editor, _| editor.tab()),
        EditorTab::History
    );
    assert!(exists(page.cx, "run r1"));
    click(page.cx, "run r1");
    let opened = page
        .cx
        .update(|_, cx| page.data.state().read(cx).opened_sessions.clone());
    assert_eq!(opened, vec!["s1".to_string()]);
    draw(page.cx);
}
