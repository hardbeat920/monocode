//! View tests for the notes page, ported from NotesView.test.ts. The save
//! model cases (ordering, debounce, queued moves) belong to the engine's
//! `Notes` entity and are ported there; these check what the page draws and
//! which actions it sends, over [`LocalNotes`].

use std::rc::Rc;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};

use super::{LocalNotes, Note, NotesData, NotesView, note_mini_card};
use crate::data::StaticProjects;
use crate::test_support::{Calls, click, draw, exists, keys, mount, type_text};

fn stored() -> Note {
    Note {
        id: "note-project-test".into(),
        slug: "plan".into(),
        title: "Plan".into(),
        body: "Keep this text.".into(),
        tags: vec!["ideas".into()],
        source_session_id: Some("original-session".into()),
        source_cwd: Some("/work/Edefyn".into()),
        created_at: 1,
        updated_at: 1,
    }
}

fn second() -> Note {
    Note {
        id: "second-note".into(),
        slug: "second".into(),
        title: "Second".into(),
        updated_at: 0,
        ..stored()
    }
}

struct Page<'a> {
    view: Entity<NotesView>,
    data: LocalNotes,
    closes: Calls<()>,
    cx: &'a mut VisualTestContext,
}

fn render(cx: &mut TestAppContext, notes: Vec<Note>) -> Page<'_> {
    let closes = Calls::<()>::new();
    let record = closes.recorder();
    let data_slot: Rc<std::cell::RefCell<Option<LocalNotes>>> = Rc::default();
    let built = data_slot.clone();
    let (view, cx) = mount(cx, move |window, cx| {
        let data = LocalNotes::new(notes, cx);
        *built.borrow_mut() = Some(data.clone());
        let projects = Rc::new(StaticProjects::new(["/work/Edefyn", "/work/portognjeeen"]));
        cx.new(|cx| {
            NotesView::new(data.rc(), projects, Some("/work/Edefyn"), window, cx)
                .on_close(move |_, _| record(()))
        })
    });
    let data = data_slot.borrow().clone().unwrap();
    Page {
        view,
        data,
        closes,
        cx,
    }
}

fn title_value(page: &mut Page) -> String {
    page.view.read_with(page.cx, |view, cx| {
        view.title_input().read(cx).value().to_string()
    })
}

fn picker_label(page: &mut Page) -> String {
    page.view.read_with(page.cx, |view, cx| {
        let picker = view.picker().read(cx);
        picker.trigger_label(cx)
    })
}

fn choose_project(page: &mut Page, path: &str) {
    let label = picker_label(page);
    click(page.cx, &format!("project-picker-trigger {label}"));
    assert!(exists(page.cx, "project-picker"));
    click(page.cx, &format!("project-picker-row {path}"));
}

#[gpui::test]
fn refreshes_an_open_note_after_an_operator_write(cx: &mut TestAppContext) {
    let mut page = render(cx, vec![stored()]);
    assert_eq!(title_value(&mut page), "Plan");
    let data = page.data.clone();
    page.cx.update(|_, cx| {
        data.write_from_outside(
            Note {
                title: "Updated by Operator".into(),
                body: "New text".into(),
                updated_at: 2,
                ..stored()
            },
            cx,
        )
    });
    draw(page.cx);
    assert_eq!(title_value(&mut page), "Updated by Operator");
    let preview = page.view.read_with(page.cx, |view, _| {
        view.page()
            .editor
            .as_ref()
            .map(|editor| editor.body.clone())
    });
    assert_eq!(preview.as_deref(), Some("New text"));
}

#[gpui::test]
fn moves_the_existing_note_and_keeps_its_content(cx: &mut TestAppContext) {
    let mut page = render(cx, vec![stored()]);
    assert!(picker_label(&mut page).contains("Move note to project, current project Edefyn"));
    choose_project(&mut page, "/work/portognjeeen");
    let saved = page
        .cx
        .update(|_, cx| page.data.note("note-project-test", cx))
        .unwrap();
    assert_eq!(saved.source_cwd.as_deref(), Some("/work/portognjeeen"));
    assert_eq!(saved.title, "Plan");
    assert_eq!(saved.body, "Keep this text.");
    assert_eq!(saved.tags, vec!["ideas".to_string()]);
    assert_eq!(saved.source_session_id.as_deref(), Some("original-session"));
    assert!(picker_label(&mut page).contains("portognjeeen"));
    assert!(page.closes.all().is_empty());
}

