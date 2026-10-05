//! Ports of UsageFooter.test.ts and UsageFooterAuth.test.ts.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{AppContext as _, TestAppContext};
use monocode_core::Platform;
use monocode_layout::terminal_tab::RunningTerminal;

use super::*;
use crate::accounts::usage_footer::{
    UsageFooter, UsageFooterCallbacks, UsageFooterProps, UsageFooterSession,
};

fn mount_footer(
    cx: &mut TestAppContext,
    host: Rc<FakeUsage>,
    props: UsageFooterProps,
    callbacks: UsageFooterCallbacks,
) -> (Entity<UsageFooter>, &'static mut VisualTestContext) {
    mount(cx, 900., 600., move |_, cx| {
        cx.new(|cx| UsageFooter::new(host, props, callbacks, cx))
    })
}

type Callback<A> = Rc<dyn Fn(A, &mut Window, &mut App)>;

fn noop<A: 'static>() -> Option<Callback<A>> {
    Some(Rc::new(|_, _, _| {}))
}

fn signed_out(provider: RateLimitProvider) -> ProviderRateLimits {
    ProviderRateLimits {
        updated_at: NOW,
        error: Some(format!("{} is not signed in", provider.as_str())),
        status: RateLimitStatus::Error,
        ..idle_rate_limits(provider)
    }
}

fn connected(provider: RateLimitProvider) -> ProviderRateLimits {
    ProviderRateLimits {
        error: None,
        status: RateLimitStatus::Ok,
        ..signed_out(provider)
    }
}

#[gpui::test]
fn replaces_the_generic_terminal_button_with_the_live_process_control(cx: &mut TestAppContext) {
    let toggled = Rc::new(RefCell::new(Vec::new()));
    let record = toggled.clone();
    let props = UsageFooterProps {
        terminals: vec![RunningTerminal {
            id: "terminal-1".into(),
            process: "npm".into(),
            cwd: "/repo".into(),
            label: "repo".into(),
        }],
        terminal_open: true,
        project_terminal_active: true,
        ..Default::default()
    };
    let callbacks = UsageFooterCallbacks {
        on_toggle_terminal: Some(Rc::new(move |id: String, _, _| {
            record.borrow_mut().push(id)
        })),
        on_new_terminal: noop(),
        on_show_terminal: noop(),
        ..Default::default()
    };
    let (_, cx) = mount_footer(cx, Rc::new(FakeUsage::default()), props, callbacks);
    assert!(exists(cx, "text:npm"));
    assert!(!exists(cx, "text:Terminal"));
    assert!(exists(cx, "button:Hide npm"));
    assert!(!exists(cx, "button:Terminal"));
    assert!(exists(cx, "footer:Terminals"));
    click(cx, "button:Hide npm");
    assert_eq!(toggled.borrow().as_slice(), ["terminal-1".to_string()]);
}

#[gpui::test]
fn keeps_the_generic_terminal_button_when_no_process_is_running(cx: &mut TestAppContext) {
    let opened = Rc::new(RefCell::new(0));
    let record = opened.clone();
    let props = UsageFooterProps {
        platform: Platform::Mac,
        ..Default::default()
    };
    let callbacks = UsageFooterCallbacks {
        on_new_terminal: Some(Rc::new(move |(), _, _| *record.borrow_mut() += 1)),
        ..Default::default()
    };
    let (_, cx) = mount_footer(cx, Rc::new(FakeUsage::default()), props, callbacks);
    assert!(exists(cx, "text:Terminal"));
    assert!(exists(cx, "button:New Terminal (⌘`)"));
    click(cx, "button:New Terminal (⌘`)");
    assert_eq!(*opened.borrow(), 1);
}

#[gpui::test]
fn lists_several_running_terminals_in_a_menu(cx: &mut TestAppContext) {
    let toggled = Rc::new(RefCell::new(Vec::new()));
    let record = toggled.clone();
    let terminal = |id: &str, process: &str| RunningTerminal {
        id: id.into(),
        process: process.into(),
        cwd: "/repo".into(),
        label: "repo".into(),
    };
    let props = UsageFooterProps {
        terminals: vec![terminal("t1", "vite"), terminal("t2", "jest")],
        ..Default::default()
    };
    let callbacks = UsageFooterCallbacks {
        on_toggle_terminal: Some(Rc::new(move |id: String, _, _| {
            record.borrow_mut().push(id)
        })),
        ..Default::default()
    };
    let (_, cx) = mount_footer(cx, Rc::new(FakeUsage::default()), props, callbacks);
    assert!(exists(cx, "text:vite · jest"));
    click(cx, "button:2 terminals are running processes");
    assert!(exists(cx, "menu:Running terminals"));
    click(cx, "menuitem:jest");
    assert_eq!(toggled.borrow().as_slice(), ["t2".to_string()]);
    assert!(!exists(cx, "menu:Running terminals"));
}

