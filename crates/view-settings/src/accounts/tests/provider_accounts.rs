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
