//! The source control views on a real repository, for checking them by eye
//! or by screenshot.
//!
//! ```text
//! cargo run -p monocode-view-scm --example scm_gallery -- [options]
//!   --view <name>        panel (default), diff, split, commit, branch,
//!                        worktree, worktrees, create-branch, switch-branch,
//!                        switch-running, create-worktree, delete-worktree,
//!                        comment, pr, pr-confirm
//!   --repo <dir>         use this repository instead of the current one
//!   --light              light theme
//!   --size <w>x<h>       window size in points
//!   --screenshot <png>   write what the window draws, then quit. The window
//!                        opens without focus and the app is not activated.
//! ```
//!
//! `panel` is the changes panel with its graph, `diff` the working tree
//! diff, and `split` both side by side, as in the app. Git mutations run for
//! real against `--repo`.

use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyView, App, AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement as _,
    Render, Styled as _, Task, Window, WindowBounds, WindowOptions, div, point, px, size,
};
use gpui_component::Root;
use monocode_editor::unified_diff::{DiffCommentTarget, UnifiedLine, UnifiedLineKind};
use monocode_ui::{AppearanceSettings, Theme, ThemePreference, u};
use monocode_view_scm::Scm;
use monocode_view_scm::git::Worktree;
use monocode_view_scm::hooks::{CommitMessageRequest, ScmHooks};
use monocode_view_scm::model::pr_actions::GithubPrAction;
use monocode_view_scm::ui::branch_picker::{BranchPicker, PickerSide};
use monocode_view_scm::ui::changes_panel::GitChangesPanel;
use monocode_view_scm::ui::dialogs::create_branch::CreateBranchDialog;
use monocode_view_scm::ui::dialogs::create_worktree::CreateWorktreeDialog;
use monocode_view_scm::ui::dialogs::delete_worktree::DeleteWorktreeDialog;
use monocode_view_scm::ui::dialogs::switch_branch::SwitchBranchDialog;
use monocode_view_scm::ui::dialogs::switch_while_running::SwitchWhileRunningDialog;
use monocode_view_scm::ui::diff_comment_composer::DiffCommentComposer;
use monocode_view_scm::ui::diffs::{CommitDiff, WorkingTreeDiff};
use monocode_view_scm::ui::pr_actions::{GithubPrActions, PrItem};
use monocode_view_scm::ui::worktree_picker::WorktreePicker;
use monocode_view_scm::ui::worktrees_page::WorktreesPage;

struct Args {
    view: String,
    repo: PathBuf,
    light: bool,
    size: (f32, f32),
    screenshot: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args {
        view: "panel".into(),
        repo: std::env::current_dir().expect("current dir"),
        light: false,
        size: (360., 760.),
        screenshot: None,
    };
    let mut size_set = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--view" => parsed.view = args.next().expect("--view <name>"),
            "--repo" => parsed.repo = args.next().map(PathBuf::from).expect("--repo <dir>"),
            "--light" => parsed.light = true,
            "--size" => {
                let value = args.next().expect("--size <w>x<h>");
                let (w, h) = value.split_once('x').expect("--size <w>x<h>");
                parsed.size = (w.parse().expect("width"), h.parse().expect("height"));
                size_set = true;
            }
            "--screenshot" => parsed.screenshot = args.next().map(PathBuf::from),
            other => eprintln!("unknown argument {other}"),
        }
    }
    if !size_set {
        parsed.size = match parsed.view.as_str() {
            "panel" => (360., 760.),
            "split" => (1240., 760.),
            "diff" | "commit" => (900., 700.),
            "worktrees" => (720., 560.),
            "pr" | "pr-confirm" => (720., 360.),
            "branch" | "worktree" => (480., 520.),
            _ => (640., 560.),
        };
    }
    parsed
}

/// Generation stand-in: writes a fixed message after a short wait.
fn gallery_hooks() -> ScmHooks {
    ScmHooks {
        generate_commit_message: Some(Rc::new(|request: CommitMessageRequest, cx: &mut App| {
            cx.background_spawn(async move {
                std::thread::sleep(Duration::from_millis(400));
                Ok(format!("Update {}", request.cwd))
            })
        })),
        add_to_chat: Some(Rc::new(|item, _| eprintln!("add to chat: {item:?}"))),
        ..Default::default()
    }
}

