//! Port of PiUsage.test.ts. The footer shows Pi's chip for a Pi session.

use std::rc::Rc;
use std::time::Duration;

use futures::channel::oneshot;
use gpui::{AppContext as _, TestAppContext};

use super::*;
use crate::accounts::usage_footer::{
    UsageFooter, UsageFooterCallbacks, UsageFooterProps, UsageFooterSession,
};

fn pi_props(model: &str, id: &str) -> UsageFooterProps {
    UsageFooterProps {
        session: Some(UsageFooterSession {
            id: Some(id.into()),
            model: Some(model.into()),
            ..UsageFooterSession::new(HarnessId::Pi)
        }),
        ..Default::default()
    }
}

fn show(
    cx: &mut TestAppContext,
    host: Rc<FakeUsage>,
    model: &str,
) -> (Entity<UsageFooter>, &'static mut VisualTestContext) {
    let props = pi_props(model, "pi-session");
    mount(cx, 900., 600., move |_, cx| {
        cx.new(|cx| UsageFooter::new(host, props, UsageFooterCallbacks::default(), cx))
    })
}

fn switch(footer: &Entity<UsageFooter>, cx: &mut VisualTestContext, model: &str, id: &str) {
    footer.update(cx, |footer, cx| footer.set_props(pi_props(model, id), cx));
    draw(cx);
}

fn unavailable(message: &str) -> ProviderRateLimits {
    unavailable_rate_limits(RateLimitProvider::Claude, message, NOW)
}

#[gpui::test]
fn renders_pi_owned_quotas_without_fetching_another_cli_account(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    let (_, cx) = show(cx, host.clone(), "pi:anthropic/claude-sonnet-4-6");
    assert_eq!(
        host.pi_fetches.borrow().as_slice(),
        [PiUsageProvider::Anthropic]
    );
    assert!(host.fetches.borrow().is_empty());
    assert!(exists(cx, "text:24% 5h"));
    click(cx, "button:Pi · Anthropic usage details");
    assert!(exists(cx, "text:Pi's saved OAuth account"));
    assert!(!exists(cx, "button:Switch Pi · Anthropic account"));
    assert!(!exists(cx, "button:Add account"));
    assert!(!exists(cx, "sign-in-panel"));
}

#[gpui::test]
fn clears_old_usage_on_provider_changes_and_ignores_late_responses(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    let (finish_old, old) = oneshot::channel();
    host.pi_answers.borrow_mut().push_back(PiAnswer::Later(old));
    let (footer, cx) = show(cx, host.clone(), "pi:anthropic/claude-sonnet-4-6");
    host.pi_answers
        .borrow_mut()
        .push_back(PiAnswer::Now(pi_quota(PiUsageProvider::OpenaiCodex, 32.)));
    switch(&footer, cx, "pi:openai-codex/gpt-5.4", "pi-session");
    assert!(exists(cx, "text:32% 5h"));
    let _ = finish_old.send(pi_quota(PiUsageProvider::Anthropic, 99.));
    draw(cx);
    assert!(!exists(cx, "text:99% 5h"));
    assert!(exists(cx, "text:32% 5h"));
    assert_eq!(
        host.pi_fetches.borrow().last(),
        Some(&PiUsageProvider::OpenaiCodex)
    );
}

#[gpui::test]
fn does_not_guess_a_provider_for_defaults_or_unsupported_models(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    let (footer, cx) = show(cx, host.clone(), "pi:default");
    for model in [
        "pi:default",
        "pi:openai/gpt-5.4",
        "pi:openrouter/anthropic/claude",
    ] {
        switch(&footer, cx, model, "pi-session");
        assert!(exists(cx, "text:pi · Usage unavailable"), "{model}");
        assert!(!exists(cx, "text:24% 5h"));
    }
    assert!(host.pi_fetches.borrow().is_empty());
}

