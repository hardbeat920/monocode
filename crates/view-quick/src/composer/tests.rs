//! Checks submission and draft preservation in a GPUI test window.

use std::cell::RefCell;

use gpui::{TestAppContext, VisualTestContext, px, size};
use monocode_core::Attachment;
use monocode_view_composer::composer::model::clipboard::ClipboardFile;

use super::*;
use crate::{HostTask, NativeClipboard, QuickGitHost, Worktree};

#[derive(Default)]
struct Host {
    launches: RefCell<Vec<QuickLaunchRequest>>,
    error: RefCell<Option<String>>,
    initial_project: RefCell<Option<String>>,
}
impl QuickGitHost for Host {
    fn branches(&self, _: &str, _: &mut App) -> Task<Option<GitBranches>> {
        Task::ready(None)
    }
    fn worktrees(&self, _: &str, _: &mut App) -> HostTask<Vec<Worktree>> {
        Task::ready(Ok(Vec::new()))
    }
    fn checkout(&self, _: &str, _: &str, _: Option<&str>, _: bool, _: &mut App) -> HostTask<()> {
        Task::ready(Ok(()))
    }
    fn create_branch(&self, _: &str, _: &str, _: bool, _: &mut App) -> HostTask<()> {
        Task::ready(Ok(()))
    }
    fn stash(&self, _: &str, _: &str, _: &mut App) -> HostTask<()> {
        Task::ready(Ok(()))
    }
    fn commit_all(&self, _: &str, _: &str, _: &mut App) -> HostTask<()> {
        Task::ready(Ok(()))
    }
}
impl QuickComposerHost for Host {
    fn snapshot(&self, _: &mut App) -> QuickSnapshot {
        let catalog = ModelCatalog::new();
        let mut snapshot = QuickSnapshot::new(LastModelChoice {
            harness: HarnessId::Claude,
            model: catalog.default_model_id(HarnessId::Claude),
        });
        let project = self
            .initial_project
            .borrow()
            .clone()
            .unwrap_or_else(|| "/repo".into());
        snapshot.projects = vec![project.clone()];
        snapshot.initial_project = Some(project);
        snapshot.catalog = catalog;
        snapshot
    }
    fn submit(&self, request: QuickLaunchRequest, _: &mut App) -> HostTask<()> {
        self.launches.borrow_mut().push(request);
        Task::ready(self.error.borrow().clone().map_or(Ok(()), Err))
    }
    fn pick_attachments(&self, _: &mut Window, _: &mut App) -> HostTask<Vec<Attachment>> {
        Task::ready(Ok(Vec::new()))
    }
    fn attachments_from_paths(&self, _: Vec<String>, _: &mut App) -> HostTask<Vec<Attachment>> {
        Task::ready(Ok(Vec::new()))
    }
    fn attachments_from_files(
        &self,
        _: Vec<ClipboardFile>,
        _: &mut App,
    ) -> HostTask<Vec<Attachment>> {
        Task::ready(Ok(Vec::new()))
    }
    fn native_clipboard(&self, _: &str, _: &mut App) -> HostTask<NativeClipboard> {
        Task::ready(Ok(NativeClipboard::default()))
    }
    fn store_attachments(&self, files: Vec<Attachment>, _: &mut App) -> HostTask<Vec<Attachment>> {
        Task::ready(Ok(files))
    }
    fn capture_screenshot(&self, _: &mut Window, _: &mut App) -> HostTask<Option<String>> {
        Task::ready(Ok(None))
    }
}
fn mount(
    cx: &mut TestAppContext,
    host: Rc<Host>,
) -> (Entity<QuickComposer>, &'static mut VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(Default::default(), cx);
        monocode_view_composer::composer::init(cx);
        super::init(cx);
    });
    let window = cx.open_window(size(px(680.), px(520.)), move |window, cx| {
        QuickComposer::new(host, window, cx)
    });
    let view = window.root(cx).expect("composer");
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    (view, cx)
}
#[gpui::test]
fn launch_clears_the_draft_and_dismisses_only_after_success(cx: &mut TestAppContext) {
    let host = Rc::new(Host::default());
    let (view, cx) = mount(cx, host.clone());
    let events = Rc::new(RefCell::new(Vec::new()));
    let out = events.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event, _| {
            out.borrow_mut().push(event.clone())
        })
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.prompt.update(cx, |prompt, cx| {
                prompt.set_text("/plan Fix the tests", 19, cx)
            });
            view.submit(true, window, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(host.launches.borrow()[0].intent, Some(QuickIntent::Plan));
    assert!(host.launches.borrow()[0].reveal);
    assert_eq!(view.read_with(cx, |view, cx| view.text(cx)), "");
    assert!(events.borrow().contains(&QuickComposerEvent::Dismiss));
}
#[gpui::test]
fn rejected_launch_keeps_text_and_exposes_the_error(cx: &mut TestAppContext) {
    let host = Rc::new(Host::default());
    *host.error.borrow_mut() = Some("Workspace unavailable".into());
    let (view, cx) = mount(cx, host);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.prompt
                .update(cx, |prompt, cx| prompt.set_text("Fix the tests", 13, cx));
            view.submit(false, window, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, cx| view.text(cx)),
        "Fix the tests"
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.error.clone()),
        Some("Workspace unavailable".into())
    );
    assert!(!view.read_with(cx, |view, _| view.busy));
}

