//! Draws provider accounts, usage, and notification controls in hidden windows.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyView, App, AppContext as _, AsyncApp, Bounds, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Task, Window,
    WindowBounds, WindowOptions, div, px, size,
};
use gpui_component::Root;
use monocode_core::HarnessId;
use monocode_ui::{AppearanceSettings, Theme, ThemePreference};
use monocode_view_settings::accounts::date_time_picker::DateTimePicker;
use monocode_view_settings::accounts::model::*;
use monocode_view_settings::accounts::notification_model::*;
use monocode_view_settings::accounts::*;

const NOW: i64 = 1_791_046_800_000;

#[derive(Default)]
struct GalleryHost {
    accounts: RefCell<Vec<ProviderAccount>>,
    preferences: RefCell<Preferences>,
}
impl GalleryHost {
    fn new() -> Self {
        Self {
            accounts: RefCell::new(
                PROVIDER_ACCOUNT_PROVIDERS
                    .iter()
                    .flat_map(|provider| {
                        [
                            ProviderAccount {
                                is_default: Some(true),
                                ..ProviderAccount::new("default", *provider, "Default account")
                            },
                            ProviderAccount::new("work", *provider, "Work"),
                        ]
                    })
                    .collect(),
            ),
            ..Default::default()
        }
    }
}
impl UsageHost for GalleryHost {
    fn now(&self) -> i64 {
        NOW
    }
    fn supports_harness_login(&self, _: HarnessId) -> bool {
        true
    }
    fn login_harness(&self, _: HarnessId, _: Option<&str>, _: &mut App) -> host::HostTask<()> {
        Task::ready(Ok(()))
    }
    fn provider_accounts(&self, provider: HarnessId, _: &App) -> Vec<ProviderAccount> {
        self.accounts
            .borrow()
            .iter()
            .filter(|account| account.provider == provider)
            .cloned()
            .collect()
    }
    fn rate_limits(
        &self,
        provider: RateLimitProvider,
        account: &str,
        _: &App,
    ) -> Option<ProviderRateLimits> {
        Some(limits(provider, account))
    }
    fn load_rate_limits(
        &self,
        provider: RateLimitProvider,
        account: &str,
        _: bool,
        _: &mut App,
    ) -> Task<ProviderRateLimits> {
        Task::ready(limits(provider, account))
    }
    fn account_identities(
        &self,
        accounts: &[ProviderAccount],
        _: &mut App,
    ) -> Task<HashMap<String, Option<ProviderAccountIdentity>>> {
        Task::ready(
            accounts
                .iter()
                .map(|account| {
                    (
                        identity_key(account),
                        Some(ProviderAccountIdentity {
                            email: Some(
                                if account.id == "work" {
                                    "dev@arcade.dev"
                                } else {
                                    "dev@example.com"
                                }
                                .into(),
                            ),
                            name: Some("Alex".into()),
                            plan: Some("Pro".into()),
                            organization: Some(
                                if account.id == "work" {
                                    "Arcade"
                                } else {
                                    "Alex's Organization"
                                }
                                .into(),
                            ),
                        }),
                    )
                })
                .collect(),
        )
    }
    fn new_provider_account(
        &self,
        provider: HarnessId,
        label: &str,
        _: &mut App,
    ) -> Result<ProviderAccount, String> {
        Ok(ProviderAccount::new(
            &format!("account-{}", self.accounts.borrow().len()),
            provider,
            label.trim(),
        ))
    }
    fn save_provider_account(&self, account: &ProviderAccount, _: &mut App) {
        self.accounts.borrow_mut().push(account.clone());
    }
    fn rename_provider_account(
        &self,
        provider: HarnessId,
        id: &str,
        label: &str,
        _: &mut App,
    ) -> Result<(), String> {
        if let Some(account) = self
            .accounts
            .borrow_mut()
            .iter_mut()
            .find(|account| account.provider == provider && account.id == id)
        {
            account.label = label.trim().into();
        }
        Ok(())
    }
    fn remove_provider_account(&self, provider: HarnessId, id: &str, _: &mut App) {
        self.accounts
            .borrow_mut()
            .retain(|account| account.provider != provider || account.id != id);
    }
    fn confirm_remove_account(&self, _: &ProviderAccount, _: &mut App) -> Task<bool> {
        Task::ready(true)
    }
    fn remove_provider_account_credentials(
        &self,
        _: HarnessId,
        _: &str,
        _: &mut App,
    ) -> host::HostTask<()> {
        Task::ready(Ok(()))
    }
}
impl NotificationsHost for GalleryHost {
    fn now(&self) -> i64 {
        NOW
    }
    fn preferences(&self, _: &App) -> Preferences {
        self.preferences.borrow().clone()
    }
    fn notification_projects(&self, _: &[String], _: &App) -> Vec<NotificationProject> {
        vec![
            NotificationProject {
                id: "repo:monocode".into(),
                name: "monocode".into(),
                detail: "github.com/arcade/monocode".into(),
                kind: NotificationProjectKind::Repository,
                paths: vec!["/Users/dev/monocode".into()],
            },
            NotificationProject {
                id: "linear:arcade".into(),
                name: "Arcade".into(),
                detail: "Linear workspace".into(),
                kind: NotificationProjectKind::Linear,
                paths: Vec::new(),
            },
        ]
    }
    fn update_preferences(
        &self,
        ids: &[String],
        patch: &PreferencePatch,
        _: &mut App,
    ) -> Result<(), String> {
        let mut prefs = self.preferences.borrow_mut();
        for id in ids {
            let pref = prefs.entry(id.clone()).or_default();
            if let Some(disabled) = &patch.disabled {
                pref.disabled = disabled.clone();
            }
            if let Some(mute) = patch.muted_until {
                pref.muted_until = mute;
            }
        }
        Ok(())
    }
}
impl HarnessUpdateHost for GalleryHost {
    fn check_for_updates(&self, _: &mut App) -> Task<Vec<HarnessUpdate>> {
        Task::ready(vec![
            HarnessUpdate {
                harness: HarnessId::Claude,
                installed: "2.1.4".into(),
                latest: "2.2.0".into(),
            },
            HarnessUpdate {
                harness: HarnessId::Codex,
                installed: "0.156.1".into(),
                latest: "0.157.0".into(),
            },
        ])
    }
    fn update_cli(&self, _: HarnessId, _: &mut App) -> host::HostTask<()> {
        Task::ready(Ok(()))
    }
    fn installed_version(&self, _: HarnessId, _: &mut App) -> host::HostTask<Option<String>> {
        Task::ready(Ok(Some("2.2.0".into())))
    }
}
fn limits(provider: RateLimitProvider, account: &str) -> ProviderRateLimits {
    ProviderRateLimits {
        session: Some(RateLimitWindow {
            used_percent: if account == "work" { 22. } else { 67. },
            window_minutes: 300,
            resets_at: Some(NOW + 7_200_000),
        }),
        weekly: Some(RateLimitWindow {
            used_percent: if account == "work" { 15. } else { 81. },
            window_minutes: 10080,
            resets_at: Some(NOW + 172_800_000),
        }),
        status: RateLimitStatus::Ok,
        updated_at: NOW,
        ..idle_rate_limits(provider)
    }
}
struct Stage {
    view: AnyView,
    scene: String,
}
impl Render for Stage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let container = div()
            .id("account-gallery")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content);
        if matches!(self.scene.as_str(), "usage" | "switch" | "footer") {
            container.justify_end().p(px(24.)).child(self.view.clone())
        } else if self.scene == "updates" {
            container.child(self.view.clone())
        } else {
            container
                .overflow_y_scroll()
                .p(px(24.))
                .child(self.view.clone())
        }
    }
}
fn main() {
    let mut args = std::env::args().skip(1);
    let mut scene = "accounts".to_string();
    let mut screenshot = None;
    let mut light = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scene" => scene = args.next().expect("--scene needs a value"),
            "--screenshot" => {
                screenshot = Some(PathBuf::from(
                    args.next().expect("--screenshot needs a path"),
                ))
            }
            "--theme" => light = args.next().as_deref() == Some("light"),
            _ => panic!("unknown argument {arg}"),
        }
    }
    gpui_platform::application()
        .with_assets(monocode_ui::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            monocode_ui::init(
                AppearanceSettings {
                    theme_preference: if light {
                        ThemePreference::Light
                    } else {
                        ThemePreference::Dark
                    },
                    ..Default::default()
                },
                cx,
            );
            let width = if scene == "date" { 360. } else { 820. };
            let height = if scene == "date" { 400. } else { 680. };
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                            None,
                            size(px(width), px(height)),
                            cx,
                        ))),
                        focus: false,
                        show: false,
                        ..Default::default()
                    },
                    |window, cx| {
                        monocode_ui::sync_window(window, cx);
                        let host = Rc::new(GalleryHost::new());
                        let view: AnyView = match scene.as_str() {
                            "accounts" | "add" => cx
                                .new(|cx| {
                                    let mut view = ProviderAccountsSettings::new(host, window, cx);
                                    if scene == "add" {
                                        view.start_add(HarnessId::Claude, window, cx);
                                    }
                                    view
                                })
                                .into(),
                            "usage" | "switch" => cx
                                .new(|cx| {
                                    let mut props = ChipProps::new(
                                        limits(RateLimitProvider::Codex, "default"),
                                        NOW,
                                    );
                                    props.accounts = UsageHost::provider_accounts(
                                        host.as_ref(),
                                        HarnessId::Codex,
                                        cx,
                                    );
                                    let actions = ChipActions {
                                        on_select_account: Some(Rc::new(|_, _, _| {})),
                                        on_manage_accounts: Some(Rc::new(|_, _, _| {})),
                                        ..Default::default()
                                    };
                                    let mut view =
                                        UsageProviderChip::new(host, props, actions, window, cx);
                                    view.set_animate(false);
                                    view.toggle(window, cx);
                                    if scene == "switch" {
                                        view.show_view(AccountView::Accounts, window, cx);
                                    }
                                    view
                                })
                                .into(),
                            "footer" => cx
                                .new(|cx| {
                                    UsageFooter::new(
                                        host,
                                        UsageFooterProps {
                                            providers: vec![
                                                RateLimitProvider::Claude,
                                                RateLimitProvider::Codex,
                                            ],
                                            project: Some("/Users/dev/monocode".into()),
                                            ..Default::default()
                                        },
                                        UsageFooterCallbacks::default(),
                                        cx,
                                    )
                                })
                                .into(),
                            "notifications" => cx
                                .new(|cx| {
                                    ProjectNotificationSettings::new(
                                        host,
                                        ProjectNotificationProps {
                                            cwd: "/Users/dev/monocode".into(),
                                            notification_project_path: Some(
                                                "/Users/dev/monocode".into(),
                                            ),
                                            notification_settings_request: 1,
                                            ..Default::default()
                                        },
                                        cx,
                                    )
                                })
                                .into(),
                            "mute" => cx
                                .new(|cx| {
                                    let mut view = NotificationMuteControl::new(
                                        host,
                                        vec!["repo:monocode".into()],
                                        "gallery",
                                        cx,
                                    );
                                    view.set_animate(false);
                                    view.toggle(window, cx);
                                    view
                                })
                                .into(),
                            "date" => cx
                                .new(|cx| {
                                    DateTimePicker::new(
                                        "2026-10-04T09:00".into(),
                                        Some("2026-10-03"),
                                        NOW,
                                        false,
                                        window,
                                        cx,
                                    )
                                })
                                .into(),
                            "updates" => {
                                cx.new(|cx| HarnessUpdateNotice::new(host, 20., cx)).into()
                            }
                            _ => panic!("unknown scene {scene}"),
                        };
                        let stage = cx.new(|_| Stage {
                            view,
                            scene: scene.clone(),
                        });
                        cx.new(|cx| Root::new(stage, window, cx))
                    },
                )
                .expect("gallery window");
            if let Some(out) = screenshot.clone() {
                capture_and_quit(window.into(), out, cx);
            }
        });
}
fn capture_and_quit(window: gpui::AnyWindowHandle, out: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        for _ in 0..15 {
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
            window
                .update(cx, |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear();
                })
                .ok();
        }
        let image = window
            .update(cx, |_, window, cx| {
                window.draw(cx).clear();
                window.render_to_image()
            })
            .expect("window")
            .expect("render");
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).expect("output directory");
        }
        image.save(&out).expect("PNG");
        eprintln!("wrote {}", out.display());
        cx.update(|cx| cx.quit());
    })
    .detach();
}
