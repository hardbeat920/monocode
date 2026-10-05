//! Port of src/features/sessions/ui/ProviderSignInPanel.tsx: the centered
//! "Authentication required" panel with the provider's sign-in button.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use monocode_core::HarnessId;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, provider_logo, u};

use super::style::{error_ink, spin_icon, text};
use crate::settings::providers::harness_logo;

/// `ProviderSignInState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignInState {
    #[default]
    Idle,
    Running,
    Complete,
    Error,
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// `ProviderSignInPanel`. `on_complete` and `complete_label` turn the
/// finished button into an action.
pub fn sign_in_panel(
    harness: HarnessId,
    state: SignInState,
    error: Option<&str>,
    on_sign_in: ClickHandler,
    on_complete: Option<ClickHandler>,
    complete_label: Option<&str>,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let title = harness.title();
    let complete = state == SignInState::Complete;
    let heading = if complete {
        format!("Signed in to {title}")
    } else {
        "Authentication required".into()
    };
    let detail = if complete {
        "You can retry your last message now.".to_string()
    } else {
        format!("Sign in to continue using {title}.")
    };
    let disabled = state == SignInState::Running || (complete && on_complete.is_none());
    let label = match state {
        SignInState::Running => "Waiting for browser…".to_string(),
        SignInState::Complete => complete_label.unwrap_or("Signed in").to_string(),
        _ => format!("Sign in to {title}"),
    };
    let ink = theme.colors.background_base;
    let glyph = match state {
        SignInState::Running => Some(spin_icon("sign-in-spin", IconName::RefreshCw, 14., ink)),
        SignInState::Complete => Some(
            icon(IconName::Check)
                .size(u(14.))
                .text_color(ink)
                .into_any_element(),
        ),
        _ => None,
    };
    let hover = theme.content(0.85);
    let selector = format!("button:{label}");
    let mut button = div()
        .id("provider-sign-in")
        .mt(u(16.))
        .flex()
        .items_center()
        .justify_center()
        .gap(u(6.))
        .h(u(32.))
        .px(u(14.))
        .rounded(u(theme.radius.lg))
        .bg(theme.colors.content)
        .text_px(12.)
        .medium()
        .text_color(ink)
        .debug_selector(move || selector)
        .children(glyph)
        .child(label);
    if disabled {
        button = button.opacity(0.55);
    } else {
        let handler = if complete {
            on_complete.unwrap_or(on_sign_in)
        } else {
            on_sign_in
        };
        button = button
            .hover(move |s| s.bg(hover))
            .on_click(move |event, window, cx| handler(event, window, cx));
    }
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .min_h(u(272.))
        .px(u(20.))
        .py(u(24.))
        .debug_selector(|| "sign-in-panel".into())
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(u(64.))
                .rounded(u(theme.radius.xxl))
                .bg(theme.content(0.06))
                .border_1()
                .border_color(theme.content(0.08))
                .shadow_sm()
                .text_color(theme.colors.content)
                .child(provider_logo(harness_logo(harness)).size(36.)),
        )
        .child(
            text(heading)
                .mt(u(14.))
                .text_px(15.)
                .line_height(u(20.))
                .medium()
                .text_color(theme.colors.content),
        )
        .child(
            text(detail)
                .mt(u(4.))
                .max_w(u(224.))
                .text_px(11.)
                .line_height(u(16.))
                .text_center()
                .text_color(theme.content(0.45)),
        )
        .child(button)
        .when_some(
            error.filter(|_| state == SignInState::Error),
            |el, error| {
                el.child(
                    text(error.to_string())
                        .mt(u(10.))
                        .max_w(u(240.))
                        .text_px(10.)
                        .line_height(u(16.))
                        .text_center()
                        .text_color(error_ink(cx)),
                )
            },
        )
        .into_any_element()
}
