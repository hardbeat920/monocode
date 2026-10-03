//! Ports of ConnectionsSettings.test.ts.

use std::rc::Rc;
use std::time::Duration;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use monocode_remote::host::protocol::{SshSetup, SshSetupPrompt};

use super::{ConnectionsSettings, Field};
use crate::fake::{Call, FakeRemote, direct_machine, ssh_machine};
use crate::host::SshBegin;
use crate::machines::describe_params;
use crate::test_support::{advance, click, draw, exists, fill, mount};

fn render<'a>(
    cx: &'a mut TestAppContext,
    fake: &FakeRemote,
) -> (Entity<ConnectionsSettings>, &'a mut VisualTestContext) {
    let host = fake.clone();
    mount(cx, move |window, cx| {
        cx.new(|cx| ConnectionsSettings::new(Rc::new(host), window, cx))
    })
}

fn read<R>(
    view: &Entity<ConnectionsSettings>,
    cx: &mut VisualTestContext,
    f: impl FnOnce(&ConnectionsSettings, &gpui::App) -> R,
) -> R {
    view.read_with(cx, |view, cx| f(view, cx))
}

/// Opens Add machine, picks SSH, types the address, and starts setup.
fn start(cx: &mut VisualTestContext) {
    click(cx, "button:Add machine");
    click(cx, "tab:SSH");
    fill(cx, "input:SSH address", "me@home");
    click(cx, "button:Set up over SSH");
}

/// One poll interval.
fn poll(cx: &mut VisualTestContext) {
    advance(cx, Duration::from_millis(400));
}

#[gpui::test]
fn starts_ssh_setup_from_settings_and_makes_the_machine_available_after_native_pairing(
    cx: &mut TestAppContext,
) {
    let fake = FakeRemote::new();
    let (view, cx) = render(cx, &fake);
    start(cx);
    assert!(fake.called(&Call::SshBegin(SshBegin {
        target: "me@home".into(),
        name: String::new(),
        port: None,
        upgrade: false,
    })));
    assert_eq!(
        read(&view, cx, |view, _| view
            .job()
            .map(|job| job.message.clone())),
        Some("Installing host…".to_string())
    );
    assert!(exists(cx, "ssh-progress"));
    {
        let mut state = fake.0.borrow_mut();
        state.machines = vec![ssh_machine()];
        state.setup.done = true;
        state.setup.machine = Some(ssh_machine());
    }
    poll(cx);
    let notice = read(&view, cx, |view, _| view.notice().to_string());
    assert!(notice.contains("Home Mac is connected"), "{notice}");
    assert_eq!(read(&view, cx, |view, _| view.machines().len()), 1);
    assert!(exists(cx, "route:machine"));
    assert_eq!(
        crate::machines::route_label(&ssh_machine()),
        "SSH · me@home"
    );
    assert!(!exists(cx, "input:SSH address"));
    assert!(!exists(cx, "ssh-progress"));
}

#[gpui::test]
fn requires_an_explicit_host_trust_answer_and_forwards_secrets_only_to_the_native_prompt(
    cx: &mut TestAppContext,
) {
    let fake = FakeRemote::new();
    fake.0.borrow_mut().setup.prompt = Some(SshSetupPrompt {
        id: "trust".into(),
        message: "Host fingerprint: SHA256:example".into(),
        confirm: true,
    });
    let (view, cx) = render(cx, &fake);
    start(cx);
    assert!(
        !fake
            .calls()
            .iter()
            .any(|call| matches!(call, Call::SshAnswer { .. }))
    );
    click(cx, "button:Trust host and continue");
    assert!(fake.called(&Call::SshAnswer {
        job_id: "setup".into(),
        prompt_id: "trust".into(),
        answer: "yes".into(),
    }));
    fake.0.borrow_mut().setup.prompt = Some(SshSetupPrompt {
        id: "password".into(),
        message: "Password:".into(),
        confirm: false,
    });
    poll(cx);
    fill(
        cx,
        "input:SSH password or passphrase",
        "secret-for-this-prompt",
    );
    click(cx, "button:Continue");
    assert!(fake.called(&Call::SshAnswer {
        job_id: "setup".into(),
        prompt_id: "password".into(),
        answer: "secret-for-this-prompt".into(),
    }));
    assert_eq!(
        read(&view, cx, |view, cx| view.value(Field::Answer, cx)),
        ""
    );
}

