//! Ports of src/features/sessions/ui/ProviderSignInPanel.tsx and
//! ProviderSignInDialog.tsx: the prompt to sign in to a provider whose CLI
//! asked for authentication.
//!
//! `loginHarness` runs in the engine; the dialog takes it as a [`Login`]
//! function and shows its progress.

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Context, EventEmitter, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, Transformation, Window, div, percentage,
};
use monocode_core::HarnessId;
use monocode_ui::widgets::{ModalSize, modal};
use monocode_ui::{IconName, ProviderLogo, Theme, UiStyled as _, icon, provider_logo, u};

/// `ProviderSignInState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderSignInState {
    #[default]
    Idle,
    Running,
    Complete,
    Error,
}

/// Runs a provider's login and resolves when it finishes.
pub type Login = Rc<dyn Fn(HarnessId, &mut App) -> Task<Result<(), String>>>;

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// The panel's title line.
pub fn sign_in_title(harness: HarnessId, state: ProviderSignInState) -> String {
    if state == ProviderSignInState::Complete {
        format!("Signed in to {}", harness.title())
    } else {
        "Authentication required".into()
    }
}

/// The panel's button label.
pub fn sign_in_button_label(
    harness: HarnessId,
    state: ProviderSignInState,
    complete_label: Option<&str>,
) -> String {
    match state {
        ProviderSignInState::Running => "Waiting for browser…".into(),
        ProviderSignInState::Complete => complete_label.unwrap_or("Signed in").into(),
        _ => format!("Sign in to {}", harness.title()),
    }
}

/// `ProviderSignInPanel`.
#[derive(IntoElement)]
pub struct ProviderSignInPanel {
    harness: HarnessId,
    state: ProviderSignInState,
    error: Option<SharedString>,
    on_sign_in: Option<Handler>,
    on_complete: Option<Handler>,
    complete_action_label: Option<SharedString>,
}

pub fn provider_sign_in_panel(
    harness: HarnessId,
    state: ProviderSignInState,
) -> ProviderSignInPanel {
    ProviderSignInPanel {
        harness,
        state,
        error: None,
        on_sign_in: None,
        on_complete: None,
        complete_action_label: None,
    }
}

impl ProviderSignInPanel {
    pub fn error(mut self, error: Option<impl Into<SharedString>>) -> Self {
        self.error = error.map(Into::into);
        self
    }

    pub fn on_sign_in(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_sign_in = Some(Rc::new(handler));
        self
    }

    pub fn on_complete(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_complete = Some(Rc::new(handler));
        self
    }

    pub fn complete_action_label(mut self, label: impl Into<SharedString>) -> Self {
        self.complete_action_label = Some(label.into());
        self
    }
}

