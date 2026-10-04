//! Empty session composition, chat artwork, arcade games, and provider login.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, Hsla, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
    Subscription, Window, div, img,
};
use monocode_app::boot::AppServices;
use monocode_core::{
    Session,
    appearance::{ChatBackgroundScope, NewThreadBackgroundEffect},
};
use monocode_engine::{attention::Attention, projects::ProjectsGlobal, runtime::Engine};
use monocode_layout::paths::{project_key, project_name};
use monocode_ui::{Theme, UiStyled as _, u};
use monocode_view_composer::composer::Composer;
use monocode_view_pages::ProjectsData as _;
use monocode_view_settings::settings::background::{
    ChatBackground, HazeVariant, ProjectBackgroundEffect, gradient_blur_background,
};
use monocode_view_workbench::{
    panes::{
        empty_session::{EmptySession, EmptySessionProps},
        provider_sign_in::{ProviderSignInDialog, ProviderSignInEvent},
    },
    terminal_dock::TerminalGridBackground,
};

#[derive(Clone, Default, PartialEq)]
struct BackgroundProps {
    path: Option<String>,
    effect: NewThreadBackgroundEffect,
    revision: i64,
    opacity: f32,
    blur: f32,
    empty: bool,
}
fn background_props(session: Option<&Session>, cx: &App) -> BackgroundProps {
    let services = AppServices::global(cx);
    let appearance =
        monocode_settings::load_app_settings(&services.kv, monocode_core::Platform::current())
            .appearance;
    let empty = session.is_none_or(|session| session.blocks.is_empty());
    let project = session.and_then(|session| {
        ProjectsGlobal::projects(cx)
            .read(cx)
            .chat_background_settings(&project_key(&session.cwd))
    });
    let (path, effect, scope, opacity, revision) = if let Some(project) = project {
        (
            Some(project.path),
            project.effect,
            project.scope,
            if empty {
                project.empty_opacity
            } else {
                project.session_opacity
            },
            ProjectsGlobal::projects(cx)
                .read(cx)
                .chat_background_image_revision(),
        )
    } else {
        (
            appearance.chat_background_path,
            appearance.new_thread_background_effect,
            appearance.chat_background_scope,
            if empty {
                appearance.chat_background_empty_opacity
            } else {
                appearance.chat_background_session_opacity
            },
            ChatBackground::image_revision(cx),
        )
    };
    BackgroundProps {
        path,
        effect,
        revision,
        opacity: if !empty && scope == ChatBackgroundScope::Empty {
            0.
        } else {
            opacity as f32
        },
        blur: appearance.chat_background_blur as f32,
        empty,
    }
}

/// A background for both new sessions and conversations, under the pane content.
pub struct SessionBackground {
    props: BackgroundProps,
    effect: Entity<ProjectBackgroundEffect>,
    _subscription: Subscription,
}
impl SessionBackground {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let effect = cx.new(|_| ProjectBackgroundEffect::new());
        let subscription = cx.observe(&effect, |_, _, cx| cx.notify());
        Self {
            props: BackgroundProps::default(),
            effect,
            _subscription: subscription,
        }
    }
    pub fn set_session(&mut self, session: Option<&Session>, cx: &mut Context<Self>) {
        let props = background_props(session, cx);
        if props != self.props {
            self.props = props;
            cx.notify();
        }
    }
}
impl Render for SessionBackground {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut layer = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .overflow_hidden();
        let Some(path) = self.props.path.as_ref().filter(|_| self.props.opacity > 0.) else {
            return layer;
        };
        if self.props.effect == NewThreadBackgroundEffect::GradientBlur {
            return layer.child(
                gradient_blur_background(
                    "session-chat-haze",
                    path.clone(),
                    self.props.revision,
                    if self.props.empty {
                        HazeVariant::Empty
                    } else {
                        HazeVariant::Session
                    },
                )
                .opacity(self.props.opacity),
            );
        }
        let light = !Theme::of(cx).is_dark();
        let picture = self.effect.update(cx, |effect, cx| {
            effect.resolve(
                Some(path),
                self.props.effect,
                self.props.revision,
                light,
                cx,
            )
        });
        if let Some(picture) = picture {
            layer = layer.child(
                div().size_full().opacity(self.props.opacity).child(
                    img(picture.source())
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                ),
            );
            if self.props.blur > 0. {
                layer = layer.child(monocode_ui::styled::glass_backdrop(
                    0.,
                    self.props.blur,
                    Hsla::transparent_black(),
                ));
            }
        }
        layer
    }
}