#[gpui::test]
fn clears_quotas_on_failed_refresh_and_can_recover_after_a_pi_login(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    let (_, cx) = show(cx, host.clone(), "pi:anthropic/claude-sonnet-4-6");
    assert!(exists(cx, "text:24% 5h"));
    host.pi_answers
        .borrow_mut()
        .push_back(PiAnswer::Now(unavailable(
            "Sign in through Pi, then refresh usage.",
        )));
    click(cx, "button:Refresh Pi usage");
    assert!(!exists(cx, "text:24% 5h"));
    assert!(exists(cx, "text:not connected"));
    host.pi_answers
        .borrow_mut()
        .push_back(PiAnswer::Now(pi_quota(PiUsageProvider::Anthropic, 12.)));
    click(cx, "button:Refresh Pi usage");
    assert!(exists(cx, "text:12% 5h"));
}

#[gpui::test]
fn isolates_a_to_b_to_a_and_same_provider_session_changes(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    let (finish_old, old) = oneshot::channel();
    host.pi_answers.borrow_mut().push_back(PiAnswer::Later(old));
    let props = pi_props("pi:anthropic/claude", "a");
    let mount_host = host.clone();
    let (footer, cx) = mount(cx, 900., 600., move |_, cx| {
        cx.new(|cx| UsageFooter::new(mount_host, props, UsageFooterCallbacks::default(), cx))
    });
    switch(&footer, cx, "pi:openai-codex/gpt", "b");
    host.pi_answers
        .borrow_mut()
        .push_back(PiAnswer::Now(pi_quota(PiUsageProvider::Anthropic, 17.)));
    switch(&footer, cx, "pi:anthropic/claude", "a");
    let _ = finish_old.send(pi_quota(PiUsageProvider::Anthropic, 99.));
    draw(cx);
    assert!(exists(cx, "text:17% 5h"));
    assert!(!exists(cx, "text:99% 5h"));
    host.pi_answers
        .borrow_mut()
        .push_back(PiAnswer::Now(pi_quota(PiUsageProvider::Anthropic, 8.)));
    switch(&footer, cx, "pi:anthropic/claude", "c");
    assert!(exists(cx, "text:8% 5h"));
    assert_eq!(host.pi_fetches.borrow().len(), 4);
}

#[gpui::test]
fn refreshes_on_native_window_focus_after_the_minimum_interval(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    let (_, cx) = show(cx, host.clone(), "pi:anthropic/claude");
    assert_eq!(host.pi_fetches.borrow().len(), 1);
    cx.executor()
        .advance_clock(Duration::from_millis(5 * 60_000));
    host.now.set(NOW + 5 * 60_000);
    host.pi_answers
        .borrow_mut()
        .push_back(PiAnswer::Now(pi_quota(PiUsageProvider::Anthropic, 13.)));
    cx.deactivate_window();
    cx.update(|window, _| window.activate_window());
    draw(cx);
    assert!(exists(cx, "text:13% 5h"));
    assert_eq!(host.pi_fetches.borrow().len(), 2);
}

#[gpui::test]
fn polls_only_while_visible_and_retries_unavailable_credentials(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    host.visible.set(false);
    let (footer, cx) = show(cx, host.clone(), "pi:anthropic/claude");
    assert!(host.pi_fetches.borrow().is_empty());

    host.visible.set(true);
    host.pi_answers
        .borrow_mut()
        .push_back(PiAnswer::Now(unavailable("Sign in through Pi.")));
    let pi = footer.read_with(cx, |footer, _| footer.pi_usage().cloned().unwrap());
    pi.update(cx, |pi, cx| pi.visibility_changed(cx));
    draw(cx);
    assert_eq!(host.pi_fetches.borrow().len(), 1);

    host.now.set(NOW + RATE_LIMIT_POLL_MS);
    cx.executor()
        .advance_clock(Duration::from_millis(RATE_LIMIT_POLL_MS as u64));
    draw(cx);
    assert_eq!(host.pi_fetches.borrow().len(), 2);
    assert!(exists(cx, "text:24% 5h"));

    host.visible.set(false);
    host.now.set(NOW + 2 * RATE_LIMIT_POLL_MS);
    cx.executor()
        .advance_clock(Duration::from_millis(RATE_LIMIT_POLL_MS as u64));
    draw(cx);
    assert_eq!(host.pi_fetches.borrow().len(), 2);
}
