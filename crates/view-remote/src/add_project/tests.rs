//! Ports of AddRemoteProjectDialog.test.ts.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use monocode_layout::paths::is_remote_project_path;
use monocode_remote::host::protocol::RemoteMachine;
use serde_json::{Value, json};

use super::{AddRemoteProjectDialog, AddRemoteProjectEvent};
use crate::fake::FakeRemote;
use crate::test_support::{click, exists, mount};

fn home_server() -> RemoteMachine {
    RemoteMachine {
        id: "machine".into(),
        name: "Home server".into(),
        endpoint: "ssh://home".into(),
        endpoints: None,
        environment_id: "env".into(),
        ssh: None,
    }
}

fn browse(params: &Value) -> Value {
    match params.get("path").and_then(Value::as_str) {
        Some("/home/me/code/app") => json!({
            "path": "/home/me/code/app",
            "parent": "/home/me/code",
            "entries": [],
        }),
        Some("/home/me/code") => json!({
            "path": "/home/me/code",
            "parent": "/home/me",
            "entries": [{ "name": "app", "path": "/home/me/code/app" }],
        }),
        _ => json!({
            "path": "/home/me",
            "parent": "/home",
            "entries": [{ "name": "code", "path": "/home/me/code" }],
        }),
    }
}

fn fake(machines: Vec<RemoteMachine>) -> FakeRemote {
    let fake = FakeRemote::new();
    fake.0.borrow_mut().machines = machines;
    fake.set_respond(|_, method, params| match method {
        "projects.browse" => Ok(browse(params)),
        "projects.open" => Ok(json!({ "id": "host-project", "cwd": params["cwd"], "name": "app" })),
        other => Err(format!("Unexpected {other}")),
    });
    fake
}

type Events = Rc<RefCell<Vec<AddRemoteProjectEvent>>>;

fn render<'a>(
    cx: &'a mut TestAppContext,
    fake: &FakeRemote,
) -> (
    Entity<AddRemoteProjectDialog>,
    &'a mut VisualTestContext,
    Events,
) {
    let host = fake.clone();
    let (view, cx) = mount(cx, move |window, cx| {
        cx.new(|cx| AddRemoteProjectDialog::new(Rc::new(host), window, cx))
    });
    let events: Events = Rc::default();
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &AddRemoteProjectEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach()
    });
    crate::test_support::draw(cx);
    (view, cx, events)
}

fn opened(events: &Events) -> Vec<String> {
    events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            AddRemoteProjectEvent::Open(key) => Some(key.clone()),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn browses_a_machines_folders_and_adds_the_chosen_one_as_a_project(cx: &mut TestAppContext) {
    let fake = fake(vec![home_server()]);
    let (view, cx, events) = render(cx, &fake);
    click(cx, "folder:code");
    click(cx, "folder:app");
    assert_eq!(
        view.read_with(cx, |view, cx| view.path(cx)),
        "/home/me/code/app"
    );
    assert!(exists(cx, "input:Folder path on the machine"));
    click(cx, "button:Open");
    let keys = opened(&events);
    assert_eq!(keys, vec!["remote://env/home/me/code/app".to_string()]);
    assert!(fake.called(&crate::fake::Call::Request {
        machine_id: "machine".into(),
        method: "projects.open".into(),
        params: json!({ "cwd": "/home/me/code/app" }),
    }));
    // It is a rail project, but never a folder on this computer.
    assert!(is_remote_project_path(&keys[0]));
}

#[gpui::test]
fn points_to_settings_when_no_machine_is_connected(cx: &mut TestAppContext) {
    let fake = fake(Vec::new());
    let (_view, cx, events) = render(cx, &fake);
    assert!(exists(cx, "no-machines"));
    assert!(exists(cx, "button:Add a machine"));
    click(cx, "button:Add a machine");
    assert_eq!(
        *events.borrow(),
        vec![
            AddRemoteProjectEvent::Cancel,
            AddRemoteProjectEvent::OpenConnections
        ]
    );
}

#[gpui::test]
fn ignores_a_project_that_finishes_opening_after_cancellation(cx: &mut TestAppContext) {
    let fake = fake(vec![home_server()]);
    fake.hold("projects.open");
    let (view, cx, events) = render(cx, &fake);
    click(cx, "button:Open");
    assert!(view.read_with(cx, |view, _| view.is_opening()));
    click(cx, "button:Cancel");
    fake.finish(
        "projects.open",
        Ok(json!({ "id": "late", "cwd": "/home/me", "name": "me" })),
    );
    cx.run_until_parked();
    assert!(opened(&events).is_empty());
    assert_eq!(*events.borrow(), vec![AddRemoteProjectEvent::Cancel]);
}

#[gpui::test]
fn escape_cancels_the_dialog(cx: &mut TestAppContext) {
    let fake = fake(vec![home_server()]);
    let (_view, cx, events) = render(cx, &fake);
    crate::test_support::keys(cx, "escape");
    assert_eq!(*events.borrow(), vec![AddRemoteProjectEvent::Cancel]);
}

#[gpui::test]
fn lists_machines_to_choose_from_when_there_are_several(cx: &mut TestAppContext) {
    let mut second = home_server();
    second.id = "studio".into();
    second.name = "Studio".into();
    second.environment_id = "env-2".into();
    let fake = fake(vec![home_server(), second]);
    let (view, cx, _events) = render(cx, &fake);
    assert!(view.read_with(cx, |view, _| view.select.is_some()));
    view.update(cx, |view, cx| view.set_machine("studio", cx));
    crate::test_support::draw(cx);
    assert!(fake.called(&crate::fake::Call::Request {
        machine_id: "studio".into(),
        method: "projects.browse".into(),
        params: json!({}),
    }));
}
