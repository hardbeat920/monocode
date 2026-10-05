//! Ports of SkillsPage.test.ts. Focus restoration to the exact opening row
//! button and the Settings page's own Escape handling are not modeled: the
//! rows are not focus stops, and closing the preview focuses the filter.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use futures::channel::oneshot;
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable as _, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, Styled as _, Subscription, Task,
    TestAppContext, VisualTestContext, Window, div,
};

use super::{DiscoveredSkill, LocalSkills, SkillsData, SkillsPage};
use crate::data::{DataTask, Listener};
use crate::test_support::{Calls, click, draw, exists, keys, mount, type_text};
use crate::widgets::MarkdownMode;

fn skills() -> Vec<DiscoveredSkill> {
    let skill =
        |name: &str, description: &str, path: &str, scope: &str, source: &str| DiscoveredSkill {
            name: name.into(),
            description: description.into(),
            path: path.into(),
            scope: scope.into(),
            source: source.into(),
        };
    vec![
        skill(
            "Project guide",
            "Project instructions",
            "D:/repo/.agents/skills/guide/SKILL.md",
            "project",
            "agents",
        ),
        skill(
            "Personal guide",
            "Personal instructions",
            "C:/Users/test/.agents/skills/guide/SKILL.md",
            "user",
            "agents",
        ),
        skill(
            "Other guide",
            "Harness instructions",
            "C:/Users/test/.claude/skills/guide/SKILL.md",
            "user",
            "claude",
        ),
    ]
}

const MARKDOWN: &str =
    "---\nname: guide\n---\n\n# Full instructions\n\nRead **everything**.\n\nLast paragraph.\n";

struct Page<'a> {
    view: Entity<SkillsPage>,
    data: LocalSkills,
    cx: &'a mut VisualTestContext,
}

fn render(cx: &mut TestAppContext) -> Page<'_> {
    let slot: Rc<RefCell<Option<LocalSkills>>> = Rc::default();
    let built = slot.clone();
    let (view, cx) = mount(cx, move |window, cx| {
        let data = LocalSkills::new(skills(), cx);
        for skill in skills() {
            data.set_file(&skill.path, MARKDOWN, cx);
        }
        *built.borrow_mut() = Some(data.clone());
        cx.new(|cx| SkillsPage::new(Rc::new(data), "D:/repo", window, cx))
    });
    let data = slot.borrow().clone().unwrap();
    Page { view, data, cx }
}

fn preview_text(page: &mut Page) -> Option<String> {
    page.view
        .read_with(page.cx, |view, _| view.preview_text().map(str::to_string))
}

fn preview_name(page: &mut Page) -> Option<String> {
    page.view.read_with(page.cx, |view, _| {
        view.preview().map(|skill| skill.name.clone())
    })
}

#[gpui::test]
fn opens_an_inline_panel_from_the_eye_icon_and_switches_skills(cx: &mut TestAppContext) {
    let mut page = render(cx);
    assert!(exists(page.cx, "skills-count"));
    click(page.cx, "Preview skill Project guide");
    assert!(exists(page.cx, "skill-preview"));
    assert_eq!(preview_name(&mut page).as_deref(), Some("Project guide"));
    click(page.cx, "skill-name Other guide");
    assert!(exists(page.cx, "skill-preview"));
    assert_eq!(preview_name(&mut page).as_deref(), Some("Other guide"));
    assert_eq!(preview_text(&mut page).as_deref(), Some(MARKDOWN));
}

#[gpui::test]
fn opens_a_disabled_personal_skill_and_shows_the_source(cx: &mut TestAppContext) {
    let mut page = render(cx);
    let data = page.data.clone();
    let path = skills()[1].path.clone();
    page.cx
        .update(|_, cx| data.save_disabled_paths(vec![path.clone()], cx).unwrap());
    draw(page.cx);
    click(page.cx, "skill-name Personal guide");
    assert_eq!(preview_text(&mut page).as_deref(), Some(MARKDOWN));
    assert!(exists(page.cx, "skill-preview-document"));
    click(page.cx, "markdown-mode-Source");
    assert!(exists(page.cx, "markdown-source"));
    let mode = page
        .view
        .read_with(page.cx, |view, cx| view.preview_mode(cx));
    assert_eq!(mode, MarkdownMode::Source);
    // Opening the preview leaves the preference alone.
    assert_eq!(
        page.cx.update(|_, cx| data.disabled_paths(cx)),
        vec![path.clone()]
    );
    assert!(
        page.view
            .read_with(page.cx, |view, _| view.is_disabled(&path))
    );
    assert!(exists(page.cx, "Copy path of Personal guide"));
    assert!(exists(page.cx, "Reveal Personal guide in file explorer"));
    click(page.cx, "markdown-mode-Preview");
}