#[gpui::test]
fn showing_after_dismiss_keeps_the_draft_and_refreshes_the_workspace(cx: &mut TestAppContext) {
    let host = Rc::new(Host::default());
    let (view, cx) = mount(cx, host.clone());
    let events = Rc::new(RefCell::new(Vec::new()));
    let out = events.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event, _| {
            out.borrow_mut().push(event.clone())
        })
    });
    let file = Attachment {
        id: "retained-file".into(),
        name: "brief.pdf".into(),
        mime_type: "application/pdf".into(),
        path: Some("/repo/brief.pdf".into()),
        ..Attachment::default()
    };
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.show(window, cx);
            view.prompt.update(cx, |prompt, cx| {
                prompt.set_text("Keep this unfinished draft", 26, cx)
            });
            view.attachments.files.push(file.clone());
            view.dismiss(cx);
        });
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| **event == QuickComposerEvent::Dismiss)
            .count(),
        1
    );
    *host.initial_project.borrow_mut() = Some("/other-repo".into());
    cx.update(|window, cx| view.update(cx, |view, cx| view.show(window, cx)));
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert_eq!(view.text(cx), "Keep this unfinished draft");
        assert_eq!(view.attachments().files(), &[file]);
        assert_eq!(view.cwd.as_deref(), Some("/other-repo"));
        assert_eq!(view.projects, ["/other-repo"]);
    });
    cx.update(|window, cx| {
        view.read_with(cx, |view, cx| {
            assert!(view.prompt.focus_handle(cx).is_focused(window));
        });
    });
    assert!(host.launches.borrow().is_empty());
}

#[gpui::test]
fn permissions_open_as_their_own_picker_and_close_after_a_pick(cx: &mut TestAppContext) {
    let host = Rc::new(Host::default());
    let (view, cx) = mount(cx, host.clone());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_picker(Picker::Permissions, window, cx)
        })
    });
    let permissions = view.read_with(cx, |view, _| {
        assert_eq!(view.picker(), Some(Picker::Permissions));
        view.permissions().cloned().expect("the permissions list")
    });
    cx.update(|window, cx| {
        assert!(permissions.focus_handle(cx).is_focused(window));
    });
    // Full access is the fourth mode.
    cx.update(|_, cx| {
        permissions.update(cx, |permissions, cx| {
            for _ in 0..3 {
                permissions.key("down", cx);
            }
            permissions.key("enter", cx);
        })
    });
    view.read_with(cx, |view, _| {
        assert_eq!(view.runtime_mode(), RuntimeMode::FullAccess);
        assert_eq!(view.picker(), None);
        assert!(view.permissions().is_none());
    });
    cx.update(|window, cx| {
        view.read_with(cx, |view, cx| {
            assert!(view.prompt.focus_handle(cx).is_focused(window));
        });
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.prompt
                .update(cx, |prompt, cx| prompt.set_text("Fix the tests", 13, cx));
            view.submit(false, window, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(
        host.launches.borrow()[0].runtime_mode,
        Some(RuntimeMode::FullAccess)
    );
}

#[gpui::test]
fn escape_closes_permissions_without_changing_the_mode(cx: &mut TestAppContext) {
    let host = Rc::new(Host::default());
    let (view, cx) = mount(cx, host);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_picker(Picker::Permissions, window, cx)
        })
    });
    let permissions = view.read_with(cx, |view, _| view.permissions().cloned().unwrap());
    cx.update(|_, cx| {
        permissions.update(cx, |permissions, cx| {
            permissions.key("down", cx);
            permissions.key("escape", cx);
        })
    });
    view.read_with(cx, |view, _| {
        assert_eq!(view.runtime_mode(), DEFAULT_RUNTIME_MODE);
        assert_eq!(view.picker(), None);
    });
}

#[gpui::test]
fn a_second_press_closes_permissions_and_the_model_picker_replaces_them(cx: &mut TestAppContext) {
    let host = Rc::new(Host::default());
    let (view, cx) = mount(cx, host);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_picker(Picker::Permissions, window, cx);
            view.open_picker(Picker::Permissions, window, cx);
        })
    });
    view.read_with(cx, |view, _| {
        assert_eq!(view.picker(), None);
        assert!(view.permissions().is_none());
    });
    // Switching from permissions to the model selector drops the list.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_picker(Picker::Permissions, window, cx);
            view.open_picker(Picker::Model, window, cx);
        })
    });
    view.read_with(cx, |view, _| {
        assert_eq!(view.picker(), Some(Picker::Model));
        assert!(view.permissions().is_none());
        assert!(view.selector().is_some());
    });
}
