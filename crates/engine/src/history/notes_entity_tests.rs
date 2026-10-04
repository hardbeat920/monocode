//! Entity tests for `Notes`: the model cases of
//! src/features/notes/ui/NotesView.test.ts (Operator refresh, project moves,
//! save ordering across reopened editors, retry), plus the notes.ts cache,
//! create, delete, filter, add to chat, and image saves.

use std::cell::RefCell;

use gpui::{AppContext, Entity, TestAppContext};
use monocode_core::{HarnessId, Session};

use super::*;
use crate::history::host::NoHistoryHost;
use crate::history::notes_backend::fake::FakeNotes;
use crate::runtime::testing::init_test_engine;

const MOVED: &str = "/work/portognjeeen";
const ORIGINAL: &str = "/work/Edefyn";

fn stored() -> Note {
    Note {
        id: "note-project-test".into(),
        slug: "plan".into(),
        title: "Plan".into(),
        body: "Keep this text.".into(),
        tags: vec!["ideas".into()],
        source_session_id: Some("original-session".into()),
        source_cwd: Some(ORIGINAL.into()),
        created_at: 1,
        updated_at: 1,
    }
}

fn second() -> Note {
    Note {
        id: "second-note".into(),
        slug: "second".into(),
        title: "Second".into(),
        ..stored()
    }
}

struct T {
    fake: Arc<FakeNotes>,
    notes: Entity<Notes>,
}

fn setup(cx: &mut TestAppContext, notes: Vec<Note>) -> T {
    init_test_engine(cx);
    let fake = FakeNotes::with(notes);
    let backend: Arc<dyn NotesBackend> = fake.clone();
    let notes = cx.new(|cx| Notes::new(backend, cx));
    notes.update(cx, |notes, cx| notes.open_page(cx));
    cx.run_until_parked();
    T { fake, notes }
}

impl T {
    fn editor(&self, cx: &mut TestAppContext) -> NoteEditorView {
        self.notes
            .read_with(cx, |notes, _| notes.editor())
            .expect("an open editor")
    }

    fn select(&self, cx: &mut TestAppContext, id: &str) {
        self.notes.update(cx, |notes, cx| notes.select(id, cx));
        cx.run_until_parked();
    }

    fn upsert_count(&self) -> usize {
        self.fake
            .commands()
            .iter()
            .filter(|c| *c == "notes_upsert")
            .count()
    }
}

fn update<R>(
    t: &T,
    cx: &mut TestAppContext,
    f: impl FnOnce(&mut Notes, &mut Context<Notes>) -> R,
) -> R {
    t.notes.update(cx, f)
}

#[gpui::test]
fn matches_intl_saved_note_ties_without_changing_recency(cx: &mut TestAppContext) {
    let mut fixtures: Vec<_> = ["filez", "fileé", "filee", "file.a", "file-a", "file_a"]
        .into_iter()
        .map(|id| Note {
            id: id.into(),
            ..stored()
        })
        .collect();
    fixtures.push(Note {
        id: "newest".into(),
        updated_at: 2,
        ..stored()
    });
    let t = setup(cx, fixtures);
    for locale in ["en", "fr", "ja", "ar"] {
        monocode_locale::with_locale(locale, || {
            let saved = t.fake.note("filee");
            update(&t, cx, |notes, cx| notes.on_saved(&saved, cx));
            let ids = t.notes.read_with(cx, |notes, _| {
                notes
                    .notes()
                    .iter()
                    .map(|note| note.id.clone())
                    .collect::<Vec<_>>()
            });
            assert_eq!(
                ids,
                [
                    "newest", "file_a", "file-a", "file.a", "filee", "fileé", "filez"
                ]
            );
        })
        .unwrap();
    }
}

#[gpui::test]
fn refreshes_an_open_note_after_an_operator_write(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    t.fake.set_note(Note {
        title: "Updated by Operator".into(),
        body: "New text".into(),
        updated_at: 2,
        ..stored()
    });
    update(&t, cx, |notes, cx| notes.notes_changed(cx));
    cx.run_until_parked();
    let editor = t.editor(cx);
    assert_eq!(editor.title, "Updated by Operator");
    assert_eq!(editor.body, "New text");
}

