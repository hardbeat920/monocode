//! Port of src/features/files/ui/FileTree.test.ts, plus the menu items and
//! the folder helpers. React render isolation (the memo test) has no GPUI
//! equivalent; the GPUI versions check the rows the tree builds instead.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    ClipboardItem, Entity, ExternalPaths, Modifiers, MouseButton, TestAppContext,
    VisualTestContext, point, px,
};

use super::*;
use crate::data::DIRS_REFRESH_DELAY;
use crate::test_support::{Call, FakeFiles};

const CWD: &str = "/project";

fn file(name: &str) -> FsEntry {
    FsEntry::file(name, format!("{CWD}/{name}"))
}

fn folder(name: &str) -> FsEntry {
    FsEntry::dir(name, format!("{CWD}/{name}"))
}

fn ignored(mut entry: FsEntry) -> FsEntry {
    entry.ignored = true;
    entry
}

struct Harness<'a> {
    fs: Rc<FakeFiles>,
    tree: Entity<FileTree>,
    events: Rc<RefCell<Vec<FileTreeEvent>>>,
    cx: &'a mut VisualTestContext,
}

fn mount(fs: Rc<FakeFiles>, cx: &mut TestAppContext) -> Harness<'_> {
    cx.update(crate::test_support::init);
    let data: Rc<dyn FilesData> = fs.clone();
    let (tree, cx) = cx.add_window_view(|window, cx| {
        let mut tree = FileTree::new(data, CWD, window, cx);
        tree.set_animate_menus(false);
        tree
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|window, cx| {
        cx.subscribe(&tree, move |_, event: &FileTreeEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
        let handle = tree.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    cx.run_until_parked();
    Harness {
        fs,
        tree,
        events,
        cx,
    }
}

fn setup(entries: Vec<FsEntry>) -> Rc<FakeFiles> {
    let fs = FakeFiles::new();
    fs.set_dir(CWD, entries);
    fs
}

impl Harness<'_> {
    fn paths(&mut self) -> Vec<String> {
        self.tree.read_with(self.cx, |tree, _| {
            tree.rows()
                .into_iter()
                .filter_map(|row| match row {
                    TreeRow::Entry { entry, .. } => Some(entry.path),
                    _ => None,
                })
                .collect()
        })
    }

    fn has_row(&mut self, name: &str) -> bool {
        let path = format!("{CWD}/{name}");
        self.paths().contains(&path)
    }

    fn select(&mut self, path: &str) {
        let path = path.to_string();
        self.tree.update(self.cx, |tree, cx| tree.select(&path, cx));
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
    }

    fn clipboard(&mut self) -> Option<String> {
        self.cx.read_from_clipboard().and_then(|item| item.text())
    }

    fn click(&mut self, selector: &'static str) {
        let bounds = self.cx.debug_bounds(selector).expect("element is drawn");
        self.cx.simulate_click(bounds.center(), Modifiers::none());
        self.cx.run_until_parked();
    }
}

#[gpui::test]
fn uses_a_worktree_branch_as_the_explorer_root_identity(cx: &mut TestAppContext) {
    let h = mount(setup(vec![file("first.ts")]), cx);
    h.tree.update(h.cx, |tree, cx| {
        tree.set_root_label(Some("mc/update-readme-tests".into()), cx)
    });
    assert_eq!(
        h.tree.read_with(h.cx, |tree, _| tree.root_name()),
        "mc/update-readme-tests"
    );
    h.tree
        .update(h.cx, |tree, cx| tree.set_root_label(Some("  ".into()), cx));
    assert_eq!(
        h.tree.read_with(h.cx, |tree, _| tree.root_name()),
        "project"
    );
}

#[gpui::test]
fn shows_git_decorations_and_opens_a_clicked_file(cx: &mut TestAppContext) {
    let mut h = mount(setup(vec![file("first.ts")]), cx);
    let statuses = GitStatusMap {
        files: [(format!("{CWD}/first.ts"), "modified".to_string())].into(),
        dirs: Default::default(),
    };
    h.tree
        .update(h.cx, |tree, cx| tree.set_git_statuses(statuses.clone(), cx));
    h.cx.run_until_parked();
    h.click("tree-row:/project/first.ts");
    assert_eq!(
        h.events.borrow().as_slice(),
        [FileTreeEvent::OpenFile {
            path: format!("{CWD}/first.ts"),
            options: FileOpenOptions::EXACT,
        }]
    );
    assert_eq!(
        h.tree
            .read_with(h.cx, |tree, _| tree.selected_path().map(str::to_string)),
        Some(format!("{CWD}/first.ts"))
    );
}

#[gpui::test]
fn expands_folders_and_refreshes_rows_after_filesystem_changes(cx: &mut TestAppContext) {
    let fs = setup(vec![file("first.ts")]);
    fs.explorer.save_expanded(CWD, Default::default());
    let mut h = mount(fs, cx);
    assert!(!h.has_row("first.ts"));
    h.click("explorer-root");
    assert!(h.has_row("first.ts"));

    h.fs.set_dir(CWD, vec![file("added.ts")]);
    h.cx.update(|_, cx| h.fs.explorer.notify_dirs_changed(cx));
    h.cx.executor()
        .advance_clock(DIRS_REFRESH_DELAY + Duration::from_millis(50));
    h.cx.run_until_parked();
    assert!(h.has_row("added.ts"));
    assert!(!h.has_row("first.ts"));
}

#[gpui::test]
fn loads_nested_folders_lazily_and_lists_folders_first(cx: &mut TestAppContext) {
    let fs = setup(vec![file("a.ts"), folder("src")]);
    fs.set_dir(
        "/project/src",
        vec![FsEntry::file("lib.rs", "/project/src/lib.rs")],
    );
    let mut h = mount(fs, cx);
    assert_eq!(h.paths(), vec!["/project/src", "/project/a.ts"]);
    h.click("tree-row:/project/src");
    assert_eq!(
        h.paths(),
        vec!["/project/src", "/project/src/lib.rs", "/project/a.ts"]
    );
    // A folder that fails to list shows its error.
    h.fs.set_dir(CWD, vec![folder("gone")]);
    h.fs.explorer.forget_dir(CWD);
    h.tree.update(h.cx, |tree, cx| tree.dirs_changed(cx));
    h.cx.run_until_parked();
    h.click("tree-row:/project/gone");
    let rows = h.tree.read_with(h.cx, |tree, _| tree.rows());
    assert!(rows.contains(&TreeRow::Message {
        depth: 1,
        text: "/project/gone: No such directory".into(),
        loading: false,
    }));
}

#[gpui::test]
fn hides_ignored_entries_by_default_and_follows_the_setting(cx: &mut TestAppContext) {
    let mut h = mount(
        setup(vec![
            ignored(folder("dist")),
            folder("src"),
            file("first.ts"),
            ignored(file("debug.log")),
        ]),
        cx,
    );
    assert!(h.has_row("src"));
    assert!(h.has_row("first.ts"));
    assert!(!h.has_row("dist"));
    assert!(!h.has_row("debug.log"));

    h.tree
        .update(h.cx, |tree, cx| tree.set_show_excluded_files(true, cx));
    assert!(h.has_row("dist"));
    assert!(h.has_row("debug.log"));

    h.tree
        .update(h.cx, |tree, cx| tree.set_show_excluded_files(false, cx));
    assert!(!h.has_row("dist"));
    assert!(!h.has_row("debug.log"));
    assert!(h.has_row("first.ts"));
}

fn outside_setup() -> Rc<FakeFiles> {
    let fs = setup(vec![folder("docs"), file("first.ts")]);
    fs.set_dir("/project/docs", vec![]);
    fs
}

#[gpui::test]
fn pastes_files_from_the_system_clipboard_into_the_selected_folder(cx: &mut TestAppContext) {
    let fs = outside_setup();
    fs.state.borrow_mut().clipboard_files = vec![
        "/Users/me/Desktop/a.txt".into(),
        "/Users/me/Desktop/b.txt".into(),
    ];
    let mut h = mount(fs, cx);
    h.select("/project/docs");
    h.keys("secondary-v");
    assert_eq!(
        h.fs.copies(),
        vec![
            (
                "/Users/me/Desktop/a.txt".to_string(),
                "/project/docs".to_string()
            ),
            (
                "/Users/me/Desktop/b.txt".to_string(),
                "/project/docs".to_string()
            ),
        ]
    );
}

#[gpui::test]
fn pastes_into_the_parent_folder_when_a_file_is_selected(cx: &mut TestAppContext) {
    let fs = outside_setup();
    fs.state.borrow_mut().clipboard_files = vec!["/Users/me/Desktop/a.txt".into()];
    let mut h = mount(fs, cx);
    h.select("/project/first.ts");
    h.keys("secondary-v");
    assert_eq!(
        h.fs.copies(),
        vec![("/Users/me/Desktop/a.txt".to_string(), CWD.to_string())]
    );
    // The copy is selected afterwards.
    assert_eq!(
        h.tree
            .read_with(h.cx, |tree, _| tree.selected_path().map(str::to_string)),
        Some("/project/a.txt".into())
    );
}

#[gpui::test]
fn does_nothing_on_paste_when_the_clipboard_holds_no_files(cx: &mut TestAppContext) {
    let mut h = mount(outside_setup(), cx);
    h.select("/project/docs");
    h.keys("secondary-v");
    assert!(h.fs.copies().is_empty());
}

#[gpui::test]
fn does_not_read_the_clipboard_when_the_context_menu_opens(cx: &mut TestAppContext) {
    let h = mount(outside_setup(), cx);
    let bounds = h.cx.debug_bounds("tree-row:/project/docs").unwrap();
    h.cx.simulate_mouse_down(bounds.center(), MouseButton::Right, Modifiers::none());
    h.cx.run_until_parked();
    let menu = h.tree.read_with(h.cx, |tree, _| {
        tree.menu().map(|(target, _)| target.clone())
    });
    assert_eq!(
        menu,
        Some(MenuTarget {
            path: "/project/docs".into(),
            is_dir: true,
            is_root: false,
        })
    );
    assert!(!h.fs.calls().contains(&Call::ClipboardFiles));
}

#[gpui::test]
fn copies_a_native_file_drop_into_the_hovered_folder(cx: &mut TestAppContext) {
    let h = mount(outside_setup(), cx);
    let over = |target: &'static str, h: &mut Harness<'_>| {
        h.tree.update(h.cx, |tree, cx| {
            tree.drag_over(Some(target), cx);
            tree.drag_over_path().map(str::to_string)
        })
    };
    let mut h = h;
    assert_eq!(over("/project/docs", &mut h), Some("/project/docs".into()));
    // Hovering a file targets its folder.
    assert_eq!(over("/project/first.ts", &mut h), Some(CWD.into()));
    h.tree.update_in(h.cx, |tree, window, cx| {
        tree.drop_files(
            vec!["/Users/me/Desktop/a.txt".into()],
            "/project/docs",
            window,
            cx,
        )
        .detach()
    });
    h.cx.run_until_parked();
    assert_eq!(
        h.fs.copies(),
        vec![(
            "/Users/me/Desktop/a.txt".to_string(),
            "/project/docs".to_string()
        )]
    );
    assert_eq!(
        h.tree
            .read_with(h.cx, |tree, _| tree.drag_over_path().map(str::to_string)),
        None
    );
}

#[gpui::test]
fn copies_files_dropped_from_the_os_through_the_drop_listener(cx: &mut TestAppContext) {
    let h = mount(outside_setup(), cx);
    let docs =
        h.cx.debug_bounds("tree-row:/project/docs")
            .unwrap()
            .center();
    let paths = ExternalPaths(vec![std::path::PathBuf::from("/Users/me/Desktop/a.txt")].into());
    h.cx.simulate_event(gpui::FileDropEvent::Entered {
        position: docs,
        paths,
    });
    h.cx.simulate_event(gpui::FileDropEvent::Pending { position: docs });
    h.cx.run_until_parked();
    assert_eq!(
        h.tree
            .read_with(h.cx, |tree, _| tree.drag_over_path().map(str::to_string)),
        Some("/project/docs".into())
    );
    h.cx.simulate_event(gpui::FileDropEvent::Submit { position: docs });
    h.cx.run_until_parked();
    assert_eq!(
        h.fs.copies(),
        vec![(
            "/Users/me/Desktop/a.txt".to_string(),
            "/project/docs".to_string()
        )]
    );
}

#[gpui::test]
fn ignores_a_native_drop_outside_the_tree(cx: &mut TestAppContext) {
    let h = mount(outside_setup(), cx);
    let outside = point(px(5000.), px(5000.));
    let paths = ExternalPaths(vec![std::path::PathBuf::from("/Users/me/Desktop/a.txt")].into());
    h.cx.simulate_event(gpui::FileDropEvent::Entered {
        position: outside,
        paths,
    });
    h.cx.simulate_event(gpui::FileDropEvent::Submit { position: outside });
    h.cx.run_until_parked();
    assert!(h.fs.copies().is_empty());
}

#[gpui::test]
fn copies_the_selected_path_on_mod_shift_c(cx: &mut TestAppContext) {
    let mut h = mount(setup(vec![file("first.ts")]), cx);
    h.cx.write_to_clipboard(ClipboardItem::new_string("before".into()));
    h.select("/project/first.ts");
    h.keys("secondary-shift-c");
    assert_eq!(h.clipboard().as_deref(), Some("/project/first.ts"));
}

#[gpui::test]
fn copies_the_project_root_path_from_the_root_row(cx: &mut TestAppContext) {
    let mut h = mount(setup(vec![file("first.ts")]), cx);
    h.select("/project/first.ts");
    h.click("explorer-root");
    h.keys("secondary-shift-c");
    assert_eq!(h.clipboard().as_deref(), Some(CWD));
}

#[gpui::test]
fn matches_the_typed_letter_not_another_key(cx: &mut TestAppContext) {
    let mut h = mount(setup(vec![file("first.ts")]), cx);
    h.cx.write_to_clipboard(ClipboardItem::new_string("before".into()));
    h.select("/project/first.ts");
    // Dvorak types "j" on the physical C key.
    h.keys("secondary-shift-j");
    assert_eq!(h.clipboard().as_deref(), Some("before"));
}

#[gpui::test]
fn cuts_and_pastes_a_file_into_a_folder(cx: &mut TestAppContext) {
    let mut h = mount(outside_setup(), cx);
    h.select("/project/first.ts");
    h.keys("secondary-x");
    assert_eq!(
        h.tree.read_with(h.cx, |tree, _| tree.clip().cloned()),
        Some(Clip {
            mode: ClipMode::Cut,
            path: "/project/first.ts".into(),
            is_dir: false,
        })
    );
    h.select("/project/docs");
    h.keys("secondary-v");
    assert_eq!(
        h.fs.calls(),
        vec![Call::Move {
            from: "/project/first.ts".into(),
            dest_parent: "/project/docs".into(),
        }]
    );
    assert_eq!(
        h.events.borrow().as_slice(),
        [FileTreeEvent::FileMoved {
            from: "/project/first.ts".into(),
            to: "/project/docs/first.ts".into(),
        }]
    );
    assert_eq!(h.tree.read_with(h.cx, |tree, _| tree.clip().cloned()), None);
}

#[gpui::test]
fn refuses_to_paste_a_folder_into_itself(cx: &mut TestAppContext) {
    let mut h = mount(outside_setup(), cx);
    h.select("/project/docs");
    h.keys("secondary-c");
    h.keys("secondary-v");
    assert_eq!(
        h.tree
            .read_with(h.cx, |tree, _| tree.op_error().map(str::to_string)),
        Some("Cannot paste a folder into itself.".into())
    );
    assert!(h.fs.calls().is_empty());
    // Escape drops only a cut.
    h.keys("escape");
    assert!(h.tree.read_with(h.cx, |tree, _| tree.clip().is_some()));
    h.keys("secondary-x escape");
    assert!(h.tree.read_with(h.cx, |tree, _| tree.clip().is_none()));
}

#[gpui::test]
fn deletes_after_confirmation_and_selects_the_parent(cx: &mut TestAppContext) {
    let fs = outside_setup();
    fs.set_dir(
        "/project/docs",
        vec![FsEntry::file("a.md", "/project/docs/a.md")],
    );
    let mut h = mount(fs, cx);
    h.click("tree-row:/project/docs");
    h.select("/project/docs/a.md");
    h.keys("backspace");
    assert!(h.cx.has_pending_prompt());
    h.cx.simulate_prompt_answer("Cancel");
    h.cx.run_until_parked();
    assert!(h.fs.calls().is_empty());

    h.keys("delete");
    h.cx.simulate_prompt_answer("Delete");
    h.cx.run_until_parked();
    assert_eq!(
        h.fs.calls(),
        vec![Call::Delete {
            path: "/project/docs/a.md".into()
        }]
    );
    assert_eq!(
        h.tree
            .read_with(h.cx, |tree, _| tree.selected_path().map(str::to_string)),
        Some("/project/docs".into())
    );
    assert!(!h.paths().contains(&"/project/docs/a.md".to_string()));
    assert_eq!(
        h.events.borrow().last(),
        Some(&FileTreeEvent::FileDeleted {
            path: "/project/docs/a.md".into()
        })
    );
}

#[gpui::test]
fn creates_a_file_in_the_selected_folder_and_opens_it(cx: &mut TestAppContext) {
    let mut h = mount(outside_setup(), cx);
    h.select("/project/docs");
    h.tree.update_in(h.cx, |tree, window, cx| {
        tree.start_create(false, None, window, cx)
    });
    let rows = h.tree.read_with(h.cx, |tree, _| tree.rows());
    assert!(rows.contains(&TreeRow::NameInput {
        depth: 1,
        is_dir: false
    }));
    h.cx.simulate_input("notes.md");
    h.keys("enter");
    assert_eq!(
        h.fs.calls(),
        vec![Call::Create {
            parent: "/project/docs".into(),
            name: "notes.md".into(),
            is_dir: false,
        }]
    );
    assert!(h.paths().contains(&"/project/docs/notes.md".to_string()));
    assert_eq!(
        h.events.borrow().last(),
        Some(&FileTreeEvent::OpenFile {
            path: "/project/docs/notes.md".into(),
            options: FileOpenOptions::EXACT,
        })
    );
    assert!(
        h.tree
            .read_with(h.cx, |tree, _| tree.name_input().is_none())
    );
}

#[gpui::test]
fn blocks_an_existing_name_until_it_changes(cx: &mut TestAppContext) {
    let mut h = mount(outside_setup(), cx);
    h.tree.update_in(h.cx, |tree, window, cx| {
        tree.start_create(false, None, window, cx)
    });
    h.cx.simulate_input("FIRST.ts");
    h.keys("enter");
    assert!(h.fs.calls().is_empty());
    let shown = h.tree.read_with(h.cx, |tree, cx| {
        tree.name_row.as_ref().and_then(|row| row.shown_issue(cx))
    });
    assert_eq!(
        shown,
        Some(Ok(NameIssue::Exists {
            name: "FIRST.ts".into()
        }))
    );
    h.keys("escape");
    assert!(
        h.tree
            .read_with(h.cx, |tree, _| tree.name_input().is_none())
    );
    assert!(h.fs.calls().is_empty());
}

#[gpui::test]
fn renames_with_f2_and_reports_the_move(cx: &mut TestAppContext) {
    let mut h = mount(outside_setup(), cx);
    h.select("/project/first.ts");
    h.keys("f2");
    let input = h
        .tree
        .read_with(h.cx, |tree, _| tree.name_input().cloned())
        .unwrap();
    // The stem is selected, so typing replaces it and keeps the extension.
    assert_eq!(
        input.read_with(h.cx, |input, _| input.selected_range()),
        0..5
    );
    h.cx.simulate_input("second");
    h.keys("enter");
    assert_eq!(
        h.fs.calls(),
        vec![Call::Rename {
            path: "/project/first.ts".into(),
            name: "second.ts".into(),
        }]
    );
    assert_eq!(
        h.events.borrow().last(),
        Some(&FileTreeEvent::FileMoved {
            from: "/project/first.ts".into(),
            to: "/project/second.ts".into(),
        })
    );
    assert_eq!(
        h.tree
            .read_with(h.cx, |tree, _| tree.selected_path().map(str::to_string)),
        Some("/project/second.ts".into())
    );
}

#[gpui::test]
fn walks_rows_with_the_arrow_keys(cx: &mut TestAppContext) {
    let fs = outside_setup();
    fs.set_dir(
        "/project/docs",
        vec![FsEntry::file("a.md", "/project/docs/a.md")],
    );
    let mut h = mount(fs, cx);
    h.keys("down");
    assert_eq!(
        h.tree
            .read_with(h.cx, |tree, _| tree.selected_path().map(str::to_string)),
        Some("/project/docs".into())
    );
    h.keys("right");
    assert!(
        h.tree
            .read_with(h.cx, |tree, _| tree.expanded().contains("/project/docs"))
    );
    h.keys("right");
    assert_eq!(
        h.tree
            .read_with(h.cx, |tree, _| tree.selected_path().map(str::to_string)),
        Some("/project/docs/a.md".into())
    );
    h.keys("enter");
    assert_eq!(
        h.events.borrow().last(),
        Some(&FileTreeEvent::OpenFile {
            path: "/project/docs/a.md".into(),
            options: FileOpenOptions::EXACT,
        })
    );
    h.keys("left left");
    assert!(
        !h.tree
            .read_with(h.cx, |tree, _| tree.expanded().contains("/project/docs"))
    );
}

#[gpui::test]
fn runs_menu_actions_on_the_target(cx: &mut TestAppContext) {
    let mut h = mount(outside_setup(), cx);
    h.tree
        .update(h.cx, |tree, cx| tree.set_open_terminal_enabled(true, cx));
    h.tree.update_in(h.cx, |tree, window, cx| {
        tree.open_menu_for("/project/first.ts", point(px(10.), px(10.)), window, cx)
    });
    // Copy Relative Path is the ninth row; walk down to it.
    let items = h.tree.read_with(h.cx, |tree, cx| {
        tree.menu().unwrap().1.read(cx).items().to_vec()
    });
    let index = items
        .iter()
        .filter(|item| item.action().is_some())
        .position(|item| item.action().unwrap().id == "copy-relative-path")
        .unwrap();
    for _ in 0..index {
        h.keys("down");
    }
    h.keys("enter");
    assert_eq!(h.clipboard().as_deref(), Some("first.ts"));
    assert!(h.tree.read_with(h.cx, |tree, _| tree.menu().is_none()));

    let target = MenuTarget {
        path: "/project/first.ts".into(),
        is_dir: false,
        is_root: false,
    };
    h.tree.update_in(h.cx, |tree, window, cx| {
        tree.run_action("open-terminal", target, window, cx)
    });
    assert_eq!(
        h.events.borrow().last(),
        Some(&FileTreeEvent::OpenTerminal { cwd: CWD.into() })
    );
}

#[gpui::test]
fn starts_a_file_drag_without_opening_the_file(cx: &mut TestAppContext) {
    let h = mount(outside_setup(), cx);
    let start =
        h.cx.debug_bounds("tree-row:/project/first.ts")
            .unwrap()
            .center();
    h.cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    h.cx.simulate_mouse_move(
        start + point(px(20.), px(20.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    h.cx.run_until_parked();
    assert!(h.cx.update(|_, cx| cx.has_active_drag()));
    assert_eq!(
        h.tree
            .read_with(h.cx, |tree, _| tree.selected_path().map(str::to_string)),
        Some("/project/first.ts".into())
    );
    h.cx.simulate_mouse_up(
        start + point(px(20.), px(20.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    h.cx.run_until_parked();
    assert!(h.events.borrow().is_empty());
    // Folders do not drag.
    let docs =
        h.cx.debug_bounds("tree-row:/project/docs")
            .unwrap()
            .center();
    h.cx.simulate_mouse_down(docs, MouseButton::Left, Modifiers::none());
    h.cx.simulate_mouse_move(
        docs + point(px(30.), px(30.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    assert!(!h.cx.update(|_, cx| cx.has_active_drag()));
    h.cx.simulate_mouse_up(
        docs + point(px(30.), px(30.)),
        MouseButton::Left,
        Modifiers::none(),
    );
}

#[test]
fn builds_the_explorer_menu_like_the_typescript() {
    let target = MenuTarget {
        path: "/p/src".into(),
        is_dir: true,
        is_root: false,
    };
    let ids: Vec<String> = explorer_items(&target, None, true, Platform::Mac)
        .iter()
        .map(|item| {
            item.action()
                .map_or("-".to_string(), |action| action.id.to_string())
        })
        .collect();
    assert_eq!(
        ids,
        [
            "new-file",
            "new-folder",
            "-",
            "cut",
            "copy",
            "paste",
            "duplicate",
            "-",
            "copy-path",
            "copy-relative-path",
            "-",
            "rename",
            "delete",
            "-",
            "open-terminal",
            "reveal",
        ]
    );
    let root = MenuTarget {
        path: "/p".into(),
        is_dir: true,
        is_root: true,
    };
    let items = explorer_items(&root, None, false, Platform::Windows);
    let action = |id: &str| {
        items
            .iter()
            .filter_map(ExplorerMenuItem::action)
            .find(|action| action.id == id)
            .cloned()
            .unwrap()
    };
    assert!(action("cut").disabled);
    assert!(action("rename").disabled);
    assert!(!action("paste").disabled);
    assert_eq!(
        action("copy-path").shortcut.as_deref(),
        Some("Ctrl+Shift+C")
    );
    assert_eq!(action("reveal").label, "Reveal in File Explorer");
    assert!(
        items
            .iter()
            .all(|item| item.action().is_none_or(|a| a.id != "open-terminal"))
    );

    // A cut folder cannot be pasted into itself or below it.
    let clip = Clip {
        mode: ClipMode::Cut,
        path: "/p/src".into(),
        is_dir: true,
    };
    let inside = MenuTarget {
        path: "/p/src/lib.rs".into(),
        is_dir: false,
        is_root: false,
    };
    let items = explorer_items(&inside, Some(&clip), false, Platform::Mac);
    let paste = items
        .iter()
        .filter_map(ExplorerMenuItem::action)
        .find(|action| action.id == "paste")
        .unwrap();
    assert!(paste.disabled);
}

#[test]
fn lists_the_folders_a_create_or_move_touches() {
    assert_eq!(
        dirs_touched_by_create("/p", "a/b/c.ts"),
        vec!["/p".to_string(), "/p/a".into(), "/p/a/b".into()]
    );
    assert_eq!(
        dirs_touched_by_move("/p/a.ts", "/p/b.ts"),
        vec!["/p".to_string()]
    );
    assert_eq!(
        dirs_touched_by_move("/p/a.ts", "/q/a.ts"),
        vec!["/p".to_string(), "/q".into()]
    );
}
