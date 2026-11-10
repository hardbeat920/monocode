use super::*;
use crate::{
    project_background::ProjectBackground,
    project_settings::{AppearanceControls, BackgroundDialog, Event, project_icon},
    project_theme::ThemeDialog,
};
use monocode_core::project_appearance::ProjectAppearance;

/// A project dialog from the project menu: its background or theme colors.
pub(super) struct OpenBackground {
    view: ProjectDialog,
    _events: Subscription,
}
#[derive(Clone)]
enum ProjectDialog {
    Background(Entity<BackgroundDialog>),
    Theme(Entity<ThemeDialog>),
}
impl Shell {
    pub(super) fn appearance(&self, path: &str) -> ProjectAppearance {
        self.workspace
            .project_appearance
            .get(path)
            .cloned()
            .unwrap_or_default()
    }
    pub(super) fn project_label(&self, path: &str) -> String {
        self.appearance(path).label(path)
    }
    pub(super) fn project_mark(&self, path: &str, side: f32) -> AnyElement {
        project_icon(&self.appearance(path), path, side)
    }
    pub(super) fn save_project_appearance(
        &mut self,
        path: String,
        mut appearance: ProjectAppearance,
        cx: &mut Context<Self>,
    ) {
        appearance.normalize();
        for session in self.sessions.values() {
            if session.read(cx).summary.cwd == path {
                session.update(cx, |session, cx| {
                    session.set_project_appearance(appearance.clone(), cx)
                });
            }
        }
        self.workspace.project_appearance.insert(path, appearance);
        self.invalidate_chrome(cx);
        self.persist(cx);
        cx.notify();
    }
    pub(super) fn project_appearance_controls(
        &self,
        path: &str,
        events: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<AppearanceControls> {
        let path = path.to_owned();
        let controls =
            cx.new(|cx| AppearanceControls::new(path.clone(), self.appearance(&path), cx));
        events.push(cx.subscribe_in(
            &controls,
            window,
            move |this, _, event: &Event, window, cx| match event {
                Event::Changed(appearance) => {
                    this.save_project_appearance(path.clone(), appearance.clone(), cx)
                }
                Event::Close => {
                    this.close_menu(cx);
                    this.focus_workspace(window, cx);
                }
            },
        ));
        controls
    }
    pub(super) fn project_settings_flyout(
        &self,
        events: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Flyout {
        let target = MenuTarget::Project {
            path: self.workspace.project.clone(),
            rail: false,
        };
        let (label, items, width, style) = self.menu_items(&target, events, window, cx);
        let menu = cx.new(|cx| Menu::embedded(label, items, cx).width(width).style(style));
        let surface = menu.read(cx).surface_region();
        let focus = menu.read(cx).content_focus();
        events.push(cx.subscribe_in(
            &menu,
            window,
            move |this, _, event: &MenuEvent, window, cx| {
                this.close_menu(cx);
                if let MenuEvent::Picked(action, row) = event {
                    this.run_menu(target.clone(), action, *row, window, cx);
                }
                if this.background_dialog.is_none() {
                    this.focus_workspace(window, cx);
                }
                cx.notify();
            },
        ));
        Flyout {
            view: menu.into(),
            width,
            padding: 8.,
            height: None,
            max_height: Some(640.),
            focus: Some(focus),
            surface: Some(surface),
        }
    }
    /// The Theme colors dialog for `path`. Its colors show live, so the
    /// project opens first.
    pub(super) fn open_project_theme(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.project != path {
            self.select_project(path.clone(), window, cx);
        }
        self.close_menu(cx);
        self.close_preview(cx);
        let prefs = &self.workspace.preferences;
        let light = prefs.theme == "light"
            || (prefs.theme == "system"
                && matches!(
                    window.appearance(),
                    gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
                ));
        let app = prefs.project_theme();
        let appearance = self.appearance(&path);
        let view = cx.new(|cx| ThemeDialog::new(path.clone(), appearance, app, light, cx));
        let events = self.project_dialog_events(&view, path, window, cx);
        window.focus(&view.focus_handle(cx), cx);
        self.background_dialog = Some(OpenBackground {
            view: ProjectDialog::Theme(view),
            _events: events,
        });
        cx.notify();
    }
    fn project_dialog_events<T: gpui::EventEmitter<Event>>(
        &mut self,
        view: &Entity<T>,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(
            view,
            window,
            move |this, _, event: &Event, window, cx| match event {
                Event::Changed(appearance) => {
                    this.save_project_appearance(path.clone(), appearance.clone(), cx)
                }
                Event::Close => this.close_background_dialog(window, cx),
            },
        )
    }
    pub(super) fn open_background_dialog(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menu(cx);
        self.close_preview(cx);
        let view = cx.new(|cx| BackgroundDialog::new(path.clone(), self.appearance(&path), cx));
        let events = self.project_dialog_events(&view, path, window, cx);
        window.focus(&view.focus_handle(cx), cx);
        self.background_dialog = Some(OpenBackground {
            view: ProjectDialog::Background(view),
            _events: events,
        });
        cx.notify();
    }
    fn close_background_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.background_dialog = None;
        self.focus_workspace(window, cx);
        cx.notify();
    }
    pub(super) fn background_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let view = self.background_dialog.as_ref()?.view.clone();
        let viewport = window.viewport_size();
        let width = 420f32.min(viewport.width.as_f32() - 24.);
        let (measured, popup_region, view): (_, _, gpui::AnyView) = match view {
            ProjectDialog::Background(view) => (
                view.read(cx).measured_height().unwrap_or(622.),
                view.read(cx).popup_region(),
                view.into(),
            ),
            ProjectDialog::Theme(view) => (
                view.read(cx).measured_height().unwrap_or(490.),
                Default::default(),
                view.into(),
            ),
        };
        let height = (measured + 2.).min(viewport.height.as_f32() - 32.);
        let anchor = Anchor::Point(gpui::point(
            px((viewport.width.as_f32() - width) / 2.),
            px((viewport.height.as_f32() - height) / 2.),
        ));
        let weak = cx.weak_entity();
        Some(
            div()
                .absolute()
                .size_full()
                .child(crate::modal::scrim("project-background-scrim"))
                .child(
                    popover("project-background-modal", anchor)
                        .modal_surface(self.workspace.boxy_layout())
                        .width(width)
                        .gap(0.)
                        .max_height(viewport.height.as_f32() - 32.)
                        .ignore(popup_region)
                        .priority(60)
                        .on_dismiss(move |window, cx| {
                            let _ = weak
                                .update(cx, |this, cx| this.close_background_dialog(window, cx));
                        })
                        .child(view),
                )
                .into_any_element(),
        )
    }
    pub(super) fn chat_with_background(
        &mut self,
        id: i64,
        chat: Option<Entity<Conversation>>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let Some(chat) = chat else {
            return div().flex_1().min_h_0();
        };
        let (path, empty) = {
            let session = chat.read(cx);
            (session.summary.cwd.clone(), !session.summary.has_messages)
        };
        let mut appearance = self.appearance(&path);
        if appearance.background.is_none() {
            use monocode_core::project_appearance::{Background, Effect, Scope};
            let p = &self.workspace.preferences;
            appearance.background = p.chat_background.as_ref().map(|path| Background {
                path: path.clone(),
                effect: match p.chat_background_effect.as_str() {
                    "dither" => Effect::Dither,
                    "ascii" => Effect::Ascii,
                    "halftone" => Effect::Halftone,
                    "scanlines" => Effect::Scanlines,
                    "gradient-blur" => Effect::GradientBlur,
                    _ => Effect::None,
                },
                scope: if p.chat_background_scope == "empty" {
                    Scope::Empty
                } else {
                    Scope::All
                },
                empty_opacity: p.chat_background_empty.clamp(5, 65) as f32 / 100.,
                session_opacity: p.chat_background_session.clamp(5, 65) as f32 / 100.,
            });
        }
        let classic = self.workspace.boxy_layout();
        chat.update(cx, |session, cx| {
            session.set_project_appearance(appearance.clone(), cx);
            session.set_classic_layout(classic, cx);
        });
        let settings = appearance.background.filter(|b| b.opacity(empty) > 0.);
        if settings.is_none() {
            self.backgrounds.remove(&id);
        }
        let background = settings.map(|settings| {
            let clip = (!self.workspace.boxy_layout()).then(|| self.background_card.clone());
            let background = self
                .backgrounds
                .entry(id)
                .or_insert_with(|| cx.new(|_| ProjectBackground::default()))
                .clone();
            background.update(cx, |background, cx| {
                background.configure(Some(settings), empty, clip, cx)
            });
            background
        });
        if self.workspace.active == Some(id) {
            let previous = self.backdrop.replace((id, background.is_some()));
            if let (Some((previous, false)), Some(background)) = (previous, &background)
                && previous != id
            {
                background.update(cx, |background, cx| background.fade_in(cx));
            }
        }
        let linked = if self.workspace.active == Some(id) && !self.review_focused {
            self.linked_notice(id, cx)
        } else {
            None
        };
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .when_some(background, |d, background| {
                d.child(background.cached(StyleRefinement::default().absolute().size_full()))
            })
            .child(
                div()
                    .relative()
                    .size_full()
                    .child(chat.cached(StyleRefinement::default().size_full())),
            )
            .children(linked.map(|notice| {
                div()
                    .absolute()
                    .top(px(12.))
                    .right(px(12.))
                    .w(px(320.))
                    .max_w_full()
                    .child(notice)
            }))
    }
}