#[gpui::test]
fn moves_the_existing_note_and_keeps_its_content_when_reopened(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    update(&t, cx, |notes, cx| notes.choose_project(MOVED, cx));
    cx.run_until_parked();
    let saved = t.fake.note("note-project-test");
    assert_eq!(
        serde_json::to_value(&saved).unwrap(),
        serde_json::to_value(Note {
            source_cwd: Some(MOVED.into()),
            updated_at: saved.updated_at,
            ..stored()
        })
        .unwrap()
    );
    assert_eq!(t.editor(cx).source_cwd.as_deref(), Some(MOVED));
    let listed = t
        .notes
        .read_with(cx, |notes, _| notes.notes()[0].source_cwd.clone());
    assert_eq!(listed.as_deref(), Some(MOVED));
    update(&t, cx, |notes, cx| notes.close_page(cx));
    update(&t, cx, |notes, cx| notes.open_page(cx));
    cx.run_until_parked();
    assert_eq!(t.editor(cx).source_cwd.as_deref(), Some(MOVED));
}

#[gpui::test]
fn serializes_title_edits_behind_an_in_flight_project_change(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    let moving = t.fake.hold_next("notes_upsert");
    update(&t, cx, |notes, cx| notes.choose_project(MOVED, cx));
    cx.run_until_parked();
    update(&t, cx, |notes, cx| {
        notes.edit_title("Updated plan", cx);
        notes.commit_title(cx);
    });
    cx.run_until_parked();
    assert_eq!(t.upsert_count(), 1);
    moving.release();
    cx.run_until_parked();
    assert_eq!(t.upsert_count(), 2);
    let saved = t.fake.note("note-project-test");
    assert_eq!(saved.title, "Updated plan");
    assert_eq!(saved.source_cwd.as_deref(), Some(MOVED));
}

#[gpui::test]
fn keeps_newer_edits_after_reopening_a_note_during_a_project_move(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored(), second()]);
    let moving = t.fake.hold_next("notes_upsert");
    update(&t, cx, |notes, cx| notes.choose_project(MOVED, cx));
    t.select(cx, "second-note");
    t.select(cx, "note-project-test");
    update(&t, cx, |notes, cx| {
        notes.edit_title("Updated after reopening", cx);
        notes.commit_title(cx);
    });
    cx.run_until_parked();
    assert_eq!(t.upsert_count(), 1);
    moving.release();
    cx.run_until_parked();
    let saved = t.fake.note("note-project-test");
    assert_eq!(saved.title, "Updated after reopening");
    assert_eq!(saved.source_cwd.as_deref(), Some(MOVED));
    t.select(cx, "second-note");
    t.select(cx, "note-project-test");
    let editor = t.editor(cx);
    assert_eq!(editor.title, "Updated after reopening");
    assert_eq!(editor.source_cwd.as_deref(), Some(MOVED));
}

#[gpui::test]
fn preserves_earlier_edits_when_changing_a_field_after_reopening_during_a_move(
    cx: &mut TestAppContext,
) {
    for field in ["title", "body", "tags"] {
        let t = setup(cx, vec![stored(), second()]);
        update(&t, cx, |notes, cx| {
            notes.edit_title("Title before moving", cx);
            notes.edit_body("Content before moving", cx);
            notes.add_tag("before", cx);
        });
        let moving = t.fake.hold_next("notes_upsert");
        update(&t, cx, |notes, cx| notes.choose_project(MOVED, cx));
        t.select(cx, "second-note");
        t.select(cx, "note-project-test");
        update(&t, cx, |notes, cx| match field {
            "title" => notes.edit_title("Title after reopening", cx),
            "body" => notes.edit_body("Content after reopening", cx),
            _ => notes.add_tag("after", cx),
        });
        cx.executor().advance_clock(NOTE_SAVE_DEBOUNCE);
        cx.run_until_parked();
        // Cover both an open editor and saves finishing after it unmounts.
        if field != "title" {
            t.select(cx, "second-note");
        }
        moving.release();
        cx.run_until_parked();

        let saved = t.fake.note("note-project-test");
        let title = if field == "title" {
            "Title after reopening"
        } else {
            "Title before moving"
        };
        let body = if field == "body" {
            "Content after reopening"
        } else {
            "Content before moving"
        };
        let tags = if field == "tags" {
            vec!["ideas".to_string(), "after".to_string()]
        } else {
            vec!["ideas".to_string(), "before".to_string()]
        };
        assert_eq!(saved.title, title, "{field}");
        assert_eq!(saved.body, body, "{field}");
        assert_eq!(saved.tags, tags, "{field}");
        assert_eq!(saved.source_cwd.as_deref(), Some(MOVED), "{field}");
        t.select(cx, "note-project-test");
        let editor = t.editor(cx);
        assert_eq!(editor.title, title);
        assert_eq!(editor.body, body);
        assert_eq!(editor.tags, tags);
        assert_eq!(editor.source_cwd.as_deref(), Some(MOVED));
    }
}