#[gpui::test]
fn closes_the_project_menu_with_escape_without_leaving_notes(cx: &mut TestAppContext) {
    let mut page = render(cx, vec![stored()]);
    let label = picker_label(&mut page);
    click(page.cx, &format!("project-picker-trigger {label}"));
    assert!(exists(page.cx, "project-picker"));
    keys(page.cx, "escape");
    assert!(!exists(page.cx, "project-picker"));
    assert!(page.closes.all().is_empty());
    // A second Escape leaves the page.
    keys(page.cx, "escape");
    assert_eq!(page.closes.len(), 1);
}

#[gpui::test]
fn lets_the_user_retry_a_project_change_after_saving_fails(cx: &mut TestAppContext) {
    let mut page = render(cx, vec![stored()]);
    let data = page.data.clone();
    page.cx.update(|_, cx| data.fail_next_save("Disk full", cx));
    choose_project(&mut page, "/work/portognjeeen");
    let saved = page
        .cx
        .update(|_, cx| data.note("note-project-test", cx))
        .unwrap();
    assert_eq!(saved.source_cwd.as_deref(), Some("/work/Edefyn"));
    assert!(exists(page.cx, "note-save-error"));
    let error = page.view.read_with(page.cx, |view, _| {
        view.page()
            .editor
            .as_ref()
            .and_then(|editor| editor.save_error.clone())
    });
    assert_eq!(error.as_deref(), Some("Disk full"));
    click(page.cx, "note-retry");
    assert!(!exists(page.cx, "note-save-error"));
    let saved = page
        .cx
        .update(|_, cx| data.note("note-project-test", cx))
        .unwrap();
    assert_eq!(saved.source_cwd.as_deref(), Some("/work/portognjeeen"));
}

#[gpui::test]
fn clears_a_failed_move_error_when_the_saved_project_is_selected_again(cx: &mut TestAppContext) {
    let mut page = render(cx, vec![stored()]);
    let data = page.data.clone();
    page.cx.update(|_, cx| data.fail_next_save("Disk full", cx));
    choose_project(&mut page, "/work/portognjeeen");
    assert!(exists(page.cx, "note-save-error"));
    choose_project(&mut page, "/work/Edefyn");
    let saved = page
        .cx
        .update(|_, cx| data.note("note-project-test", cx))
        .unwrap();
    assert_eq!(saved.source_cwd.as_deref(), Some("/work/Edefyn"));
    assert!(picker_label(&mut page).contains("Edefyn"));
    assert!(!exists(page.cx, "note-save-error"));
}

#[gpui::test]
fn selecting_a_card_opens_its_note(cx: &mut TestAppContext) {
    let mut page = render(cx, vec![stored(), second()]);
    assert_eq!(title_value(&mut page), "Plan");
    click(page.cx, "note-card Second");
    assert_eq!(title_value(&mut page), "Second");
    click(page.cx, "note-card Plan");
    assert_eq!(title_value(&mut page), "Plan");
}

#[gpui::test]
fn filters_by_title_body_tag_and_project(cx: &mut TestAppContext) {
    let mut page = render(cx, vec![stored(), second()]);
    let visible = |page: &mut Page| {
        page.view.read_with(page.cx, |view, _| {
            view.page()
                .visible
                .iter()
                .map(|note| note.title.clone())
                .collect::<Vec<_>>()
        })
    };
    let data = page.data.clone();
    page.cx.update(|_, cx| data.set_query("#ideas", cx));
    draw(page.cx);
    assert_eq!(visible(&mut page), vec!["Plan", "Second"]);
    page.cx.update(|_, cx| data.set_query("second", cx));
    draw(page.cx);
    assert_eq!(visible(&mut page), vec!["Second"]);
    page.cx
        .update(|_, cx| data.set_query("nothing like this", cx));
    draw(page.cx);
    assert!(visible(&mut page).is_empty());
    // The selected note stays open even when the filter hides it.
    assert_eq!(title_value(&mut page), "Plan");
}

