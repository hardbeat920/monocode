//! Port of src/features/projects/ui/ProjectBackgroundDialog.tsx: one
//! project's chat background image, effect, scope, and visibility.
//!
//! The dialog frame follows monocode-ui's modal with Modal.tsx's
//! `fitViewport`: centered vertically and kept inside the window.

use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ObjectFit, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
    Subscription, Task, Window, deferred, div, img, prelude::FluentBuilder as _,
};
use monocode_core::appearance::{
    CHAT_BACKGROUND_OPACITY_MAX, CHAT_BACKGROUND_OPACITY_MIN, ChatBackgroundScope,
    NEW_THREAD_BACKGROUND_EFFECT_DEFAULT, NewThreadBackgroundEffect,
};
use monocode_core::js;
use monocode_settings::Kv;
use monocode_ui::styled::glass_backdrop;
use monocode_ui::widgets::icon_button;
use monocode_ui::{IconName, Theme, UiStyled as _, u};

use super::background::{
    BackgroundImage, ChatBackground, HazeVariant, ProjectBackgroundEffect, gradient_blur_background,
};
use super::controls::{segmented, slider, spinner_icon};
use super::host::{ProjectBackgroundHost, ProjectBackgroundSettings};
use super::select::{Select, SelectOption, SelectStyle};
use super::store;

type CloseHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Which visibility slider moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpacityKind {
    Empty,
    Session,
}