pub struct SessionEmpty {
    view: Entity<EmptySession>,
    composer: Entity<Composer>,
    props: EmptySessionProps,
    centered: bool,
    arcade: Option<Entity<TerminalGridBackground>>,
    unavailable: Option<String>,
    remote: bool,
}
impl SessionEmpty {
    pub fn new(composer: Entity<Composer>, cx: &mut Context<Self>) -> Self {
        let props = EmptySessionProps::default();
        let view = cx.new(|_| EmptySession::new(props.clone()));
        Self {
            view,
            composer,
            props,
            centered: false,
            arcade: None,
            unavailable: None,
            remote: false,
        }
    }
    pub fn set_session(
        &mut self,
        session: Option<&Session>,
        centered: bool,
        visible: bool,
        cx: &mut Context<Self>,
    ) {
        let services = AppServices::global(cx);
        let unavailable = session
            .filter(|session| {
                services.availability.has_probed_harness_availability()
                    && !services.availability.is_harness_available(session.harness)
            })
            .map(|session| {
                monocode_harness::core::availability::harness_unavailable_hint(session.harness)
            });
        self.apply(session, centered, visible, false, unavailable, cx);
    }
    pub fn set_remote_session(
        &mut self,
        session: Option<&Session>,
        centered: bool,
        visible: bool,
        provider_hint: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.apply(session, centered, visible, true, provider_hint, cx);
    }
    fn apply(
        &mut self,
        session: Option<&Session>,
        centered: bool,
        visible: bool,
        remote: bool,
        unavailable: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let cwd = session.map_or("~", |session| session.cwd.as_str());
        let services = AppServices::global(cx);
        let props = EmptySessionProps {
            cwd: cwd.into(),
            project: monocode_engine::projects::recents::looks_like_project(cwd).then(|| {
                crate::adapters::projects_data::AppProjectsData::new(cx)
                    .map(|data| data.mark(cwd, cx).label)
                    .unwrap_or_else(|| project_name(cwd))
            }),
            has_chat_background: background_props(session, cx).path.is_some(),
            arcade_enabled: monocode_settings::settings_store::load_grid_arcade_enabled(
                &services.kv,
            ),
        };
        if self.remote != remote {
            self.remote = remote;
            cx.notify();
        }
        if unavailable != self.unavailable {
            self.unavailable = unavailable;
            cx.notify();
        }
        if props != self.props {
            self.view
                .update(cx, |view, cx| view.set_props(props.clone(), cx));
            self.props = props;
        }
        if centered != self.centered {
            self.centered = centered;
            self.view.update(cx, |view, cx| {
                view.set_composer(centered.then(|| self.composer.clone().into()), cx)
            });
        }
        let games = visible
            && session.is_none_or(|session| session.blocks.is_empty())
            && self.props.arcade_enabled
            && !self.props.has_chat_background;
        if games != self.arcade.is_some() {
            self.arcade = games.then(|| cx.new(TerminalGridBackground::new));
            self.view.update(cx, |view, cx| {
                view.set_arcade(self.arcade.as_ref().map(|arcade| arcade.clone().into()), cx)
            });
        }
    }
}
impl Render for SessionEmpty {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut frame = div().relative().size_full().child(self.view.clone());
        if self.centered
            && let Some(hint) = self.unavailable.clone()
        {
            let theme = Theme::of(cx);
            let muted = theme.content(0.60);
            let accent = theme.colors.accent;
            let remote = self.remote;
            frame = frame.child(
                div()
                    .absolute()
                    .bottom(u(24.))
                    .left_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .gap(u(8.))
                    .text_px(12.)
                    .text_color(muted)
                    .child(hint)
                    .child(
                        div()
                            .id("empty-session-provider-settings")
                            .text_color(accent)
                            .cursor_pointer()
                            .child(if remote {
                                "Manage machines"
                            } else {
                                "Provider settings"
                            })
                            .on_click(move |_, window, cx| {
                                crate::pages::settings::reveal_section(
                                    if remote {
                                        monocode_core::settings::SettingsSectionId::Connections
                                    } else {
                                        monocode_core::settings::SettingsSectionId::Providers
                                    },
                                    window,
                                    cx,
                                );
                                monocode_app::bridge::shell::ShellRequests::send(
                                    monocode_app::bridge::shell::ShellRequest::OpenPage(
                                        monocode_app::bridge::shell::ShellPage::Settings,
                                    ),
                                    cx,
                                );
                            }),
                    ),
            );
        }
        frame
    }
}

