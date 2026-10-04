//! Tests for the file editor surface, ported from FilePaneNavigation.test.ts
//! and FilePaneLineEndings.test.ts. The TypeScript reached the editor
//! through `FilePane`; these mount the surface the pane uses for a file.
//!
//! "does not autosave an external change detected during formatting" has no
//! port: `CodeEditor` formats synchronously, so no disk change can arrive
//! between formatting and the write.

use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui::{Entity, FocusHandle, TestAppContext, VisualTestContext};
use monocode_editor::code_editor::AUTOSAVE_DELAY;
use monocode_git::fs::{GitChangedFile, GitFileDiff};

use super::*;
use crate::test_support::FakeFiles;

const THREE_LINES: &str = "first line\nsecond line\nthird line";

struct Harness<'a> {
    fs: Rc<FakeFiles>,
    surface: Entity<FileEditorSurface>,
    cx: &'a mut VisualTestContext,
}

fn mount<'a>(
    fs: Rc<FakeFiles>,
    path: &str,
    settings: EditorSettings,
    cx: &'a mut TestAppContext,
) -> Harness<'a> {
    cx.update(|cx| {
        crate::test_support::init(cx);
        monocode_markdown::init(cx);
    });
    let data: Rc<dyn FilesData> = fs.clone();
    let path = path.to_string();
    let (surface, cx) = cx.add_window_view(move |window, cx| {
        let mut surface = FileEditorSurface::new(data, path, "/repo", window, cx);
        surface.set_settings(settings, window, cx);
        surface
    });
    cx.update(|window, _| window.activate_window());
    surface.update_in(cx, |surface, window, cx| {
        surface.set_active(true, window, cx)
    });
    cx.run_until_parked();
    Harness { fs, surface, cx }
}

fn autosave() -> EditorSettings {
    EditorSettings {
        autosave: true,
        ..EditorSettings::default()
    }
}

impl Harness<'_> {
    fn editor(&mut self) -> Entity<CodeEditor> {
        self.surface
            .read_with(self.cx, |surface, _| surface.editor().cloned())
            .expect("the editor is mounted")
    }

    fn text(&mut self) -> String {
        let editor = self.editor();
        editor.read_with(self.cx, |editor, cx| editor.text(cx))
    }

    fn selection(&mut self) -> Range<usize> {
        let editor = self.editor();
        editor.read_with(self.cx, |editor, cx| {
            editor.editor_state().read(cx).selected_range()
        })
    }

    fn edit(&mut self, range: Range<usize>, insert: &str) {
        let state = self
            .editor()
            .read_with(self.cx, |editor, _| editor.editor_state().clone());
        let insert = insert.to_string();
        state.update_in(self.cx, |state, window, cx| {
            state.set_selected_range(range, cx);
            state.replace(insert, window, cx);
        });
        self.cx.run_until_parked();
    }

    fn select(&mut self, offset: usize) {
        let state = self
            .editor()
            .read_with(self.cx, |editor, _| editor.editor_state().clone());
        state.update(self.cx, |state, cx| {
            state.set_selected_range(offset..offset, cx)
        });
        self.cx.run_until_parked();
    }

    fn save(&mut self) {
        let editor = self.editor();
        editor.update_in(self.cx, |editor, window, cx| editor.focus(window, cx));
        self.cx.simulate_keystrokes("secondary-s");
        self.cx.run_until_parked();
    }

    fn navigate(&mut self, line: usize, column: usize, token: u64) {
        let path = self
            .surface
            .read_with(self.cx, |surface, _| surface.path().to_string());
        self.surface.update_in(self.cx, |surface, window, cx| {
            surface.set_navigation(
                Some(EditorNavigation {
                    path,
                    line,
                    column: Some(column),
                    token,
                }),
                window,
                cx,
            )
        });
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear());
        self.cx.run_until_parked();
    }

    /// Change the file on disk and fire its watch, like
    /// `invalidateWatchedFiles`.
    fn change_on_disk(&mut self, path: &str, content: &str) {
        self.fs.set_file(path, content);
        self.cx.update(|_, cx| self.fs.touch(path, cx));
        self.cx
            .executor()
            .advance_clock(DISK_RELOAD_DELAY + Duration::from_millis(10));
        self.cx.run_until_parked();
    }

    fn advance(&mut self, duration: Duration) {
        self.cx.executor().advance_clock(duration);
        self.cx.run_until_parked();
    }

    /// Move focus off the editor. Blur listeners run when the window
    /// draws, as they do on the next frame in the app.
    fn focus_elsewhere(&mut self) -> FocusHandle {
        let handle = self.cx.update(|window, cx| {
            let handle = cx.focus_handle();
            window.focus(&handle, cx);
            handle
        });
        self.cx.update(|window, cx| window.draw(cx).clear());
        self.cx.run_until_parked();
        handle
    }

    fn editor_focused(&mut self) -> bool {
        let editor = self.editor();
        self.cx
            .update(|window, cx| editor.read(cx).focus_handle(cx).is_focused(window))
    }
}

