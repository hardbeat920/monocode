//! WorkspacePicker.tsx had no tests of its own; the composer and worktree
//! tests drove it. These cover the toggle, the mode popover, the existing
//! worktree submenu, and the base branch picker.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

use super::*;
use crate::panes::test_support::{draw, init};

fn branch(name: &str, remote: Option<&str>) -> BaseBranch {
    BaseBranch {
        name: name.into(),
        remote: remote.map(str::to_string),
    }
}

#[test]
fn toggles_between_the_checkout_and_a_new_worktree() {
    assert_eq!(
        toggle_workspace_mode(WorkspaceMode::Current, Some("main")),
        Some((WorkspaceMode::Worktree, Some("main".into())))
    );
    assert_eq!(toggle_workspace_mode(WorkspaceMode::Current, None), None);
    assert_eq!(
        toggle_workspace_mode(WorkspaceMode::Worktree, Some("main")),
        Some((WorkspaceMode::Current, None))
    );
    let shortcut = |text: &str| is_workspace_mode_shortcut(&Keystroke::parse(text).unwrap());
    assert!(shortcut("cmd-shift-g"));
    assert!(shortcut("ctrl-shift-g"));
    assert!(!shortcut("cmd-g"));
    assert!(!shortcut("cmd-alt-shift-g"));
}

#[test]
fn lists_unique_base_branches_matching_the_query() {
    let branches = [
        branch("main", None),
        branch("release", Some("origin")),
        branch("main", None),
        branch("feature/grid", None),
    ];
    let refs = |query: &str| {
        base_branch_rows(&branches, query)
            .iter()
            .map(branch_ref)
            .collect::<Vec<_>>()
    };
    assert_eq!(refs(""), ["main", "origin/release", "feature/grid"]);
    assert_eq!(refs(" REL "), ["origin/release"]);
    assert!(refs("nothing").is_empty());
}

struct Harness<'a> {
    picker: Entity<WorkspacePicker>,
    events: Rc<RefCell<Vec<WorkspacePickerEvent>>>,
    cx: &'a mut VisualTestContext,
}

fn props(mode: WorkspaceMode) -> WorkspacePickerProps {
    WorkspacePickerProps {
        cwd: "/Users/me/code/agent-terminal".into(),
        mode,
        branches: Some(ProjectBranches {
            current: Some("main".into()),
            branches: vec![
                branch("main", None),
                branch("release", Some("origin")),
                branch("feature/grid", None),
            ],
        }),
        settled: true,
        can_select_worktree: true,
        can_open_settings: true,
        ..WorkspacePickerProps::default()
    }
}

fn mount(props: WorkspacePickerProps, cx: &mut TestAppContext) -> Harness<'_> {
    cx.update(init);
    let (picker, cx) = cx.add_window_view(move |window, cx| {
        let mut picker = WorkspacePicker::new(props, window, cx);
        picker.set_animate(false);
        // Keep the triggers low in the window so popovers open above them.
        picker
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&picker, move |_, event: &WorkspacePickerEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    draw(cx);
    Harness { picker, events, cx }
}

impl Harness<'_> {
    fn click(&mut self, selector: &'static str) {
        let at = self
            .cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("no element {selector}"))
            .center();
        self.cx.simulate_click(at, Modifiers::none());
        draw(self.cx);
    }

    fn hover(&mut self, selector: &'static str) {
        let at = self.cx.debug_bounds(selector).unwrap().center();
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        draw(self.cx);
    }

    fn events(&self) -> Vec<WorkspacePickerEvent> {
        self.events.borrow().clone()
    }
}

#[gpui::test]
fn picks_a_new_worktree_from_the_mode_popover(cx: &mut TestAppContext) {
    let mut h = mount(
        WorkspacePickerProps {
            popover_side: PopoverSide::Bottom,
            ..props(WorkspaceMode::Current)
        },
        cx,
    );
    h.click("workspace-mode-trigger");
    assert!(h.picker.read_with(h.cx, |picker, _| picker.is_mode_open()));
    assert!(h.events().contains(&WorkspacePickerEvent::OpenChange(true)));
    h.click("workspace-mode:Worktree");
    assert!(h.events().contains(&WorkspacePickerEvent::ModeChange {
        mode: WorkspaceMode::Worktree,
        base: Some("main".into()),
    }));
    assert!(h.events().contains(&WorkspacePickerEvent::Close));
    assert!(!h.picker.read_with(h.cx, |picker, _| picker.is_mode_open()));
}

