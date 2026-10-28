use super::*;
use gpui::App;
use monocode_core::{
    accounts::Profile,
    usage::{self, Limit},
};
pub(crate) struct UsageResumed;
impl EventEmitter<UsageResumed> for Conversation {}
impl Conversation {
    /// Update notifications refresh the account-specific picker after a CLI update.
    pub(crate) fn refresh_models_after_harness_update(&mut self, cx: &mut Context<Self>) {
        monocode_core::catalog::invalidate(
            Kind::from_id(&self.summary.provider),
            &self.settings.account,
        );
        self.refresh_catalog(cx);
        cx.notify();
    }

    pub(super) fn auth_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let kind = Kind::from_id(&self.summary.provider);
        let action = monocode_core::accounts::AuthAction::SignIn;
        if self.busy || monocode_core::accounts::login_arguments(kind, action).is_none() {
            return None;
        }
        let status = self.status.to_ascii_lowercase();
        if ![
            "not signed in",
            "not authenticated",
            "authentication required",
            "please log in",
            "please login",
            "no longer signed in",
        ]
        .iter()
        .any(|s| status.contains(s))
        {
            return None;
        }
        let id = self.settings.account.id.clone();
        Some(
            div()
                .mx_2()
                .mb_2()
                .p_3()
                .rounded_lg()
                .bg(content(0.04))
                .flex()
                .items_center()
                .gap_2()
                .child(provider_icon(kind.id(), 20.))
                .child(
                    div()
                        .flex_1()
                        .child(format!("Sign in to {} to continue", kind.label())),
                )
                .child(
                    crate::settings::widgets::secondary_button(
                        "conversation-sign-in",
                        "Sign in",
                        true,
                        false,
                    )
                    .child("Sign in")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if let Some(center) = crate::accounts::get(cx) {
                            center.update(cx, |center, cx| {
                                center.begin_auth(kind, &id, action, None, cx)
                            });
                        }
                    })),
                )
                .into_any_element(),
        )
    }
    pub(crate) fn attach_account(&mut self, cx: &App) {
        if self.is_remote() {
            self.settings.account = Default::default();
            return;
        }
        if self.settings.account.id == monocode_core::accounts::DEFAULT {
            self.settings.account.root = None;
            return;
        }
        if let Some(center) = crate::accounts::get(cx) {
            self.settings.account.root = Some(center.read(cx).root.clone());
        }
    }
    pub(crate) fn account_model(&self) -> Option<&str> {
        self.model.as_deref().or(self.resolved_model.as_deref())
    }
    pub(crate) fn set_blank_account(&mut self, profile: Profile, cx: &mut Context<Self>) {
        if self.busy || self.summary.has_messages {
            return;
        }
        self.settings.account = profile;
        self.catalog.clear();
        self.resolved_model = None;
        self.refresh_catalog(cx);
        self.save_settings(cx);
        cx.emit(SessionChanged);
        cx.notify();
    }
    pub(super) fn detect_usage_limit(&mut self, cx: &mut Context<Self>) {
        if let Some(limit) = self
            .settings
            .usage_limit
            .clone()
            .or_else(|| Limit::from_error(&self.status))
        {
            if let Some(pending) = &mut self.settings.pending_provider_switch {
                pending.skip_outgoing_recap = true;
            }
            self.settings.usage_limit = Some(limit);
            self.save_settings(cx);
            self.follow_ups.state.paused = true;
            self.save_follow_ups(cx);
            let kind = Kind::from_id(&self.summary.provider);
            if let Some(center) = crate::accounts::get(cx) {
                let key = crate::accounts::Key::new(
                    kind,
                    self.settings.account.clone(),
                    self.account_model(),
                );
                center.update(cx, |center, cx| center.refresh(key, true, cx));
            }
            self.tick_usage(cx);
        }
    }
    /// Account recovery also works for saved conversations from before native
    /// limit metadata was recorded, and after dismissing the banner. Inspect
    /// only the final error/notice: an older limit must not affect a later turn.
    pub(crate) fn account_recovery_limit(&self, cx: &App) -> Option<Limit> {
        self.settings
            .usage_limit
            .clone()
            .or_else(|| Limit::from_error(&self.status))
            .or_else(|| {
                let rows = self.transcript.read(cx).source_rows();
                let last = rows.last()?;
                if !matches!(last.role.as_str(), "error" | "system") {
                    return None;
                }
                let start = rows.iter().rposition(|row| row.first).unwrap_or(0);
                let text: String = rows[start..].iter().map(|row| row.text.as_str()).collect();
                Limit::from_error(&text)
            })
    }
    pub(super) fn tick_usage(&mut self, cx: &mut Context<Self>) {
        let Some(limit) = self.settings.usage_limit.clone() else {
            return;
        };
        if limit.resets_at.is_none() {
            let kind = Kind::from_id(&self.summary.provider);
            let key = crate::accounts::Key::new(
                kind,
                self.settings.account.clone(),
                self.account_model(),
            );
            if let Some(center) = crate::accounts::get(cx) {
                center.update(cx, |center, cx| center.refresh(key.clone(), false, cx));
            }
            let reset = crate::accounts::get(cx).and_then(|center| {
                center
                    .read(cx)
                    .usage
                    .get(&key)
                    .and_then(|state| state.snapshot.as_ref())
                    .and_then(|s| s.exhausted_reset())
            });
            if let Some(at) = reset {
                self.settings.usage_limit.as_mut().unwrap().resets_at = Some(at);
                self.save_settings(cx);
            }
        }
        if self
            .settings
            .usage_limit
            .as_ref()
            .is_some_and(|l| l.resume_due(usage::now_ms(), self.busy))
        {
            self.resume_usage(cx);
        }
        cx.notify();
    }
    pub(crate) fn resume_usage(&mut self, cx: &mut Context<Self>) {
        if self.needs_history_restore() {
            self.defer_history_action(|this, cx| this.resume_usage(cx), cx);
            return;
        }
        if self.busy
            || self.stopping
            || self.account_switching
            || self.settings.usage_limit.take().is_none()
        {
            return;
        }
        self.status = "Ready".into();
        self.save_settings(cx);
        if self.settings.mono.is_some() {
            cx.emit(UsageResumed);
            cx.emit(SessionChanged);
            cx.notify();
            return;
        }
        self.launch_prompt("Continue from where you left off.".into(), cx);
        cx.emit(SessionChanged);
        cx.notify();
    }
    pub(super) fn recover_usage_after_model_change(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.stopping || self.settings.usage_limit.take().is_none() {
            return;
        }
        self.status = "Ready".into();
        self.follow_ups.state.paused = false;
        for message in &mut self.follow_ups.state.messages {
            message.error = None;
        }
        self.save_settings(cx);
        self.save_follow_ups(cx);
        if self.settings.mono.is_some() {
            cx.emit(UsageResumed);
        }
        cx.emit(SessionChanged);
        cx.notify();
    }
    /// Moves this conversation to another account in place. A limited
    /// session resumes on the new account; an idle one just waits for the
    /// next message there.
    pub(crate) fn switch_account(&mut self, profile: Profile, cx: &mut Context<Self>) {
        if self.editing_last_turn.pending() {
            return;
        }
        if self.needs_history_restore() {
            self.defer_history_action(move |this, cx| this.switch_account(profile, cx), cx);
            return;
        }
        if self.busy
            || self.stopping
            || self.account_switching
            || self.settings.account.id == profile.id
        {
            return;
        }
        let limit = self.account_recovery_limit(cx);
        let limited = limit.is_some();
        if limited {
            self.settings.usage_limit = limit;
        }
        self.attach_account(cx);
        let from = self.settings.account.clone();
        let thread = self.summary.provider_id.clone();
        let expected = thread.clone();
        let kind = Kind::from_id(&self.summary.provider);
        let can_transfer = !(kind == Kind::Codex && self.settings.mono.is_some());
        let to = profile.clone();
        self.account_switching = true;
        let work = cx.background_executor().spawn(async move {
            thread
                .as_ref()
                .is_some_and(|id| can_transfer && from.transfer(&to, kind, id).is_ok())
        });
        let old = self.settings.account.id.clone();
        cx.spawn(async move |this, cx| {
            let carried = work.await;
            let _ = this.update(cx, |this, cx| {
                this.account_switching = false;
                if this.busy
                    || (limited && this.settings.usage_limit.is_none())
                    || this.settings.account.id != old
                    || this.summary.provider_id != expected
                    || crate::accounts::get(cx)
                        .is_some_and(|center| !center.read(cx).book.exists(kind, &profile.id))
                {
                    return;
                }
                if !carried {
                    if this.settings.pending_provider_switch.is_none() {
                        this.settings.pending_provider_switch =
                            Some(monocode_core::agent::ProviderSwitch {
                                from: this.summary.provider.clone(),
                                model_label: this.model_label().to_string(),
                                cwd: this.execution_cwd().to_owned(),
                                model: this.model.clone(),
                                model_settings: this.settings.model_settings.clone(),
                                effort: this.settings.effort.clone(),
                                fast: this.settings.fast,
                                service_tier: this.settings.service_tier.clone(),
                                thinking: this.settings.thinking,
                                skip_outgoing_recap: limited,
                                account: this.settings.account.clone(),
                                provider_id: this.summary.provider_id.clone(),
                            });
                    }
                    this.summary.provider_id = None;
                    let store = this.store.clone();
                    let id = this.summary.id;
                    let expected = expected.clone();
                    cx.background_executor()
                        .spawn(async move {
                            let _ = locked(&store)
                                .and_then(|s| s.reset_provider_id_if(id, expected.as_deref()));
                        })
                        .detach();
                    this.settings.handoff = Some(monocode_core::agent::Handoff {
                        from: this.summary.provider.clone(),
                        brief: monocode_core::response::handoff(
                            this.transcript.read(cx).source_rows(),
                        ),
                    });
                } else if let Some(id) = &this.summary.provider_id {
                    monocode_core::provider::forget_session(
                        kind,
                        id,
                        &old,
                        Path::new(this.execution_cwd()),
                    );
                }
                this.settings.account = profile;
                this.catalog.clear();
                this.refresh_catalog(cx);
                this.save_settings(cx);
                if limited {
                    this.resume_usage(cx);
                } else {
                    cx.emit(SessionChanged);
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn account_unavailable(&mut self, cx: &mut Context<Self>) {
        self.stop_if_busy(cx);
        self.follow_ups.state.paused = true;
        self.follow_ups.requested = None;
        self.save_follow_ups(cx);
        self.catalog_epoch += 1;
        self.catalog.clear();
        self.status =
            "This provider account is no longer signed in. Choose another account to continue."
                .into();
        cx.notify();
    }
    pub(crate) fn account_authenticated(&mut self, cx: &mut Context<Self>) {
        self.attach_account(cx);
        self.refresh_catalog(cx);
        cx.notify();
    }
    pub(super) fn usage_notice(&self, mono: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let limit = self.settings.usage_limit.as_ref()?;
        let now = usage::now_ms();
        let waiting = limit.resets_at.is_some_and(|at| at > now);
        let armed = limit.resume_at_reset;
        let switching = self.account_switching;
        let kind = Kind::from_id(&self.summary.provider);
        let reset = limit.resets_at.map(|at| {
            if at <= now {
                return "Limit has reset".to_owned();
            }
            let when = chrono::DateTime::from_timestamp_millis(at)
                .map(|d| {
                    let local = d.with_timezone(&chrono::Local);
                    let today = chrono::Local::now().date_naive() == local.date_naive();
                    local
                        .format(if today {
                            "%-I:%M %p"
                        } else {
                            "%b %-d, %-I:%M %p"
                        })
                        .to_string()
                })
                .unwrap_or_default();
            format!(
                "Resets {when} · in {}",
                usage::Window {
                    used_percent: 100.,
                    minutes: 300,
                    resets_at: Some(at)
                }
                .countdown(now)
            )
        });
        let mut controls = div()
            .flex()
            .when(!mono, |d| d.flex_shrink_0())
            .items_center()
            .gap(px(if mono { 12. } else { 8. }))
            .when(mono, |d| d.flex_wrap().gap_y(px(6.)))
            .when(mono, |d| {
                d.child(
                    usage_button(
                        "usage-model-picker",
                        "Choose another model",
                        false,
                        switching,
                    )
                    .h(px(26.))
                    .gap(px(4.))
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(content(1.))
                    .bg(content(0.1))
                    .hover(|s| s.bg(content(0.15)))
                    .child("Choose another model")
                    .child(icon("chevron-down", 11.).text_color(content(0.5)))
                    .when(!switching, |d| {
                        d.on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_menu(ComposerMenu::Model, window, cx)
                        }))
                    }),
                )
            });
        if monocode_core::accounts::supports_profiles(kind)
            && crate::accounts::get(cx).is_some_and(|center| {
                center
                    .read(cx)
                    .book
                    .list(kind)
                    .iter()
                    .any(|a| a.id != self.settings.account.id)
            })
        {
            controls = controls.child(
                usage_button(
                    "usage-account-picker",
                    "Choose another account",
                    false,
                    switching,
                )
                .relative()
                .gap(px(4.))
                .when(mono, |d| {
                    d.h(px(26.))
                        .text_size(px(11.))
                        .text_color(content(1.))
                        .bg(content(0.1))
                        .hover(|s| s.bg(content(0.15)))
                })
                .child(self.account_anchor.probe())
                .child(if switching {
                    "Switching…"
                } else if mono {
                    "Choose another account"
                } else {
                    "Switch account"
                })
                .child(icon("chevron-down", 12.).text_color(content(0.5)))
                .when(!switching, |d| {
                    d.on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_menu(ComposerMenu::Account, window, cx)
                    }))
                }),
            );
        }
        let resume = if waiting {
            if armed {
                "Resuming at reset"
            } else {
                "Resume at reset"
            }
        } else {
            "Resume"
        };
        controls = controls
            .child(
                usage_button(
                    "resume-usage-limit",
                    if waiting && armed {
                        "Cancel the automatic resume"
                    } else {
                        resume
                    },
                    waiting && armed,
                    switching,
                )
                .when(mono && !(waiting && armed), |d| d.text_color(content(0.6)))
                .child(
                    icon(if waiting { "clock" } else { "play" }, 14.)
                        .text_color(if waiting && armed {
                            gpui::rgb(AMBER_400).into()
                        } else {
                            content(if mono { 0.6 } else { 0.55 })
                        })
                        .group_hover("resume-usage-limit", |s| s.text_color(content(1.))),
                )
                .child(resume)
                .when(!switching, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        if waiting {
                            if let Some(limit) = &mut this.settings.usage_limit {
                                limit.resume_at_reset = !limit.resume_at_reset;
                                this.save_settings(cx);
                                cx.notify();
                            }
                        } else {
                            this.resume_usage(cx);
                        }
                    }))
                }),
            )
            .when(!mono, |controls| {
                controls.child(
                    usage_button(
                        "dismiss-usage-limit",
                        "Dismiss usage limit notice",
                        false,
                        false,
                    )
                    .w(px(24.))
                    .px_0()
                    .child(
                        icon("close", 14.)
                            .text_color(content(0.55))
                            .group_hover("dismiss-usage-limit", |s| s.text_color(content(1.))),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.usage_limit = None;
                        this.save_settings(cx);
                        cx.notify();
                    })),
                )
            });
        let heading = div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(8.))
            .whitespace_nowrap()
            .child(icon("gauge", 14.).text_color(gpui::rgb(AMBER_400)))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(content(0.85))
                    .child(if mono {
                        format!("{} usage limit reached", kind.label())
                    } else {
                        "Usage limit reached".into()
                    }),
            );
        let notice = div()
            .id("usage-limit-notice")
            .debug_selector(|| "usage-limit-notice".into())
            .role(Role::Status)
            .aria_label("Usage limit reached")
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(content(if mono { 0.6 } else { 0.55 }));
        Some(if mono {
            notice.mx(px(6.)).mb(px(8.)).rounded(px(8.)).border_1().border_color(tint(AMBER_400,0.2)).bg(tint(AMBER_400,0.06))
                .px(px(12.)).py(px(10.)).child(heading)
                .child(div().pl(px(22.))
                    .child(div().mt(px(4.)).text_size(px(12.)).line_height(px(19.5)).text_color(content(0.6))
                        .child("Your conversation is saved. Choose another model or account to keep going."))
                    .children(reset.map(|label| div().mt(px(4.)).text_color(content(0.45)).child(label)))
                    .child(controls.mt(px(8.))))
                .into_any_element()
        } else {
            notice
                .px(px(8.))
                .child(
                    div()
                        .id("usage-limit-banner")
                        .debug_selector(|| "usage-limit-banner".into())
                        .h(px(32.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded_t(px(10.))
                        .border_1()
                        .border_b_0()
                        .border_color(tint(AMBER_400, 0.25))
                        .bg(tint(AMBER_400, 0.1))
                        .px(px(8.))
                        .child(heading)
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .child(reset.unwrap_or_default()),
                        )
                        .child(controls),
                )
                .into_any_element()
        })
    }
}

