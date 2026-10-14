//! Semantic layer against the fake CDP server (`browser::testing`): a small
//! page model answers the AX, DOM, Runtime and Input commands the layer
//! sends, and the tests check both the results and the exact command trail.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::super::link::{CdpSessionId, Link, TargetId};
use super::super::policy::{FrameSession, Lease, Policy, PolicyState};
use super::super::protocol::{
    ActionKind, ActionResult, BrowserSettings, CommonInput, DialogType, LoadState, Outcome, Reason,
    TabId, TabState,
};
use super::super::targets::{self, TabHandle};
use super::super::testing::{default_response, Event, FakeCdp, Received, Response};
use super::{execute, Exec};

const MAIN: &str = "S1";
const CHILD: &str = "S9";

fn ax(id: &str, role: &str, name: &str, backend: i64, children: &[&str]) -> Value {
    json!({
        "nodeId": id,
        "ignored": false,
        "role": { "type": "role", "value": role },
        "name": { "type": "computedString", "value": name },
        "backendDOMNodeId": backend,
        "childIds": children,
    })
}

/// What the fake page answers.
struct Model {
    facts: HashMap<(String, i64), Value>,
    removed: HashSet<i64>,
    hits: Vec<Value>,
    options: Vec<String>,
    page_text: String,
    read_text: String,
    scroll_y: f64,
    /// Events emitted after `mouseReleased` is answered.
    on_release: Vec<Event>,
    /// Never answer `mouseReleased` (a dialog opened by the click handler).
    hold_release: bool,
    history_index: i64,
    /// What `monocodeCursor("move")` reports as the glide time.
    glide_ms: u64,
}

impl Default for Model {
    fn default() -> Self {
        Model {
            facts: HashMap::new(),
            removed: HashSet::new(),
            hits: Vec::new(),
            options: vec!["s".into(), "m".into(), "l".into()],
            page_text: "Welcome".into(),
            read_text: "abcdefghij".into(),
            scroll_y: 0.0,
            on_release: Vec::new(),
            hold_release: false,
            history_index: 1,
            glide_ms: 0,
        }
    }
}

fn result(value: Value) -> Response {
    Response::Result(json!({ "result": { "type": "object", "value": value } }))
}

fn respond(
    model: &Mutex<Model>,
    received: &Received,
    counter: &std::sync::atomic::AtomicU64,
) -> Response {
    let mut model = model.lock().unwrap();
    let session = received.session.clone().unwrap_or_default();
    let params = &received.params;
    match received.method.as_str() {
        "Accessibility.getFullAXTree" if session == MAIN => Response::Result(json!({ "nodes": [
            ax("1", "RootWebArea", "\u{25C6} MonoCode \u{B7} Shop", 1, &["2", "3", "4", "5", "6", "7"]),
            ax("2", "button", "Save", 11, &[]),
            ax("3", "textbox", "Email", 12, &[]),
            ax("4", "Iframe", "", 13, &[]),
            ax("5", "combobox", "Size", 14, &[]),
            ax("6", "button", "Upload", 15, &[]),
            ax("7", "button", "Choose file", 16, &[]),
        ]})),
        "Accessibility.getFullAXTree" => Response::Result(json!({ "nodes": [
            ax("c1", "RootWebArea", "Ad", 1, &["c2"]),
            ax("c2", "link", "Buy", 5, &[]),
        ]})),
        "DOM.describeNode" => Response::Result(json!({ "node": { "frameId": "OOPIF1" } })),
        "Page.createIsolatedWorld" => Response::Result(json!({ "executionContextId": 70 })),
        "DOM.resolveNode" => {
            let backend = params["backendNodeId"].as_i64().unwrap_or(0);
            if model.removed.contains(&backend) {
                Response::Error {
                    code: -32000,
                    message: "No node with given id found".into(),
                }
            } else {
                Response::Result(
                    json!({ "object": { "objectId": format!("{session}:{backend}") } }),
                )
            }
        }
        "Runtime.callFunctionOn" => {
            let function = params["functionDeclaration"].as_str().unwrap_or("");
            let args: Vec<Value> = params["arguments"]
                .as_array()
                .map(|a| a.iter().map(|v| v["value"].clone()).collect())
                .unwrap_or_default();
            let object = params["objectId"].as_str().unwrap_or("");
            let backend: i64 = object
                .split(':')
                .nth(1)
                .and_then(|b| b.parse().ok())
                .unwrap_or(0);
            if function.contains("function monocodeActionability(") {
                let facts = model
                    .facts
                    .get(&(session.clone(), backend))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let mut base = json!({
                    "connected": true, "visible": true, "enabled": true, "editable": false,
                    "text_like": false, "file_input": false, "multiple": false, "select": false,
                    "tag": "button", "rect": { "x": 10, "y": 10, "width": 100, "height": 30 },
                });
                for (k, v) in facts.as_object().unwrap() {
                    base[k] = v.clone();
                }
                return result(base);
            }
            if function.contains("function monocodeHitTest(") {
                if model.hits.is_empty() {
                    return result(json!({ "status": "ok" }));
                }
                return result(model.hits.remove(0));
            }
            if function.contains("function monocodeSelectAll(") {
                return result(json!({ "ok": true, "length": 3 }));
            }
            if function.contains("function monocodeSelect(") {
                let wanted: Vec<String> = serde_json::from_value(args[0].clone()).unwrap();
                let missing: Vec<&String> = wanted
                    .iter()
                    .filter(|v| !model.options.contains(v))
                    .collect();
                if !missing.is_empty() {
                    return result(json!({ "status": "missing", "missing": missing,
                        "available": [{ "value": "s", "label": "Small" }] }));
                }
                return result(json!({ "status": "ok", "selected": wanted }));
            }
            if function.contains("function monocodePage(") {
                return match args[0].as_str().unwrap_or("") {
                    "mutations" => result(json!({ "count": 0 })),
                    "text" => result(json!({
                        "found": model.page_text.contains(args[1].as_str().unwrap_or("\u{0}"))
                    })),
                    "ready" => result(json!({ "state": "complete" })),
                    "scroll" => {
                        if let Some(dy) = args.get(1).and_then(|v| v["dy"].as_f64()) {
                            model.scroll_y += dy;
                        }
                        result(json!({ "x": 0, "y": model.scroll_y }))
                    }
                    _ => result(Value::Null),
                };
            }
            if function.contains("function monocodeCursor(") {
                return match args[0].as_str().unwrap_or("") {
                    "move" if args[5] == json!(true) => result(json!({ "ms": 0 })),
                    "move" => result(json!({ "ms": model.glide_ms })),
                    _ => result(json!({ "ok": true })),
                };
            }
            if function.contains("function monocodeRead(") {
                let offset = args[1].as_u64().unwrap_or(0) as usize;
                let limit = args[2].as_u64().unwrap_or(0) as usize;
                let text: String = model.read_text.chars().skip(offset).take(limit).collect();
                return result(json!({ "text": text, "total": model.read_text.chars().count() }));
            }
            result(Value::Null)
        }
        "DOM.getContentQuads" => Response::Result(json!({
            "quads": [[10.0, 10.0, 110.0, 10.0, 110.0, 40.0, 10.0, 40.0]]
        })),
        "DOM.getBoxModel" => Response::Result(json!({ "model": {
            "content": [100.0, 200.0, 400.0, 200.0, 400.0, 500.0, 100.0, 500.0],
            "border": [10.0, 10.0, 110.0, 10.0, 110.0, 40.0, 10.0, 40.0],
        }})),
        "Page.getLayoutMetrics" => Response::Result(json!({
            "cssLayoutViewport": { "pageX": 0, "pageY": 50, "clientWidth": 800, "clientHeight": 600 },
            "cssContentSize": { "x": 0, "y": 0, "width": 800, "height": 3000 },
        })),
        "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" && model.hold_release => {
            Response::Hold
        }
        "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
            let events = std::mem::take(&mut model.on_release);
            Response::ResultThen(json!({}), events)
        }
        "Page.captureScreenshot" => Response::Result(json!({ "data": "iVBORw0KGgoAAAANSUhEUg==" })),
        "Page.getNavigationHistory" => Response::Result(json!({
            "currentIndex": model.history_index,
            "entries": [
                { "id": 1, "url": "https://example.com/a", "title": "A" },
                { "id": 2, "url": "https://example.com/b", "title": "B" },
            ],
        })),
        "Page.navigateToHistoryEntry" | "Page.navigate" => {
            let url = params["url"]
                .as_str()
                .unwrap_or("https://example.com/a")
                .to_string();
            Response::ResultThen(
                json!({ "frameId": "F1", "loaderId": "L9" }),
                vec![
                    Event::new(
                        "Page.frameNavigated",
                        json!({ "frame": { "id": "F1", "loaderId": "L9", "url": url } }),
                        Some(MAIN),
                    ),
                    Event::new(
                        "Page.lifecycleEvent",
                        json!({ "frameId": "F1", "loaderId": "L9", "name": "load" }),
                        Some(MAIN),
                    ),
                ],
            )
        }
        _ => default_response(received, counter),
    }
}

