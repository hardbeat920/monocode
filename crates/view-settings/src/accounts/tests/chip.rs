//! Port of UsageProviderChip.test.ts.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{AppContext as _, Task, TestAppContext};

use super::*;
use crate::accounts::usage_chip::{ChipActions, ChipProps, UsageProviderChip};

fn codex_limits() -> ProviderRateLimits {
    ProviderRateLimits {
        provider: RateLimitProvider::Codex,
        session: Some(RateLimitWindow {
            used_percent: 42.0,
            window_minutes: 300,
            resets_at: Some(NOW + 2 * 3_600_000),
        }),
        weekly: Some(RateLimitWindow {
            used_percent: 81.0,
            window_minutes: 10_080,
            resets_at: Some(NOW + 2 * 86_400_000 + 23 * 3_600_000),
        }),
        monthly: None,
        reset_credits: Some(RateLimitResetCredits {
            available_count: 2,
            credits: Some(vec![
                RateLimitResetCredit {
                    id: "reset-1".into(),
                    reset_type: ResetCreditType::CodexRateLimits,
                    status: ResetCreditStatus::Available,
                    granted_at: Some(NOW - 86_400_000),
                    expires_at: Some(NOW + 12 * 86_400_000),
                    title: Some("Referral reward".into()),
                    description: Some("One Codex rate-limit reset".into()),
                },
                RateLimitResetCredit {
                    id: "reset-2".into(),
                    reset_type: ResetCreditType::CodexRateLimits,
                    status: ResetCreditStatus::Available,
                    granted_at: Some(NOW - 43_200_000),
                    expires_at: Some(NOW + 18 * 86_400_000),
                    title: Some("Backup reset".into()),
                    description: Some("A second Codex rate-limit reset".into()),
                },
            ]),
        }),
        updated_at: NOW,
        error: None,
        status: RateLimitStatus::Ok,
    }
}

fn account(id: &str, label: &str, default: bool) -> ProviderAccount {
    ProviderAccount {
        is_default: default.then_some(true),
        ..ProviderAccount::new(id, HarnessId::Codex, label)
    }
}

/// Records the accounts a chip selected.
#[derive(Default)]
struct Selections(Rc<RefCell<Vec<String>>>);

impl Selections {
    fn actions(&self) -> ChipActions {
        let selected = self.0.clone();
        ChipActions {
            on_select_account: Some(Rc::new(move |id: String, _, _| {
                selected.borrow_mut().push(id)
            })),
            on_add_account: Some(Rc::new(|_, _, _| {
                Task::ready(Err("not in this test".into()))
            })),
            ..Default::default()
        }
    }

    fn all(&self) -> Vec<String> {
        self.0.borrow().clone()
    }
}

fn mount_chip(
    cx: &mut TestAppContext,
    host: Rc<FakeUsage>,
    props: ChipProps,
    actions: ChipActions,
) -> (Entity<UsageProviderChip>, &'static mut VisualTestContext) {
    mount(cx, 900., 700., move |window, cx| {
        cx.new(|cx| UsageProviderChip::new(host, props, actions, window, cx))
    })
}

#[gpui::test]
fn offers_the_provider_owned_login_flow_for_an_expired_claude_session(cx: &mut TestAppContext) {
    let limits = ProviderRateLimits {
        error: Some("Claude sign-in expired".into()),
        status: RateLimitStatus::Error,
        updated_at: NOW,
        ..idle_rate_limits(RateLimitProvider::Claude)
    };
    let calls = Rc::new(RefCell::new(0));
    let counter = calls.clone();
    let actions = ChipActions {
        on_reconnect: Some(Rc::new(move |(), _, _| {
            *counter.borrow_mut() += 1;
            Task::ready(Ok(()))
        })),
        ..Default::default()
    };
    let host = Rc::new(FakeUsage::default());
    let (_, cx) = mount_chip(cx, host, ChipProps::new(limits, NOW), actions);
    click(cx, "button:Claude Code usage details");
    assert!(exists(cx, "sign-in-panel"));
    click(cx, "button:Sign in to Claude Code");
    assert_eq!(*calls.borrow(), 1);
    assert!(exists(cx, "text:Signed in to Claude Code"));
}

