//! The workbench gallery: the pane views on mock data, one scene at a time.
//!
//! ```text
//! cargo run -p monocode-view-workbench --example workbench_gallery -j 4 -- [options]
//!   --scene <name>       panes, menus, empty, opus, astra, signin, review (default panes)
//!   --theme dark|light   color scheme (default dark)
//!   --size WxH           window size in points (default 1280x800)
//!   --screenshot <png>   write what the window draws, then quit
//! ```
//!
//! A screenshot run opens its window without focus and never activates the
//! app, so it cannot take the keyboard from whatever is in front.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyView, App, AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement as _,
    Render, Styled as _, Task, Window, WindowBackgroundAppearance, WindowBounds, WindowOptions,
    div, point, px, size,
};
use monocode_core::HarnessId;
use monocode_core::session::WorkspaceMode;
use monocode_layout::pane_drop::PaneDrop;
use monocode_layout::{FilePaneTab, LayoutNode, PaneEdge, SplitDir, leaf};
use monocode_ui::widgets::PopoverSide;
use monocode_ui::{AppearanceSettings, IconName, Theme, ThemePreference, UiStyled as _, icon, u};
use monocode_view_workbench::panes::agent_tab_view::{AgentTabSession, AgentTabView};
use monocode_view_workbench::panes::empty_session::{EmptySession, EmptySessionProps};
use monocode_view_workbench::panes::notices::{
    UsageLimit, UsageLimitNotice, discussion_empty, relative_reset_formatter, sessions_empty,
};
use monocode_view_workbench::panes::pane_tree::{PaneLeaf, PaneLeafKind, PaneTree};
use monocode_view_workbench::panes::provider_sign_in::{Login, ProviderSignInDialog};
use monocode_view_workbench::panes::session_review::{
    CheckpointFile, SessionReview, SessionReviewHost, SessionReviewProps,
};
use monocode_view_workbench::panes::surface_tabs::{
    ClipboardOnlyActions, SurfaceTabs, SurfaceTabsProps,
};
use monocode_view_workbench::panes::tab_group_menu::{
    SubmenuEntry, TabGroupMenu, TabGroupMenuExtraItem, TabGroupMenuProps,
};
use monocode_view_workbench::panes::welcome::{ComposerBand, ModelWelcome, WelcomeKind};
use monocode_view_workbench::panes::workspace_picker::{
    BaseBranch, InitialPicker, ProjectBranches, WorkspacePicker, WorkspacePickerProps,
    WorktreeEntry,
};

struct Options {
    scene: String,
    light: bool,
    size: (f32, f32),
    screenshot: Option<PathBuf>,
}

fn parse_options() -> Options {
    let mut args = std::env::args().skip(1);
    let mut options = Options {
        scene: "panes".into(),
        light: false,
        size: (1280., 800.),
        screenshot: None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scene" => options.scene = args.next().unwrap_or_default(),
            "--theme" => options.light = args.next().as_deref() == Some("light"),
            "--size" => {
                if let Some((w, h)) = args.next().as_deref().and_then(|v| v.split_once('x')) {
                    options.size = (w.parse().unwrap_or(1280.), h.parse().unwrap_or(800.));
                }
            }
            "--screenshot" => options.screenshot = args.next().map(PathBuf::from),
            other => eprintln!("ignoring {other}"),
        }
    }
    options
}

/// A transcript stand-in: a few paragraphs and a finished-turn line.
struct MockTranscript {
    paragraphs: Vec<&'static str>,
    composer: Entity<MockComposer>,
}

impl Render for MockTranscript {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let mut body = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(u(14.))
            .px(u(20.))
            .pt(u(14.));
        for paragraph in &self.paragraphs {
            body = body.child(
                div()
                    .text_px(theme.text.body + 1.)
                    .leading(1.55)
                    .text_color(theme.content(0.85))
                    .child(*paragraph),
            );
        }
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap(u(8.))
                .text_px(theme.text.body)
                .text_color(theme.content(0.45))
                .child(
                    icon(IconName::Check)
                        .size(u(14.))
                        .text_color(theme.content(0.45)),
                )
                .child("Worked for 2m 39s"),
        );
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(body)
            .child(div().p(u(6.)).child(self.composer.clone()))
    }
}

/// A composer stand-in with the top bar the workspace picker sits in.
struct MockComposer {
    picker: Option<Entity<WorkspacePicker>>,
}