#[gpui::test]
fn stays_shut_without_a_base_and_toggles_with_the_shortcut(cx: &mut TestAppContext) {
    let mut h = mount(
        WorkspacePickerProps {
            branches: None,
            settled: false,
            ..props(WorkspaceMode::Current)
        },
        cx,
    );
    h.click("workspace-mode-trigger");
    assert!(!h.picker.read_with(h.cx, |picker, _| picker.is_mode_open()));
    assert!(!h.picker.update(h.cx, |picker, cx| picker.toggle_mode(cx)));

    h.picker.update_in(h.cx, |picker, window, cx| {
        picker.set_props(props(WorkspaceMode::Worktree), window, cx)
    });
    assert!(h.picker.update(h.cx, |picker, cx| picker.toggle_mode(cx)));
    assert_eq!(
        h.events().last(),
        Some(&WorkspacePickerEvent::ModeChange {
            mode: WorkspaceMode::Current,
            base: None
        })
    );
}

#[gpui::test]
fn lists_existing_worktrees_and_reports_a_failed_switch(cx: &mut TestAppContext) {
    let mut h = mount(
        WorkspacePickerProps {
            popover_side: PopoverSide::Bottom,
            ..props(WorkspaceMode::Current)
        },
        cx,
    );
    h.click("workspace-mode-trigger");
    h.hover("workspace-existing-worktree");
    assert!(h.events().contains(&WorkspacePickerEvent::LoadWorktrees {
        cwd: "/Users/me/code/agent-terminal".into()
    }));
    let tree = |path: &str, branch: &str, main: bool| WorktreeEntry {
        path: path.into(),
        branch: Some(branch.into()),
        head: "abc1234def".into(),
        is_main: main,
        missing: false,
    };
    h.picker.update(h.cx, |picker, cx| {
        picker.set_worktrees(
            Ok(vec![
                tree("/Users/me/code/agent-terminal", "main", true),
                tree("/Users/me/code/agent-terminal-grid", "feature/grid", false),
            ]),
            cx,
        )
    });
    draw(h.cx);
    assert!(
        h.cx.debug_bounds("worktree:/Users/me/code/agent-terminal")
            .is_none()
    );
    // The pointer travels into the submenu before it clicks.
    h.hover("worktree:/Users/me/code/agent-terminal-grid");
    h.click("worktree:/Users/me/code/agent-terminal-grid");
    assert!(
        h.events()
            .contains(&WorkspacePickerEvent::SelectWorktree(tree(
                "/Users/me/code/agent-terminal-grid",
                "feature/grid",
                false
            )))
    );
    h.picker.update_in(h.cx, |picker, window, cx| {
        picker.worktree_selected(Err("Worktree is locked".into()), window, cx)
    });
    draw(h.cx);
    assert!(h.cx.debug_bounds("worktree-error").is_some());
    h.picker.update_in(h.cx, |picker, window, cx| {
        picker.worktree_selected(Ok(()), window, cx)
    });
    draw(h.cx);
    assert!(!h.picker.read_with(h.cx, |picker, _| picker.is_mode_open()));
}

#[gpui::test]
fn filters_and_picks_a_base_branch_from_the_keyboard(cx: &mut TestAppContext) {
    let mut h = mount(
        WorkspacePickerProps {
            popover_side: PopoverSide::Bottom,
            ..props(WorkspaceMode::Worktree)
        },
        cx,
    );
    h.click("workspace-base-trigger");
    assert!(h.picker.read_with(h.cx, |picker, _| picker.is_base_open()));
    assert!(h.cx.debug_bounds("base-branch:origin/release").is_some());
    h.cx.simulate_input("re");
    draw(h.cx);
    assert!(h.cx.debug_bounds("base-branch:main").is_none());
    h.cx.simulate_keystrokes("down");
    draw(h.cx);
    h.cx.simulate_keystrokes("enter");
    draw(h.cx);
    assert!(
        h.events()
            .contains(&WorkspacePickerEvent::BaseChange("feature/grid".into()))
    );
    assert!(!h.picker.read_with(h.cx, |picker, _| picker.is_base_open()));
}