#[gpui::test]
fn opens_a_column_of_detailed_progress_bars(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    let (chip, cx) = mount_chip(
        cx,
        host,
        ChipProps::new(codex_limits(), NOW),
        ChipActions::default(),
    );
    assert!(!chip.read_with(cx, |chip, _| chip.is_open()));
    assert!(exists(cx, "minibar=19"));
    click(cx, "button:Codex usage details");
    assert!(chip.read_with(cx, |chip, _| chip.is_open()));
    assert!(exists(cx, "dialog:Codex usage details"));
    for label in [
        "text:5-hour limit",
        "text:Weekly limit",
        "text:58% remaining",
        "text:19% remaining",
        "progressbar:5-hour limit remaining=58",
        "fill:5-hour limit remaining=58",
        "progressbar:Weekly limit remaining=19",
        "fill:Weekly limit remaining=19",
    ] {
        assert!(exists(cx, label), "{label}");
    }
    assert!(!exists(cx, "progressbar:Monthly limit remaining=100"));
}

#[gpui::test]
fn shows_a_full_bar_before_usage_and_an_empty_bar_when_exhausted(cx: &mut TestAppContext) {
    let mut limits = codex_limits();
    limits.session.as_mut().unwrap().used_percent = 0.0;
    limits.weekly.as_mut().unwrap().used_percent = 100.0;
    let host = Rc::new(FakeUsage::default());
    let (_, cx) = mount_chip(
        cx,
        host,
        ChipProps::new(limits, NOW),
        ChipActions::default(),
    );
    assert!(exists(cx, "minibar=0"));
    click(cx, "button:Codex usage details");
    assert!(exists(cx, "progressbar:5-hour limit remaining=100"));
    assert!(exists(cx, "fill:5-hour limit remaining=100"));
    assert!(exists(cx, "progressbar:Weekly limit remaining=0"));
    assert!(exists(cx, "fill:Weekly limit remaining=0"));
}

#[gpui::test]
fn switches_between_named_accounts_from_the_usage_popover(cx: &mut TestAppContext) {
    let selections = Selections::default();
    let props = ChipProps {
        account_id: Some("default".into()),
        accounts: vec![
            account("default", "Default account", true),
            account("account-work", "Work", false),
        ],
        ..ChipProps::new(codex_limits(), NOW)
    };
    let host = Rc::new(FakeUsage::default());
    let (chip, cx) = mount_chip(cx, host, props, selections.actions());
    click(cx, "button:Codex usage details");
    click(cx, "button:Switch Codex account");
    assert!(exists(cx, "text:Codex accounts"));
    assert!(exists(cx, "text:Default account"));
    assert!(exists(cx, "text:58% left"));
    assert!(exists(cx, "progressbar:5h limit remaining=58"));
    assert!(exists(cx, "fill:5h limit remaining=58"));
    click(cx, "button:Work");
    assert_eq!(selections.all(), ["account-work"]);
    assert!(!chip.read_with(cx, |chip, _| chip.is_open()));
    assert!(!exists(cx, "dialog:Codex usage details"));
}