struct Harness {
    server: Arc<FakeCdp>,
    link: Link,
    policy: Arc<Policy>,
    handle: TabHandle,
    model: Arc<Mutex<Model>>,
    settings: BrowserSettings,
    cwd: PathBuf,
    next_id: Mutex<u64>,
}

impl Harness {
    fn new() -> Harness {
        let cwd =
            std::env::temp_dir().join(format!("monocode-sem-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&cwd).unwrap();
        let model = Arc::new(Mutex::new(Model::default()));
        let counter = Arc::new(std::sync::atomic::AtomicU64::new(100));
        let shared = model.clone();
        let server = Arc::new(FakeCdp::start_with(move |received| {
            respond(&shared, received, &counter)
        }));
        let mut state = PolicyState::new(3, BrowserSettings::default());
        state.set_toggle(true);
        state.turn_started("s1", "main", cwd.to_str().unwrap(), Some(true));
        let tab = state.new_tab_id();
        let mut frames = HashMap::new();
        frames.insert(
            "OOPIF1".to_string(),
            FrameSession {
                cdp_session: CdpSessionId(CHILD.into()),
                loader_id: None,
                execution_context_id: None,
            },
        );
        state.insert_lease(Lease {
            tab: tab.clone(),
            target_id: TargetId("T1".into()),
            cdp_session: Some(CdpSessionId(MAIN.into())),
            owner: "s1".into(),
            state: TabState::Open,
            main_frame_id: Some("F1".into()),
            main_loader_id: Some("L1".into()),
            main_execution_context: None,
            frames,
            busy: false,
            snapshot_version: 0,
            pending_dialog: None,
            opened_by: None,
            popups_blocked: 0,
            url: "https://example.com/".into(),
            title: "\u{25C6} MonoCode \u{B7} Shop".into(),
            created_at_ms: 0,
            last_action_at_ms: 0,
        });
        let policy = Arc::new(Policy::new(state));
        let link = Link::new(policy.clone());
        link.open(&server.url, Duration::from_secs(5)).unwrap();
        targets::spawn_pump(link.clone(), Arc::new(|_| {})).unwrap();
        Harness {
            server,
            link,
            policy,
            handle: TabHandle {
                tab,
                target: TargetId("T1".into()),
                session: CdpSessionId(MAIN.into()),
            },
            model,
            settings: BrowserSettings::default(),
            cwd,
            next_id: Mutex::new(0),
        }
    }

    fn tab(&self) -> &TabId {
        &self.handle.tab
    }

    fn act(&self, kind: ActionKind, mut input: Value) -> ActionResult {
        input["tab"] = json!(self.tab());
        let id = {
            let mut next = self.next_id.lock().unwrap();
            *next += 1;
            format!("r{next}")
        };
        let common: CommonInput = serde_json::from_value(input.clone()).unwrap_or_default();
        let admitted = self.policy.read().admit(
            "s1",
            &id,
            kind,
            Some(self.tab()),
            kind.effective_timeout(common.timeout_ms),
        );
        let ctx = match admitted {
            Ok(ctx) => ctx,
            Err(reason) => return ActionResult::from_reason(&id, reason),
        };
        execute(Exec {
            link: &self.link,
            ctx: &ctx,
            kind,
            input: &input,
            handle: Some(&self.handle),
            settings: &self.settings,
            commands_before: self.link.commands_sent(),
        })
    }

    fn sent(&self) -> Vec<Received> {
        self.server.received()
    }

    fn mark(&self) -> usize {
        self.sent().len()
    }

    fn since(&self, mark: usize) -> Vec<Received> {
        self.sent().into_iter().skip(mark).collect()
    }

    fn snapshot(&self) -> String {
        let shot = self.act(ActionKind::Snapshot, json!({}));
        assert_eq!(shot.outcome, Outcome::Done, "{shot:?}");
        shot.data.unwrap()["snapshot"].as_str().unwrap().to_string()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.link.close(Vec::new(), Duration::from_millis(200));
        let _ = std::fs::remove_dir_all(&self.cwd);
    }
}

fn methods(log: &[Received]) -> Vec<String> {
    log.iter().map(|r| r.method.clone()).collect()
}

fn mouse(log: &[Received]) -> Vec<Received> {
    log.iter()
        .filter(|r| r.method == "Input.dispatchMouseEvent")
        .cloned()
        .collect()
}

#[test]
fn snapshot_covers_oopifs_and_click_goes_through_the_pipeline() {
    let h = Harness::new();
    let text = h.snapshot();
    assert!(
        text.starts_with("url: https://example.com/\ntitle: \"Shop\"\nsnapshot: v1, 9 refs\n"),
        "{text}"
    );
    assert!(text.contains("- document \"Shop\" [ref=e1]"), "{text}");
    assert!(text.contains("  - button \"Save\" [ref=e2]"), "{text}");
    assert!(text.contains("  - iframe [frame] [ref=e4]"), "{text}");
    assert!(text.contains("    - document \"Ad\" [ref=e5]"), "{text}");
    assert!(text.contains("      - link \"Buy\" [ref=e6]"), "{text}");
    assert!(!text.contains("MonoCode"), "marker leaked: {text}");
    // The OOPIF tree came from the child session.
    assert!(h
        .sent()
        .iter()
        .any(|r| r.method == "Accessibility.getFullAXTree" && r.session.as_deref() == Some(CHILD)));
    assert_eq!(h.policy.read().lease(h.tab()).unwrap().snapshot_version, 1);

    let mark = h.mark();
    let click = h.act(ActionKind::Click, json!({ "ref": "e2" }));
    assert_eq!(click.outcome, Outcome::Done, "{click:?}");
    assert!(
        click.message.starts_with("Clicked e2 (button \"Save\")."),
        "{}",
        click.message
    );
    let log = h.since(mark);
    let names = methods(&log);
    // Never Runtime.evaluate; resolve → checks → scroll → quads → hit test → input.
    assert!(!names.iter().any(|m| m == "Runtime.evaluate"));
    let first_input = names
        .iter()
        .position(|m| m == "Input.dispatchMouseEvent")
        .unwrap();
    for before in [
        "DOM.resolveNode",
        "DOM.scrollIntoViewIfNeeded",
        "DOM.getContentQuads",
    ] {
        let at = names.iter().position(|m| m == before).unwrap();
        assert!(at < first_input, "{before} after input: {names:?}");
    }
    let events = mouse(&log);
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| e.params["type"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["mouseMoved", "mousePressed", "mouseReleased"]);
    assert_eq!(events[1].params["x"], json!(60.0));
    assert_eq!(events[1].params["y"], json!(25.0));
    assert_eq!(events[1].params["button"], json!("left"));
    assert_eq!(events[1].params["clickCount"], json!(1));
    assert!(events.iter().all(|e| e.session.as_deref() == Some(MAIN)));
    // Every script ran in our isolated world, never in the page's.
    for call in log.iter().filter(|r| r.method == "Runtime.callFunctionOn") {
        assert!(
            call.params.get("objectId").is_some() || call.params["executionContextId"] == json!(70),
            "{call:?}"
        );
    }
    assert!(log
        .iter()
        .all(|r| r.method != "Page.createIsolatedWorld"
            || r.params["worldName"] == json!("__monocode")));

    // A link inside the OOPIF: DOM work in the child session, input in the
    // main one, shifted by the <iframe>'s content box.
    let mark = h.mark();
    let click = h.act(
        ActionKind::Click,
        json!({ "ref": "e6", "count": 2, "modifiers": ["Shift"] }),
    );
    assert_eq!(click.outcome, Outcome::Done, "{click:?}");
    let log = h.since(mark);
    let quads = log
        .iter()
        .find(|r| r.method == "DOM.getContentQuads")
        .unwrap();
    assert_eq!(quads.session.as_deref(), Some(CHILD));
    let owner = log.iter().find(|r| r.method == "DOM.getBoxModel").unwrap();
    assert_eq!(owner.session.as_deref(), Some(MAIN));
    assert_eq!(owner.params["backendNodeId"], json!(13));
    let events = mouse(&log);
    assert_eq!(events.len(), 5);
    assert_eq!(events[1].params["x"], json!(160.0));
    assert_eq!(events[1].params["y"], json!(225.0));
    assert_eq!(events[1].params["modifiers"], json!(8));
    assert_eq!(events[3].params["clickCount"], json!(2));
    assert!(events.iter().all(|e| e.session.as_deref() == Some(MAIN)));
    // Refs are stable across snapshots of the same document.
    let again = h.snapshot();
    assert!(again.contains("snapshot: v2, 9 refs"));
    assert!(again.contains("button \"Save\" [ref=e2]"));
    let scoped = h.act(ActionKind::Snapshot, json!({ "scope": "e4" }));
    let scoped_text = scoped.data.unwrap()["snapshot"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        scoped_text.contains("- iframe [frame] [ref=e4]"),
        "{scoped_text}"
    );
    assert!(!scoped_text.contains("Save"), "{scoped_text}");
}

#[test]
fn stale_and_bad_refs_fail_before_any_command() {
    let h = Harness::new();
    let mark = h.mark();
    let early = h.act(ActionKind::Click, json!({ "ref": "e2" }));
    assert_eq!(early.reason, Some(Reason::RefStale));
    assert_eq!(early.outcome, Outcome::Failed);
    let bad = h.act(ActionKind::Click, json!({ "ref": "#save" }));
    assert_eq!(bad.reason, Some(Reason::InvalidRequest));
    assert_eq!(bad.outcome, Outcome::Blocked);
    let typo = h.act(ActionKind::Click, json!({ "ref": "e2", "selector": "x" }));
    assert_eq!(typo.reason, Some(Reason::InvalidRequest));
    assert!(h.since(mark).is_empty());

    h.snapshot();
    h.snapshot();
    h.snapshot();
    let mark = h.mark();
    // Only the last two snapshot versions may be cited.
    let old = h.act(ActionKind::Hover, json!({ "ref": "e2@1" }));
    assert_eq!(old.reason, Some(Reason::RefStale));
    let unknown = h.act(ActionKind::Hover, json!({ "ref": "e99" }));
    assert_eq!(unknown.reason, Some(Reason::RefStale));
    assert!(h.since(mark).is_empty());
    let cited = h.act(ActionKind::Hover, json!({ "ref": "e2@2" }));
    assert_eq!(cited.outcome, Outcome::Done, "{cited:?}");

    // The node left the DOM: resolveNode says so, nothing is clicked.
    h.model.lock().unwrap().removed.insert(11);
    let mark = h.mark();
    let gone = h.act(ActionKind::Click, json!({ "ref": "e2" }));
    assert_eq!(gone.reason, Some(Reason::RefStale), "{gone:?}");
    assert_eq!(gone.outcome, Outcome::Failed);
    assert!(mouse(&h.since(mark)).is_empty());

    // The main frame navigated to a new document: every ref is stale.
    h.policy.write().lease_mut(h.tab()).unwrap().main_loader_id = Some("L2".into());
    let mark = h.mark();
    let navigated = h.act(ActionKind::Click, json!({ "ref": "e3" }));
    assert_eq!(navigated.reason, Some(Reason::RefStale));
    assert!(navigated.message.contains("navigated"));
    assert!(h.since(mark).is_empty());
}

#[test]
fn actionability_failures_never_send_input() {
    let h = Harness::new();
    h.snapshot();
    {
        let mut model = h.model.lock().unwrap();
        model
            .facts
            .insert((MAIN.into(), 11), json!({ "visible": false }));
        model
            .facts
            .insert((MAIN.into(), 15), json!({ "enabled": false }));
        model
            .facts
            .insert((MAIN.into(), 16), json!({ "connected": false }));
    }
    let mark = h.mark();
    let hidden = h.act(ActionKind::Click, json!({ "ref": "e2" }));
    assert_eq!(hidden.reason, Some(Reason::NotVisible));
    let disabled = h.act(ActionKind::Click, json!({ "ref": "e8" }));
    assert_eq!(disabled.reason, Some(Reason::NotEnabled));
    let detached = h.act(ActionKind::Click, json!({ "ref": "e9" }));
    assert_eq!(detached.reason, Some(Reason::Detached));
    let not_text = h.act(ActionKind::Type, json!({ "ref": "e7", "text": "x" }));
    assert_eq!(not_text.reason, Some(Reason::NotEditable));
    for result in [&hidden, &disabled, &detached, &not_text] {
        assert_eq!(result.outcome, Outcome::Failed, "{result:?}");
    }
    // Covered three times in a row: obscured, with what covers it.
    {
        let mut model = h.model.lock().unwrap();
        model.facts.clear();
        for _ in 0..3 {
            model
                .hits
                .push(json!({ "status": "obscured", "by": "div#cookie \"Accept all\"" }));
        }
    }
    let covered = h.act(ActionKind::Click, json!({ "ref": "e2" }));
    assert_eq!(covered.reason, Some(Reason::Obscured));
    assert!(
        covered.message.contains("div#cookie"),
        "{}",
        covered.message
    );
    assert!(mouse(&h.since(mark)).is_empty());
    // Covered once, then gone: the click goes through.
    h.model
        .lock()
        .unwrap()
        .hits
        .push(json!({ "status": "obscured", "by": "div.toast" }));
    let retried = h.act(ActionKind::Click, json!({ "ref": "e2" }));
    assert_eq!(retried.outcome, Outcome::Done, "{retried:?}");
}

#[test]
fn type_press_and_select_send_the_spec_sequences() {
    let h = Harness::new();
    h.snapshot();
    h.model.lock().unwrap().facts.insert(
        (MAIN.into(), 12),
        json!({ "editable": true, "text_like": true }),
    );
    let secret = "correct horse battery staple";
    let mark = h.mark();
    let typed = h.act(
        ActionKind::Type,
        json!({ "ref": "e3", "text": secret, "clear": true, "submit": true }),
    );
    assert_eq!(typed.outcome, Outcome::Done, "{typed:?}");
    assert!(
        !typed.message.contains("horse"),
        "text leaked: {}",
        typed.message
    );
    assert!(
        typed.message.contains("Typed 28 characters"),
        "{}",
        typed.message
    );
    let log = h.since(mark);
    let names = methods(&log);
    let focus = names.iter().position(|m| m == "DOM.focus").unwrap();
    let insert = names.iter().position(|m| m == "Input.insertText").unwrap();
    assert!(focus < insert);
    assert_eq!(log[insert].params["text"], json!(secret));
    assert_eq!(log[insert].session.as_deref(), Some(MAIN));
    let select_all = log
        .iter()
        .position(|r| {
            r.method == "Runtime.callFunctionOn"
                && r.params["functionDeclaration"]
                    .as_str()
                    .unwrap()
                    .contains("monocodeSelectAll")
        })
        .unwrap();
    assert!(focus < select_all && select_all < insert);
    let keys: Vec<&Received> = log
        .iter()
        .filter(|r| r.method == "Input.dispatchKeyEvent")
        .collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].params["type"], json!("keyDown"));
    assert_eq!(keys[0].params["text"], json!("\r"));
    assert_eq!(keys[1].params["type"], json!("keyUp"));