fn with_file(path: &str, content: &str) -> Rc<FakeFiles> {
    let fs = FakeFiles::new();
    fs.set_file(path, content);
    fs
}

fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].matches('\n').count() + 1
}

// FilePaneLineEndings.test.ts

#[gpui::test]
fn saves_a_crlf_file_back_with_crlf_line_endings(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/notes.txt", "alpha\r\nbeta\r\n"),
        "/repo/notes.txt",
        EditorSettings::default(),
        cx,
    );
    // The buffer itself is LF-only.
    assert_eq!(h.text(), "alpha\nbeta\n");
    h.edit(0..0, "intro\n");
    h.save();
    assert_eq!(
        h.fs.writes(),
        vec![(
            "/repo/notes.txt".to_string(),
            "intro\r\nalpha\r\nbeta\r\n".to_string()
        )]
    );
}

#[gpui::test]
fn automatically_saves_after_typing_stops(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/notes.txt", "alpha\n"),
        "/repo/notes.txt",
        autosave(),
        cx,
    );
    h.edit(0..0, "first ");
    h.advance(AUTOSAVE_DELAY - Duration::from_millis(1));
    assert!(h.fs.writes().is_empty());
    h.edit(0..0, "second ");
    h.advance(AUTOSAVE_DELAY - Duration::from_millis(1));
    assert!(h.fs.writes().is_empty());
    h.advance(Duration::from_millis(1));
    assert_eq!(
        h.fs.writes(),
        vec![(
            "/repo/notes.txt".to_string(),
            "second first alpha\n".to_string()
        )]
    );
}

#[gpui::test]
fn keeps_changes_dirty_when_autosave_is_disabled(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/notes.txt", "alpha\n"),
        "/repo/notes.txt",
        EditorSettings::default(),
        cx,
    );
    h.edit(0..0, "changed ");
    h.advance(AUTOSAVE_DELAY);
    assert!(h.fs.writes().is_empty());
    let editor = h.editor();
    assert!(editor.read_with(h.cx, |editor, _| editor.is_dirty()));
}

#[gpui::test]
fn does_not_autosave_over_an_external_file_change(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/notes.txt", "alpha\n"),
        "/repo/notes.txt",
        autosave(),
        cx,
    );
    h.edit(0..0, "local ");
    h.change_on_disk("/repo/notes.txt", "external\n");
    h.advance(AUTOSAVE_DELAY);
    assert!(h.fs.writes().is_empty());
    assert_eq!(h.text(), "local alpha\n");
}

#[gpui::test]
fn restores_a_pending_autosave_after_a_manual_save_fails(cx: &mut TestAppContext) {
    let fs = with_file("/repo/notes.txt", "alpha\n");
    fs.state.borrow_mut().write_error = Some("disk unavailable".into());
    let mut h = mount(fs, "/repo/notes.txt", autosave(), cx);
    h.edit(0..0, "changed ");
    h.save();
    assert_eq!(h.fs.writes().len(), 1);
    let editor = h.editor();
    assert_eq!(
        editor.read_with(h.cx, |editor, _| editor.save_state().clone()),
        monocode_editor::SaveState::Error("disk unavailable".into())
    );
    h.advance(AUTOSAVE_DELAY);
    let writes = h.fs.writes();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[1].1, "changed alpha\n");
}