impl Render for MockComposer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let chip = |label: &'static str| {
            div()
                .flex()
                .items_center()
                .h(u(28.))
                .px(u(8.))
                .rounded(u(theme.radius.md))
                .bg(theme.content(0.05))
                .text_px(theme.text.label)
                .text_color(theme.content(0.75))
                .child(label)
        };
        let mut top = div()
            .flex()
            .items_center()
            .gap(u(12.))
            .px(u(14.))
            .pt(u(10.))
            .text_px(12.)
            .text_color(theme.content(0.55))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .font_family(theme.fonts.mono.clone())
                    .child(
                        icon(IconName::Folder)
                            .size(u(14.))
                            .text_color(theme.content(0.55)),
                    )
                    .child("~/code/agent-terminal"),
            );
        if let Some(picker) = self.picker.clone() {
            top = top.child(picker);
        } else {
            top = top.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .child(
                        icon(IconName::GitBranch)
                            .size(u(14.))
                            .text_color(theme.content(0.55)),
                    )
                    .child("main"),
            );
        }
        div()
            .flex()
            .flex_col()
            .w_full()
            .rounded(u(16.))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.03))
            .child(top)
            .child(
                div()
                    .px(u(14.))
                    .py(u(12.))
                    .text_px(theme.text.body + 1.)
                    .text_color(theme.content(0.40))
                    .child("Ask, build, / for skills..."),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .px(u(8.))
                    .pb(u(8.))
                    .child(chip("+"))
                    .child(chip("Claude Opus 5.5"))
                    .child(chip("High"))
                    .child(chip("Supervised")),
            )
    }
}

/// A file pane stand-in: the tab strip over a few lines of code.
struct MockFilePane {
    tabs: Entity<SurfaceTabs>,
}

impl Render for MockFilePane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let lines = [
            "export function light(",
            "  out: Float32Array,",
            "  cols: number,",
            "  rows: number,",
            "  x: number,",
            "  y: number,",
            "  value: number,",
            ") {",
            "  if (x < 0 || y < 0 || x >= cols || y >= rows) return;",
            "  const index = y * cols + x;",
            "  if (out[index] < value) out[index] = value;",
            "}",
        ];
        let mut code = div()
            .flex()
            .flex_col()
            .pt(u(6.))
            .font_family(theme.fonts.mono.clone())
            .text_px(13.)
            .leading(1.6);
        for (index, line) in lines.iter().enumerate() {
            code = code.child(
                div()
                    .flex()
                    .gap(u(18.))
                    .child(
                        div()
                            .w(u(40.))
                            .flex()
                            .justify_end()
                            .text_color(theme.content(0.30))
                            .child(format!("{}", index + 84)),
                    )
                    .child(div().text_color(theme.content(0.85)).child(*line)),
            );
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(self.tabs.clone())
            .child(code)
    }
}

/// A checkpoint store with a fixed set of changed files.
struct MockReview;

impl SessionReviewHost for MockReview {
    fn status(&self, _: &str, _: &str, _: &mut App) -> Task<Result<Vec<CheckpointFile>, String>> {
        let file = |relative: &str, additions, deletions, exact| CheckpointFile {
            path: format!("/Users/me/code/agent-terminal/{relative}"),
            relative: relative.into(),
            additions,
            deletions,
            exact,
            undoable: true,
        };
        Task::ready(Ok(vec![
            file("src/surfaces/gridArcade.ts", 100, 217, true),
            file("src/surfaces/speechBubble.ts", 155, 0, true),
            file("src/surfaces/speechBubble.test.ts", 111, 0, true),
            file("src/chrome/HarnessIcon.tsx", 12, 4, false),
            file("src/chrome/TerminalGridBackground.tsx", 9, 3, true),
        ]))
    }

    fn keep(&self, _: &str, _: &str, _: &mut App) -> Task<Result<Vec<CheckpointFile>, String>> {
        Task::ready(Ok(Vec::new()))
    }

    fn undo(&self, _: &str, _: &str, _: &mut App) -> Task<Result<Vec<CheckpointFile>, String>> {
        Task::ready(Ok(Vec::new()))
    }
}

struct Gallery {
    body: AnyView,
    overlays: Vec<AnyView>,
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .relative()
            .size_full()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .child(self.body.clone())
            .children(self.overlays.clone())
    }
}

fn composer(picker: Option<Entity<WorkspacePicker>>, cx: &mut App) -> Entity<MockComposer> {
    cx.new(|_| MockComposer { picker })
}