#[gpui::test]
fn keeps_the_ssh_address_after_a_failed_install_and_cancels_active_setup_when_settings_closes(
    cx: &mut TestAppContext,
) {
    let fake = FakeRemote::new();
    {
        let mut state = fake.0.borrow_mut();
        state.setup.done = true;
        state.setup.error = Some("Host package is unavailable".into());
    }
    let (view, cx) = render(cx, &fake);
    start(cx);
    assert!(
        read(&view, cx, |view, _| view.error().to_string()).contains("Host package is unavailable")
    );
    assert_eq!(
        read(&view, cx, |view, cx| view.value(Field::Target, cx)),
        "me@home"
    );
    fake.0.borrow_mut().setup = SshSetup {
        id: "setup".into(),
        message: "Connecting…".into(),
        prompt: None,
        done: false,
        error: None,
        machine: None,
    };
    click(cx, "button:Set up over SSH");
    assert!(!fake.called(&Call::SshCancel("setup".into())));
    drop(view);
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert!(fake.called(&Call::SshCancel("setup".into())));
}

#[gpui::test]
fn offers_an_explicit_host_update_for_an_ssh_machine_without_pushed_changes(
    cx: &mut TestAppContext,
) {
    let fake = FakeRemote::new();
    fake.0.borrow_mut().machines = vec![ssh_machine()];
    let (view, cx) = render(cx, &fake);
    let label = read(&view, cx, |view, _| view.status_label("machine"));
    assert!(
        label.contains("host older than 0.5 · update available"),
        "{label}"
    );
    assert!(exists(cx, "button:Update Host"));
    click(cx, "button:Update Host");
    assert!(fake.called(&Call::SshReconnect {
        machine_id: "machine".into(),
        upgrade: true,
    }));
}

#[gpui::test]
fn advertises_every_supported_provider_when_checking_a_host(cx: &mut TestAppContext) {
    let fake = FakeRemote::new();
    fake.0.borrow_mut().machines = vec![ssh_machine()];
    let (_view, _cx) = render(cx, &fake);
    assert!(fake.called(&Call::Request {
        machine_id: "machine".into(),
        method: "environment.describe".into(),
        params: describe_params(),
    }));
}

fn open_remove<'a>(
    cx: &'a mut TestAppContext,
    fake: &FakeRemote,
) -> (Entity<ConnectionsSettings>, &'a mut VisualTestContext) {
    fake.0.borrow_mut().machines = vec![ssh_machine()];
    let (view, cx) = render(cx, fake);
    click(cx, "Remove Home Mac");
    (view, cx)
}

#[gpui::test]
fn explains_removal_and_removes_the_saved_connection_without_stopping_or_revoking(
    cx: &mut TestAppContext,
) {
    let fake = FakeRemote::new();
    let (view, cx) = open_remove(cx, &fake);
    assert!(!fake.called(&Call::Disconnect("machine".into())));
    assert!(exists(cx, "Confirm removing Home Mac"));
    assert_eq!(
        read(&view, cx, |view, _| view.removing().map(str::to_string)),
        Some("machine".into())
    );
    click(cx, "button:Remove from this desktop only");
    assert!(fake.called(&Call::Disconnect("machine".into())));
    assert!(!fake.requested("devices.revokeSelf"));
    assert!(
        !fake
            .calls()
            .iter()
            .any(|call| format!("{call:?}").contains("\"stop\""))
    );
    let notice = read(&view, cx, |view, _| view.notice().to_string());
    assert!(
        notice.contains("still accepts this desktop's credential"),
        "{notice}"
    );
}

#[gpui::test]
fn revokes_this_desktops_credential_before_removing_the_connection(cx: &mut TestAppContext) {
    let fake = FakeRemote::new();
    let (view, cx) = open_remove(cx, &fake);
    click(cx, "button:Revoke access and remove");
    let calls = fake.sequence();
    let revoke = calls.iter().position(|call| call == "devices.revokeSelf");
    let disconnect = calls.iter().position(|call| call == "remote_disconnect");
    assert!(revoke.is_some());
    assert!(disconnect > revoke);
    assert!(read(&view, cx, |view, _| view.notice().to_string()).contains("access was revoked"));
}