pub struct ProjectBackgroundDialog {
    project: String,
    name: String,
    kv: Kv,
    host: Rc<dyn ProjectBackgroundHost>,
    path: Option<String>,
    empty_opacity: f64,
    session_opacity: f64,
    scope: ChatBackgroundScope,
    effect: NewThreadBackgroundEffect,
    revision: i64,
    busy: bool,
    error: Option<String>,
    preview: Entity<ProjectBackgroundEffect>,
    effect_select: Entity<Select>,
    focus: FocusHandle,
    on_close: Option<CloseHandler>,
    job: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

fn effect_options() -> Vec<SelectOption> {
    NewThreadBackgroundEffect::ALL
        .iter()
        .map(|effect| SelectOption::new(effect.as_str(), effect.label()))
        .collect()
}

impl ProjectBackgroundDialog {
    pub fn new(
        project: impl Into<String>,
        name: impl Into<String>,
        kv: Kv,
        host: Rc<dyn ProjectBackgroundHost>,
        cx: &mut Context<Self>,
    ) -> Self {
        let project = project.into();
        let initial = host.load_settings(&project, cx);
        let appearance = store::load_appearance(&kv, monocode_core::Platform::current());
        let effect = initial
            .as_ref()
            .map(|settings| settings.effect)
            .unwrap_or(NEW_THREAD_BACKGROUND_EFFECT_DEFAULT);
        let preview = cx.new(|_| ProjectBackgroundEffect::new());
        let this = cx.entity().downgrade();
        let layer = Theme::of(cx).layer.dialog_popover;
        let effect_select = cx.new(|cx| {
            Select::new(
                "Project background effect",
                effect.as_str(),
                effect_options(),
                cx,
            )
            .style(SelectStyle::Transparent)
            .layer(layer)
            .on_change(move |value, _, cx| {
                let next = NewThreadBackgroundEffect::parse(Some(value));
                this.update(cx, |this, cx| this.update_effect(next, cx))
                    .ok();
            })
        });
        let subscriptions = vec![cx.observe(&preview, |_, _, cx| cx.notify())];
        Self {
            path: initial.as_ref().map(|settings| settings.path.clone()),
            empty_opacity: initial
                .as_ref()
                .map(|settings| settings.empty_opacity)
                .unwrap_or(appearance.chat_background_empty_opacity),
            session_opacity: initial
                .as_ref()
                .map(|settings| settings.session_opacity)
                .unwrap_or(appearance.chat_background_session_opacity),
            scope: initial
                .as_ref()
                .map(|settings| settings.scope)
                .unwrap_or(appearance.chat_background_scope),
            effect,
            revision: host.image_revision(cx),
            project,
            name: name.into(),
            kv,
            host,
            busy: false,
            error: None,
            preview,
            effect_select,
            focus: cx.focus_handle(),
            on_close: None,
            job: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    pub fn effect(&self) -> NewThreadBackgroundEffect {
        self.effect
    }

    pub fn empty_opacity(&self) -> f64 {
        self.empty_opacity
    }

    pub fn session_opacity(&self) -> f64 {
        self.session_opacity
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub fn effect_select(&self) -> &Entity<Select> {
        &self.effect_select
    }

    /// `save`.
    fn save(&mut self, image_changed: bool, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let settings = ProjectBackgroundSettings {
            path,
            empty_opacity: self.empty_opacity,
            session_opacity: self.session_opacity,
            scope: self.scope,
            effect: self.effect,
        };
        self.host
            .save_settings(&self.project, &settings, image_changed, cx);
        self.revision = self.host.image_revision(cx);
    }

    /// `choose`.
    pub fn choose(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        let picked = self.host.pick_and_save(&self.project, cx);
        self.job = Some(cx.spawn(async move |this, cx| {
            let result = picked.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Some(path)) => {
                        this.path = Some(path);
                        this.save(true, cx);
                    }
                    Ok(None) => {}
                    Err(error) => this.error = Some(error),
                }
                this.busy = false;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// `removeImage`.
    pub fn remove_image(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        let cleared = self.host.clear_image(&self.project, cx);
        self.job = Some(cx.spawn(async move |this, cx| {
            let result = cleared.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.host.clear_setting(&this.project, cx);
                        let appearance =
                            store::load_appearance(&this.kv, monocode_core::Platform::current());
                        this.path = None;
                        this.empty_opacity = appearance.chat_background_empty_opacity;
                        this.session_opacity = appearance.chat_background_session_opacity;
                        this.scope = appearance.chat_background_scope;
                        this.effect = NEW_THREAD_BACKGROUND_EFFECT_DEFAULT;
                        let effect = this.effect;
                        this.effect_select
                            .update(cx, |select, cx| select.set_value(effect.as_str(), cx));
                        this.revision = this.host.image_revision(cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                this.busy = false;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// `updateOpacity`.
    pub fn update_opacity(&mut self, kind: OpacityKind, percent: f64, cx: &mut Context<Self>) {
        let next =
            (percent / 100.0).clamp(CHAT_BACKGROUND_OPACITY_MIN, CHAT_BACKGROUND_OPACITY_MAX);
        match kind {
            OpacityKind::Empty => self.empty_opacity = next,
            OpacityKind::Session => self.session_opacity = next,
        }
        self.save(false, cx);
        cx.notify();
    }

    /// `updateScope`.
    pub fn update_scope(&mut self, next: ChatBackgroundScope, cx: &mut Context<Self>) {
        self.scope = next;
        self.save(false, cx);
        cx.notify();
    }

    /// `updateEffect`.
    pub fn update_effect(&mut self, next: NewThreadBackgroundEffect, cx: &mut Context<Self>) {
        self.effect = next;
        self.effect_select
            .update(cx, |select, cx| select.set_value(next.as_str(), cx));
        self.save(false, cx);
        cx.notify();
    }

    fn close(&self, window: &mut Window, cx: &mut App) {
        if let Some(close) = self.on_close.clone() {
            close(window, cx);
        }
    }

    /// The preview's image and effect: the project's own, else the global
    /// background.
    fn preview_source(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(
        BackgroundImage,
        NewThreadBackgroundEffect,
        SharedString,
        i64,
    )> {
        let light = Theme::of(cx).scheme == monocode_ui::ColorScheme::Light;
        match self.path.clone() {
            Some(path) => {
                let (effect, revision) = (self.effect, self.revision);
                let prepared = self.preview.update(cx, |preview, cx| {
                    preview.resolve(Some(&path), effect, revision, light, cx)
                });
                let image =
                    prepared.unwrap_or_else(|| BackgroundImage::Original(path.clone().into()));
                Some((image, effect, path.into(), revision))
            }
            None => {
                let appearance =
                    store::load_appearance(&self.kv, monocode_core::Platform::current());
                let path = appearance.chat_background_path?;
                let revision = ChatBackground::image_revision(cx);
                Some((
                    BackgroundImage::Original(path.clone().into()),
                    appearance.new_thread_background_effect,
                    path.into(),
                    revision,
                ))
            }
        }
    }

    fn preview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let opacity = self.empty_opacity as f32;
        let source = self.preview_source(cx);
        let haze = source
            .as_ref()
            .is_some_and(|(_, effect, _, _)| *effect == NewThreadBackgroundEffect::GradientBlur);
        let inner: AnyElement = match source {
            Some((_, NewThreadBackgroundEffect::GradientBlur, path, revision)) => div()
                .relative()
                .h(u(160.))
                .child(
                    gradient_blur_background(
                        "project-background-preview",
                        path,
                        revision,
                        HazeVariant::Preview,
                    )
                    .opacity(opacity),
                )
                .into_any_element(),
            Some((image, _, _, _)) => div()
                .h(u(160.))
                .w_full()
                .opacity(opacity)
                .debug_selector(|| "project-background-image".into())
                .child(
                    img(image.source())
                        .size_full()
                        .rounded(u(theme.radius.xl - 1.))
                        .object_fit(ObjectFit::Cover),
                )
                .into_any_element(),
            None => div()
                .flex()
                .h(u(160.))
                .items_center()
                .justify_center()
                .text_px(theme.text.label)
                .text_color(theme.content(0.40))
                .child("No background selected")
                .into_any_element(),
        };
        div()
            .overflow_hidden()
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(if haze {
                theme.colors.background_base
            } else {
                theme.content(0.05)
            })
            .child(inner)
            .into_any_element()
    }

    fn labeled_row(label: &'static str, control: impl IntoElement, cx: &App) -> AnyElement {
        let theme = Theme::of(cx);
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(u(16.))
            .pt(u(16.))
            .border_t_1()
            .border_color(theme.colors.stroke)
            .child(
                div()
                    .text_px(theme.text.body)
                    .medium()
                    .text_color(theme.colors.content)
                    .child(label),
            )
            .child(control)
            .into_any_element()
    }
}

impl Focusable for ProjectBackgroundDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ProjectBackgroundDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let preview = self.preview(cx);
        let busy = self.busy;
        let hover_fill = theme.content(0.10);
        let hover_ink = theme.colors.content;
        let mut choose = div()
            .id("project-background-choose")
            .mt(u(8.))
            .flex()
            .w_full()
            .items_center()
            .justify_center()
            .gap(u(6.))
            .px(u(10.))
            .py(u(6.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .text_px(theme.text.label)
            .text_color(theme.content(0.70))
            .debug_selector(|| "button:project-background-choose".into())
            .when(busy, |el| {
                el.child(spinner_icon(
                    "project-background-busy",
                    14.,
                    theme.content(0.70),
                ))
            })
            .child(if self.path.is_some() {
                "Change image"
            } else {
                "Choose image"
            });
        if busy {
            choose = choose.opacity(0.4);
        } else {
            choose = choose
                .hover(move |s| s.bg(hover_fill).text_color(hover_ink))
                .on_click(cx.listener(|this, _, _, cx| this.choose(cx)));
        }
        let mut image_block = div().flex().flex_col().child(preview).child(choose).child(
            div()
                .mt(u(6.))
                .text_px(theme.text.caption)
                .leading(theme.leading.relaxed)
                .text_color(theme.content(0.45))
                .child(if self.path.is_some() {
                    "This image overrides the global background for this project."
                } else {
                    "This project currently follows the global Appearance setting."
                }),
        );
        if let Some(error) = self.error.clone() {
            image_block = image_block.child(
                div()
                    .mt(u(6.))
                    .text_px(theme.text.label)
                    .text_color(theme.colors.danger)
                    .child(error),
            );
        }
        let mut body = div()
            .flex()
            .flex_col()
            .gap(u(20.))
            .p(u(16.))
            .child(image_block);
        if self.path.is_some() {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(u(16.))
                    .pt(u(16.))
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .text_px(theme.text.body)
                                    .medium()
                                    .text_color(theme.colors.content)
                                    .child("Background effect"),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_px(theme.text.caption)
                                    .text_color(theme.content(0.45))
                                    .debug_selector(|| "project-effect-description".into())
                                    .child(self.effect.description()),
                            ),
                    )
                    .child(
                        div()
                            .w(u(144.))
                            .flex_none()
                            .child(self.effect_select.clone()),
                    ),
            );
        }
        let scope = self.scope;
        body = body.child(Self::labeled_row(
            "Show on",
            segmented(
                "Show project background on",
                scope.as_str(),
                [("empty", "Empty only"), ("all", "All sessions")],
            )
            .compact(176.)
            .on_change(cx.listener(|this, value: &str, _, cx| {
                this.update_scope(ChatBackgroundScope::parse(Some(value)), cx)
            })),
            cx,
        ));
        let min = js::round(CHAT_BACKGROUND_OPACITY_MIN * 100.0);
        let max = js::round(CHAT_BACKGROUND_OPACITY_MAX * 100.0);
        let empty = js::round(self.empty_opacity * 100.0);
        let session = js::round(self.session_opacity * 100.0);
        body = body
            .child(Self::labeled_row(
                "Empty chat visibility",
                slider(
                    "Project background visibility in empty chats",
                    empty,
                    format!("{empty}%"),
                    min,
                    max,
                )
                .on_change(cx.listener(|this, value: &f64, _, cx| {
                    this.update_opacity(OpacityKind::Empty, *value, cx)
                })),
                cx,
            ))
            .child(Self::labeled_row(
                "Session visibility",
                slider(
                    "Project background visibility in sessions",
                    session,
                    format!("{session}%"),
                    min,
                    max,
                )
                .on_change(cx.listener(|this, value: &f64, _, cx| {
                    this.update_opacity(OpacityKind::Session, *value, cx)
                })),
                cx,
            ));
        if self.path.is_some() {
            let danger = theme.colors.danger;
            let fill = gpui::Hsla { a: 0.1, ..danger };
            let border = gpui::Hsla { a: 0.4, ..danger };
            let mut remove = div()
                .id("project-background-remove")
                .w_full()
                .flex()
                .justify_center()
                .px(u(10.))
                .py(u(6.))
                .rounded(u(theme.radius.md))
                .border_1()
                .border_color(theme.content(0.10))
                .text_px(theme.text.label)
                .text_color(danger)
                .debug_selector(|| "button:project-background-remove".into())
                .child("Remove background image");
            if busy {
                remove = remove.opacity(0.4);
            } else {
                remove = remove
                    .hover(move |s| s.bg(fill).border_color(border))
                    .on_click(cx.listener(|this, _, _, cx| this.remove_image(cx)));
            }
            body = body.child(remove);
        }

        // The modal frame (Modal.tsx, `size="sm"`, `fitViewport`).
        let close_button = icon_button("project-background-close", IconName::X)
            .size(28.)
            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)));
        let header = div()
            .flex()
            .flex_none()
            .items_start()
            .gap(u(8.))
            .px(u(16.))
            .pt(u(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pt(u(2.))
                    .child(
                        div()
                            .text_px(theme.text.title)
                            .medium()
                            .leading(theme.leading.tight)
                            .text_color(theme.colors.content)
                            .child("Background Image"),
                    )
                    .child(
                        div()
                            .mt(u(2.))
                            .truncate()
                            .text_px(theme.text.label)
                            .leading(theme.leading.snug)
                            .text_color(theme.content(0.50))
                            .child(format!("Choose a background image for {}", self.name)),
                    ),
            )
            .child(close_button);
        let panel = div()
            .id("project-background-dialog")
            .relative()
            .flex()
            .flex_col()
            .w(u(420.))
            .max_w_full()
            .max_h_full()
            .rounded(u(theme.radius.xxl))
            .border_1()
            .border_color(theme.colors.modal_border)
            .shadow_2xl()
            .overflow_hidden()
            .track_focus(&self.focus)
            .debug_selector(|| "project-background-dialog".into())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(glass_backdrop(
                theme.radius.xxl,
                24.,
                theme.colors.modal_backdrop,
            ))
            .child(header)
            .child(
                div()
                    .id("project-background-body")
                    .relative()
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .child(body),
            );
        let close = self.on_close.clone();
        deferred(
            div()
                .id("project-background-layer")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(theme.colors.modal_overlay)
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            if let Some(close) = close.clone() {
                                close(window, cx);
                            }
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .p(u(16.))
                        .child(panel),
                ),
        )
        .with_priority(theme.layer.dialog)
    }
}