#[gpui::test]
fn preserves_queued_save_line_endings_after_switching_files(cx: &mut TestAppContext) {
    let fs = with_file("/repo/crlf.txt", "alpha\r\nbeta\r\n");
    fs.set_file("/repo/lf.txt", "other\nfile\n");
    fs.state.borrow_mut().hold_writes = true;
    let mut h = mount(fs, "/repo/crlf.txt", EditorSettings::default(), cx);
    h.edit(0..0, "first\n");
    h.save();
    assert_eq!(
        h.fs.writes(),
        vec![(
            "/repo/crlf.txt".to_string(),
            "first\r\nalpha\r\nbeta\r\n".to_string()
        )]
    );
    h.edit(0..0, "second\n");
    h.save();
    assert_eq!(h.fs.writes().len(), 1);

    h.surface.update_in(h.cx, |surface, window, cx| {
        surface.set_path("/repo/lf.txt", window, cx)
    });
    h.cx.run_until_parked();
    assert_eq!(h.text(), "other\nfile\n");

    h.fs.release_writes();
    h.cx.run_until_parked();
    assert_eq!(
        h.fs.writes(),
        vec![
            (
                "/repo/crlf.txt".to_string(),
                "first\r\nalpha\r\nbeta\r\n".to_string()
            ),
            (
                "/repo/crlf.txt".to_string(),
                "second\r\nfirst\r\nalpha\r\nbeta\r\n".to_string()
            ),
        ]
    );
    h.fs.release_writes();
}

// FilePaneNavigation.test.ts

#[gpui::test]
fn shows_markdown_source_and_navigates_to_its_referenced_line(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/navigation.md", THREE_LINES),
        "/repo/navigation.md",
        EditorSettings::default(),
        cx,
    );
    assert_eq!(
        h.surface.read_with(h.cx, |surface, _| surface.mode()),
        MarkdownViewMode::Preview
    );
    h.navigate(2, 2, 1);
    assert_eq!(
        h.surface.read_with(h.cx, |surface, _| surface.mode()),
        MarkdownViewMode::Source
    );
    assert_eq!(h.selection(), 12..12);
    assert_eq!(h.text(), THREE_LINES);
    h.surface.update_in(h.cx, |surface, window, cx| {
        surface.set_mode(MarkdownViewMode::Preview, window, cx)
    });
    assert_eq!(
        h.surface.read_with(h.cx, |surface, _| surface.mode()),
        MarkdownViewMode::Preview
    );
}

#[gpui::test]
fn clamps_a_stale_source_location_to_the_last_line(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/short.txt", THREE_LINES),
        "/repo/short.txt",
        EditorSettings::default(),
        cx,
    );
    h.navigate(999, 2, 1);
    let head = h.selection().start;
    assert_eq!(line_of(THREE_LINES, head), 3);
}

#[gpui::test]
fn reapplies_the_requested_location_when_a_reload_adds_its_line(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/growing.txt", "first line"),
        "/repo/growing.txt",
        EditorSettings::default(),
        cx,
    );
    h.navigate(3, 2, 1);
    assert_eq!(h.selection(), 1..1);
    h.change_on_disk("/repo/growing.txt", THREE_LINES);
    assert_eq!(h.text(), THREE_LINES);
    // Line 3 starts at 23; column 2 is one past it.
    assert_eq!(h.selection(), 24..24);
}