#[gpui::test]
fn reveals_emails_independently_of_account_switching_and_hides_them_on_reopening(
    cx: &mut TestAppContext,
) {
    let host = Rc::new(FakeUsage::default());
    host.identities.borrow_mut().insert(
        "codex:default".into(),
        ProviderAccountIdentity {
            email: Some("user@example.com".into()),
            plan: Some("Pro".into()),
            ..Default::default()
        },
    );
    let selections = Selections::default();
    let props = ChipProps {
        account_id: Some("default".into()),
        accounts: vec![account("default", "Main", true)],
        ..ChipProps::new(codex_limits(), NOW)
    };
    let (chip, cx) = mount_chip(cx, host, props, selections.actions());
    let chip_open = |cx: &mut VisualTestContext| chip.read_with(cx, |chip, _| chip.is_open());
    click(cx, "button:Codex usage details");
    // Hidden emails stay masked until revealed.
    assert!(exists(cx, "email:Reveal email"));
    assert!(exists(cx, "text:Pro"));
    click(cx, "email:Reveal email");
    assert!(exists(cx, "email:Hide email"));
    assert!(exists(cx, "text:Codex usage"));
    assert!(!exists(cx, "text:Codex accounts"));
    assert!(selections.all().is_empty());
    click(cx, "email:Hide email");
    assert!(exists(cx, "email:Reveal email"));

    // Reopening hides it again.
    click(cx, "email:Reveal email");
    click(cx, "button:Codex usage details");
    click(cx, "button:Codex usage details");
    assert!(chip_open(cx));
    assert!(exists(cx, "email:Reveal email"));

    // The picker masks it too, and clicking it does not switch accounts.
    // The switch target sits under the email, so press its left edge.
    click_edge(cx, "button:Switch Codex account");
    assert!(exists(cx, "email:Reveal email"));
    click(cx, "email:Reveal email");
    assert!(selections.all().is_empty());
    assert!(exists(cx, "text:Codex accounts"));
    click_edge(cx, "button:Main");
    assert_eq!(selections.all(), ["default"]);
}

#[gpui::test]
fn keeps_account_switching_available_when_the_pinned_account_was_removed(cx: &mut TestAppContext) {
    let selections = Selections::default();
    let props = ChipProps {
        account_id: Some("account-missing".into()),
        accounts: vec![
            account("default", "Default account", true),
            account("account-work", "Work", false),
        ],
        ..ChipProps::new(codex_limits(), NOW)
    };
    let host = Rc::new(FakeUsage::default());
    let (_, cx) = mount_chip(cx, host, props, selections.actions());
    assert!(!exists(cx, "text:Default account"));
    click(cx, "button:Codex usage details");
    assert!(exists(cx, "text:Removed account"));
    click(cx, "button:Switch Codex account");
    assert!(exists(cx, "text:Default account"));
    assert!(exists(cx, "text:Work"));
}

#[gpui::test]
fn shows_and_deliberately_consumes_a_banked_reset(cx: &mut TestAppContext) {
    let consumed = Rc::new(RefCell::new(Vec::new()));
    let record = consumed.clone();
    let actions = ChipActions {
        on_consume_reset: Some(Rc::new(move |credit: Option<String>, _, _| {
            record.borrow_mut().push(credit);
            Task::ready(Ok(CodexRateLimitResetOutcome::Reset))
        })),
        ..Default::default()
    };
    let host = Rc::new(FakeUsage::default());
    let (_, cx) = mount_chip(cx, host, ChipProps::new(codex_limits(), NOW), actions);
    click(cx, "button:Codex usage details");
    assert!(exists(cx, "text:2 resets available"));
    assert!(exists(
        cx,
        &format!(
            "mascot:happy:{}",
            crate::accounts::mascot::project_mascot("Codex", None).0
        )
    ));
    assert!(exists(cx, "text:Referral reward"));
    assert!(exists(cx, "text:Backup reset"));
    assert!(exists(cx, "text:Expires in 12d"));
    assert!(exists(cx, "list:Available banked resets"));
    assert!(exists(cx, "button:Use reset:reset-1"));
    assert!(exists(cx, "button:Use reset:reset-2"));
    scroll(cx, "dialog:Codex usage details", 400.);
    click(cx, "button:Use reset:reset-2");
    assert!(exists(cx, "text:Spend this reset now?"));
    scroll(cx, "dialog:Codex usage details", 400.);
    click(cx, "button:Confirm");
    assert_eq!(consumed.borrow().as_slice(), [Some("reset-2".to_string())]);
    assert!(exists(cx, "text:Codex usage was reset."));
}

#[gpui::test]
fn uses_the_projects_picked_mascot_when_banked_resets_are_available(cx: &mut TestAppContext) {
    let project = "/repo/mascot-lab";
    let host = Rc::new(FakeUsage::default());
    host.mascots
        .borrow_mut()
        .insert(monocode_layout::paths::project_key(project), "cat".into());
    let props = ChipProps {
        project: Some(project.into()),
        ..ChipProps::new(codex_limits(), NOW)
    };
    let (_, cx) = mount_chip(cx, host, props, ChipActions::default());
    click(cx, "button:Codex usage details");
    assert!(exists(cx, "mascot:happy:cat"));
}