    // Long text goes in 4000-character chunks.
    let mark = h.mark();
    let long = "y".repeat(9000);
    let typed = h.act(ActionKind::Type, json!({ "ref": "e3", "text": long }));
    assert_eq!(typed.outcome, Outcome::Done);
    let chunks: Vec<usize> = h
        .since(mark)
        .iter()
        .filter(|r| r.method == "Input.insertText")
        .map(|r| r.params["text"].as_str().unwrap().len())
        .collect();
    assert_eq!(chunks, vec![4000, 4000, 1000]);

    // press: modifiers as a bit mask, no text for shortcuts.
    let mark = h.mark();
    let pressed = h.act(ActionKind::Press, json!({ "key": "Control+a" }));
    assert_eq!(pressed.outcome, Outcome::Done, "{pressed:?}");
    let keys: Vec<Received> = h
        .since(mark)
        .into_iter()
        .filter(|r| r.method == "Input.dispatchKeyEvent")
        .collect();
    assert_eq!(keys[0].params["type"], json!("rawKeyDown"));
    assert_eq!(keys[0].params["modifiers"], json!(2));
    assert_eq!(keys[0].params["code"], json!("KeyA"));
    assert!(keys[0].params.get("text").is_none());
    let mark = h.mark();
    let unknown = h.act(ActionKind::Press, json!({ "key": "Hyper+Q" }));
    assert_eq!(unknown.reason, Some(Reason::InvalidRequest));
    assert_eq!(unknown.outcome, Outcome::Blocked);
    assert!(h.since(mark).is_empty());

