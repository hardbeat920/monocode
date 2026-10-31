use super::*;
use crate::theme::*;
use gpui::{AnyElement, FontWeight, Role, SharedString, div, px};

impl Accounts {
    pub(crate) fn has_modal_auth(&self) -> bool {
        self.signin
            .as_ref()
            .is_some_and(|s| s.surface == Surface::Settings)
    }
    pub(crate) fn has_footer_auth(&self, kind: Kind, id: &str) -> bool {
        self.footer_signin(kind, id).is_some()
    }
    fn footer_signin(&self, kind: Kind, id: &str) -> Option<&SignIn> {
        self.signin.as_ref().filter(|s| {
            s.surface == Surface::Footer
                && s.kind == kind
                && s.profile.id == id
                && s.action == AuthAction::SignIn
                && s.adding.is_none()
        })
    }
    /// ProviderSignInPanel.tsx, embedded in the usage popover.
    pub(crate) fn footer_auth_panel(
        &self,
        kind: Kind,
        id: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let signin = self.footer_signin(kind, id);
        let complete = signin.is_some_and(|s| s.complete);
        let error = signin.and_then(|s| s.error.clone());
        let running = signin.is_some_and(|s| !s.complete && s.error.is_none());
        let auth_running = self
            .signin
            .as_ref()
            .is_some_and(|s| !s.complete && s.error.is_none());
        let disabled = auth_running || complete || self.working;
        let label: SharedString = if running {
            "Waiting for browser…".into()
        } else if complete {
            "Signed in".into()
        } else {
            format!("Sign in to {}", kind.label()).into()
        };
        let click_id = id.to_owned();
        let keyboard_id = id.to_owned();
        let mut button_text = shell_fill();
        button_text.a = 1.;
        div()
            .id("footer-provider-signin-panel")
            .role(Role::Status)
            .aria_label(if complete {
                "Signed in"
            } else {
                "Authentication required"
            })
            .min_h(px(272.))
            .px(px(20.))
            .py(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .text_color(content(1.))
            .child(
                div()
                    .size(px(64.))
                    .flex_shrink_0()
                    .rounded(px(16.))
                    .bg(content(0.06))
                    .border_1()
                    .border_color(content(0.08))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(provider_icon(kind.id(), 36.)),
            )
            .child(
                div()
                    .mt(px(14.))
                    .text_size(px(15.))
                    .line_height(px(20.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_center()
                    .child(if complete {
                        format!("Signed in to {}", kind.label())
                    } else {
                        "Authentication required".into()
                    }),
            )
            .child(
                div()
                    .mt_1()
                    .max_w(px(224.))
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(content(0.45))
                    .text_center()
                    .child(if complete {
                        "You can retry your last message now.".into()
                    } else {
                        format!("Sign in to continue using {}.", kind.label())
                    }),
            )
            .child(
                div()
                    .id("footer-provider-auth-action")
                    .role(Role::Button)
                    .aria_label(label.clone())
                    .track_focus(&self.dialog_focus)
                    .tab_index(0)
                    .mt_4()
                    .h(px(32.))
                    .px(px(14.))
                    .rounded_lg()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .bg(content(1.))
                    .text_color(button_text)
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::MEDIUM)
                    .whitespace_nowrap()
                    .when(disabled, |d| d.opacity(0.55))
                    .when(!disabled, |d| {
                        d.cursor_pointer().hover(|s| s.bg(content(0.85)))
                    })
                    .when(running, |d| {
                        d.child(crate::pulse::looping(
                            "footer-provider-auth-spinner",
                            std::time::Duration::from_secs(1),
                            crate::pulse::Rate::Smooth,
                            move |delta| {
                                icon("refresh-cw", 14.)
                                    .text_color(button_text)
                                    .with_transformation(gpui::Transformation::rotate(
                                        gpui::percentage(delta),
                                    ))
                            },
                        ))
                    })
                    .when(complete, |d| {
                        d.child(icon("check", 14.).text_color(button_text))
                    })
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !disabled {
                            this.begin_footer_auth(kind, &click_id, cx);
                        }
                    }))
                    .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                        if !disabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.begin_footer_auth(kind, &keyboard_id, cx);
                            cx.stop_propagation();
                        }
                    })),
            )
            .children(error.map(|error| {
                div()
                    .id("footer-provider-signin-error")
                    .role(Role::Alert)
                    .mt(px(10.))
                    .max_w(px(240.))
                    .text_size(px(10.))
                    .line_height(px(16.))
                    .text_color(gpui::rgb(0xf87171))
                    .text_center()
                    .child(error)
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn settings_add_signin_stays_inline_after_completion(cx: &mut gpui::TestAppContext) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let view =
            cx.new(|cx| Accounts::new(PathBuf::from("/fixture"), Book::default(), store, cx));
        view.update(cx, |accounts, cx| {
            accounts.signin = Some(SignIn {
                kind: Kind::Codex,
                profile: Profile::default(),
                action: AuthAction::SignIn,
                surface: Surface::InlineSettings,
                cancel: Arc::new(AtomicBool::new(false)),
                complete: false,
                error: None,
                adding: None,
            });
            assert!(!accounts.has_modal_auth());
            accounts.signin.as_mut().unwrap().complete = true;
            assert!(!accounts.has_modal_auth());
            assert!(!accounts.has_footer_auth(Kind::Codex, accounts::DEFAULT));
            let _ = cx;
        });
    }
    #[gpui::test]
    fn footer_signin_never_opens_a_modal_or_steals_window_focus(cx: &mut gpui::TestAppContext) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| Accounts::new(PathBuf::from("/fixture"), Book::default(), store, cx))
            })
            .unwrap()
        });
        let previous = window
            .update(cx, |accounts, window, cx| {
                let previous = cx.focus_handle();
                window.focus(&previous, cx);
                accounts.signin = Some(SignIn {
                    kind: Kind::Codex,
                    profile: Profile::default(),
                    action: AuthAction::SignIn,
                    surface: Surface::Footer,
                    cancel: Arc::new(AtomicBool::new(false)),
                    complete: false,
                    error: None,
                    adding: None,
                });
                cx.notify();
                previous
            })
            .unwrap();
        for (complete, error) in [(false, None), (false, Some("Retry sign-in")), (true, None)] {
            window
                .update(cx, |accounts, _, cx| {
                    let signin = accounts.signin.as_mut().unwrap();
                    signin.complete = complete;
                    signin.error = error.map(str::to_owned);
                    cx.notify();
                })
                .unwrap();
            cx.refresh().unwrap();
            window
                .update(cx, |accounts, window, cx| {
                    assert!(previous.is_focused(window));
                    assert!(!accounts.dialog_presented);
                    assert!(!accounts.has_modal_auth());
                    assert!(accounts.has_footer_auth(Kind::Codex, accounts::DEFAULT));
                    assert!(!accounts.has_footer_auth(Kind::Claude, accounts::DEFAULT));
                    assert!(!accounts.has_footer_auth(Kind::Codex, "another-account"));
                    let _ = accounts.footer_auth_panel(Kind::Codex, accounts::DEFAULT, cx);
                })
                .unwrap();
        }
        // The existing Settings presentation still owns its dialog focus.
        window
            .update(cx, |accounts, _, cx| {
                accounts.signin.as_mut().unwrap().surface = Surface::Settings;
                cx.notify();
            })
            .unwrap();
        cx.refresh().unwrap();
        window
            .update(cx, |accounts, window, _| {
                assert!(accounts.dialog_presented);
                assert!(accounts.dialog_focus.is_focused(window));
            })
            .unwrap();
    }
}