/// Uses the engine's once-per-turn prompt policy and the session's account.
pub struct SessionSignIn {
    session_id: String,
    request_key: Option<String>,
    dialog: Option<Entity<ProviderSignInDialog>>,
    dialog_subscription: Option<Subscription>,
    _subscription: Subscription,
}
impl SessionSignIn {
    pub fn new(session_id: String, cx: &mut Context<Self>) -> Self {
        let approvals = Attention::global(cx).approvals.clone();
        let subscription = cx.observe(&approvals, |this, _, cx| this.sync(cx));
        let mut this = Self {
            session_id,
            request_key: None,
            dialog: None,
            dialog_subscription: None,
            _subscription: subscription,
        };
        this.sync(cx);
        this
    }
    fn sync(&mut self, cx: &mut Context<Self>) {
        let request = Attention::global(cx)
            .approvals
            .read(cx)
            .sign_in_request()
            .filter(|request| request.session_id == self.session_id)
            .cloned();
        let key = request.as_ref().map(|request| request.key.clone());
        if key == self.request_key {
            return;
        }
        self.request_key = key;
        self.dialog = None;
        self.dialog_subscription = None;
        if let Some(request) = request {
            let login = monocode_harness::core::auth::HarnessLogin::new(
                AppServices::global(cx).children.clone(),
                &format!("native-{}", self.session_id),
            );
            let id = self.session_id.clone();
            let callback = Rc::new(move |harness, cx: &mut App| {
                let account = Engine::sessions(cx)
                    .read(cx)
                    .get(&id)
                    .and_then(|session| session.provider_account_id.clone());
                let task = login.login_harness(harness, account.as_deref());
                cx.background_executor()
                    .spawn(async move { task.await.map_err(|error| error.to_string()) })
            });
            let dialog = cx.new(|_| ProviderSignInDialog::new(request.harness, callback));
            self.dialog_subscription = Some(cx.subscribe(&dialog, |_, _, event, cx| {
                if *event == ProviderSignInEvent::Close {
                    let approvals = Attention::global(cx).approvals.clone();
                    approvals.update(cx, |approvals, cx| approvals.dismiss_sign_in(cx));
                }
            }));
            self.dialog = Some(dialog);
        }
        cx.notify();
    }
}
impl Render for SessionSignIn {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().children(self.dialog.iter().cloned())
    }
}

/// Starts a welcome only for a picker action, without replaying restored models.
pub struct ModelWelcomeLayer {
    scene: Option<Entity<monocode_view_workbench::panes::welcome::ModelWelcome>>,
    subscription: Option<Subscription>,
}
impl ModelWelcomeLayer {
    pub fn new(_: &mut Context<Self>) -> Self {
        Self {
            scene: None,
            subscription: None,
        }
    }
    pub fn picked(
        &mut self,
        harness: monocode_core::HarnessId,
        model: &str,
        cx: &mut Context<Self>,
    ) {
        let selected = AppServices::global(cx)
            .catalog
            .snapshot()
            .resolve_model(harness, Some(model));
        self.picked_model(&selected, cx);
    }
    pub fn picked_model(
        &mut self,
        selected: &monocode_core::models::AgentModel,
        cx: &mut Context<Self>,
    ) {
        use monocode_view_workbench::panes::welcome::{ModelWelcome, welcome_kind};
        self.subscription = None;
        self.scene = None;
        if let Some(kind) = welcome_kind(selected) {
            let scene = cx.new(|cx| ModelWelcome::new(kind, cx));
            self.subscription = Some(cx.subscribe(&scene, |this, _, _, cx| {
                this.scene = None;
                this.subscription = None;
                cx.notify();
            }));
            self.scene = Some(scene);
        }
        cx.notify();
    }
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.subscription = None;
        if self.scene.take().is_some() {
            cx.notify();
        }
    }
    pub fn set_composer_band(
        &mut self,
        band: Option<monocode_view_workbench::panes::welcome::ComposerBand>,
        cx: &mut Context<Self>,
    ) {
        if let Some(scene) = &self.scene {
            scene.update(cx, |scene, cx| scene.set_composer_band(band, cx));
        }
    }
}
impl Render for ModelWelcomeLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .children(self.scene.iter().cloned())
    }
}
