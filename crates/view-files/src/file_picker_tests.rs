//! Port of src/features/files/ui/FilePicker.test.ts.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

use super::*;
use crate::test_support::FakeFiles;

struct Harness<'a> {
    fs: Rc<FakeFiles>,
    picker: Entity<FilePicker>,
    events: Rc<RefCell<Vec<FilePickerEvent>>>,
    cx: &'a mut VisualTestContext,
}

fn repo_files() -> Arc<Vec<ProjectFile>> {
    Arc::new(vec![ProjectFile::new(
        "App.tsx",
        "/repo/src/App.tsx",
        "src/App.tsx",
    )])
}

fn local_fs() -> Rc<FakeFiles> {
    let fs = FakeFiles::new();
    {
        let mut state = fs.state.borrow_mut();
        state.mock_ranking = true;
        state.hang_project_loads = true;
        state.project_files.insert("/repo".into(), repo_files());
    }
    fs
}

fn open<'a>(fs: Rc<FakeFiles>, cwd: &str, query: &str, cx: &'a mut TestAppContext) -> Harness<'a> {
    cx.update(crate::test_support::init);
    let data: Rc<dyn FilesData> = fs.clone();
    let cwd = cwd.to_string();
    let query = query.to_string();
    let (picker, cx) = cx.add_window_view(move |window, cx| {
        FilePicker::new(data, cwd, Vec::new(), &query, window, cx)
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&picker, move |_, event: &FilePickerEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    cx.run_until_parked();
    Harness {
        fs,
        picker,
        events,
        cx,
    }
}

impl Harness<'_> {
    /// Replace the query by selecting everything and typing.
    fn type_query(&mut self, text: &str) {
        self.cx.simulate_keystrokes("cmd-a");
        self.cx.simulate_input(text);
        self.cx.run_until_parked();
    }

    fn title(&mut self) -> &'static str {
        self.picker
            .read_with(self.cx, |picker, cx| picker.title(cx))
    }

    fn labels(&mut self) -> Vec<String> {
        self.picker.read_with(self.cx, |picker, cx| {
            if picker.palette_mode(cx) {
                picker
                    .action_results()
                    .iter()
                    .map(|ranked| ranked.action.label.to_string())
                    .collect()
            } else {
                picker
                    .results()
                    .iter()
                    .map(|ranked| ranked.file.name.clone())
                    .collect()
            }
        })
    }

    fn rank_calls(&self) -> usize {
        self.fs.state.borrow().rank_calls
    }
}

#[test]
fn formats_reload_shortcut_hints_for_macos_and_other_platforms() {
    assert_eq!(reload_action_hint("⌘", "⇧"), "⌘⇧R");
    assert_eq!(reload_action_hint("Ctrl+", "Shift+"), "Ctrl+Shift+R");
}

#[gpui::test]
fn opens_from_an_initial_command_query_without_ranking_project_files(cx: &mut TestAppContext) {
    let mut h = open(local_fs(), "/repo", ">", cx);
    assert_eq!(h.title(), "Command Palette");
    let query = h.picker.read_with(h.cx, |picker, cx| picker.query(cx));
    assert_eq!(query, ">");
    assert!(PLACEHOLDER.contains("type > for commands"));
    assert_eq!(h.labels(), vec!["Reload MonoCode"]);
    let hint = h.picker.read_with(h.cx, |picker, _| {
        picker.action_results()[0].action.hint.clone()
    });
    let platform = Platform::current();
    assert_eq!(
        hint.as_deref(),
        Some(reload_action_hint(platform.mod_label(), platform.shift_label()).as_str())
    );
    assert_eq!(h.rank_calls(), 0);
}

#[gpui::test]
fn only_a_leading_angle_bracket_is_command_mode(cx: &mut TestAppContext) {
    let mut h = open(local_fs(), "/repo", "", cx);
    h.type_query("App>");
    assert_eq!(h.title(), "Go to File");
    assert_eq!(h.labels(), vec!["App.tsx"]);

    let before = h.rank_calls();
    h.type_query(">");
    assert_eq!(h.title(), "Command Palette");
    assert_eq!(h.labels(), vec!["Reload MonoCode"]);
    assert_eq!(h.rank_calls(), before);

    h.type_query("App");
    assert_eq!(h.title(), "Go to File");
    assert_eq!(h.labels(), vec!["App.tsx"]);
    assert!(h.rank_calls() > before);
}

#[gpui::test]
fn fuzzy_filters_and_highlights_commands_with_palette_empty_copy(cx: &mut TestAppContext) {
    let mut h = open(local_fs(), "/repo", ">", cx);
    h.type_query("> rmc");
    let positions = h.picker.read_with(h.cx, |picker, _| {
        picker.action_results()[0].positions.clone()
    });
    assert_eq!(
        crate::match_text::match_ranges("Reload MonoCode", &positions).len(),
        3
    );

    h.type_query("> missing");
    let empty = h
        .picker
        .read_with(h.cx, |picker, cx| picker.empty_message(cx));
    assert_eq!(empty.as_deref(), Some("No matching commands"));
}