    // select: all-or-nothing, through our page function.
    h.model.lock().unwrap().facts.insert(
        (MAIN.into(), 14),
        json!({ "select": true, "tag": "select" }),
    );
    let picked = h.act(ActionKind::Select, json!({ "ref": "e7", "values": ["m"] }));
    assert_eq!(picked.outcome, Outcome::Done, "{picked:?}");
    assert_eq!(picked.data.unwrap()["selected"], json!(["m"]));
    let missing = h.act(
        ActionKind::Select,
        json!({ "ref": "e7", "values": ["xxl"] }),
    );
    assert_eq!(missing.reason, Some(Reason::OptionNotFound));
    assert_eq!(missing.outcome, Outcome::Failed);
    assert!(missing.message.contains("xxl"));
    let not_select = h.act(ActionKind::Select, json!({ "ref": "e2", "values": ["m"] }));
    assert_eq!(not_select.reason, Some(Reason::OptionNotFound));
}

#[test]
fn dialogs_are_reported_never_answered_until_asked() {
    let h = Harness::new();
    h.snapshot();
    // The click handler opens confirm(): the browser holds the
    // mouseReleased answer until the dialog is gone.
    let server = h.server.clone();
    let opener = std::thread::spawn(move || {
        assert!(server.wait_for(
            |log| log
                .iter()
                .any(|r| r.params["type"] == json!("mouseReleased")),
            Duration::from_secs(5)
        ));
        server.emit(
            "Page.javascriptDialogOpening",
            json!({ "url": "https://example.com/", "message": "Delete 3 items?", "type": "confirm", "hasBrowserHandler": false }),
            Some(MAIN),
        );
    });
    {
        // Hold the answer to mouseReleased like Chrome does.
        h.model.lock().unwrap().hold_release = true;
    }
    let started = std::time::Instant::now();
    let click = h.act(ActionKind::Click, json!({ "ref": "e2" }));
    opener.join().unwrap();
    assert_eq!(click.outcome, Outcome::Done, "{click:?}");
    let dialog = click.observed.dialog.clone().expect("dialog reported");
    assert_eq!(dialog.kind, DialogType::Confirm);
    assert_eq!(dialog.message, "Delete 3 items?");
    assert!(click.message.contains("browser_dialog"));
    assert!(started.elapsed() < Duration::from_secs(5));
    // Nothing answered it on its own.
    assert!(!h
        .sent()
        .iter()
        .any(|r| r.method == "Page.handleJavaScriptDialog"));
    // The pump recorded it; any other action is blocked by admission.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while h
        .policy
        .read()
        .lease(h.tab())
        .unwrap()
        .pending_dialog
        .is_none()
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    let blocked = h.act(ActionKind::Snapshot, json!({}));
    assert_eq!(blocked.reason, Some(Reason::DialogPending));
    let answered = h.act(ActionKind::Dialog, json!({ "action": "dismiss" }));
    assert_eq!(answered.outcome, Outcome::Done, "{answered:?}");
    let sent = h
        .sent()
        .into_iter()
        .find(|r| r.method == "Page.handleJavaScriptDialog")
        .unwrap();
    assert_eq!(sent.params, json!({ "accept": false }));
    assert_eq!(answered.data.unwrap()["type"], json!("confirm"));
    let none = h.act(ActionKind::Dialog, json!({ "action": "accept" }));
    assert_eq!(none.reason, Some(Reason::NoDialog));
    assert_eq!(none.outcome, Outcome::Failed);
}

