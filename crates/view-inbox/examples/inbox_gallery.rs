//! Renders the inbox views to PNGs, offscreen, with synthetic data.
//!
//! ```sh
//! cargo run -p monocode-view-inbox --example inbox_gallery -- target/inbox-gallery
//! cargo run -p monocode-view-inbox --example inbox_gallery -- target/inbox-gallery pr-checks
//! ```
//!
//! Each scene opens a hidden headless window (never shown or focused) with
//! the platform text system and the headless renderer, lets popovers and
//! images settle, and writes `<scene>.png` through
//! `Window::render_to_image`.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyView, App, AppContext as _, Context, HeadlessAppContext, IntoElement, ParentElement as _,
    Render, Styled as _, Window, WindowHandle, div, px, size,
};
use monocode_core::inbox::{
    LinkedWorkItemActivityCounts, LinkedWorkItemActivityEntry, LinkedWorkItemActivityKind,
    LinkedWorkItemUpdateCard, LinkedWorkItemUpdateStatus, WorkItemKind,
};
use monocode_ui::{AppearanceSettings, Theme, ThemePreference};
use monocode_view_inbox::data::{InboxListData, InboxProvider, InboxServices, LinkedWorkItem};
use monocode_view_inbox::fixtures::{
    FakeList, FakeServices, sample_items, sample_list_state, sample_pr, sample_repair,
    sample_services,
};
use monocode_view_inbox::list::notification_menu::{
    InboxNotificationData, InboxNotificationMenu, NotificationMenuState,
};
use monocode_view_inbox::list::view::{InboxView, InboxViewConfig};
use monocode_view_inbox::pr::detail::DetailTab;
use monocode_view_inbox::pr::link_dialog::LinkSessionWorkItemDialog;
use monocode_view_inbox::pr::linked_notice::{LinkedWorkItemUpdateNotice, NoticeHandlers};
use monocode_view_inbox::pr::linked_panel::{LinkedPanelProps, LinkedWorkItemPanel};

/// The window body: the scene's view on the main pane's background.
struct Stage {
    view: AnyView,
    overlay: Option<AnyView>,
}

impl Render for Stage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .relative()
            .size_full()
            .flex()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal))
            .child(self.view.clone())
            .children(self.overlay.clone())
    }
}

type Build = Box<dyn FnOnce(&mut Window, &mut App) -> Stage>;

struct Scene {
    name: &'static str,
    width: f32,
    height: f32,
    light: bool,
    build: Build,
}

fn scene(
    name: &'static str,
    width: f32,
    height: f32,
    build: impl FnOnce(&mut Window, &mut App) -> Stage + 'static,
) -> Scene {
    Scene {
        name,
        width,
        height,
        light: false,
        build: Box::new(build),
    }
}

fn inbox(
    window: &mut Window,
    cx: &mut App,
    setup: impl FnOnce(&mut InboxView, &Rc<FakeServices>, &mut Window, &mut Context<InboxView>)
    + 'static,
) -> Stage {
    inbox_with(window, cx, true, setup)
}

fn inbox_with(
    window: &mut Window,
    cx: &mut App,
    repairs: bool,
    setup: impl FnOnce(&mut InboxView, &Rc<FakeServices>, &mut Window, &mut Context<InboxView>)
    + 'static,
) -> Stage {
    let services = sample_services();
    if repairs {
        services.state.borrow_mut().repairs = vec![sample_repair()];
    }
    let list = FakeList::new(sample_list_state(), sample_items());
    let services_dyn: Rc<dyn InboxServices> = services.clone();
    let list_dyn: Rc<dyn InboxListData> = Rc::new(list);
    let view = cx.new(|cx| {
        let mut view = InboxView::new(
            services_dyn,
            list_dyn,
            InboxViewConfig {
                beside_rail: true,
                can_start: true,
                can_repair: true,
                ..Default::default()
            },
            window,
            cx,
        );
        view.set_animate(false, cx);
        setup(&mut view, &services, window, cx);
        view
    });
    Stage {
        view: view.into(),
        overlay: None,
    }
}

fn with_tab(
    tab: DetailTab,
) -> impl FnOnce(&mut InboxView, &Rc<FakeServices>, &mut Window, &mut Context<InboxView>) {
    move |view, _, window, cx| {
        if let Some(detail) = view.detail().cloned() {
            detail.update(cx, |detail, cx| detail.set_tab(tab, window, cx));
        }
    }
}

struct FakeNotifications;

