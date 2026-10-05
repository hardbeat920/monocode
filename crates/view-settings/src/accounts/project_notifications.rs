//! Port of src/features/notifications/ui/ProjectNotificationSettings.tsx:
//! the Inbox page's first card, one row per project with its categories,
//! its mute control, and a bulk mode that mutes several projects at once.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Transformation, Window, div, img, percentage,
    prelude::FluentBuilder as _,
};
use monocode_core::paths::path_key;
use monocode_layout::paths::{project_key, project_name};
use monocode_ui::widgets::{switch, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::host::NotificationsHost;
use super::mascot::project_mascot_icon;
use super::mute_control::{MuteControlEvent, MuteExpiry, NotificationMuteControl, SAVE_ERROR};
use super::notification_model::{
    NotificationCategory, NotificationProject, NotificationProjectKind, PreferencePatch,
    ProjectNotificationPreference, is_project_muted, project_categories,
};
use super::style::{palette, parse_css_color, text};
use crate::settings::controls::{TriggerBounds, secondary_button};

/// The card's props.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectNotificationProps {
    pub cwd: String,
    /// Recent project paths.
    pub recents: Vec<String>,
    /// The project a quick action asked to show.
    pub notification_project_path: Option<String>,
    /// Changes for each quick action, including repeated requests for one
    /// project.
    pub notification_settings_request: u64,
    /// The card flashes while Settings reveals it.
    pub highlighted: bool,
}

/// `ProjectNotificationSettings`.
pub struct ProjectNotificationSettings {
    host: Rc<dyn NotificationsHost>,
    props: ProjectNotificationProps,
    error: Option<String>,
    selected: Vec<String>,
    selecting: bool,
    expanded: Option<String>,
    focused_request: Option<(String, u64)>,
    controls: HashMap<String, Entity<NotificationMuteControl>>,
    bulk: Option<Entity<NotificationMuteControl>>,
    cards: HashMap<String, FocusHandle>,
    width: TriggerBounds,
    expiry: MuteExpiry,
    animate: bool,
    _subscriptions: Vec<Subscription>,
}

impl ProjectNotificationSettings {
    pub fn props(&self) -> &ProjectNotificationProps {
        &self.props
    }

    pub(crate) fn keep(&mut self, subscription: gpui::Subscription) {
        self._subscriptions.push(subscription);
    }
    pub fn new(
        host: Rc<dyn NotificationsHost>,
        props: ProjectNotificationProps,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak = cx.entity().downgrade();
        let observe = host.observe(
            Box::new(move |cx| {
                weak.update(cx, |this, cx| {
                    this.expiry.reset();
                    cx.notify();
                })
                .ok();
            }),
            cx,
        );
        Self {
            host,
            props,
            error: None,
            selected: Vec::new(),
            selecting: false,
            expanded: None,
            focused_request: None,
            controls: HashMap::new(),
            bulk: None,
            cards: HashMap::new(),
            width: TriggerBounds::default(),
            expiry: MuteExpiry::new(),
            animate: true,
            _subscriptions: observe.into_iter().collect(),
        }
    }

    pub fn set_props(&mut self, props: ProjectNotificationProps, cx: &mut Context<Self>) {
        self.props = props;
        cx.notify();
    }

    pub fn set_animate(&mut self, animate: bool, cx: &mut Context<Self>) {
        self.animate = animate;
        for control in self.controls.values().chain(self.bulk.iter()) {
            control.update(cx, |control, _| control.set_animate(animate));
        }
    }

    pub fn expanded(&self) -> Option<&str> {
        self.expanded.as_deref()
    }

    pub fn selecting(&self) -> bool {
        self.selecting
    }

    /// The mute control on a project's row, once it has rendered.
    pub fn control(&self, project_id: &str) -> Option<&Entity<NotificationMuteControl>> {
        self.controls.get(project_id)
    }

    /// The bulk mute control, while projects are selected.
    pub fn bulk_control(&self) -> Option<&Entity<NotificationMuteControl>> {
        self.bulk.as_ref()
    }

    /// The focus handle of a project's card.
    pub fn card_focus(&self, project_id: &str) -> Option<&FocusHandle> {
        self.cards.get(project_id)
    }