fn workspace_picker(window: &mut Window, cx: &mut App) -> Entity<WorkspacePicker> {
    cx.new(|cx| {
        let mut picker = WorkspacePicker::new(
            WorkspacePickerProps {
                cwd: "/Users/me/code/agent-terminal".into(),
                mode: WorkspaceMode::Worktree,
                branches: Some(ProjectBranches {
                    current: Some("main".into()),
                    branches: vec![
                        BaseBranch {
                            name: "main".into(),
                            remote: None,
                        },
                        BaseBranch {
                            name: "main".into(),
                            remote: Some("origin".into()),
                        },
                    ],
                }),
                settled: true,
                can_select_worktree: true,
                can_open_settings: true,
                popover_side: PopoverSide::Top,
                ..WorkspacePickerProps::default()
            },
            window,
            cx,
        );
        picker.set_animate(false);
        picker
    })
}

fn empty_session(composer: Entity<MockComposer>, cx: &mut App) -> Entity<EmptySession> {
    cx.new(|cx| {
        let mut screen = EmptySession::new(EmptySessionProps {
            cwd: "/Users/me/code/agent-terminal".into(),
            project: Some("agent-terminal".into()),
            has_chat_background: false,
            arcade_enabled: true,
        });
        screen.set_composer(Some(composer.into()), cx);
        screen
    })
}

fn surface_tabs(cx: &mut App) -> Entity<SurfaceTabs> {
    let mut grid = FilePaneTab::new(
        "f-grid",
        "/Users/me/code/agent-terminal/src/surfaces/gridArcade.ts",
        "/Users/me/code/agent-terminal",
    );
    grid.preview = None;
    let bubble = FilePaneTab::new(
        "f-bubble",
        "/Users/me/code/agent-terminal/src/surfaces/speechBubble.ts",
        "/Users/me/code/agent-terminal",
    );
    let mut terminal = FilePaneTab::new("f-term", "vite", "/Users/me/code/agent-terminal");
    terminal.terminal = Some(true);
    let changes =
        monocode_layout::new_changes_tab("/Users/me/code/agent-terminal", None, None, None);
    let props = SurfaceTabsProps {
        files: vec![grid, bubble, terminal, changes],
        active_file_id: "f-grid".into(),
        dirty_file_ids: ["f-bubble".to_string()].into_iter().collect(),
        file_error_counts: [("f-bubble".to_string(), 2)].into_iter().collect(),
        can_pin: true,
        can_drag_pane: true,
        ..SurfaceTabsProps::default()
    };
    cx.new(|cx| SurfaceTabs::new(props, Rc::new(ClipboardOnlyActions), cx))
}

/// The three-pane layout from docs/screenshot.jpg: a chat on the left, a
/// chat over a file pane on the right.
fn pane_tree(right_top: AnyView, cx: &mut App) -> Entity<PaneTree> {
    let layout = LayoutNode::split(
        "root",
        SplitDir::Right,
        vec![
            leaf("s1"),
            LayoutNode::split(
                "right",
                SplitDir::Down,
                vec![leaf("s2"), leaf("f1")],
                vec![0.58, 0.42],
            ),
        ],
        vec![0.5, 0.5],
    );
    let left_composer = composer(None, cx);
    let transcript = cx.new(|_| MockTranscript {
        paragraphs: vec![
            "Each game now plays all the way through instead of cutting off mid-move. It ends in a WIN or LOSE, that word shows in the center, then a different game starts at random.",
            "Snake, invaders, pong, and breakout all have real finish conditions, so you should see the round conclude before the loop continues.",
            "Reverted. The attract-mode loop is back: snake, invaders, pong, and breakout, fading from one to the next.",
        ],
        composer: left_composer,
    });
    let tabs = surface_tabs(cx);
    let file_pane = cx.new(|_| MockFilePane { tabs });
    cx.new(|cx| {
        let mut tree = PaneTree::new(layout, "s1", cx);
        tree.set_leaves(
            [
                PaneLeaf {
                    id: "s1".into(),
                    kind: PaneLeafKind::Session {
                        title: "Empty chat dots spell MONOCODE".into(),
                    },
                    view: transcript.into(),
                },
                PaneLeaf {
                    id: "s2".into(),
                    kind: PaneLeafKind::Session {
                        title: "Repeated folder permission prompts".into(),
                    },
                    view: right_top,
                },
                PaneLeaf {
                    id: "f1".into(),
                    kind: PaneLeafKind::Surface,
                    view: file_pane.into(),
                },
            ],
            cx,
        );
        tree
    })
}