#[gpui::test]
fn the_switch_hides_a_skill_from_the_catalog(cx: &mut TestAppContext) {
    let page = render(cx);
    let path = skills()[0].path.clone();
    click(page.cx, "Include Project guide in MonoCode catalog");
    let data = page.data.clone();
    assert_eq!(
        page.cx.update(|_, cx| data.disabled_paths(cx)),
        vec![path.clone()]
    );
    click(page.cx, "Include Project guide in MonoCode catalog");
    assert!(page.cx.update(|_, cx| data.disabled_paths(cx)).is_empty());
}

#[gpui::test]
fn closes_with_the_button_or_escape_and_keeps_the_filter(cx: &mut TestAppContext) {
    let mut page = render(cx);
    click(page.cx, "Filter skills");
    type_text(page.cx, "project");
    let count = page
        .view
        .read_with(page.cx, |view, _| view.filtered().len());
    assert_eq!(count, 1);
    click(page.cx, "skill-name Project guide");
    click(page.cx, "Close skill preview");
    assert!(preview_name(&mut page).is_none());
    click(page.cx, "skill-name Project guide");
    keys(page.cx, "escape");
    assert!(preview_name(&mut page).is_none());
    let query = page.view.read_with(page.cx, |view, cx| {
        view.filter_input().read(cx).value().to_string()
    });
    assert_eq!(query, "project");
    // The filter has focus back.
    let focused = page.cx.update(|window, cx| {
        let input = page.view.read(cx).filter_input().clone();
        input.focus_handle(cx).is_focused(window)
    });
    assert!(focused);
}

/// Escape closes the preview before anything around the page sees it.
#[gpui::test]
fn escape_closes_the_preview_before_the_settings_page(cx: &mut TestAppContext) {
    struct Settings {
        page: Entity<SkillsPage>,
        focus: FocusHandle,
        closes: Rc<dyn Fn()>,
    }
    impl Render for Settings {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let closes = self.closes.clone();
            div()
                .size_full()
                .track_focus(&self.focus)
                .on_key_down(cx.listener(move |_, event: &KeyDownEvent, _, _| {
                    if event.keystroke.key == "escape" {
                        closes();
                    }
                }))
                .child(self.page.clone())
        }
    }
    let closes = Calls::<()>::new();
    let record = closes.recorder();
    let page_slot: Rc<RefCell<Option<Entity<SkillsPage>>>> = Rc::default();
    let built = page_slot.clone();
    let (_, cx) = mount(cx, move |window, cx| {
        let data = LocalSkills::new(skills(), cx);
        for skill in skills() {
            data.set_file(&skill.path, MARKDOWN, cx);
        }
        let page = cx.new(|cx| SkillsPage::new(Rc::new(data), "D:/repo", window, cx));
        *built.borrow_mut() = Some(page.clone());
        cx.new(|cx| Settings {
            page,
            focus: cx.focus_handle(),
            closes: Rc::new(move || record(())),
        })
    });
    let page = page_slot.borrow().clone().unwrap();
    click(cx, "skill-name Project guide");
    keys(cx, "escape");
    assert!(page.read_with(cx, |page, _| page.preview().is_none()));
    assert_eq!(closes.len(), 0);
    keys(cx, "escape");
    assert_eq!(closes.len(), 1);
}

#[gpui::test]
fn add_skill_creates_a_starter_skill_and_rescans(cx: &mut TestAppContext) {
    let page = render(cx);
    click(page.cx, "Add skill");
    assert!(page.view.read_with(page.cx, |view, _| view.is_adding()));
    type_text(page.cx, "release-notes");
    let form = page
        .view
        .read_with(page.cx, |view, _| view.form().cloned().unwrap());
    page.cx
        .update(|window, cx| form.update(cx, |form, cx| form.submit(window, cx)));
    draw(page.cx);
    let data = page.data.clone();
    let created = page
        .cx
        .update(|_, cx| data.state().read(cx).created.clone());
    assert_eq!(
        created,
        vec![("D:/repo".to_string(), "release-notes".to_string(), true)]
    );
    assert!(!page.view.read_with(page.cx, |view, _| view.is_adding()));
    let names: Vec<String> = page.view.read_with(page.cx, |view, _| {
        view.skills()
            .unwrap()
            .iter()
            .map(|skill| skill.name.clone())
            .collect()
    });
    assert!(names.contains(&"release-notes".to_string()));
}