#[gpui::test]
fn runs_the_selected_command_and_closes_on_click(cx: &mut TestAppContext) {
    let h = open(local_fs(), "/repo", ">", cx);
    let bounds = h.cx.debug_bounds("picker-option-0").expect("option drawn");
    h.cx.simulate_click(bounds.center(), Modifiers::none());
    h.cx.run_until_parked();
    assert_eq!(
        h.events.borrow().as_slice(),
        [
            FilePickerEvent::RunAction("reload".into()),
            FilePickerEvent::Close
        ]
    );
}

#[gpui::test]
fn runs_the_selected_command_and_closes_on_enter(cx: &mut TestAppContext) {
    let h = open(local_fs(), "/repo", ">", cx);
    h.cx.simulate_keystrokes("enter");
    h.cx.run_until_parked();
    assert_eq!(
        h.events.borrow().as_slice(),
        [
            FilePickerEvent::RunAction("reload".into()),
            FilePickerEvent::Close
        ]
    );
}

#[gpui::test]
fn uses_the_shared_file_list_on_a_remote_project(cx: &mut TestAppContext) {
    let cwd = "remote://env/home/me/repo";
    let fs = FakeFiles::new();
    {
        let mut state = fs.state.borrow_mut();
        state.mock_ranking = true;
        state.load_results.insert(
            cwd.into(),
            Arc::new(vec![ProjectFile::new(
                "App.tsx",
                "remote://env/home/me/repo/src/App.tsx",
                "src/App.tsx",
            )]),
        );
    }
    let mut h = open(fs, cwd, "app", cx);
    assert_eq!(
        h.fs.state.borrow().load_calls,
        vec![(cwd.to_string(), true)]
    );
    assert_eq!(h.labels(), vec!["App.tsx"]);
    h.cx.simulate_keystrokes("enter");
    h.cx.run_until_parked();
    assert_eq!(
        h.events.borrow().as_slice(),
        [
            FilePickerEvent::OpenFile {
                path: "remote://env/home/me/repo/src/App.tsx".into(),
                options: FileOpenOptions::EXACT,
            },
            FilePickerEvent::Close,
        ]
    );
    assert_eq!(
        h.fs.state.borrow().recents,
        vec!["remote://env/home/me/repo/src/App.tsx".to_string()]
    );
}

#[gpui::test]
fn shows_a_connection_error_from_the_shared_file_list(cx: &mut TestAppContext) {
    let cwd = "remote://env/home/me/repo";
    let fs = FakeFiles::new();
    fs.state
        .borrow_mut()
        .project_errors
        .insert(cwd.into(), "Machine is not connected".into());
    let h = open(fs, cwd, "", cx);
    let empty = h
        .picker
        .read_with(h.cx, |picker, cx| picker.empty_message(cx));
    assert_eq!(empty.as_deref(), Some("Machine is not connected"));
}

#[gpui::test]
fn moves_the_highlight_with_the_arrow_keys_and_closes_on_escape(cx: &mut TestAppContext) {
    let fs = local_fs();
    fs.state.borrow_mut().project_files.insert(
        "/repo".into(),
        Arc::new(vec![
            ProjectFile::new("App.tsx", "/repo/src/App.tsx", "src/App.tsx"),
            ProjectFile::new("app.css", "/repo/src/app.css", "src/app.css"),
        ]),
    );
    let h = open(fs, "/repo", "app", cx);
    h.cx.simulate_keystrokes("down");
    assert_eq!(h.picker.read_with(h.cx, |picker, _| picker.active()), 1);
    h.cx.simulate_keystrokes("down");
    assert_eq!(h.picker.read_with(h.cx, |picker, _| picker.active()), 0);
    h.cx.simulate_keystrokes("up");
    assert_eq!(h.picker.read_with(h.cx, |picker, _| picker.active()), 1);
    h.cx.simulate_keystrokes("escape");
    assert_eq!(h.events.borrow().as_slice(), [FilePickerEvent::Close]);
}

#[test]
fn picks_the_empty_copy_like_the_typescript() {
    let label = |cwd: &str,
                 query: &str,
                 loading: bool,
                 error: Option<&str>,
                 files: usize,
                 matches: usize| {
        empty_label(cwd, query, loading, error, files, matches, false, 0)
    };
    assert_eq!(
        label("~", "", false, None, 0, 0).as_deref(),
        Some("Open a project to search files")
    );
    assert_eq!(
        label("/repo", "", true, None, 0, 0).as_deref(),
        Some("Indexing files…")
    );
    assert_eq!(
        label("/repo", "", false, None, 0, 0).as_deref(),
        Some("No files found")
    );
    assert_eq!(
        label("/repo", " ", false, None, 3, 0).as_deref(),
        Some("Type a file name to search")
    );
    assert_eq!(
        label("/repo", "zz", false, None, 3, 0).as_deref(),
        Some("No matching files")
    );
    assert_eq!(
        label("/repo", "a", false, Some("boom"), 0, 0).as_deref(),
        Some("boom")
    );
    assert_eq!(label("/repo", "a", false, None, 3, 1), None);
}