fn tab_group_menu(window: &mut Window, cx: &mut App) -> Entity<TabGroupMenu> {
    let mut mute = TabGroupMenuExtraItem::new(
        "notifications-mute",
        "Mute notifications",
        IconName::BellOff,
    );
    mute.submenu = Some(
        [
            ("mute:1", "1 hour"),
            ("mute:8", "8 hours"),
            ("mute:24", "1 day"),
        ]
        .into_iter()
        .map(|(id, label)| SubmenuEntry::Item {
            id: id.into(),
            label: label.into(),
            disabled: false,
        })
        .collect(),
    );
    cx.new(|cx| {
        let mut menu = TabGroupMenu::new(
            TabGroupMenuProps {
                position: point(px(40.), px(48.)),
                group_id: "/Users/me/code/agent-terminal".into(),
                label: "agent-terminal".into(),
                color_index: Some(1),
                current_color: "#4ea1f6".into(),
                logo_project: Some("/Users/me/code/agent-terminal".into()),
                mascot_project: "agent-terminal".into(),
                extra_items: vec![mute],
                ..TabGroupMenuProps::default()
            },
            window,
            cx,
        );
        menu.set_animate(false);
        menu
    })
}

fn build(scene: &str, window: &mut Window, cx: &mut App) -> Gallery {
    match scene {
        "menus" | "submenus" => {
            let picker = workspace_picker(window, cx);
            let right_composer = composer(Some(picker.clone()), cx);
            let empty = empty_session(right_composer, cx);
            let tree = pane_tree(empty.into(), cx);
            let menu = tab_group_menu(window, cx);
            picker.update(cx, |picker, cx| {
                picker.open(InitialPicker::Workspace, window, cx)
            });
            if scene == "submenus" {
                menu.update(cx, |menu, cx| menu.show_submenu("notifications-mute", cx));
                picker.update(cx, |picker, cx| {
                    picker.show_worktree_menu(cx);
                    picker.set_worktrees(
                        Ok(vec![
                            WorktreeEntry {
                                path: "/Users/me/code/agent-terminal".into(),
                                branch: Some("main".into()),
                                head: "0dd3b29aa".into(),
                                is_main: true,
                                missing: false,
                            },
                            WorktreeEntry {
                                path: "/Users/me/code/agent-terminal-arcade".into(),
                                branch: Some("arcade-endings".into()),
                                head: "a45fd18c2".into(),
                                is_main: false,
                                missing: false,
                            },
                            WorktreeEntry {
                                path: "/Users/me/code/agent-terminal-review".into(),
                                branch: None,
                                head: "875a7f7e1".into(),
                                is_main: false,
                                missing: false,
                            },
                        ]),
                        cx,
                    );
                });
            }
            Gallery {
                body: tree.into(),
                overlays: vec![menu.into()],
            }
        }
        "empty" => {
            let picker = workspace_picker(window, cx);
            let empty = empty_session(composer(Some(picker), cx), cx);
            Gallery {
                body: empty.into(),
                overlays: Vec::new(),
            }
        }
        "opus" | "astra" => {
            let empty = empty_session(composer(None, cx), cx);
            let kind = if scene == "opus" {
                WelcomeKind::Opus
            } else {
                WelcomeKind::Astra
            };
            let welcome = cx.new(|cx| {
                let mut welcome = ModelWelcome::new(kind, cx);
                let height = window.viewport_size().height;
                let middle = f32::from(height) / 2.0;
                welcome.set_composer_band(
                    Some(ComposerBand {
                        top: middle - 40.0,
                        bottom: middle + 80.0,
                    }),
                    cx,
                );
                welcome.freeze_at(
                    Duration::from_millis(if kind == WelcomeKind::Opus {
                        2600
                    } else {
                        2000
                    }),
                    cx,
                );
                welcome
            });
            Gallery {
                body: empty.into(),
                overlays: vec![welcome.into()],
            }
        }
        "signin" => {
            let empty = empty_session(composer(None, cx), cx);
            let login: Login = Rc::new(|_, _| Task::ready(Ok(())));
            let dialog = cx.new(|_| {
                let mut dialog = ProviderSignInDialog::new(HarnessId::Grok, login);
                dialog.set_animate(false);
                dialog
            });
            Gallery {
                body: empty.into(),
                overlays: vec![dialog.into()],
            }
        }
        "review" => {
            let review = cx.new(|cx| {
                SessionReview::new(
                    SessionReviewProps {
                        session_id: "s1".into(),
                        cwd: "/Users/me/code/agent-terminal".into(),
                        enabled: true,
                        busy: false,
                        undo_locked: false,
                    },
                    Rc::new(MockReview),
                    cx,
                )
            });
            let now = monocode_view_workbench::panes::notices::epoch_ms();
            let limit = cx.new(|cx| {
                UsageLimitNotice::new(
                    UsageLimit {
                        resets_at: Some(now + (4 * 60 + 42) * 60_000 + 30_000),
                        resume_at_reset: false,
                    },
                    relative_reset_formatter(),
                    cx,
                )
            });
            let agent = cx.new(|cx| {
                let mut view = AgentTabView::new("Audit the engine");
                view.set_session(
                    Some(AgentTabSession {
                        id: "worker".into(),
                        title: "Audit the engine".into(),
                        harness: HarnessId::Codex,
                        cwd: "/Users/me/code/agent-terminal".into(),
                        model_name: "GPT-5.5".into(),
                    }),
                    cx,
                );
                view
            });
            let column = cx.new(|_| ReviewColumn {
                review,
                limit,
                agent,
            });
            Gallery {
                body: column.into(),
                overlays: Vec::new(),
            }
        }
        _ => {
            let picker = workspace_picker(window, cx);
            let right_composer = composer(Some(picker), cx);
            let empty = empty_session(right_composer, cx);
            let tree = pane_tree(empty.into(), cx);
            tree.update(cx, |tree, cx| {
                tree.set_external_drop(
                    Some(PaneDrop {
                        from_id: "tab-3".into(),
                        over_id: Some("s2".into()),
                        edge: PaneEdge::Right,
                    }),
                    cx,
                )
            });
            Gallery {
                body: tree.into(),
                overlays: Vec::new(),
            }
        }
    }
}