#[test]
fn click_that_navigates_waits_for_load_and_invalidates_refs() {
    let h = Harness::new();
    h.snapshot();
    h.model.lock().unwrap().on_release = vec![
        Event::new(
            "Page.frameRequestedNavigation",
            json!({ "frameId": "F1", "reason": "anchorClick", "url": "https://example.com/next", "disposition": "currentTab" }),
            Some(MAIN),
        ),
        Event::new(
            "Page.frameNavigated",
            json!({ "frame": { "id": "F1", "loaderId": "L5", "url": "https://example.com/next" } }),
            Some(MAIN),
        ),
        Event::new(
            "Page.lifecycleEvent",
            json!({ "frameId": "F1", "loaderId": "L5", "name": "load" }),
            Some(MAIN),
        ),
    ];
    let click = h.act(ActionKind::Click, json!({ "ref": "e2" }));
    assert_eq!(click.outcome, Outcome::Done, "{click:?}");
    assert!(click.observed.navigated);
    assert_eq!(click.observed.load_state, Some(LoadState::Load));
    assert!(click.observed.snapshot_stale);
    assert_eq!(
        click.observed.url.as_deref(),
        Some("https://example.com/next")
    );
    let mark = h.mark();
    let stale = h.act(ActionKind::Click, json!({ "ref": "e2" }));
    assert_eq!(stale.reason, Some(Reason::RefStale));
    assert!(h.since(mark).is_empty());
}