impl gpui::RenderOnce for ProviderSignInPanel {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let title = self.harness.title();
        let complete = self.state == ProviderSignInState::Complete;
        let disabled =
            self.state == ProviderSignInState::Running || (complete && self.on_complete.is_none());
        let label = sign_in_button_label(
            self.harness,
            self.state,
            self.complete_action_label.as_deref(),
        );
        let glyph: AnyElement = match ProviderLogo::from_id(self.harness.as_str()) {
            Some(logo) => provider_logo(logo).size(36.).into_any_element(),
            None => icon(IconName::Bot)
                .size(u(36.))
                .text_color(theme.colors.content)
                .into_any_element(),
        };
        let leading: Option<AnyElement> = match self.state {
            ProviderSignInState::Running => Some(
                icon(IconName::RefreshCw)
                    .size(u(14.))
                    .text_color(theme.colors.background_base)
                    .with_animation(
                        "sign-in-spin",
                        Animation::new(std::time::Duration::from_secs(1)).repeat(),
                        |svg, t| svg.with_transformation(Transformation::rotate(percentage(t))),
                    )
                    .into_any_element(),
            ),
            ProviderSignInState::Complete => Some(
                icon(IconName::Check)
                    .size(u(14.))
                    .text_color(theme.colors.background_base)
                    .into_any_element(),
            ),
            _ => None,
        };
        let action = if complete {
            self.on_complete.clone()
        } else {
            None
        }
        .or(self.on_sign_in.clone());
        let hover = theme.content(0.85);
        let mut button = div()
            .id("provider-sign-in-button")
            .debug_selector(|| "provider-sign-in-button".into())
            .mt(u(16.))
            .flex()
            .h(u(32.))
            .items_center()
            .justify_center()
            .gap(u(6.))
            .rounded(u(theme.radius.lg))
            .bg(theme.colors.content)
            .px(u(14.))
            .text_px(12.)
            .medium()
            .text_color(theme.colors.background_base)
            .when_some(leading, |button, leading| button.child(leading))
            .child(label);
        if disabled {
            button = button.opacity(0.55);
        } else {
            button = button.hover(move |s| s.bg(hover));
            if let Some(action) = action {
                button = button.on_click(move |_, window, cx| action(window, cx));
            }
        }
        let mut panel = div()
            .debug_selector(|| "provider-sign-in".into())
            .flex()
            .flex_col()
            .min_h(u(272.))
            .items_center()
            .justify_center()
            .px(u(20.))
            .py(u(24.))
            .child(
                div()
                    .flex()
                    .size(u(64.))
                    .items_center()
                    .justify_center()
                    .rounded(u(16.))
                    .bg(theme.content(0.06))
                    .border_1()
                    .border_color(theme.content(0.08))
                    .shadow_sm()
                    .child(glyph),
            )
            .child(
                div()
                    .mt(u(14.))
                    .text_px(15.)
                    .medium()
                    .line_height(u(20.))
                    .text_color(theme.colors.content)
                    .child(sign_in_title(self.harness, self.state)),
            )
            .child(
                div()
                    .mt(u(4.))
                    .max_w(u(224.))
                    .text_center()
                    .text_px(11.)
                    .line_height(u(16.))
                    .text_color(theme.content(0.45))
                    .child(if complete {
                        "You can retry your last message now.".to_string()
                    } else {
                        format!("Sign in to continue using {title}.")
                    }),
            )
            .child(button);
        if let (ProviderSignInState::Error, Some(error)) = (self.state, self.error) {
            panel = panel.child(
                div()
                    .debug_selector(|| "provider-sign-in-error".into())
                    .mt(u(10.))
                    .max_w(u(240.))
                    .text_center()
                    .text_px(10.)
                    .line_height(u(16.))
                    .text_color(monocode_ui::color::hex(0xfb2c36))
                    .child(error),
            );
        }
        panel
    }
}

/// What the dialog reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSignInEvent {
    /// `onClose`: the close button, the backdrop, or Continue.
    Close,
}

/// `ProviderSignInDialog`.
pub struct ProviderSignInDialog {
    harness: HarnessId,
    state: ProviderSignInState,
    error: Option<String>,
    login: Login,
    task: Option<Task<()>>,
    animate: bool,
}

impl EventEmitter<ProviderSignInEvent> for ProviderSignInDialog {}

impl ProviderSignInDialog {
    pub fn new(harness: HarnessId, login: Login) -> Self {
        Self {
            harness,
            state: ProviderSignInState::Idle,
            error: None,
            login,
            task: None,
            animate: true,
        }
    }

    /// A new provider resets the dialog.
    pub fn set_harness(&mut self, harness: HarnessId, cx: &mut Context<Self>) {
        if self.harness != harness {
            self.harness = harness;
            self.state = ProviderSignInState::Idle;
            self.error = None;
            self.task = None;
            cx.notify();
        }
    }

    pub fn set_animate(&mut self, animate: bool) {
        self.animate = animate;
    }