#[gpui::test]
fn keeps_the_connection_when_the_host_cannot_revoke_its_credential(cx: &mut TestAppContext) {
    let fake = FakeRemote::new();
    fake.set_respond(|_, method, _| {
        if method == "devices.revokeSelf" {
            Err("Machine is unreachable".into())
        } else {
            Ok(serde_json::json!({ "environmentId": "env", "providers": ["codex"] }))
        }
    });
    let (view, cx) = open_remove(cx, &fake);
    click(cx, "button:Revoke access and remove");
    assert!(!fake.called(&Call::Disconnect("machine".into())));
    assert!(
        read(&view, cx, |view, _| view.error().to_string()).contains("Could not revoke access")
    );
    assert!(exists(cx, "button:Remove from this desktop only"));
}

#[gpui::test]
fn pairs_a_machine_from_the_link_that_connect_prints(cx: &mut TestAppContext) {
    let fake = FakeRemote::new();
    let (view, cx) = render(cx, &fake);
    click(cx, "button:Add machine");
    assert_eq!(
        read(&view, cx, |view, _| view.command()),
        "npx monocode-host@1.2.3 connect"
    );
    assert!(exists(cx, "connect-command"));
    let link = "monocode://pair?v=1&id=env-2";
    fill(cx, "input:Pairing link", link);
    click(cx, "button:Pair");
    assert!(fake.called(&Call::Pair {
        link: link.into(),
        name: String::new(),
    }));
    assert!(read(&view, cx, |view, _| view.notice().to_string()).contains("Studio is connected"));
    assert!(!exists(cx, "input:Pairing link"));
}

#[gpui::test]
fn shows_a_paired_machines_address_and_retries_every_route_when_it_is_offline(
    cx: &mut TestAppContext,
) {
    let fake = FakeRemote::new();
    fake.0.borrow_mut().machines = vec![direct_machine()];
    fake.set_respond(|_, _, _| Err("Machine is unreachable. 10.0.0.2:3774 did not answer".into()));
    let (view, cx) = render(cx, &fake);
    assert_eq!(
        crate::machines::route_label(&direct_machine()),
        "10.0.0.2:3774 (+1)"
    );
    assert!(exists(cx, "route:direct"));
    let label = read(&view, cx, |view, _| view.status_label("direct"));
    assert!(
        label.contains("Offline · Machine is unreachable"),
        "{label}"
    );
    assert!(!exists(cx, "button:Reconnect"));
    fake.set_respond(|_, _, _| {
        Ok(serde_json::json!({ "environmentId": "env-2", "providers": ["codex"] }))
    });
    click(cx, "button:Retry");
    poll(cx);
    assert!(fake.called(&Call::Retry("direct".into())));
    let label = read(&view, cx, |view, _| view.status_label("direct"));
    assert!(label.contains("Connected · host older than 0.5"), "{label}");
    // Without SSH, updating happens on the machine itself.
    assert!(read(&view, cx, |view, _| view
        .status("direct")
        .is_some_and(|status| status.needs_update(Some("1.2.3")))));
    assert_eq!(
        read(&view, cx, |view, _| view.command()),
        "npx monocode-host@1.2.3 connect"
    );
}

#[gpui::test]
fn names_the_machine_in_a_plain_field(cx: &mut TestAppContext) {
    // GPUI fields never spellcheck, autocorrect, or capitalize, which the
    // React test checked through input attributes.
    let fake = FakeRemote::new();
    let (view, cx) = render(cx, &fake);
    click(cx, "button:Add machine");
    assert!(exists(cx, "input:Name"));
    fill(cx, "input:Name", "Home Mac mini");
    assert_eq!(
        read(&view, cx, |view, cx| view.value(Field::Name, cx)),
        "Home Mac mini"
    );
}

#[gpui::test]
fn checks_machines_again_every_ten_seconds_until_setup_starts(cx: &mut TestAppContext) {
    let fake = FakeRemote::new();
    fake.0.borrow_mut().machines = vec![direct_machine()];
    let (_view, cx) = render(cx, &fake);
    let describes = |fake: &FakeRemote| {
        fake.calls()
            .iter()
            .filter(|call| matches!(call, Call::Request { method, .. } if method == "environment.describe"))
            .count()
    };
    let first = describes(&fake);
    assert!(first >= 1);
    advance(cx, Duration::from_secs(10));
    assert!(describes(&fake) > first);
    draw(cx);
}