impl Shell {
    pub(super) fn project_notification_menu(&self, cwd: &str) -> MenuAction {
        let now = monocode_core::time::now_ms();
        let prefs = &self.workspace.preferences;
        let muted = prefs.known_notification_project_id(cwd).is_some_and(|id| {
            let preference = &prefs.project_notifications[id];
            preference.get("mutedUntil") == Some(&serde_json::Value::Null)
                || preference["mutedUntil"].as_u64().is_some_and(|at| at > now)
        });
        if muted {
            MenuAction::new("mute:resume", "Resume notifications").icon("bell")
        } else {
            MenuAction::new("notifications-mute", "Mute notifications")
                .icon("bell-off")
                .submenu(vec![
                    MenuAction::new("mute:1h", "For 1 hour"),
                    MenuAction::new("mute:4h", "For 4 hours"),
                    MenuAction::new("mute:8h", "For 8 hours"),
                    MenuAction::new("mute:forever", "Until resumed"),
                    MenuAction::new("mute:custom", "Custom…"),
                ])
        }
    }

    pub(super) fn run_project_notification_menu(
        &mut self,
        cwd: &str,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action == "mute:custom" {
            self.open_settings(crate::settings::Section::Inbox, window, cx);
            return;
        }
        if action == "mute:resume" {
            self.workspace.preferences.resume_notification_project(cwd);
        } else {
            let hours = match action {
                "mute:1h" => Some(1),
                "mute:4h" => Some(4),
                "mute:8h" => Some(8),
                "mute:forever" => None,
                _ => return,
            };
            let until = hours.map(|hours| monocode_core::time::now_ms() + hours * 3_600_000);
            self.workspace
                .preferences
                .mute_notification_project(cwd, until);
        }
        self.apply_preferences(cx);
        self.sync_settings(cx);
        self.persist(cx);
        cx.notify();
    }
}