#[gpui::test]
fn keeps_file_contents_and_the_diff_in_the_same_pane(cx: &mut TestAppContext) {
    let fs = with_file("/repo/review.txt", THREE_LINES);
    {
        let mut state = fs.state.borrow_mut();
        state.git_files = vec![GitChangedFile {
            path: "/repo/review.txt".into(),
            relative: "review.txt".into(),
            status: "modified".into(),
            additions: 1,
            deletions: 1,
            staged: false,
            unstaged: true,
        }];
        state.git_diffs.insert(
            "review.txt".into(),
            GitFileDiff {
                path: "/repo/review.txt".into(),
                relative: "review.txt".into(),
                status: "modified".into(),
                original: "first line\nold line\nthird line".into(),
                current: THREE_LINES.into(),
                binary: false,
                too_large: false,
            },
        );
    }
    let mut h = mount(fs, "/repo/review.txt", EditorSettings::default(), cx);
    h.surface
        .update(h.cx, |surface, cx| surface.set_show_diff(true, cx));
    h.cx.run_until_parked();
    h.navigate(2, 2, 1);
    assert!(h.text().contains("second line"));
    let editor = h.editor();
    assert_eq!(
        editor.read_with(h.cx, |editor, _| editor.diff_stats()),
        (1, 1)
    );
    let base = h
        .surface
        .read_with(h.cx, |surface, _| surface.git_base().cloned())
        .unwrap();
    assert_eq!(base.kind, GitFileDiffKind::Unstaged);
    assert!(!base.eol_only);
}

#[gpui::test]
fn notes_a_line_ending_only_change(cx: &mut TestAppContext) {
    let fs = with_file("/repo/crlf.txt", "a\r\nb\r\n");
    {
        let mut state = fs.state.borrow_mut();
        state.git_files = vec![GitChangedFile {
            path: "/repo/crlf.txt".into(),
            relative: "crlf.txt".into(),
            status: "modified".into(),
            additions: 2,
            deletions: 2,
            staged: true,
            unstaged: false,
        }];
        state.git_diffs.insert(
            "crlf.txt".into(),
            GitFileDiff {
                path: "/repo/crlf.txt".into(),
                relative: "crlf.txt".into(),
                status: "modified".into(),
                original: "a\nb\n".into(),
                current: "a\r\nb\r\n".into(),
                binary: false,
                too_large: false,
            },
        );
    }
    let h = mount(fs, "/repo/crlf.txt", EditorSettings::default(), cx);
    h.surface
        .update(h.cx, |surface, cx| surface.set_show_diff(true, cx));
    h.cx.run_until_parked();
    let base = h
        .surface
        .read_with(h.cx, |surface, _| surface.git_base().cloned())
        .unwrap();
    assert_eq!(base.kind, GitFileDiffKind::Staged);
    assert!(base.eol_only);
    assert_eq!(base.line_ending, LineEnding::Lf);
}

#[gpui::test]
fn preserves_a_moved_selection_and_outside_focus_after_a_reload(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/moved.txt", THREE_LINES),
        "/repo/moved.txt",
        EditorSettings::default(),
        cx,
    );
    h.navigate(2, 2, 1);
    assert_eq!(h.selection(), 12..12);
    h.select(23);
    let _elsewhere = h.focus_elsewhere();
    h.change_on_disk("/repo/moved.txt", &format!("{THREE_LINES}\nfourth line"));
    assert_eq!(h.selection(), 23..23);
    assert!(!h.editor_focused());
}

#[gpui::test]
fn does_not_replay_completed_navigation(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/completed.txt", THREE_LINES),
        "/repo/completed.txt",
        EditorSettings::default(),
        cx,
    );
    h.navigate(2, 2, 1);
    assert_eq!(h.selection(), 12..12);
    let _elsewhere = h.focus_elsewhere();
    h.change_on_disk(
        "/repo/completed.txt",
        &format!("{THREE_LINES}\nfourth line"),
    );
    assert_eq!(h.selection(), 12..12);
    assert!(!h.editor_focused());
}

#[gpui::test]
fn cancels_a_clamped_pending_navigation_on_selection(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/pending.txt", "first line"),
        "/repo/pending.txt",
        EditorSettings::default(),
        cx,
    );
    h.navigate(3, 2, 1);
    assert_eq!(h.selection(), 1..1);
    h.select(0);
    h.change_on_disk("/repo/pending.txt", THREE_LINES);
    assert_eq!(h.selection(), 0..0);
}