    /// `useNotificationProjects([cwd, notificationProjectPath, ...recents])`,
    /// sorted by name.
    pub fn projects(&self, cx: &App) -> Vec<NotificationProject> {
        let mut paths = vec![
            self.props.cwd.clone(),
            self.props
                .notification_project_path
                .clone()
                .unwrap_or_default(),
        ];
        paths.extend(self.props.recents.iter().cloned());
        let mut projects = self.host.notification_projects(&paths, cx);
        projects.sort_by(|a, b| monocode_locale::compare(&a.name, &b.name));
        projects
    }

    fn target_id(&self, projects: &[NotificationProject]) -> Option<String> {
        let path = self.props.notification_project_path.as_deref()?;
        let key = path_key(path);
        projects
            .iter()
            .find(|project| project.paths.iter().any(|path| path_key(path) == key))
            .map(|project| project.id.clone())
    }

    /// The quick action effect: open and focus the requested project once
    /// per request.
    fn sync_focus(
        &mut self,
        projects: &[NotificationProject],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.props.notification_project_path.clone() else {
            self.focused_request = None;
            return;
        };
        let request = self.props.notification_settings_request;
        if self.focused_request.as_ref() == Some(&(path.clone(), request)) {
            return;
        }
        let target = self.target_id(projects);
        let Some(card) = target.as_ref().and_then(|id| self.cards.get(id)).cloned() else {
            // The card has not rendered yet; try again on the next frame.
            return;
        };
        self.expanded = target;
        window.focus(&card, cx);
        self.focused_request = Some((path, request));
        cx.notify();
    }

    /// `setCategory`.
    pub fn set_category(
        &mut self,
        project_id: &str,
        category: NotificationCategory,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        let disabled = self
            .host
            .preferences(cx)
            .get(project_id)
            .map(|preference| preference.disabled.clone())
            .unwrap_or_default();
        let next = if enabled {
            disabled.into_iter().filter(|id| *id != category).collect()
        } else {
            let mut next = disabled;
            next.push(category);
            next
        };
        match self.host.update_preferences(
            &[project_id.to_string()],
            &PreferencePatch::disabled(next),
            cx,
        ) {
            Ok(()) => self.error = None,
            Err(_) => self.error = Some(SAVE_ERROR.into()),
        }
        cx.notify();
    }

    pub fn toggle_selecting(&mut self, cx: &mut Context<Self>) {
        self.selecting = !self.selecting;
        self.selected.clear();
        cx.notify();
    }

    pub fn toggle_expanded(&mut self, project_id: &str, cx: &mut Context<Self>) {
        self.expanded = if self.expanded.as_deref() == Some(project_id) {
            None
        } else {
            Some(project_id.to_string())
        };
        cx.notify();
    }

    fn mute_control(
        &mut self,
        project_id: &str,
        cx: &mut Context<Self>,
    ) -> Entity<NotificationMuteControl> {
        if let Some(control) = self.controls.get(project_id) {
            return control.clone();
        }
        let host = self.host.clone();
        let ids = vec![project_id.to_string()];
        let scope = format!("project:{project_id}");
        let animate = self.animate;
        let control = cx.new(|cx| {
            let mut control = NotificationMuteControl::new(host, ids, scope, cx);
            control.set_animate(animate);
            control
        });
        self._subscriptions
            .push(cx.subscribe(&control, |this, _, _: &MuteControlEvent, cx| {
                this.expiry.reset();
                cx.notify();
            }));
        self.controls
            .insert(project_id.to_string(), control.clone());
        control
    }