#[gpui::test]
fn lets_the_user_retry_a_project_change_after_saving_fails(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    t.fake.fail_next("notes_upsert", "Disk full");
    update(&t, cx, |notes, cx| notes.choose_project(MOVED, cx));
    cx.run_until_parked();
    assert_eq!(
        t.fake.note("note-project-test").source_cwd.as_deref(),
        Some(ORIGINAL)
    );
    assert_eq!(t.editor(cx).save_error.as_deref(), Some("Disk full"));

    let retrying = t.fake.hold_next("notes_upsert");
    t.fake.fail_next("notes_upsert", "Still no space");
    update(&t, cx, |notes, cx| notes.retry_save(cx));
    cx.run_until_parked();
    assert_eq!(t.editor(cx).save_error, None);
    assert_eq!(
        t.fake.note("note-project-test").source_cwd.as_deref(),
        Some(ORIGINAL)
    );
    retrying.release();
    cx.run_until_parked();
    assert_eq!(t.editor(cx).save_error.as_deref(), Some("Still no space"));

    update(&t, cx, |notes, cx| notes.retry_save(cx));
    cx.run_until_parked();
    assert_eq!(
        t.fake.note("note-project-test").source_cwd.as_deref(),
        Some(MOVED)
    );
    assert_eq!(t.editor(cx).save_error, None);
}

#[gpui::test]
fn keeps_a_completed_move_after_returning_to_the_note_while_it_saves(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored(), second()]);
    let moving = t.fake.hold_next("notes_upsert");
    update(&t, cx, |notes, cx| notes.choose_project(MOVED, cx));
    t.select(cx, "second-note");
    t.select(cx, "note-project-test");
    moving.release();
    cx.run_until_parked();
    assert_eq!(
        t.fake.note("note-project-test").source_cwd.as_deref(),
        Some(MOVED)
    );
    assert_eq!(t.editor(cx).source_cwd.as_deref(), Some(MOVED));
    t.select(cx, "second-note");
    assert_eq!(
        t.fake.note("note-project-test").source_cwd.as_deref(),
        Some(MOVED)
    );
}

#[gpui::test]
fn clears_a_failed_move_error_when_the_saved_project_is_selected_again(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    t.fake.fail_next("notes_upsert", "Disk full");
    update(&t, cx, |notes, cx| notes.choose_project(MOVED, cx));
    cx.run_until_parked();
    assert_eq!(t.editor(cx).save_error.as_deref(), Some("Disk full"));
    update(&t, cx, |notes, cx| notes.choose_project(ORIGINAL, cx));
    cx.run_until_parked();
    assert_eq!(
        t.fake.note("note-project-test").source_cwd.as_deref(),
        Some(ORIGINAL)
    );
    let editor = t.editor(cx);
    assert_eq!(editor.source_cwd.as_deref(), Some(ORIGINAL));
    assert_eq!(editor.save_error, None);
}

#[gpui::test]
fn autosaves_after_typing_stops(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    update(&t, cx, |notes, cx| notes.edit_body("Draft", cx));
    cx.executor().advance_clock(NOTE_SAVE_DEBOUNCE / 2);
    update(&t, cx, |notes, cx| notes.edit_body("Draft two", cx));
    cx.executor().advance_clock(NOTE_SAVE_DEBOUNCE / 2);
    cx.run_until_parked();
    assert_eq!(t.upsert_count(), 0);
    cx.executor().advance_clock(NOTE_SAVE_DEBOUNCE);
    cx.run_until_parked();
    assert_eq!(t.upsert_count(), 1);
    assert_eq!(t.fake.note("note-project-test").body, "Draft two");
    // An unchanged save sends nothing.
    update(&t, cx, |notes, cx| notes.commit_title(cx));
    cx.run_until_parked();
    assert_eq!(t.upsert_count(), 1);
}