#[test]
fn popups_from_a_click_are_reported_as_new_tabs() {
    let h = Harness::new();
    h.snapshot();
    h.model.lock().unwrap().on_release = vec![Event::new(
        "Target.attachedToTarget",
        json!({ "sessionId": "P1", "targetInfo": { "targetId": "TP", "type": "page", "url": "https://popup.example/", "title": "", "attached": true, "openerId": "T1" }, "waitingForDebugger": true }),
        Some(MAIN),
    )];
    let click = h.act(ActionKind::Click, json!({ "ref": "e2" }));
    assert_eq!(click.outcome, Outcome::Done, "{click:?}");
    let new_tab = click.observed.new_tab.clone().expect("new tab reported");
    assert!(new_tab.tab.is_well_formed());
    assert_eq!(new_tab.url, "https://popup.example/");
    assert!(click.message.contains(&new_tab.tab.0));
    assert!(!click.observed.popup_blocked);
}

#[test]
fn upload_checks_paths_first_and_uses_the_file_input_or_the_chooser() {
    let h = Harness::new();
    h.snapshot();
    let file = h.cwd.join("cv.pdf");
    std::fs::write(&file, b"%PDF-1.4").unwrap();
    let outside = std::env::temp_dir().join(format!(
        "monocode-out-{}.txt",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::write(&outside, b"x").unwrap();
    let dot = h.cwd.join(".ssh");
    std::fs::create_dir_all(&dot).unwrap();
    std::fs::write(dot.join("id_ed25519"), b"key").unwrap();

    let mark = h.mark();
    for denied in [
        outside.to_str().unwrap().to_string(),
        dot.join("id_ed25519").to_str().unwrap().to_string(),
        "relative.txt".to_string(),
    ] {
        let result = h.act(
            ActionKind::Upload,
            json!({ "ref": "e9", "paths": [denied] }),
        );
        assert_eq!(result.reason, Some(Reason::PathDenied), "{result:?}");
        assert_eq!(result.outcome, Outcome::Blocked);
    }
    assert!(
        h.since(mark).is_empty(),
        "path checks happen before any command"
    );
    let _ = std::fs::remove_file(&outside);

    // Direct path: a file input gets DOM.setFileInputFiles, no click.
    h.model.lock().unwrap().facts.insert(
        (MAIN.into(), 16),
        json!({ "file_input": true, "visible": false, "tag": "input" }),
    );
    let mark = h.mark();
    let direct = h.act(
        ActionKind::Upload,
        json!({ "ref": "e9", "paths": [file.to_str().unwrap()] }),
    );
    assert_eq!(direct.outcome, Outcome::Done, "{direct:?}");
    assert_eq!(direct.data.unwrap()["files"], json!(["cv.pdf"]));
    let log = h.since(mark);
    let set = log
        .iter()
        .find(|r| r.method == "DOM.setFileInputFiles")
        .unwrap();
    assert_eq!(set.params["backendNodeId"], json!(16));
    let canonical = std::fs::canonicalize(&file).unwrap();
    assert_eq!(set.params["files"], json!([canonical.to_str().unwrap()]));
    assert!(mouse(&log).is_empty());
    let two = h.act(
        ActionKind::Upload,
        json!({ "ref": "e9", "paths": [file.to_str().unwrap(), file.to_str().unwrap()] }),
    );
    assert_eq!(two.reason, Some(Reason::InvalidRequest));

    // Button path: intercept on, click, chooser event, files, intercept off.
    h.model.lock().unwrap().on_release = vec![Event::new(
        "Page.fileChooserOpened",
        json!({ "frameId": "F1", "mode": "selectSingle", "backendNodeId": 777 }),
        Some(MAIN),
    )];
    let mark = h.mark();
    let via_button = h.act(
        ActionKind::Upload,
        json!({ "ref": "e8", "paths": [file.to_str().unwrap()] }),
    );
    assert_eq!(via_button.outcome, Outcome::Done, "{via_button:?}");
    let log = h.since(mark);
    let names = methods(&log);
    let on = log
        .iter()
        .position(|r| {
            r.method == "Page.setInterceptFileChooserDialog" && r.params["enabled"] == json!(true)
        })
        .unwrap();
    let release = log
        .iter()
        .position(|r| r.params["type"] == json!("mouseReleased"))
        .unwrap();
    let set = names
        .iter()
        .position(|m| m == "DOM.setFileInputFiles")
        .unwrap();
    let off = log
        .iter()
        .position(|r| {
            r.method == "Page.setInterceptFileChooserDialog" && r.params["enabled"] == json!(false)
        })
        .unwrap();
    assert!(on < release && release < set && set < off, "{names:?}");
    assert_eq!(log[set].params["backendNodeId"], json!(777));

    // No chooser: chooser_timeout, and interception is switched off again.
    let mark = h.mark();
    let silent = h.act(
        ActionKind::Upload,
        json!({ "ref": "e8", "paths": [file.to_str().unwrap()], "timeout_ms": 900 }),
    );
    assert_eq!(silent.reason, Some(Reason::ChooserTimeout), "{silent:?}");
    let log = h.since(mark);
    assert!(log
        .iter()
        .any(|r| r.method == "Page.setInterceptFileChooserDialog"
            && r.params["enabled"] == json!(false)));
}

#[test]
fn navigate_history_wait_screenshot_read_and_scroll() {
    let h = Harness::new();
    let mark = h.mark();
    let denied = h.act(ActionKind::Navigate, json!({ "url": "chrome://settings" }));
    assert_eq!(denied.reason, Some(Reason::UrlDenied));
    assert_eq!(denied.outcome, Outcome::Blocked);
    assert!(h.since(mark).is_empty());
    let opened = h.act(
        ActionKind::Navigate,
        json!({ "url": "https://example.com/b" }),
    );
    assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
    assert_eq!(opened.observed.load_state, Some(LoadState::Load));
    assert!(opened.observed.navigated);

    let back = h.act(ActionKind::Back, json!({}));
    assert_eq!(back.outcome, Outcome::Done, "{back:?}");
    let entry = h
        .sent()
        .into_iter()
        .find(|r| r.method == "Page.navigateToHistoryEntry")
        .unwrap();
    assert_eq!(entry.params["entryId"], json!(1));
    let forward = h.act(ActionKind::Forward, json!({}));
    assert_eq!(forward.reason, Some(Reason::NavigationFailed));

    // wait
    let slept = h.act(ActionKind::Wait, json!({ "for": "time", "value": 50 }));
    assert_eq!(slept.outcome, Outcome::Done);
    let text = h.act(
        ActionKind::Wait,
        json!({ "for": "text", "value": "Welcome" }),
    );
    assert_eq!(text.outcome, Outcome::Done, "{text:?}");
    let missing = h.act(
        ActionKind::Wait,
        json!({ "for": "text", "value": "Goodbye", "timeout_ms": 300 }),
    );
    assert_eq!(missing.reason, Some(Reason::Timeout));
    assert_eq!(missing.outcome, Outcome::Failed);
    let url = h.act(
        ActionKind::Wait,
        json!({ "for": "url", "value": "https://example.com/" }),
    );
    assert_eq!(url.outcome, Outcome::Done, "{url:?}");
    let load = h.act(ActionKind::Wait, json!({ "for": "load" }));
    assert_eq!(load.outcome, Outcome::Done, "{load:?}");
    let no_value = h.act(ActionKind::Wait, json!({ "for": "text" }));
    assert_eq!(no_value.reason, Some(Reason::InvalidRequest));

    // screenshot: viewport, element clip in page coordinates, full page.
    let shot = h.act(ActionKind::Screenshot, json!({}));
    assert_eq!(shot.outcome, Outcome::Done, "{shot:?}");
    let data = shot.data.unwrap();
    assert_eq!(data["mime_type"], json!("image/png"));
    assert!(data["base64"].as_str().unwrap().starts_with("iVBOR"));
    h.snapshot();
    let mark = h.mark();
    let clipped = h.act(
        ActionKind::Screenshot,
        json!({ "ref": "e2", "format": "jpeg", "quality": 50 }),
    );
    assert_eq!(clipped.outcome, Outcome::Done, "{clipped:?}");
    let capture = h
        .since(mark)
        .into_iter()
        .find(|r| r.method == "Page.captureScreenshot")
        .unwrap();
    assert_eq!(capture.params["clip"]["y"], json!(60.0));
    assert_eq!(capture.params["clip"]["width"], json!(100.0));
    assert_eq!(capture.params["format"], json!("jpeg"));
    assert_eq!(capture.params["quality"], json!(50));
    assert!(!h.sent().iter().any(|r| r.method == "Page.bringToFront"));

    // read with offsets.
    let first = h.act(ActionKind::Read, json!({}));
    assert_eq!(first.outcome, Outcome::Done, "{first:?}");
    assert_eq!(first.data.unwrap()["text"], json!("abcdefghij"));
    h.model.lock().unwrap().read_text = "z".repeat(60_000);
    let part = h.act(
        ActionKind::Read,
        json!({ "ref": "e2", "format": "markdown" }),
    );
    let data = part.data.unwrap();
    assert_eq!(data["next_offset"], json!(50_000));
    assert_eq!(data["total"], json!(60_000));
    let rest = h.act(ActionKind::Read, json!({ "offset": 50_000 }));
    let data = rest.data.unwrap();
    assert_eq!(data["text"].as_str().unwrap().len(), 10_000);
    assert!(data.get("next_offset").is_none());

    // scroll: the fake ignores the wheel, so the fallback scrolls.
    let scrolled = h.act(
        ActionKind::Scroll,
        json!({ "direction": "down", "amount": 300 }),
    );
    assert_eq!(scrolled.outcome, Outcome::Done, "{scrolled:?}");
    assert_eq!(scrolled.data.unwrap()["y"], json!(300.0));
    let into_view = h.act(ActionKind::Scroll, json!({ "ref": "e2" }));
    assert_eq!(into_view.outcome, Outcome::Done, "{into_view:?}");
}

#[test]
fn revocation_mid_action_sends_nothing_further() {
    let h = Harness::new();
    h.snapshot();
    let ctx = h
        .policy
        .read()
        .admit(
            "s1",
            "rv",
            ActionKind::Click,
            Some(h.tab()),
            Duration::from_secs(5),
        )
        .unwrap();
    h.policy.write().turn_finished("s1");
    let mark = h.mark();
    let input = json!({ "tab": h.tab(), "ref": "e2" });
    let result = execute(Exec {
        link: &h.link,
        ctx: &ctx,
        kind: ActionKind::Click,
        input: &input,
        handle: Some(&h.handle),
        settings: &h.settings,
        commands_before: h.link.commands_sent(),
    });
    assert_eq!(result.outcome, Outcome::Blocked, "{result:?}");
    assert_eq!(result.reason, Some(Reason::Revoked));
    assert!(h.since(mark).is_empty());
}

/// Which `monocodeCursor` operation a logged command is, if any.
fn cursor_op(r: &Received) -> Option<String> {
    let function = r.params["functionDeclaration"].as_str()?;
    if r.method != "Runtime.callFunctionOn" || !function.contains("function monocodeCursor(") {
        return None;
    }
    r.params["arguments"][0]["value"]
        .as_str()
        .map(str::to_string)
}

#[test]
fn the_visible_cursor_glides_before_a_click_and_stays_out_of_screenshots() {
    let h = Harness::new();
    h.snapshot();

    // No glide (hidden tab, reduced motion): one cursor move before the
    // input, the press right before the mouse goes down, no extra checks.
    let mark = h.mark();
    let click = h.act(ActionKind::Click, json!({ "ref": "e2" }));
    assert_eq!(click.outcome, Outcome::Done, "{click:?}");
    let log = h.since(mark);
    let ops: Vec<String> = log.iter().filter_map(cursor_op).collect();
    assert_eq!(ops, vec!["move", "click"]);
    let mv = log
        .iter()
        .position(|r| cursor_op(r).as_deref() == Some("move"))
        .unwrap();
    let press = log
        .iter()
        .position(|r| cursor_op(r).as_deref() == Some("click"))
        .unwrap();
    let first_input = log
        .iter()
        .position(|r| r.method == "Input.dispatchMouseEvent")
        .unwrap();
    assert!(mv < press && press < first_input);
    assert_eq!(log[mv].params["arguments"][1]["value"], json!(60.0));
    assert_eq!(log[mv].params["arguments"][2]["value"], json!(25.0));
    // The cursor lives in our isolated world, on the main frame.
    assert_eq!(log[mv].params["executionContextId"], json!(70));
    assert_eq!(log[mv].session.as_deref(), Some(MAIN));
    assert_eq!(
        methods(&log)
            .iter()
            .filter(|m| *m == "DOM.getContentQuads")
            .count(),
        1
    );

    // With a glide the action waits for it, then checks the point again
    // (the page had time to move) before pressing. The second move starts
    // where the first one ended.
    h.model.lock().unwrap().glide_ms = 30;
    let mark = h.mark();
    let click = h.act(ActionKind::Click, json!({ "ref": "e2" }));
    assert_eq!(click.outcome, Outcome::Done, "{click:?}");
    let log = h.since(mark);
    let mv = log
        .iter()
        .position(|r| cursor_op(r).as_deref() == Some("move"))
        .unwrap();
    assert_eq!(log[mv].params["arguments"][3]["value"], json!(60.0));
    assert_eq!(log[mv].params["arguments"][4]["value"], json!(25.0));
    let after: Vec<String> = methods(&log[mv + 1..]);
    let first_input = after
        .iter()
        .position(|m| m == "Input.dispatchMouseEvent")
        .unwrap();
    assert!(
        after[..first_input]
            .iter()
            .any(|m| m == "DOM.getContentQuads"),
        "{after:?}"
    );
    let hit_after_glide = log[mv + 1..].iter().any(|r| {
        r.params["functionDeclaration"]
            .as_str()
            .is_some_and(|f| f.contains("function monocodeHitTest("))
    });
    assert!(hit_after_glide);

    // The same point, so no jump.
    assert!(!log.iter().any(|r| cursor_op(r).as_deref() == Some("move")
        && r.params["arguments"][5]["value"] == json!(true)));

    // Hover glides too.
    let mark = h.mark();
    let hover = h.act(ActionKind::Hover, json!({ "ref": "e2" }));
    assert_eq!(hover.outcome, Outcome::Done, "{hover:?}");
    let ops: Vec<String> = h.since(mark).iter().filter_map(cursor_op).collect();
    assert_eq!(ops, vec!["move"]);

    // Screenshots hide the cursor and show it again afterwards.
    let mark = h.mark();
    let shot = h.act(ActionKind::Screenshot, json!({}));
    assert_eq!(shot.outcome, Outcome::Done, "{shot:?}");
    let log = h.since(mark);
    let conceal = log
        .iter()
        .position(|r| cursor_op(r).as_deref() == Some("conceal"))
        .unwrap();
    let capture = log
        .iter()
        .position(|r| r.method == "Page.captureScreenshot")
        .unwrap();
    let reveal = log
        .iter()
        .position(|r| cursor_op(r).as_deref() == Some("reveal"))
        .unwrap();
    assert!(conceal < capture && capture < reveal);
}

#[test]
fn screenshots_of_tabs_without_a_cursor_send_no_cursor_calls() {
    let h = Harness::new();
    let shot = h.act(ActionKind::Screenshot, json!({}));
    assert_eq!(shot.outcome, Outcome::Done, "{shot:?}");
    assert!(!h.sent().iter().any(|r| cursor_op(r).is_some()));
}