#[gpui::test]
fn typing_a_comma_adds_a_tag_and_the_chip_removes_it(cx: &mut TestAppContext) {
    let mut page = render(cx, vec![stored()]);
    click(page.cx, "note-tag-input");
    type_text(page.cx, "Big Plan,");
    let tags = |page: &mut Page| {
        page.cx
            .update(|_, cx| page.data.note("note-project-test", cx))
            .unwrap()
            .tags
    };
    assert_eq!(
        tags(&mut page),
        vec!["ideas".to_string(), "big-plan".to_string()]
    );
    let input = page.view.read_with(page.cx, |view, cx| {
        view.tag_input().read(cx).value().to_string()
    });
    assert_eq!(input, "");
    click(page.cx, "remove-tag ideas");
    assert_eq!(tags(&mut page), vec!["big-plan".to_string()]);
}

#[gpui::test]
fn add_to_chat_sends_the_note_and_leaves(cx: &mut TestAppContext) {
    let page = render(cx, vec![stored()]);
    click(page.cx, "note-add-to-chat");
    let added = page
        .cx
        .update(|_, cx| page.data.state().read(cx).added_to_chat.clone());
    assert_eq!(added, vec!["note-project-test".to_string()]);
    assert_eq!(page.closes.len(), 1);
}

#[gpui::test]
fn an_empty_list_explains_how_to_add_notes(cx: &mut TestAppContext) {
    let page = render(cx, Vec::new());
    let page_state = page.view.read_with(page.cx, |view, _| view.page().clone());
    assert!(page_state.notes.is_empty());
    assert!(page_state.editor.is_none());
    click(page.cx, "new-note");
    let notes = page
        .view
        .read_with(page.cx, |view, _| view.page().notes.len());
    assert_eq!(notes, 1);
    // A new untitled note opens in the Source tab.
    let mode = page.view.read_with(page.cx, |view, cx| view.mode(cx));
    assert_eq!(mode, crate::widgets::MarkdownMode::Source);
    assert!(exists(page.cx, "note-source"));
}

#[gpui::test]
fn deleting_selects_the_next_note(cx: &mut TestAppContext) {
    let mut page = render(cx, vec![stored(), second()]);
    click(page.cx, "note-delete");
    assert_eq!(title_value(&mut page), "Second");
    let notes = page
        .view
        .read_with(page.cx, |view, _| view.page().notes.len());
    assert_eq!(notes, 1);
}

#[gpui::test]
fn the_source_tab_edits_the_body(cx: &mut TestAppContext) {
    let page = render(cx, vec![stored()]);
    click(page.cx, "note-tab-source");
    assert!(exists(page.cx, "note-source"));
    let source = page
        .view
        .read_with(page.cx, |view, _| view.source_editor().clone());
    page.cx.update(|window, cx| {
        source.update(cx, |field, cx| {
            field.focus(window, cx);
            field.set_selected_range(15..15, cx);
        })
    });
    type_text(page.cx, "\n# Next");
    let body = page
        .cx
        .update(|_, cx| page.data.note("note-project-test", cx))
        .unwrap()
        .body;
    assert_eq!(body, "Keep this text.\n# Next");
}

#[gpui::test]
fn renders_the_note_chip(cx: &mut TestAppContext) {
    use monocode_core::notes::NoteCardMeta;
    let dismissed = Calls::<()>::new();
    let record = dismissed.recorder();
    struct Chip {
        on_dismiss: Rc<dyn Fn()>,
    }
    impl gpui::Render for Chip {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            let dismiss = self.on_dismiss.clone();
            note_mini_card(
                NoteCardMeta {
                    id: "n1".into(),
                    slug: "plan".into(),
                    title: "Plan".into(),
                    source_cwd: Some("/work/Edefyn".into()),
                    extra: Default::default(),
                },
                None,
            )
            .on_dismiss(move |_, _| dismiss())
        }
    }
    let (_, cx) = mount(cx, move |_, cx| {
        cx.new(|_| Chip {
            on_dismiss: Rc::new(move || record(())),
        })
    });
    click(cx, "remove note Plan");
    assert_eq!(dismissed.len(), 1);
}