fn usage_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
    disabled: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .group(id)
        .role(Role::Button)
        .aria_label(label)
        .h(px(24.))
        .px(px(6.))
        .rounded_md()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .text_size(px(12.))
        .line_height(px(16.))
        .whitespace_nowrap()
        .text_color(if primary {
            gpui::rgb(AMBER_400).into()
        } else {
            content(0.55)
        })
        .when(disabled, |d| d.opacity(0.45))
        .when(!disabled, |d| {
            d.cursor_pointer()
                .hover(move |s| s.bg(content(0.1)).text_color(content(1.)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn account_recovery_uses_the_terminal_limit_without_reviving_an_older_failure(
        cx: &mut gpui::TestAppContext,
    ) {
        use monocode_core::transcript::Row;
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let summary = locked(&store)
            .unwrap()
            .create("claude", "/fixture")
            .unwrap();
        let view = cx.new(|cx| Conversation::new(summary, store, cx));
        view.update(cx, |view, cx| {
            let row = |role: &str, text: &str| Row {
                role: role.into(),
                text: text.into(),
                first: true,
                sent_at: None,
            };
            for role in ["error", "system"] {
                view.transcript.update(cx, |v, cx| {
                    v.patch(
                        0,
                        vec![row("user", "Keep going"), row(role, "rate_limit")],
                        cx,
                    );
                });
                assert!(view.account_recovery_limit(cx).is_some());
            }
            view.transcript.update(cx, |v, cx| {
                v.patch(
                    2,
                    vec![row("user", "Try again"), row("assistant", "Completed")],
                    cx,
                );
            });
            assert!(view.account_recovery_limit(cx).is_none());
            view.transcript.update(cx, |v, cx| {
                v.patch(4, vec![row("system", "Network timeout")], cx);
            });
            assert!(view.account_recovery_limit(cx).is_none());
        });
    }

    #[gpui::test]
    fn claude_limit_code_opens_recovery_and_preserves_the_reported_reset(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let summary = locked(&store)
            .unwrap()
            .create("claude", "/fixture")
            .unwrap();
        let view = cx.new(|cx| Conversation::new(summary, store, cx));
        view.update(cx, |view, cx| {
            view.status = "rate_limit".into();
            view.detect_usage_limit(cx);
            assert!(view.settings.usage_limit.is_some());
            assert!(view.follow_ups.state.paused);
            let reported = Limit {
                resets_at: Some(usage::now_ms() + 3_600_000),
                resume_at_reset: false,
            };
            view.settings.usage_limit = Some(reported.clone());
            view.detect_usage_limit(cx);
            assert_eq!(view.settings.usage_limit, Some(reported));
        });
    }

    #[gpui::test]
    fn usage_banner_matches_the_composer_and_can_arm_cancel_and_dismiss(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let mut summary = locked(&store)
            .unwrap()
            .create("claude", "/fixture")
            .unwrap();
        summary.has_messages = true;
        let id = summary.id;
        let saved = store.clone();
        let window = cx.update(|cx| {
            crate::accounts::install(store.clone(), cx);
            crate::accounts::get(cx).unwrap().update(cx, |center, _| {
                center.book.add(Kind::Claude, "Other account").unwrap();
            });
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, store, cx);
                    view.loaded = true;
                    view.status = "Claude Code reached its usage limit.".into();
                    view.settings.usage_limit = Some(Limit {
                        resets_at: Some(usage::now_ms() + 3_600_000),
                        resume_at_reset: false,
                    });
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        let banner = visual.debug_bounds("usage-limit-banner").unwrap();
        let composer = visual.debug_bounds("composer-box").unwrap();
        let resume = visual.debug_bounds("resume-usage-limit").unwrap();
        let account = visual.debug_bounds("usage-account-picker").unwrap();
        let dismiss = visual.debug_bounds("dismiss-usage-limit").unwrap();
        assert_eq!(banner.size.height, px(32.));
        assert_eq!(resume.size.height, px(24.));
        assert_eq!(dismiss.size, gpui::size(px(24.), px(24.)));
        assert_eq!(banner.bottom(), composer.top());
        assert_eq!(banner.left(), composer.left() + px(8.));
        assert_eq!(banner.right(), composer.right() - px(8.));
        assert_eq!(account.right() + px(8.), resume.left());
        assert_eq!(resume.right() + px(8.), dismiss.left());
        assert!(visual.debug_bounds("usage-model-picker").is_none());
        assert!(visual.debug_bounds("provider-status").is_none());

        visual.simulate_click(resume.center(), Default::default());
        assert!(
            window
                .update(visual, |v, _, _| v
                    .settings
                    .usage_limit
                    .as_ref()
                    .unwrap()
                    .resume_at_reset)
                .unwrap()
        );
        assert!(
            locked(&saved)
                .unwrap()
                .agent_settings(id)
                .unwrap()
                .unwrap()
                .usage_limit
                .unwrap()
                .resume_at_reset
        );
        visual.refresh().unwrap();
        let resume = visual.debug_bounds("resume-usage-limit").unwrap();
        visual.simulate_click(resume.center(), Default::default());
        assert!(
            !window
                .update(visual, |v, _, _| v
                    .settings
                    .usage_limit
                    .as_ref()
                    .unwrap()
                    .resume_at_reset)
                .unwrap()
        );

        // Unknown or elapsed reset times keep a manual Resume control and the
        // same right-aligned actions, as in React's composer variant.
        for resets_at in [None, Some(usage::now_ms() - 1_000)] {
            window
                .update(visual, |v, _, cx| {
                    v.settings.usage_limit.as_mut().unwrap().resets_at = resets_at;
                    cx.notify();
                })
                .unwrap();
            visual.refresh().unwrap();
            assert_eq!(
                visual.debug_bounds("resume-usage-limit").unwrap().right(),
                resume.right()
            );
        }
        visual.simulate_click(dismiss.center(), Default::default());
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("usage-limit-notice").is_none());
        assert!(
            window
                .update(visual, |v, _, _| v.settings.usage_limit.is_none())
                .unwrap()
        );
    }

    #[gpui::test]
    fn subscription_limits_persist_and_release_a_mono_only_once_when_armed(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let summary = locked(&store).unwrap().create("codex", "/fixture").unwrap();
        let id = summary.id;
        let view = cx.new(|cx| Conversation::new(summary, store.clone(), cx));
        cx.run_until_parked();
        view.update(cx, |v, cx| {
            v.settings.mono = Some(monocode_core::agent::MonoContext {
                mono: Default::default(),
                root: "/fixture".into(),
                executable: "/fixture/binary".into(),
            });
            v.status = "Usage limit reached: Fixture subscription exhausted".into();
            v.detect_usage_limit(cx);
            assert!(v.settings.usage_limit.is_some());
            assert!(v.follow_ups.state.paused);
        });
        cx.run_until_parked();
        assert!(
            locked(&store)
                .unwrap()
                .agent_settings(id)
                .unwrap()
                .unwrap()
                .usage_limit
                .is_some()
        );
        let resumed = std::rc::Rc::new(std::cell::Cell::new(0));
        cx.update(|cx| {
            let count = resumed.clone();
            cx.subscribe(&view, move |_, _: &UsageResumed, _| {
                count.set(count.get() + 1)
            })
            .detach();
        });
        view.update(cx, |v, cx| {
            v.settings.usage_limit.as_mut().unwrap().resets_at = Some(usage::now_ms() - 31_000);
            v.tick_usage(cx);
            assert!(v.settings.usage_limit.is_some());
            v.settings.usage_limit.as_mut().unwrap().resume_at_reset = true;
            v.tick_usage(cx);
            assert!(v.settings.usage_limit.is_none());
            v.tick_usage(cx);
        });
        cx.run_until_parked();
        assert_eq!(resumed.get(), 1);
    }
}