#[gpui::test]
fn fills_a_blank_title_from_the_body(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    update(&t, cx, |notes, cx| {
        notes.edit_body("# Heading\n\ntext", cx);
        notes.edit_title("   ", cx);
        notes.commit_title(cx);
    });
    cx.run_until_parked();
    assert_eq!(t.fake.note("note-project-test").title, "Heading");
}

#[gpui::test]
fn caches_the_listing_and_shares_one_in_flight(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    let before = t.fake.commands().len();
    let (first, second) = update(&t, cx, |notes, cx| {
        (notes.load_notes(false, cx), notes.load_notes(false, cx))
    });
    cx.run_until_parked();
    assert_eq!(futures::FutureExt::now_or_never(first).unwrap().len(), 1);
    assert_eq!(futures::FutureExt::now_or_never(second).unwrap().len(), 1);
    assert_eq!(t.fake.commands().len(), before);
    update(&t, cx, |notes, _| notes.invalidate_notes());
    drop(update(&t, cx, |notes, cx| notes.load_notes(false, cx)));
    drop(update(&t, cx, |notes, cx| notes.load_notes(false, cx)));
    cx.run_until_parked();
    assert_eq!(t.fake.commands().len(), before + 1);
    assert!(
        t.notes
            .read_with(cx, |notes, _| notes.peek_notes().is_some())
    );
}

#[gpui::test]
fn creates_an_untitled_note_in_the_active_project_and_selects_it(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    update(&t, cx, |notes, cx| notes.set_query("zzz", cx));
    update(&t, cx, |notes, cx| notes.create(Some("/work/Active"), cx)).detach();
    cx.run_until_parked();
    let upsert = t.fake.upserts().last().cloned().unwrap();
    assert_eq!(upsert.title, "Untitled");
    assert_eq!(upsert.source_cwd.as_deref(), Some("/work/Active"));
    t.notes.read_with(cx, |notes, _| {
        assert_eq!(notes.selected_id(), Some(upsert.id.as_str()));
        assert!(notes.query().is_empty());
        assert!(!notes.is_creating());
        assert!(notes.editor().unwrap().blank);
    });
    update(&t, cx, |notes, cx| notes.create(Some("~"), cx)).detach();
    cx.run_until_parked();
    assert_eq!(t.fake.upserts().last().unwrap().source_cwd, None);
}

#[gpui::test]
fn deletes_the_open_note_and_selects_the_next(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored(), second()]);
    update(&t, cx, |notes, cx| {
        notes.edit_title("unsaved", cx);
        notes.delete_current(cx);
    });
    cx.executor().advance_clock(NOTE_SAVE_DEBOUNCE);
    cx.run_until_parked();
    assert_eq!(t.upsert_count(), 0);
    assert!(t.fake.commands().contains(&"notes_delete".to_string()));
    t.notes.read_with(cx, |notes, _| {
        assert_eq!(notes.selected_id(), Some("second-note"));
        assert_eq!(notes.editor().unwrap().note_id, "second-note");
    });
}

#[gpui::test]
fn filters_by_title_body_slug_tag_and_project(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored(), second()]);
    let visible = |t: &T, cx: &mut TestAppContext, query: &str| -> Vec<String> {
        t.notes.update(cx, |notes, cx| notes.set_query(query, cx));
        t.notes.read_with(cx, |notes, _| {
            notes.visible().into_iter().map(|n| n.id).collect()
        })
    };
    assert_eq!(visible(&t, cx, "second"), vec!["second-note"]);
    assert_eq!(visible(&t, cx, "#ideas").len(), 2);
    assert_eq!(visible(&t, cx, "edefyn").len(), 2);
    assert!(visible(&t, cx, "missing").is_empty());
    // The selection survives a filter that hides it.
    assert_eq!(
        t.notes
            .read_with(cx, |notes, _| notes.selected().map(|n| n.id)),
        Some("note-project-test".to_string())
    );
}

#[gpui::test]
fn adds_the_edited_note_to_a_chat_and_closes(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&t.notes, move |_, event: &NotesEvent, _| {
            seen.borrow_mut().push(event.clone())
        })
        .detach()
    });
    update(&t, cx, |notes, cx| {
        notes.edit_body("Use a cookie.", cx);
        notes.add_to_chat(cx);
    });
    cx.run_until_parked();
    let events = events.borrow();
    let NotesEvent::AddToChat(card) = &events[0];
    assert_eq!(card.body, "Use a cookie.");
    assert_eq!(card.slug, "plan");
    assert!(!t.notes.read_with(cx, |notes, _| notes.is_open()));
}