fn git_stdout(repo: &PathBuf, args: &[&str]) -> String {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

struct Gallery {
    content: AnyView,
    padded: bool,
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let mut root = div()
            .size_full()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone());
        if self.padded {
            root = root.p(u(24.));
        }
        root.child(self.content.clone())
    }
}

/// Pins a picker to the bottom of the window, where the composer has it.
struct Bottom {
    child: AnyView,
}

impl Render for Bottom {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_end()
            .child(self.child.clone())
    }
}

struct Split {
    panel: Entity<GitChangesPanel>,
    diff: Entity<WorkingTreeDiff>,
}

impl Render for Split {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .flex()
            .size_full()
            .child(div().flex_1().min_w_0().h_full().child(self.diff.clone()))
            .child(
                div()
                    .w(u(360.))
                    .h_full()
                    .flex_none()
                    .border_l_1()
                    .border_color(theme.colors.stroke)
                    .child(self.panel.clone()),
            )
    }
}

fn sample_pr() -> PrItem {
    PrItem {
        project_path: "/tmp/web".into(),
        repo: "acme/web".into(),
        number: 42,
        title: "Ship the new inbox".into(),
        url: "https://github.com/acme/web/pull/42".into(),
        state: "open".into(),
        draft: false,
        updated_at: "2026-09-16T08:00:00Z".into(),
    }
}

fn sample_tree() -> Worktree {
    Worktree {
        path: "/Users/me/code/monocode-worktrees/feature-graph".into(),
        branch: Some("feature/graph".into()),
        head: "abc1234".into(),
        is_main: false,
        locked: false,
        prunable: false,
        missing: false,
        dirty: Some(true),
        unpushed: Some(2),
        session_ids: vec!["one".into(), "two".into()],
    }
}