    fn ensure_bulk_control(
        &mut self,
        ids: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Entity<NotificationMuteControl> {
        if let Some(bulk) = &self.bulk {
            bulk.update(cx, |control, cx| control.set_project_ids(ids, cx));
            return bulk.clone();
        }
        let host = self.host.clone();
        let animate = self.animate;
        let control = cx.new(|cx| {
            let mut control = NotificationMuteControl::new(host, ids, "bulk", cx);
            control.set_animate(animate);
            control
        });
        self._subscriptions
            .push(cx.subscribe(&control, |this, _, _: &MuteControlEvent, cx| {
                this.expiry.reset();
                cx.notify();
            }));
        self.bulk = Some(control.clone());
        control
    }

    /// The project's icon: its logo, its mascot, or a folder.
    fn project_icon(&self, project: &NotificationProject, cx: &App) -> AnyElement {
        let theme = Theme::of(cx);
        let reference = self
            .props
            .notification_project_path
            .as_deref()
            .filter(|path| !path.is_empty())
            .unwrap_or(&self.props.cwd);
        let reference = path_key(reference);
        let path = project
            .paths
            .iter()
            .find(|path| path_key(path) == reference)
            .or_else(|| project.paths.first());
        let frame = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(16.));
        let Some(path) = path else {
            return frame
                .child(
                    icon(IconName::Folder)
                        .size(u(16.))
                        .text_color(theme.content(0.40)),
                )
                .into_any_element();
        };
        let key = project_key(path);
        let seed = project_name(path);
        let appearance = self.host.project_appearance(&key, &seed, cx);
        if let Some(logo) = appearance.logo {
            return frame
                .child(
                    img(std::path::PathBuf::from(logo))
                        .size(u(16.))
                        .rounded(u(2.)),
                )
                .into_any_element();
        }
        let color = parse_css_color(&appearance.color).unwrap_or(theme.colors.accent);
        frame
            .child(project_mascot_icon(
                &seed,
                appearance.mascot.as_deref(),
                color,
                12.,
            ))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_project(
        &mut self,
        project: &NotificationProject,
        preference: Option<&ProjectNotificationPreference>,
        now: i64,
        wide: bool,
        last: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let categories = project_categories(project.kind);
        let disabled: Vec<NotificationCategory> = preference
            .map(|preference| preference.disabled.clone())
            .unwrap_or_default();
        let enabled_count = categories
            .iter()
            .filter(|category| !disabled.contains(category))
            .count();
        let muted = preference.is_some_and(|preference| is_project_muted(preference, now));
        let expanded = self.expanded.as_deref() == Some(project.id.as_str());
        let status = if muted {
            "All notifications paused".to_string()
        } else if enabled_count == categories.len() {
            "All categories enabled".to_string()
        } else {
            format!("{enabled_count} of {} enabled", categories.len())
        };
        let status = if project.kind == NotificationProjectKind::Local {
            format!("Local project · {status}")
        } else {
            status
        };
        let focus = self
            .cards
            .entry(project.id.clone())
            .or_insert_with(|| cx.focus_handle())
            .clone();
        let control = self.mute_control(&project.id, cx);
        let name = project.name.clone();
        let id = project.id.clone();
        let mut left = div()
            .flex()
            .min_w(u(200.))
            .flex_1()
            .items_center()
            .gap(u(12.));
        if self.selecting {
            let checked = self.selected.contains(&project.id);
            let toggle_id = project.id.clone();
            left = left.child(checkbox(
                format!("Select {}", project.name),
                checked,
                false,
                cx.listener(move |this, _, _, cx| {
                    if this.selected.contains(&toggle_id) {
                        this.selected.retain(|id| *id != toggle_id);
                    } else {
                        this.selected.push(toggle_id.clone());
                    }
                    cx.notify();
                }),
                cx,
            ));
        }
        let hover_ink = theme.content(0.75);
        let chevron = icon(IconName::ChevronRight)
            .size(u(14.))
            .text_color(theme.content(0.40))
            .when(expanded, |svg| {
                svg.with_transformation(Transformation::rotate(percentage(0.25)))
            });
        let expand_selector = format!("button:Notification categories for {name}");
        let expand_id = id.clone();
        left = left.child(
            div()
                .id(SharedString::from(format!("expand-{id}")))
                .group(SharedString::from(format!("expand-{id}")))
                .flex()
                .min_w_0()
                .flex_1()
                .items_center()
                .gap(u(12.))
                .rounded(u(theme.radius.md))
                .debug_selector(move || expand_selector)
                .tooltip(tooltip(format!("{} ({})", project.name, project.detail)))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_expanded(&expand_id, cx)))
                .child(self.project_icon(project, cx))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(
                            text(project.name.clone())
                                .truncate()
                                .text_px(13.)
                                .medium()
                                .text_color(theme.colors.content)
                                .group_hover(
                                    SharedString::from(format!("expand-{id}")),
                                    move |s| s.text_color(hover_ink),
                                ),
                        )
                        .child(
                            text(status)
                                .mt(u(4.))
                                .text_px(12.)
                                .leading(theme.leading.relaxed)
                                .text_color(theme.content(0.45)),
                        ),
                )
                .child(chevron),
        );
        let row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_x(u(24.))
            .gap_y(u(12.))
            .px(u(16.))
            .py(u(14.))
            .child(left)
            .child(div().ml_auto().max_w_full().child(control));
        let mut card = div()
            .id(SharedString::from(format!("card-{id}")))
            .track_focus(&focus)
            .relative()
            .min_w_0()
            .when(!last, |el| {
                el.border_b_1().border_color(theme.content(0.05))
            })
            .debug_selector(move || format!("fieldset:{name}"))
            .child(row);
        if focus.is_focused(window) {
            // `focus-visible:ring-1 ring-inset ring-accent/50`.
            card = card.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .border_1()
                    .border_color(theme.accent(0.50)),
            );
        }
        if expanded {
            let hint_selector = format!("hint:{}", project.name);
            let mut panel = div().pl(u(if !wide {
                0.
            } else if self.selecting {
                56.
            } else {
                28.
            }));
            if muted {
                panel = panel.child(
                    text("Your category choices apply when notifications resume. You can edit them while muted.")
                        .pt(u(14.))
                        .text_px(12.)
                        .leading(theme.leading.relaxed)
                        .text_color(theme.content(0.45))
                        .debug_selector(move || hint_selector),
                );
            }
            let count = categories.len();
            for (index, category) in categories.into_iter().enumerate() {
                let on = !disabled.contains(&category);
                let label = format!("{} for {}", category.label(), project.name);
                let switch_selector = format!("switch:{label}");
                let state_selector = format!(
                    "switch-state:{label}={}{}",
                    if on { "on" } else { "off" },
                    if muted { ":described" } else { "" }
                );
                let project_id = project.id.clone();
                let toggle_id = project.id.clone();
                let hover_ink = theme.content(0.75);
                panel = panel.child(
                    div()
                        .id(SharedString::from(format!(
                            "category-{}-{}",
                            project.id,
                            category.as_str()
                        )))
                        .flex()
                        .min_h(u(44.))
                        .items_center()
                        .justify_between()
                        .gap(u(24.))
                        .py(u(14.))
                        .text_px(13.)
                        .text_color(theme.colors.content)
                        .hover(move |s| s.text_color(hover_ink))
                        .when(index + 1 < count, |el| {
                            el.border_b_1().border_color(theme.content(0.05))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_category(&project_id, category, !on, cx)
                        }))
                        .child(category.label())
                        .child(
                            div()
                                .flex_none()
                                .debug_selector(move || switch_selector)
                                .child(div().debug_selector(move || state_selector).child(switch(
                                    SharedString::from(format!(
                                        "switch-{toggle_id}-{}",
                                        category.as_str()
                                    )),
                                    on,
                                ))),
                        ),
                );
            }
            let panel_selector = format!("panel:{}", project.name);
            card = card.child(
                div()
                    .border_t_1()
                    .border_color(theme.content(0.05))
                    .px(u(16.))
                    .debug_selector(move || panel_selector)
                    .child(panel),
            );
        }
        card.into_any_element()
    }
}