type ReadSender = oneshot::Sender<Result<String, String>>;

/// Reads that land only when the test answers them.
#[derive(Clone, Default)]
struct HeldReads {
    pending: Rc<RefCell<HashMap<String, ReadSender>>>,
}

impl HeldReads {
    fn answer(&self, path: &str, result: Result<String, String>) {
        if let Some(sender) = self.pending.borrow_mut().remove(path) {
            sender.send(result).ok();
        }
    }
}

impl SkillsData for HeldReads {
    fn list_skills(&self, _: &str, _: &mut App) -> DataTask<Vec<DiscoveredSkill>> {
        Task::ready(Ok(skills()))
    }

    fn read_text_file(&self, path: &str, cx: &mut App) -> DataTask<String> {
        let (sender, receiver) = oneshot::channel();
        self.pending.borrow_mut().insert(path.into(), sender);
        cx.background_spawn(async move { receiver.await.unwrap_or_else(|_| Err("dropped".into())) })
    }

    fn disabled_paths(&self, _: &App) -> Vec<String> {
        Vec::new()
    }

    fn save_disabled_paths(&self, _: Vec<String>, _: &mut App) -> Result<(), String> {
        Ok(())
    }

    fn subscribe(&self, _: Listener, _: &mut App) -> Subscription {
        Subscription::new(|| {})
    }

    fn invalidate(&self, _: &mut App) {}

    fn create_blank_skill(&self, _: &str, _: &str, _: bool, _: &mut App) -> DataTask<()> {
        Task::ready(Ok(()))
    }

    fn reveal(&self, _: &str, _: &mut App) -> DataTask<()> {
        Task::ready(Ok(()))
    }
}

fn render_held(cx: &mut TestAppContext) -> (Entity<SkillsPage>, HeldReads, &mut VisualTestContext) {
    let held = HeldReads::default();
    let data = held.clone();
    let (view, cx) = mount(cx, move |window, cx| {
        cx.new(|cx| SkillsPage::new(Rc::new(data), "D:/repo", window, cx))
    });
    (view, held, cx)
}

#[gpui::test]
fn never_shows_the_previous_document_while_a_new_read_is_pending(cx: &mut TestAppContext) {
    let (view, held, cx) = render_held(cx);
    click(cx, "skill-name Project guide");
    held.answer(&skills()[0].path, Ok("# Previous document".into()));
    draw(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.preview_text().map(str::to_string))
            .as_deref(),
        Some("# Previous document")
    );
    click(cx, "skill-name Other guide");
    assert!(exists(cx, "skill-preview-loading"));
    assert!(view.read_with(cx, |view, _| view.preview_text().is_none()));
    held.answer(&skills()[2].path, Ok("# Selected document".into()));
    draw(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.preview_text().map(str::to_string))
            .as_deref(),
        Some("# Selected document")
    );
}

#[gpui::test]
fn shows_a_readable_error_and_can_open_another_skill(cx: &mut TestAppContext) {
    let (view, held, cx) = render_held(cx);
    click(cx, "skill-name Project guide");
    held.answer(&skills()[0].path, Err("Permission denied".into()));
    draw(cx);
    assert!(exists(cx, "skill-preview-error"));
    assert_eq!(
        view.read_with(cx, |view, _| view.preview_error().map(str::to_string))
            .as_deref(),
        Some("Could not read SKILL.md. Permission denied")
    );
    click(cx, "Close skill preview");
    click(cx, "skill-name Other guide");
    held.answer(&skills()[2].path, Ok(MARKDOWN.into()));
    draw(cx);
    assert!(!exists(cx, "skill-preview-error"));
    assert!(view.read_with(cx, |view, _| view.preview_error().is_none()));
}

#[gpui::test]
fn keeps_the_current_skill_when_a_previous_read_lands_late(cx: &mut TestAppContext) {
    for outcome in [
        Ok("# Old instructions".to_string()),
        Err("Old read failed".to_string()),
    ] {
        let mut test = TestAppContext::single();
        let (view, held, cx) = render_held(&mut test);
        click(cx, "skill-name Project guide");
        assert!(exists(cx, "skill-preview-loading"));
        click(cx, "skill-name Other guide");
        held.answer(&skills()[2].path, Ok("# Current instructions".into()));
        draw(cx);
        held.answer(&skills()[0].path, outcome.clone());
        draw(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.preview_text(), Some("# Current instructions"));
            assert!(view.preview_error().is_none());
        });
    }
    let _ = cx;
}