#[gpui::test]
fn reuses_usage_on_remount_and_only_fetches_again_on_refresh(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    host.answer(
        RateLimitProvider::Codex,
        connected(RateLimitProvider::Codex),
    );
    let props = UsageFooterProps {
        providers: vec![RateLimitProvider::Codex],
        ..Default::default()
    };
    let (_, first) = mount_footer(
        cx,
        host.clone(),
        props.clone(),
        UsageFooterCallbacks::default(),
    );
    assert_eq!(host.fetch_count(RateLimitProvider::Codex), 1);
    first.update(|window, _| window.remove_window());
    first.run_until_parked();

    // A new footer over the same cache reads the snapshot.
    let (_, second) = mount_footer(cx, host.clone(), props, UsageFooterCallbacks::default());
    assert_eq!(host.fetch_count(RateLimitProvider::Codex), 1);
    assert!(exists(second, "button:Codex usage details"));

    click(second, "button:Refresh usage");
    assert_eq!(host.fetch_count(RateLimitProvider::Codex), 2);
}

#[gpui::test]
fn keeps_a_healthy_grok_provider_label_non_interactive(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    host.login_support.borrow_mut().push(HarnessId::Grok);
    let props = UsageFooterProps {
        session: Some(UsageFooterSession {
            id: Some("grok-session".into()),
            ..UsageFooterSession::new(HarnessId::Grok)
        }),
        ..Default::default()
    };
    let (_, cx) = mount_footer(cx, host, props, UsageFooterCallbacks::default());
    assert!(exists(cx, "text:grok"));
    assert!(!exists(cx, "text:sign in"));
    assert!(!exists(cx, "button:Grok Build sign-in required"));
    assert!(!exists(cx, "dialog:Grok Build sign-in"));
}

#[gpui::test]
fn opens_grok_sign_in_from_its_footer_provider_popover(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    host.login_support.borrow_mut().push(HarnessId::Grok);
    let session = UsageFooterSession {
        id: Some("grok-session".into()),
        auth_required: true,
        ..UsageFooterSession::new(HarnessId::Grok)
    };
    let props = UsageFooterProps {
        session: Some(session),
        ..Default::default()
    };
    let (_, cx) = mount_footer(cx, host.clone(), props, UsageFooterCallbacks::default());
    assert!(exists(cx, "text:grok"));
    assert!(exists(cx, "text:sign in"));
    click(cx, "button:Grok Build sign-in required");
    assert!(exists(cx, "dialog:Grok Build sign-in"));
    assert!(exists(cx, "text:Authentication required"));
    assert!(exists(cx, "sign-in-panel"));

    click(cx, "button:Sign in to Grok Build");
    assert_eq!(host.login_calls(), [HarnessId::Grok]);
    assert!(exists(cx, "button:Waiting for browser…"));

    host.finish_login(HarnessId::Grok, Ok(()));
    draw(cx);
    assert!(!exists(cx, "dialog:Grok Build sign-in"));
    assert!(!exists(cx, "text:sign in"));
}

#[gpui::test]
fn starts_a_waiting_recovery_after_another_provider_login_fails(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    host.answer(
        RateLimitProvider::Claude,
        signed_out(RateLimitProvider::Claude),
    );
    host.answer(
        RateLimitProvider::Codex,
        signed_out(RateLimitProvider::Codex),
    );
    host.answer(
        RateLimitProvider::Codex,
        connected(RateLimitProvider::Codex),
    );
    let props = UsageFooterProps {
        providers: vec![RateLimitProvider::Claude, RateLimitProvider::Codex],
        ..Default::default()
    };
    let (_, cx) = mount_footer(cx, host.clone(), props, UsageFooterCallbacks::default());

    click(cx, "button:Claude Code usage details");
    click(cx, "button:Sign in to Claude Code");
    assert_eq!(host.login_calls(), [HarnessId::Claude]);

    click(cx, "button:Codex usage details");
    click(cx, "button:Sign in to Codex");
    assert_eq!(host.login_calls(), [HarnessId::Claude]);

    host.finish_login(HarnessId::Claude, Err("Claude login failed".into()));
    draw(cx);
    assert_eq!(host.login_calls(), [HarnessId::Claude, HarnessId::Codex]);
    // The failed provider keeps its snapshot with the error.
    let claude = host.cache.borrow().get("claude:default").cloned().unwrap();
    assert_eq!(claude.error.as_deref(), Some("Claude login failed"));
}