    pub fn state(&self) -> ProviderSignInState {
        self.state
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// `signIn`.
    pub fn sign_in(&mut self, cx: &mut Context<Self>) {
        self.state = ProviderSignInState::Running;
        self.error = None;
        let harness = self.harness;
        let login = (self.login)(harness, cx);
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = login.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.state = ProviderSignInState::Complete,
                    Err(reason) => {
                        this.state = ProviderSignInState::Error;
                        this.error = Some(if reason.is_empty() {
                            format!("Could not sign in to {}.", harness.title())
                        } else {
                            reason
                        });
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
}

impl Render for ProviderSignInDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let close = this.clone();
        let complete = this.clone();
        modal("provider-sign-in", "Authentication required")
            .description(format!(
                "Sign in to continue using {}.",
                self.harness.title()
            ))
            .size(ModalSize::Sm)
            .minimal_header()
            .animate(self.animate)
            .on_close(move |_, cx| {
                close
                    .update(cx, |_, cx| cx.emit(ProviderSignInEvent::Close))
                    .ok();
            })
            .child(
                provider_sign_in_panel(self.harness, self.state)
                    .error(self.error.clone())
                    .complete_action_label("Continue")
                    .on_sign_in(move |_, cx| {
                        this.update(cx, |this, cx| this.sign_in(cx)).ok();
                    })
                    .on_complete(move |_, cx| {
                        complete
                            .update(cx, |_, cx| cx.emit(ProviderSignInEvent::Close))
                            .ok();
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    //! Port of ProviderSignInDialog.test.ts.

    use std::cell::RefCell;

    use gpui::{Modifiers, TestAppContext};

    use super::*;
    use crate::panes::test_support::{draw, init};

    #[gpui::test]
    fn launches_provider_login_and_offers_a_clear_continuation_state(cx: &mut TestAppContext) {
        cx.update(init);
        let calls: Rc<RefCell<Vec<HarnessId>>> = Rc::default();
        let recorded = calls.clone();
        let login: Login = Rc::new(move |harness, _| {
            recorded.borrow_mut().push(harness);
            Task::ready(Ok(()))
        });
        let (dialog, cx) = cx.add_window_view(move |_, _| {
            let mut dialog = ProviderSignInDialog::new(HarnessId::Grok, login);
            dialog.set_animate(false);
            dialog
        });
        let closes: Rc<RefCell<usize>> = Rc::default();
        let sink = closes.clone();
        cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, _: &ProviderSignInEvent, _| {
                *sink.borrow_mut() += 1
            })
            .detach();
        });
        draw(cx);
        assert!(cx.debug_bounds("provider-sign-in").is_some());
        assert_eq!(
            sign_in_title(HarnessId::Grok, ProviderSignInState::Idle),
            "Authentication required"
        );
        assert_eq!(
            sign_in_button_label(HarnessId::Grok, ProviderSignInState::Idle, Some("Continue")),
            "Sign in to Grok Build"
        );

        let button = cx.debug_bounds("provider-sign-in-button").unwrap().center();
        cx.simulate_click(button, Modifiers::none());
        draw(cx);
        assert_eq!(calls.borrow().as_slice(), [HarnessId::Grok]);
        assert_eq!(
            dialog.read_with(cx, |dialog, _| dialog.state()),
            ProviderSignInState::Complete
        );
        assert_eq!(
            sign_in_title(HarnessId::Grok, ProviderSignInState::Complete),
            "Signed in to Grok Build"
        );

        let button = cx.debug_bounds("provider-sign-in-button").unwrap().center();
        cx.simulate_click(button, Modifiers::none());
        draw(cx);
        assert_eq!(*closes.borrow(), 1);
    }

    #[gpui::test]
    fn shows_why_a_login_failed(cx: &mut TestAppContext) {
        cx.update(init);
        let login: Login = Rc::new(|_, _| Task::ready(Err(String::new())));
        let (dialog, cx) =
            cx.add_window_view(move |_, _| ProviderSignInDialog::new(HarnessId::Claude, login));
        dialog.update(cx, |dialog, cx| dialog.sign_in(cx));
        draw(cx);
        assert_eq!(
            dialog.read_with(cx, |dialog, _| dialog.state()),
            ProviderSignInState::Error
        );
        assert_eq!(
            dialog.read_with(cx, |dialog, _| dialog.error().map(str::to_string)),
            Some("Could not sign in to Claude Code.".into())
        );
        assert!(cx.debug_bounds("provider-sign-in-error").is_some());
    }
}