impl InboxNotificationData for FakeNotifications {
    fn subscribe(&self, _: monocode_view_inbox::data::Listener, _: &mut App) -> gpui::Subscription {
        gpui::Subscription::new(|| {})
    }
    fn state(&self, _: &App) -> NotificationMenuState {
        NotificationMenuState {
            project_count: 3,
            muted_count: 1,
            has_unread: true,
            can_open_settings: true,
        }
    }
    fn now_ms(&self) -> i64 {
        monocode_view_inbox::fixtures::NOW
    }
    fn mark_all_read(&self, _: &mut App) -> bool {
        true
    }
    fn mute_all(&self, _: Option<i64>, _: &mut App) -> Result<(), String> {
        Ok(())
    }
    fn resume_muted(&self, _: &mut App) -> Result<(), String> {
        Ok(())
    }
    fn open_settings(&self, _: &mut App) {}
}

fn notice_card(state: &str) -> LinkedWorkItemUpdateCard {
    LinkedWorkItemUpdateCard {
        kind: WorkItemKind::Pr,
        repo: "monocode/monocode".into(),
        number: 412,
        title: "Stream tool output into the transcript while commands run".into(),
        url: "https://github.com/monocode/monocode/pull/412".into(),
        state: state.into(),
        since: 0,
        updated_at: 1,
        status: LinkedWorkItemUpdateStatus::Ready,
        counts: LinkedWorkItemActivityCounts {
            comments: 2,
            reviews: 1,
            commits: 0,
        },
        entries: vec![
            LinkedWorkItemActivityEntry {
                id: "r1".into(),
                kind: LinkedWorkItemActivityKind::Review,
                author: "sam-o".into(),
                text: "Two failing jobs look related to the new snapshot tests.".into(),
                created_at: "2026-09-30T14:40:00Z".into(),
                url: "https://github.com/monocode/monocode/pull/412#pullrequestreview-1".into(),
            },
            LinkedWorkItemActivityEntry {
                id: "c2".into(),
                kind: LinkedWorkItemActivityKind::Comment,
                author: "maya".into(),
                text: "Please cover the empty state before merging, the transcript flashes when no output arrives.".into(),
                created_at: "2026-09-30T14:10:00Z".into(),
                url: "https://github.com/monocode/monocode/pull/412#issuecomment-2".into(),
            },
        ],
        truncated: false,
    }
}

fn blank(cx: &mut App) -> AnyView {
    cx.new(|_| Blank).into()
}

struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn scenes() -> Vec<Scene> {
    let mut list = vec![
        scene("inbox-pr-summary", 1280., 860., |window, cx| {
            inbox(window, cx, |_, _, _, _| {})
        }),
        scene("inbox-pr-checks", 1280., 980., |window, cx| {
            inbox(window, cx, with_tab(DetailTab::Checks))
        }),
        scene("inbox-pr-checks-evidence", 1280., 1240., |window, cx| {
            inbox_with(window, cx, false, with_tab(DetailTab::Checks))
        }),
        scene("inbox-pr-code", 1280., 860., |window, cx| {
            inbox(window, cx, with_tab(DetailTab::Code))
        }),
        scene("inbox-filters", 1280., 860., |window, cx| {
            inbox(window, cx, |view, _, _, cx| view.toggle_filter_menu(cx))
        }),
        scene("inbox-connect", 1280., 600., |window, cx| {
            inbox(window, cx, |view, _, _, cx| view.toggle_connect_menu(cx))
        }),
        scene("inbox-linear", 1280., 700., |window, cx| {
            inbox(window, cx, |view, _, _, cx| {
                view.set_source(InboxProvider::Linear, cx)
            })
        }),
        scene("inbox-merge-confirm", 1280., 700., |window, cx| {
            inbox(window, cx, |view, _, _, cx| {
                if let Some(detail) = view.detail().cloned() {
                    detail.update(cx, |detail, cx| {
                        detail.ask_to_run(monocode_view_inbox::data::GithubPrAction::Squash, cx)
                    });
                }
            })
        }),
        scene("inbox-repair-form", 1280., 980., |window, cx| {
            inbox(window, cx, |view, _, window, cx| {
                if let Some(detail) = view.detail().cloned() {
                    detail.update(cx, |detail, cx| {
                        detail.set_tab(DetailTab::Checks, window, cx);
                        if let Some(checks) = detail.checks_view().cloned() {
                            checks.update(cx, |checks, cx| checks.fix_all(window, cx));
                        }
                    });
                }
            })
        }),
        scene("linked-panel", 1100., 860., |window, cx| {
            let services = sample_services();
            let services_dyn: Rc<dyn InboxServices> = services;
            let item = sample_pr();
            let target = LinkedWorkItem {
                kind: WorkItemKind::Pr,
                repo: item.repo.clone(),
                number: item.number,
                url: item.url.clone(),
                extra: Default::default(),
            };
            let panel = cx.new(|cx| {
                LinkedWorkItemPanel::new(
                    services_dyn,
                    target,
                    LinkedPanelProps {
                        cwd: "/Users/dev/monocode".into(),
                        projects: Vec::new(),
                        visible: true,
                        can_repair: true,
                    },
                    window,
                    cx,
                )
            });
            let body = cx.new(|_| PanelStage { panel }).into();
            Stage {
                view: body,
                overlay: None,
            }
        }),
        scene("linked-notice", 760., 520., |_, cx| {
            let services: Rc<dyn InboxServices> = sample_services();
            let handlers = NoticeHandlers {
                on_acknowledge: Rc::new(|_, _| {}),
                on_dismiss: Rc::new(|_, _| {}),
                on_open_discussion: Rc::new(|_, _| {}),
                on_add_to_chat: Rc::new(|_, _, _| {}),
                on_announce: None,
                on_archive_session: Some(Rc::new(|_| gpui::Task::ready(Ok(true)))),
                on_delete_session: Some(Rc::new(|_| gpui::Task::ready(Ok(true)))),
            };
            let notice = cx.new(|cx| {
                let mut notice = LinkedWorkItemUpdateNotice::new(
                    services,
                    "session-1".into(),
                    Some(notice_card("merged")),
                    handlers,
                    cx,
                );
                notice.set_animate(false);
                notice
            });
            Stage {
                view: blank(cx),
                overlay: Some(notice.into()),
            }
        }),
        scene("notification-menu", 560., 360., |window, cx| {
            let data: Rc<dyn InboxNotificationData> = Rc::new(FakeNotifications);
            let menu = cx.new(|cx| {
                let mut menu = InboxNotificationMenu::new(data, gpui::point(px(24.), px(24.)), cx);
                menu.set_animate(false);
                menu.pick("mute", window, cx);
                menu
            });
            Stage {
                view: blank(cx),
                overlay: Some(menu.into()),
            }
        }),
        scene("link-dialog", 760., 520., |window, cx| {
            let dialog = cx.new(|cx| {
                let mut dialog = LinkSessionWorkItemDialog::new(
                    None,
                    "Fix streaming flicker in tool cards".into(),
                    window,
                    cx,
                );
                dialog.set_animate(false);
                dialog
            });
            Stage {
                view: blank(cx),
                overlay: Some(dialog.into()),
            }
        }),
    ];
    let mut light = scene("inbox-pr-checks-light", 1280., 980., |window, cx| {
        inbox(window, cx, with_tab(DetailTab::Checks))
    });
    light.light = true;
    list.push(light);
    let mut light_summary = scene("inbox-pr-summary-light", 1280., 860., |window, cx| {
        inbox(window, cx, |_, _, _, _| {})
    });
    light_summary.light = true;
    list.push(light_summary);
    list
}