#[gpui::test]
fn cancels_a_clamped_pending_navigation_on_blur(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/pending.txt", "first line"),
        "/repo/pending.txt",
        EditorSettings::default(),
        cx,
    );
    h.navigate(3, 2, 1);
    assert_eq!(h.selection(), 1..1);
    let _elsewhere = h.focus_elsewhere();
    h.change_on_disk("/repo/pending.txt", THREE_LINES);
    assert_eq!(h.selection(), 1..1);
    assert!(!h.editor_focused());
}

#[gpui::test]
fn allows_a_new_navigation_token_to_revisit_the_same_location(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/revisit.txt", THREE_LINES),
        "/repo/revisit.txt",
        EditorSettings::default(),
        cx,
    );
    h.navigate(2, 2, 1);
    assert_eq!(h.selection(), 12..12);
    h.select(0);
    h.navigate(2, 2, 1);
    assert_eq!(h.selection(), 0..0);
    h.navigate(2, 2, 2);
    assert_eq!(h.selection(), 12..12);
}

#[gpui::test]
fn shows_a_retry_card_when_the_file_cannot_be_read(cx: &mut TestAppContext) {
    let fs = FakeFiles::new();
    fs.state
        .borrow_mut()
        .read_errors
        .insert("/repo/locked.txt".into(), "Permission denied".into());
    let h = mount(fs, "/repo/locked.txt", EditorSettings::default(), cx);
    assert_eq!(
        h.surface
            .read_with(h.cx, |surface, _| surface.load_state().clone()),
        LoadState::Error("Permission denied".into())
    );
    h.fs.state.borrow_mut().read_errors.clear();
    h.fs.set_file("/repo/locked.txt", "ok");
    h.surface
        .update_in(h.cx, |surface, window, cx| surface.retry(window, cx));
    h.cx.run_until_parked();
    assert_eq!(
        h.surface
            .read_with(h.cx, |surface, _| surface.load_state().clone()),
        LoadState::Ready
    );
}

#[test]
fn recognizes_markdown_and_svg_paths() {
    assert!(is_markdown_path("/r/README.MD"));
    assert!(is_markdown_path("/r/a.mdx"));
    assert!(!is_markdown_path("/r/md"));
    assert!(is_svg_path("/r/logo.SVG"));
    assert!(!is_svg_path("/r/logo.png"));
}

#[gpui::test]
fn offers_add_to_chat_for_a_selection(cx: &mut TestAppContext) {
    let mut h = mount(
        with_file("/repo/src/lib.rs", THREE_LINES),
        "/repo/src/lib.rs",
        EditorSettings::default(),
        cx,
    );
    let editor = h.editor();
    let state = editor.read_with(h.cx, |editor, _| editor.editor_state().clone());
    let select = |range: Range<usize>, h: &mut Harness<'_>| {
        state.update(h.cx, |state, cx| state.set_selected_range(range, cx));
        h.cx.update(|window, cx| window.draw(cx).clear());
        h.cx.run_until_parked();
        h.surface.read_with(h.cx, |surface, _| {
            surface
                .selection_target()
                .map(|target| target.selection.clone())
        })
    };
    assert_eq!(
        select(0..15, &mut h),
        Some(EditorCodeSelection {
            path: "src/lib.rs".into(),
            start_line: 1,
            end_line: 2,
        })
    );
    // A selection that ends on a line break stays on the line before it.
    assert_eq!(
        select(11..23, &mut h).map(|s| (s.start_line, s.end_line)),
        Some((2, 2))
    );
    assert_eq!(select(5..5, &mut h), None);

    let events = Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = events.clone();
    h.cx.update(|_, cx| {
        cx.subscribe(&h.surface, move |_, event: &FileEditorEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    select(0..10, &mut h);
    h.surface.update(h.cx, |surface, cx| {
        let selection = surface.selection_target().unwrap().selection.clone();
        cx.emit(FileEditorEvent::AddToChat(selection));
    });
    assert!(
        matches!(events.borrow().last(), Some(FileEditorEvent::AddToChat(s)) if s.end_line == 1)
    );
}