/// `ProjectSelection`: the square checkbox, with a dash when `mixed`.
fn checkbox(
    label: String,
    checked: bool,
    mixed: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let filled = checked || mixed;
    let hover = theme.content(0.40);
    let selector = format!(
        "checkbox:{label}={}",
        if mixed {
            "mixed"
        } else if checked {
            "on"
        } else {
            "off"
        }
    );
    let mut boxed = div()
        .id(SharedString::from(format!("checkbox-{label}")))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(u(16.))
        .rounded(u(theme.radius.sm))
        .border_1()
        .debug_selector(move || selector)
        .on_click(on_click);
    boxed = if filled {
        boxed
            .border_color(theme.colors.accent)
            .bg(theme.colors.accent)
    } else {
        boxed
            .border_color(theme.content(0.20))
            .hover(move |s| s.border_color(hover))
    };
    if mixed {
        boxed = boxed.child(
            icon(IconName::Minus)
                .size(u(12.))
                .text_color(palette::white()),
        );
    } else if checked {
        boxed = boxed.child(
            icon(IconName::Check)
                .size(u(12.))
                .text_color(palette::white()),
        );
    }
    let wrapper_selector = format!("checkbox:{label}");
    div()
        .debug_selector(move || wrapper_selector)
        .child(boxed)
        .into_any_element()
}

