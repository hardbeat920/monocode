//! Named profiles are persisted only after their own login succeeds.
use super::*;
use crate::accounts::ProviderAccountsSettings;
use gpui::{AppContext as _, TestAppContext};

fn mount_editor(
    cx: &mut TestAppContext,
    host: Rc<FakeUsage>,
) -> (
    Entity<ProviderAccountsSettings>,
    &'static mut VisualTestContext,
) {
    mount(cx, 900., 700., move |window, cx| {
        cx.new(|cx| ProviderAccountsSettings::new(host, window, cx))
    })
}

#[gpui::test]
fn saves_a_named_profile_after_its_login_succeeds(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    host.set_accounts(
        HarnessId::Codex,
        vec![ProviderAccount::new(
            "default",
            HarnessId::Codex,
            "Default account",
        )],
    );
    let (view, cx) = mount_editor(cx, host.clone());
    view.update_in(cx, |view, window, cx| {
        view.start_add(HarnessId::Codex, window, cx);
        view.label_input()
            .update(cx, |label, cx| label.set_value("  Work  ", window, cx));
        view.submit(window, cx);
    });
    assert!(host.saved.borrow().is_empty());
    assert_eq!(
        host.logins.borrow()[0].account_id.as_deref(),
        Some("account-1")
    );
    assert!(view.read_with(cx, |view, _| view.editor().is_some()));
    host.finish_login(HarnessId::Codex, Ok(()));
    cx.run_until_parked();
    assert_eq!(host.saved.borrow()[0].label, "Work");
    assert!(view.read_with(cx, |view, _| view.editor().is_none()));
}

#[gpui::test]
fn failed_login_keeps_the_editor_and_does_not_save_the_account(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    let (view, cx) = mount_editor(cx, host.clone());
    view.update_in(cx, |view, window, cx| {
        view.start_add(HarnessId::Claude, window, cx);
        view.label_input()
            .update(cx, |label, cx| label.set_value("Personal", window, cx));
        view.submit(window, cx);
    });
    host.finish_login(HarnessId::Claude, Err("Login was cancelled".into()));
    cx.run_until_parked();
    assert!(host.saved.borrow().is_empty());
    assert!(view.read_with(cx, |view, _| view.editor().is_some()));
    assert_eq!(
        view.read_with(cx, |view, _| view.error().map(str::to_owned)),
        Some("Login was cancelled".to_owned())
    );
}

/// A Claude default account 23% into its 5h window, signed in as
/// user@example.com.
fn claude_usage_host() -> Rc<FakeUsage> {
    let host = Rc::new(FakeUsage::default());
    host.cache.borrow_mut().insert(
        "claude:default".into(),
        ProviderRateLimits {
            session: Some(RateLimitWindow {
                used_percent: 23.0,
                window_minutes: 300,
                resets_at: Some(NOW + 3_600_000),
            }),
            status: RateLimitStatus::Ok,
            updated_at: NOW,
            ..idle_rate_limits(RateLimitProvider::Claude)
        },
    );
    host.identities.borrow_mut().insert(
        "claude:default".into(),
        ProviderAccountIdentity {
            email: Some("user@example.com".into()),
            plan: Some("Pro".into()),
            ..Default::default()
        },
    );
    host
}

// Port of ProviderAccountUsage.test.ts.
#[gpui::test]
fn flips_the_meter_when_another_window_turns_on_remaining_usage(cx: &mut TestAppContext) {
    let host = claude_usage_host();
    let (_, cx) = mount_editor(cx, host.clone());
    assert!(exists(cx, "progressbar:5h limit used=23"));
    assert!(exists(cx, "fill:5h limit used=23"));
    assert!(exists(cx, "text:23%"));

    cx.update(|_, cx| host.set_show_remaining(true, cx));
    draw(cx);

    assert!(exists(cx, "progressbar:5h limit remaining=77"));
    assert!(exists(cx, "fill:5h limit remaining=77"));
    assert!(exists(cx, "text:77% left"));
}

// Ports of PrivateEmail.test.ts.
#[gpui::test]
fn masks_a_revealed_email_again_when_masking_is_turned_off_and_back_on(cx: &mut TestAppContext) {
    let host = claude_usage_host();
    host.mask_emails.set(true);
    let (_, cx) = mount_editor(cx, host.clone());
    click(cx, "email:Reveal email");
    assert!(exists(cx, "email:Hide email"));

    cx.update(|_, cx| host.set_mask_emails(false, cx));
    draw(cx);
    assert!(!exists(cx, "email:Hide email"));
    assert!(exists(cx, "email-text:user@example.com"));

    cx.update(|_, cx| host.set_mask_emails(true, cx));
    draw(cx);
    assert!(!exists(cx, "email:Hide email"));
    assert!(exists(cx, "email:Reveal email"));
}

#[gpui::test]
fn masks_an_email_when_another_window_turns_masking_on(cx: &mut TestAppContext) {
    let host = claude_usage_host();
    let (_, cx) = mount_editor(cx, host.clone());
    assert!(!exists(cx, "email:Reveal email"));
    assert!(exists(cx, "email-text:user@example.com"));

    cx.update(|_, cx| host.set_mask_emails(true, cx));
    draw(cx);

    assert!(exists(cx, "email:Reveal email"));
    assert!(!exists(cx, "email-text:user@example.com"));
}