#[gpui::test]
fn hides_the_banked_resets_card_when_no_resets_are_available(cx: &mut TestAppContext) {
    let mut limits = codex_limits();
    limits.reset_credits = Some(RateLimitResetCredits {
        available_count: 0,
        credits: Some(Vec::new()),
    });
    let host = Rc::new(FakeUsage::default());
    let (_, cx) = mount_chip(
        cx,
        host,
        ChipProps::new(limits, NOW),
        ChipActions::default(),
    );
    click(cx, "button:Codex usage details");
    assert!(!exists(cx, "text:Banked resets"));
    assert!(!exists(cx, "list:Available banked resets"));
}

#[gpui::test]
fn keeps_aggregate_only_resets_visible_as_claimable_rows(cx: &mut TestAppContext) {
    let mut limits = codex_limits();
    let first = limits
        .reset_credits
        .as_ref()
        .and_then(|credits| credits.credits.as_ref())
        .map(|credits| credits[..1].to_vec());
    limits.reset_credits = Some(RateLimitResetCredits {
        available_count: 2,
        credits: first,
    });
    let actions = ChipActions {
        on_consume_reset: Some(Rc::new(|_, _, _| {
            Task::ready(Ok(CodexRateLimitResetOutcome::Reset))
        })),
        ..Default::default()
    };
    let host = Rc::new(FakeUsage::default());
    let (_, cx) = mount_chip(cx, host, ChipProps::new(limits, NOW), actions);
    click(cx, "button:Codex usage details");
    assert!(exists(cx, "text:Referral reward"));
    assert!(exists(cx, "text:Banked reset 2"));
    assert!(exists(cx, "button:Use reset:reset-1"));
    assert!(exists(cx, "button:Use reset:unlisted-1"));
}

#[gpui::test]
fn escape_closes_the_popover_and_an_outside_press_dismisses_it(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUsage::default());
    let (chip, cx) = mount_chip(
        cx,
        host,
        ChipProps::new(codex_limits(), NOW),
        ChipActions::default(),
    );
    click(cx, "button:Codex usage details");
    keys(cx, "escape");
    assert!(!chip.read_with(cx, |chip, _| chip.is_open()));
    click(cx, "button:Codex usage details");
    cx.simulate_click(gpui::point(px(800.), px(20.)), gpui::Modifiers::none());
    draw(cx);
    assert!(!chip.read_with(cx, |chip, _| chip.is_open()));
}

#[gpui::test]
fn adds_an_account_from_the_picker(cx: &mut TestAppContext) {
    let added = Rc::new(RefCell::new(Vec::new()));
    let record = added.clone();
    let actions = ChipActions {
        on_select_account: Some(Rc::new(|_, _, _| {})),
        on_add_account: Some(Rc::new(move |label: String, _, _| {
            record.borrow_mut().push(label.clone());
            Task::ready(Ok(ProviderAccount::new(
                "account-new",
                HarnessId::Codex,
                &label,
            )))
        })),
        ..Default::default()
    };
    let props = ChipProps {
        account_id: Some("default".into()),
        accounts: vec![account("default", "Default account", true)],
        ..ChipProps::new(codex_limits(), NOW)
    };
    let host = Rc::new(FakeUsage::default());
    let (chip, cx) = mount_chip(cx, host, props, actions);
    click(cx, "button:Codex usage details");
    click(cx, "button:Switch Codex account");
    click(cx, "button:Add account");
    assert!(exists(cx, "text:Add Codex account"));
    let input = chip.read_with(cx, |chip, _| chip.add_label_input().clone());
    type_into(cx, &input, "Work");
    click(cx, "button:Sign in and add account");
    assert_eq!(added.borrow().as_slice(), ["Work".to_string()]);
    assert!(!chip.read_with(cx, |chip, _| chip.is_open()));
}