/// The session cards and notices, stacked.
struct ReviewColumn {
    review: Entity<SessionReview>,
    limit: Entity<UsageLimitNotice>,
    agent: Entity<AgentTabView>,
}

impl Render for ReviewColumn {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .flex()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(u(640.))
                    .gap(u(16.))
                    .pt(u(24.))
                    .child(self.review.clone())
                    .child(div().px(u(8.)).child(self.limit.clone()))
                    .child(
                        div()
                            .h(u(220.))
                            .mx(u(16.))
                            .border_1()
                            .border_color(theme.colors.stroke)
                            .rounded(u(12.))
                            .child(self.agent.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .border_l_1()
                    .border_color(theme.colors.stroke)
                    .child(
                        div()
                            .flex_1()
                            .child(sessions_empty("Sessions you start will show up here")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .border_t_1()
                            .border_color(theme.colors.stroke)
                            .child(discussion_empty("Explore this item with your agent.")),
                    ),
            )
    }
}

fn main() {
    let options = parse_options();
    gpui_platform::application()
        .with_assets(monocode_ui::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            let appearance = AppearanceSettings {
                theme_preference: ThemePreference::parse(Some(if options.light {
                    "light"
                } else {
                    "dark"
                })),
                ..AppearanceSettings::default()
            };
            monocode_ui::init(appearance, cx);
            monocode_view_workbench::panes::init(cx);
            let bounds = Bounds::centered(None, size(px(options.size.0), px(options.size.1)), cx);
            let scene = options.scene.clone();
            let screenshot = options.screenshot.is_some();
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        window_background: WindowBackgroundAppearance::Opaque,
                        // A screenshot run must not take the keyboard.
                        focus: !screenshot,
                        ..Default::default()
                    },
                    |window, cx| {
                        monocode_ui::sync_window(window, cx);
                        let gallery = build(&scene, window, cx);
                        cx.new(|_| gallery)
                    },
                )
                .expect("open window");
            let Some(out) = options.screenshot.clone() else {
                cx.activate(true);
                return;
            };
            cx.spawn(async move |cx| {
                let any: gpui::AnyWindowHandle = window.into();
                for _ in 0..20 {
                    cx.background_executor()
                        .timer(Duration::from_millis(60))
                        .await;
                    any.update(cx, |_, window, _| window.refresh()).ok();
                }
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