#[gpui::test]
fn applies_mentioned_notes_to_a_turn(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    let applied = update(&t, cx, |notes, cx| {
        notes.apply_notes_to_turn("See @note/plan", cx)
    });
    cx.run_until_parked();
    assert_eq!(
        futures::FutureExt::now_or_never(applied).unwrap(),
        "See @note/plan\n\n---\nReferenced note \"Plan\":\n\nKeep this text."
    );
}

#[gpui::test]
fn saves_dropped_and_pasted_images_and_inserts_them(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    let none = update(&t, cx, |notes, cx| {
        notes.save_images_from_paths("note-project-test", &["/tmp/readme.md".into()], cx)
    });
    assert_eq!(
        futures::FutureExt::now_or_never(none)
            .unwrap()
            .err()
            .as_deref(),
        Some(NO_IMAGES_ERROR)
    );

    let load = update(&t, cx, |notes, cx| {
        notes.save_images_from_paths(
            "note-project-test",
            &["/tmp/shot.png".into(), "/tmp/shot.png".into()],
            cx,
        )
    });
    let cursor = update(&t, cx, |notes, cx| notes.insert_images(load, 4, 4, cx));
    cx.run_until_parked();
    let cursor = futures::FutureExt::now_or_never(cursor).unwrap().unwrap();
    let body = t.editor(cx).body;
    assert_eq!(
        body,
        "Keep\n\n![shot.png](/note-assets/note-project-test/1-shot.png)\n\n this text."
    );
    assert_eq!(
        cursor,
        "Keep\n\n![shot.png](/note-assets/note-project-test/1-shot.png)".len()
    );

    let pasted = update(&t, cx, |notes, cx| {
        notes.save_images_from_data(
            "note-project-test",
            vec![NoteImageData {
                name: "paste.png".into(),
                data: "aGk=".into(),
            }],
            cx,
        )
    });
    cx.run_until_parked();
    assert_eq!(
        futures::FutureExt::now_or_never(pasted)
            .unwrap()
            .unwrap()
            .len(),
        1
    );
    let commands = t.fake.commands();
    let tail: Vec<&str> = commands[commands.len() - 3..]
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        tail,
        vec!["write_attachment", "notes_save_image", "delete_path"]
    );
}

#[gpui::test]
fn reports_an_image_failure_in_the_editor(cx: &mut TestAppContext) {
    let t = setup(cx, vec![stored()]);
    t.fake.fail_next("notes_save_image", "Too large");
    let load = update(&t, cx, |notes, cx| {
        notes.save_images_from_paths("note-project-test", &["/tmp/huge.png".into()], cx)
    });
    let cursor = update(&t, cx, |notes, cx| notes.insert_images(load, 0, 0, cx));
    assert!(t.editor(cx).image_busy);
    cx.run_until_parked();
    assert_eq!(futures::FutureExt::now_or_never(cursor), Some(None));
    let editor = t.editor(cx);
    assert_eq!(editor.save_error.as_deref(), Some("Too large"));
    assert!(!editor.image_busy);
}

#[gpui::test]
fn opens_a_chat_for_a_note_and_dismisses_its_chip(cx: &mut TestAppContext) {
    init_test_engine(cx);
    let card = NoteComposerCard {
        id: "n1".into(),
        slug: "auth".into(),
        title: "  Auth  ".into(),
        source_cwd: Some("~".into()),
        body: "Use a cookie.".into(),
    };
    let id = cx
        .update(|cx| {
            add_note_to_chat(
                &card,
                &[None, Some("/work/fallback")],
                None,
                &NoHistoryHost,
                cx,
            )
        })
        .unwrap();
    let sessions = cx.update(|cx| Engine::sessions(cx));
    let opened: Session = sessions.read_with(cx, |s, _| s.get(&id).cloned().unwrap());
    assert_eq!(opened.cwd, "/work/fallback");
    assert_eq!(opened.title, "Auth");
    assert_eq!(opened.harness, HarnessId::Cursor);
    assert_eq!(opened.note_card.as_ref().map(|c| c.id.as_str()), Some("n1"));
    cx.update(|cx| dismiss_note_card(&id, cx));
    assert!(sessions.read_with(cx, |s, _| s.get(&id).unwrap().note_card.is_none()));
}