impl Render for ProjectNotificationSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.expiry.schedule(&self.host, cx);
        let theme = Theme::of(cx).clone();
        let projects = self.projects(cx);
        self.sync_focus(&projects, window, cx);
        let now = self.host.now();
        let preferences = self.host.preferences(cx);
        let selected_ids: Vec<String> = self
            .selected
            .iter()
            .filter(|id| projects.iter().any(|project| project.id == **id))
            .cloned()
            .collect();
        let wide = self.width.get().is_none_or(|bounds| {
            f32::from(bounds.size.width) / f32::from(window.rem_size()) * 16.0 >= 400.0
        });

        let mut header = div()
            .flex()
            .flex_wrap()
            .items_end()
            .gap(u(16.))
            .pb(u(10.))
            .child(
                div()
                    .min_w(u(240.))
                    .flex_1()
                    .child(
                        text("Project notifications")
                            .text_px(13.)
                            .semibold()
                            .text_color(theme.colors.content),
                    )
                    .child(
                        div()
                            .mt(u(4.))
                            .text_px(12.)
                            .leading(theme.leading.relaxed)
                            .text_color(theme.content(0.45))
                            .child("Choose sounds, banners and sidebar indicators by category. Mute pauses them without changing your choices. Unread items stay marked in Inbox."),
                    ),
            );
        if !projects.is_empty() {
            header = header.child(
                div().flex_none().pb(u(2.)).child(
                    secondary_button(
                        "select-projects",
                        if self.selecting {
                            "Done"
                        } else {
                            "Select projects"
                        },
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_selecting(cx))),
                ),
            );
        }

        let mut card = div()
            .overflow_hidden()
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(if self.props.highlighted {
                theme.accent(0.60)
            } else {
                theme.content(0.10)
            })
            .bg(theme.content(0.03))
            .when(self.props.highlighted, |el| {
                el.debug_selector(|| "flash-project-notifications".into())
            });
        if let Some(error) = self.error.clone() {
            card = card.child(
                div()
                    .px(u(16.))
                    .py(u(14.))
                    .text_px(12.)
                    .text_color(theme.colors.danger)
                    .debug_selector(|| "alert".into())
                    .child(text(error)),
            );
        }
        if projects.is_empty() {
            card = card.child(
                text("Open a project or connect an Inbox provider to configure its notifications.")
                    .px(u(16.))
                    .py(u(14.))
                    .text_px(12.)
                    .leading(theme.leading.relaxed)
                    .text_color(theme.content(0.45)),
            );
        } else {
            if self.selecting {
                let all = selected_ids.len() == projects.len();
                let mixed = !selected_ids.is_empty() && selected_ids.len() < projects.len();
                let ids: Vec<String> = projects.iter().map(|project| project.id.clone()).collect();
                let label = if selected_ids.is_empty() {
                    "Select all projects".to_string()
                } else {
                    format!("{} selected", selected_ids.len())
                };
                let mut bar = div()
                    .flex()
                    .min_h(u(36.))
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(u(12.))
                    .border_b_1()
                    .border_color(theme.content(0.05))
                    .px(u(16.))
                    .py(u(14.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(10.))
                            .text_px(12.)
                            .text_color(theme.content(0.55))
                            .child(checkbox(
                                "Select all projects".into(),
                                all,
                                mixed,
                                cx.listener(move |this, _, _, cx| {
                                    let checked = !(this.selected.len() == ids.len());
                                    this.selected = if checked { ids.clone() } else { Vec::new() };
                                    cx.notify();
                                }),
                                cx,
                            ))
                            .child(text(label)),
                    );
                if !selected_ids.is_empty() {
                    let bulk = self.ensure_bulk_control(selected_ids.clone(), cx);
                    bar = bar.child(
                        div()
                            .debug_selector(|| "group:Mute selected projects".into())
                            .child(bulk),
                    );
                }
                card = card.child(bar);
            }
            let count = projects.len();
            for (index, project) in projects.iter().enumerate() {
                let preference = preferences.get(&project.id).cloned();
                let row = self.render_project(
                    project,
                    preference.as_ref(),
                    now,
                    wide,
                    index + 1 == count,
                    window,
                    cx,
                );
                card = card.child(row);
            }
        }
        div()
            .relative()
            .flex()
            .flex_col()
            .debug_selector(|| "section:Project notifications".into())
            .child(self.width.probe())
            .child(header)
            .child(card)
    }
}