struct PanelStage {
    panel: gpui::Entity<LinkedWorkItemPanel>,
}

impl Render for PanelStage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .relative()
            .flex()
            .size_full()
            .child(
                div()
                    .flex_1()
                    .p(px(24.))
                    .text_color(theme.content(0.35))
                    .child("Session transcript"),
            )
            .child(self.panel.clone())
    }
}

fn capture(cx: &mut HeadlessAppContext, scene: Scene, out: &Path) {
    let appearance = AppearanceSettings {
        theme_preference: if scene.light {
            ThemePreference::Light
        } else {
            ThemePreference::Dark
        },
        ..Default::default()
    };
    cx.update(|cx| monocode_ui::set_appearance(appearance, cx));
    let build = scene.build;
    let window: WindowHandle<Stage> = cx
        .open_window(
            size(px(scene.width), px(scene.height)),
            move |window, cx| {
                monocode_ui::sync_window(window, cx);
                let stage = build(window, cx);
                cx.new(|_| stage)
            },
        )
        .expect("open window");
    let draw = |cx: &mut HeadlessAppContext| {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
        })
        .expect("draw");
        cx.run_until_parked();
    };
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(450) {
        draw(cx);
        std::thread::sleep(Duration::from_millis(16));
    }
    draw(cx);
    let image = cx.capture_screenshot(window.into()).expect("capture");
    let path = out.join(format!("{}.png", scene.name));
    image.save(&path).expect("save png");
    eprintln!(
        "wrote {} ({}x{})",
        path.display(),
        image.width(),
        image.height()
    );
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .ok();
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/inbox-gallery"));
    let only = std::env::args().nth(2);
    std::fs::create_dir_all(&out).expect("create output dir");
    let platform = gpui_platform::current_platform(true);
    let mut cx = HeadlessAppContext::with_platform(
        platform.text_system(),
        Arc::new(monocode_ui::Assets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
        monocode_editor::init(cx);
        monocode_view_inbox::init(cx);
    });
    monocode_view_inbox::set_autofocus(false);
    for scene in scenes() {
        if only.as_deref().is_some_and(|only| only != scene.name) {
            continue;
        }
        capture(&mut cx, scene, &out);
    }
}