/// Builds the view. Returns it and whether it wants page padding.
fn build(args: &Args, scm: Scm, window: &mut Window, cx: &mut App) -> (AnyView, bool) {
    let cwd = args.repo.to_string_lossy().to_string();
    let wrap = |el: AnyView| (el, false);
    match args.view.as_str() {
        "panel" => wrap(cx.new(|cx| GitChangesPanel::new(scm, cwd, true, window, cx)).into()),
        "diff" => wrap(cx.new(|cx| WorkingTreeDiff::new(scm, cwd, None, None, window, cx)).into()),
        "split" => {
            let panel = cx.new(|cx| GitChangesPanel::new(scm.clone(), cwd.clone(), true, window, cx));
            let diff = cx.new(|cx| WorkingTreeDiff::new(scm, cwd, None, None, window, cx));
            wrap(cx.new(|_| Split { panel, diff }).into())
        }
        "commit" => {
            let sha = git_stdout(&args.repo, &["rev-parse", "HEAD"]);
            wrap(cx.new(|cx| CommitDiff::new(scm, cwd, sha, cx)).into())
        }
        "branch" => {
            let picker = cx.new(|cx| {
                let mut picker = BranchPicker::new(scm, cwd, None, window, cx);
                picker.set_side(PickerSide::Top);
                picker
            });
            // Open once the branches have loaded.
            let opener = picker.clone();
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor().timer(Duration::from_millis(600)).await;
                    let _ = opener.update_in(cx, |picker, window, cx| picker.open_popover(window, cx));
                })
                .detach();
            (cx.new(|_| Bottom { child: picker.into() }).into(), true)
        }
        "worktree" => {
            let picker = cx.new(|cx| {
                let select = Rc::new(|_: Worktree, _: &mut Window, _: &mut App| Task::ready(Ok(())));
                let mut picker = WorktreePicker::new(scm, cwd.clone(), cwd, false, false, select, window, cx);
                picker.set_can_manage(true);
                picker
            });
            let opener = picker.clone();
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor().timer(Duration::from_millis(600)).await;
                    let _ = opener.update_in(cx, |picker, window, cx| picker.toggle(window, cx));
                })
                .detach();
            (cx.new(|_| Bottom { child: picker.into() }).into(), true)
        }
        "worktrees" => {
            let remove = Rc::new(|_, _: &mut App| Task::ready(Err("The gallery does not delete worktrees".to_string())));
            (cx.new(|cx| WorktreesPage::new(scm, cwd, remove, window, cx)).into(), true)
        }
        "create-branch" => wrap(cx.new(|cx| CreateBranchDialog::new(window, cx)).into()),
        "switch-branch" => wrap(cx.new(|cx| SwitchBranchDialog::new(scm, cwd, "feature/graph", false, window, cx)).into()),
        "switch-running" => wrap(
            cx.new(|_| {
                SwitchWhileRunningDialog::new(
                    "feature/graph",
                    false,
                    "Host rejected request: Switching branches changes the files 2 running sessions use: Fix the parser, Add tests.",
                )
            })
            .into(),
        ),
        "create-worktree" => {
            let root = format!("{cwd}-worktrees");
            wrap(cx.new(|cx| CreateWorktreeDialog::new(scm, cwd.clone(), cwd, Some(root), window, cx)).into())
        }
        "delete-worktree" => {
            let remove = Rc::new(|_, _: &mut Window, _: &mut App| Task::ready(Ok(())));
            wrap(cx.new(|_| DeleteWorktreeDialog::new(cwd, sample_tree(), 2, remove)).into())
        }
        "comment" => {
            let target = DiffCommentTarget {
                path: "src/auth.ts".into(),
                line: UnifiedLine {
                    kind: UnifiedLineKind::Add,
                    text: "const token = readCookie();".into(),
                    old_number: None,
                    new_number: Some(42),
                    pos: None,
                },
            };
            wrap(cx.new(|cx| DiffCommentComposer::new(scm, target, point(px(40.), px(40.)), window, cx)).into())
        }
        "pr" | "pr-confirm" => {
            let confirm = args.view == "pr-confirm";
            let actions = cx.new(|cx| {
                let mut actions = GithubPrActions::new(scm, sample_pr(), "main", "feature/inbox");
                if confirm {
                    actions.choose_merge(GithubPrAction::Squash, cx);
                    actions.ask(GithubPrAction::Squash, cx);
                }
                actions
            });
            (actions.into(), true)
        }
        other => {
            eprintln!("unknown view {other}");
            std::process::exit(2);
        }
    }
}

fn main() {
    let args = parse_args();
    gpui_platform::application()
        .with_assets(monocode_ui::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            let mut appearance = AppearanceSettings::default();
            if args.light {
                appearance.theme_preference = ThemePreference::parse(Some("light"));
            }
            monocode_ui::init(appearance, cx);
            monocode_editor::init(cx);
            let scm = Scm::local(gallery_hooks(), cx);
            let screenshot = args.screenshot.clone();
            let bounds = Bounds::centered(None, size(px(args.size.0), px(args.size.1)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        // A screenshot run must never take focus.
                        focus: screenshot.is_none(),
                        ..Default::default()
                    },
                    |window, cx| {
                        monocode_ui::sync_window(window, cx);
                        let (content, padded) = build(&args, scm, window, cx);
                        let gallery = cx.new(|_| Gallery { content, padded });
                        cx.new(|cx| Root::new(gallery, window, cx))
                    },
                )
                .expect("open window");
            let Some(out) = screenshot else {
                cx.activate(true);
                return;
            };
            cx.spawn(async move |cx| {
                // Let git answer, images load, and highlighting finish.
                for _ in 0..30 {
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                    let _ = window.update(cx, |_, window, _| window.refresh());
                }
                let any: gpui::AnyWindowHandle = window.into();
                let result = any.update(cx, |_, window, cx| {
                    window.dispatch_event(
                        gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                            position: point(px(-1000.), px(-1000.)),
                            ..Default::default()
                        }),
                        cx,
                    );
                    window.draw(cx).clear();
                    window.render_to_image().map(|image| image.save(&out))
                });
                match result {
                    Ok(Ok(Ok(()))) => eprintln!("wrote {}", out.display()),
                    other => eprintln!("screenshot failed: {other:?}"),
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}
