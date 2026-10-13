//! Resilience tests (work package 5; spec 6, 7, 14 and 16): the whole host
//! (`BrowserHost::handle_request` → policy → semantic layer → link) against
//! the fake CDP server, under the situations a real browser and a real user
//! produce: two sessions at once, the browser dying or going away mid-action,
//! tabs closing under the agent, OFF and revocation in the middle of an
//! action, and the agent side (MCP server and CLI) reaching all of it through
//! the control transport. The MonoCode browser (managed mode) and ask-first
//! access get the same treatment at the end of the file.
//!
//! The fake page is deliberately small: one button and one text box per tab,
//! enough for the semantic layer to snapshot and click. Knobs on [`Page`]
//! hold or delay single commands so a test can act while an action is in the
//! middle of its protocol steps.

use std::collections::{HashMap, HashSet};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::agent::client::Credentials;
use super::agent::{cli, mcp};
use super::discovery::ACTIVE_PORT_FILE;
use super::link::LinkStatus;
use super::protocol::{
    AccessMode, ActionRequest, ActionResult, BrowserChoice, BrowserMode, BrowserSettings,
    ConnectionState, Outcome, Reason, TabId, TabState,
};
use super::testing::{default_response, Event, FakeCdp, Received, Response};
use super::{audit, BrowserHost};

/// What the fake browser does with one command, keyed by method, or by
/// `Input.dispatchMouseEvent:<type>` for mouse events.
#[derive(Default)]
struct Page {
    /// Never answered while listed.
    hold: HashSet<String>,
    /// Answered after the delay.
    delay: HashMap<String, Duration>,
    /// CDP session id → target id, from `Target.attachToTarget`.
    sessions: HashMap<String, String>,
    /// `mousePressed` frames per CDP session: the page's click counter.
    presses: HashMap<String, u32>,
    /// `Target.getTargets` lists the attached pages plus one page the user
    /// opened ("U1"); off, it lists nothing (the default fake).
    list_pages: bool,
}

fn key(received: &Received) -> String {
    if received.method == "Input.dispatchMouseEvent" {
        format!(
            "{}:{}",
            received.method,
            received.params["type"].as_str().unwrap_or("")
        )
    } else {
        received.method.clone()
    }
}

fn value(value: Value) -> Response {
    Response::Result(json!({ "result": { "type": "object", "value": value } }))
}

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

fn respond(page: &Mutex<Page>, received: &Received, counter: &AtomicU64) -> Response {
    let mut page = page.lock().unwrap();
    let k = key(received);
    if page.hold.contains(&k) {
        return Response::Hold;
    }
    let session = received.session.clone().unwrap_or_default();
    let params = &received.params;
    if k == "Input.dispatchMouseEvent:mousePressed" {
        *page.presses.entry(session.clone()).or_default() += 1;
    }
    let response = match received.method.as_str() {
        "Target.getTargets" if page.list_pages => {
            let mut targets: Vec<&String> = page.sessions.values().collect();
            targets.sort();
            targets.dedup();
            let mut infos: Vec<Value> = targets
                .into_iter()
                .map(|t| json!({ "targetId": t, "type": "page", "url": "https://example.com/" }))
                .collect();
            infos.push(json!({ "targetId": "U1", "type": "page", "url": "https://mail.example/" }));
            infos.push(json!({ "targetId": "W1", "type": "service_worker", "url": "" }));
            Response::Result(json!({ "targetInfos": infos }))
        }
        "Target.attachToTarget" => {
            let n = counter.fetch_add(1, Ordering::Relaxed);
            let cdp = format!("S{n}");
            let target = params["targetId"].as_str().unwrap_or("").to_string();
            page.sessions.insert(cdp.clone(), target);
            Response::Result(json!({ "sessionId": cdp }))
        }
        "Target.closeTarget" => {
            // Chrome answers, then reports the detach of our flat session.
            let target = params["targetId"].as_str().unwrap_or("");
            let events = page
                .sessions
                .iter()
                .filter(|(_, t)| t.as_str() == target)
                .map(|(s, t)| {
                    Event::new(
                        "Target.detachedFromTarget",
                        json!({ "sessionId": s, "targetId": t }),
                        None,
                    )
                })
                .collect();
            Response::ResultThen(json!({ "success": true }), events)
        }
        "Page.navigate" => {
            let target = page.sessions.get(&session).cloned().unwrap_or_default();
            let n = counter.fetch_add(1, Ordering::Relaxed);
            let loader = format!("L{n}");
            let url = params["url"].as_str().unwrap_or("about:blank");
            Response::ResultThen(
                json!({ "frameId": target, "loaderId": loader }),
                vec![
                    Event::new(
                        "Page.frameNavigated",
                        json!({ "frame": { "id": target, "loaderId": loader, "url": url } }),
                        Some(&session),
                    ),
                    Event::new(
                        "Page.lifecycleEvent",
                        json!({ "frameId": target, "loaderId": loader, "name": "load" }),
                        Some(&session),
                    ),
                ],
            )
        }
        "Accessibility.getFullAXTree" => Response::Result(json!({ "nodes": [
            ax("1", "RootWebArea", "Shop", 1, &["2", "3"]),
            ax("2", "button", "Save", 11, &[]),
            ax("3", "textbox", "Email", 12, &[]),
        ]})),
        "DOM.resolveNode" => Response::Result(json!({ "object": {
            "objectId": format!("{session}:{}", params["backendNodeId"])
        }})),
        "DOM.getContentQuads" => Response::Result(json!({
            "quads": [[10.0, 10.0, 110.0, 10.0, 110.0, 40.0, 10.0, 40.0]]
        })),
        "Runtime.callFunctionOn" => {
            let function = params["functionDeclaration"].as_str().unwrap_or("");
            let first = params["arguments"][0]["value"].as_str().unwrap_or("");
            if function.contains("function monocodeActionability(") {
                value(json!({
                    "connected": true, "visible": true, "enabled": true, "editable": false,
                    "text_like": false, "file_input": false, "multiple": false, "select": false,
                    "tag": "button", "rect": { "x": 10, "y": 10, "width": 100, "height": 30 },
                }))
            } else if function.contains("function monocodeHitTest(") {
                value(json!({ "status": "ok" }))
            } else if function.contains("function monocodePage(") {
                match first {
                    "mutations" => value(json!({ "count": 0 })),
                    "ready" => value(json!({ "state": "complete" })),
                    "text" => value(json!({ "found": false })),
                    _ => value(Value::Null),
                }
            } else {
                value(Value::Null)
            }
        }
        _ => default_response(received, counter),
    };
    match (page.delay.get(&k), response) {
        (Some(delay), Response::Result(value)) => Response::Delay(*delay, value),
        (_, response) => response,
    }
}

static REQUESTS: AtomicU64 = AtomicU64::new(1);

fn request_id() -> String {
    format!("rq{}", REQUESTS.fetch_add(1, Ordering::Relaxed))
}

/// Point the profile's `DevToolsActivePort` at `server`.
fn write_port_file(dir: &std::path::Path, server: &FakeCdp) {
    let rest = server.url.strip_prefix("ws://127.0.0.1:").unwrap();
    let (port, path) = rest.split_once('/').unwrap();
    std::fs::write(dir.join(ACTIVE_PORT_FILE), format!("{port}\n/{path}\n")).unwrap();
}

/// A fake browser, its profile directory and a host connected to it.
struct World {
    server: FakeCdp,
    page: Arc<Mutex<Page>>,
    host: Arc<BrowserHost>,
    dir: PathBuf,
}

impl World {
    fn start(tweak: impl FnOnce(&mut BrowserSettings)) -> World {
        let (server, page) = fake_browser();
        let dir = std::env::temp_dir().join(format!("monocode-res-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        write_port_file(&dir, &server);
        let mut settings = BrowserSettings {
            browser: BrowserChoice::UserDataDir {
                path: dir.display().to_string(),
            },
            mode: BrowserMode::Attach,
            access: AccessMode::Always,
            ..BrowserSettings::default()
        };
        tweak(&mut settings);
        let host = BrowserHost::for_tests(settings);
        host.set_toggle(true);
        let world = World {
            server,
            page,
            host,
            dir,
        };
        world.wait_state(ConnectionState::Connected);
        world
    }

    /// Managed mode (the default): the profile folder is MonoCode's own and a
    /// "MonoCode browser" already listens in it, so nothing is launched. The
    /// switch is ON and nothing is connected yet.
    fn managed(access: AccessMode) -> World {
        World::launched(BrowserMode::Managed, access)
    }

    /// [`World::managed`] in `mode` (managed or private).
    fn launched(mode: BrowserMode, access: AccessMode) -> World {
        let (server, page) = fake_browser();
        let dir = std::env::temp_dir().join(format!("monocode-res-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        write_port_file(&dir, &server);
        let host = BrowserHost::for_tests(BrowserSettings {
            mode,
            access,
            ..BrowserSettings::default()
        });
        host.set_profile_dir(dir.clone());
        host.set_toggle(true);
        let world = World {
            server,
            page,
            host,
            dir,
        };
        world.wait_state(ConnectionState::Idle);
        world
    }

    /// Whether an agent of `session` waits for the user's answer, as the
    /// frontend sees it.
    fn requested(&self, session: &str) -> bool {
        self.host
            .state()
            .sessions
            .iter()
            .any(|s| s.session_id == session && s.requested)
    }

    /// Run one action on its own thread (an agent's tool call).
    fn spawn_act(
        self: &Arc<Self>,
        session: &'static str,
        action: &'static str,
        input: Value,
    ) -> thread::JoinHandle<(ActionResult, Duration)> {
        let world = self.clone();
        thread::spawn(move || {
            let started = Instant::now();
            let result = world.act(session, action, input);
            (result, started.elapsed())
        })
    }

    fn wait_state(&self, state: ConnectionState) {
        assert!(
            wait_until(Duration::from_secs(5), || self.host.connection().state
                == state),
            "expected {state:?}, got {:?}",
            self.host.connection()
        );
    }

    fn grant(&self, session: &str) {
        self.host.turn_started(session, "main", "/repo", Some(true));
    }

    fn act(&self, session: &str, action: &str, input: Value) -> ActionResult {
        self.act_as(session, &request_id(), action, input)
    }

    fn act_as(&self, session: &str, id: &str, action: &str, input: Value) -> ActionResult {
        self.host.handle_request(
            session,
            ActionRequest {
                request_id: id.into(),
                action: action.into(),
                input,
            },
        )
    }

    fn open(&self, session: &str) -> TabId {
        let opened = self.act(
            session,
            "tabs.open",
            json!({ "url": "https://example.com/" }),
        );
        assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
        TabId(opened.data.unwrap()["tab"].as_str().unwrap().to_string())
    }

    /// Snapshot the tab and return the ref of its "Save" button.
    fn save_ref(&self, session: &str, tab: &TabId) -> String {
        let shot = self.act(session, "snapshot", json!({ "tab": tab }));
        assert_eq!(shot.outcome, Outcome::Done, "{shot:?}");
        save_ref_in(shot.data.unwrap()["snapshot"].as_str().unwrap())
    }

    fn click(&self, session: &str, tab: &TabId, element: &str) -> ActionResult {
        self.act(session, "click", json!({ "tab": tab, "ref": element }))
    }

    fn cdp_session(&self, tab: &TabId) -> String {
        self.host
            .policy()
            .read()
            .lease(tab)
            .and_then(|l| l.cdp_session.clone())
            .map(|s| s.0)
            .expect("attached tab")
    }

    fn sent(&self) -> Vec<Received> {
        self.server.received()
    }

    /// How many frames the fake has received so far.
    fn mark(&self) -> usize {
        self.sent().len()
    }

    /// Wait for a frame matching `pred` received after `mark`.
    fn wait_sent(&self, mark: usize, pred: impl Fn(&Received) -> bool) {
        assert!(
            self.server.wait_for(
                |log| log.iter().skip(mark).any(&pred),
                Duration::from_secs(5)
            ),
            "expected command never arrived"
        );
    }

    fn presses(&self, cdp: &str) -> u32 {
        self.page
            .lock()
            .unwrap()
            .presses
            .get(cdp)
            .copied()
            .unwrap_or(0)
    }

    fn hold(&self, key: &str) {
        self.page.lock().unwrap().hold.insert(key.into());
    }

    fn delay(&self, key: &str, by: Duration) {
        self.page.lock().unwrap().delay.insert(key.into(), by);
    }

    fn release_knobs(&self) {
        let mut page = self.page.lock().unwrap();
        page.hold.clear();
        page.delay.clear();
    }

    fn open_lease_rows(&self) -> i64 {
        self.host
            .db
            .with(audit::open_lease_count)
            .expect("lease rows")
    }

    fn event_count(&self, kind: &str) -> usize {
        self.host
            .events_list(500)
            .iter()
            .filter(|e| e.kind == kind)
            .count()
    }

    /// Frames the link wrote after `at` (method names), from the link's own
    /// write log matched against what the fake received on this connection.
    fn written_after(&self, at: Instant) -> Vec<String> {
        let received = self.sent();
        self.host
            .link()
            .write_log()
            .into_iter()
            .filter(|(_, written)| *written > at)
            .map(|(id, _)| {
                received
                    .iter()
                    .rev()
                    .find(|r| r.id == id)
                    .map(|r| r.method.clone())
                    .unwrap_or_else(|| format!("#{id}"))
            })
            .collect()
    }

    fn revoked_at(&self) -> Instant {
        self.host
            .policy()
            .read()
            .last_revocation
            .expect("a revocation happened")
    }

    /// Nothing is left running or reserved in the policy.
    fn assert_idle(&self) {
        let state = self.host.policy().read();
        assert_eq!(state.active_actions, 0, "actions still counted");
        assert!(
            state.leases().all(|l| !l.busy),
            "a lease is still busy: {:?}",
            state
                .leases()
                .filter(|l| l.busy)
                .map(|l| &l.tab)
                .collect::<Vec<_>>()
        );
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.release_knobs();
        self.host.set_toggle(false);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fake_browser() -> (FakeCdp, Arc<Mutex<Page>>) {
    let page = Arc::new(Mutex::new(Page::default()));
    let counter = Arc::new(AtomicU64::new(1));
    let shared = page.clone();
    let server = FakeCdp::start_with(move |received| respond(&shared, received, &counter));
    (server, page)
}

fn save_ref_in(snapshot: &str) -> String {
    let line = snapshot
        .lines()
        .find(|l| l.contains("button \"Save\""))
        .unwrap_or_else(|| panic!("no Save button in {snapshot}"));
    let start = line.find("[ref=").expect("ref") + 5;
    line[start..line[start..].find(']').unwrap() + start].to_string()
}

fn wait_until(timeout: Duration, pred: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    pred()
}

fn tabs_of(result: &ActionResult) -> Vec<String> {
    result.data.as_ref().unwrap()["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["tab"].as_str().unwrap().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Two sessions at the same time (spec 7 "Pravila", 9.6)
// ---------------------------------------------------------------------------

#[test]
fn two_sessions_work_in_parallel_without_seeing_each_other() {
    let world = Arc::new(World::start(|_| {}));
    world.grant("s1");
    world.grant("s2");

    // Both sessions open two tabs and click in them, all at once.
    let workers: Vec<_> = ["s1", "s1", "s2", "s2"]
        .into_iter()
        .map(|session| {
            let world = world.clone();
            thread::spawn(move || {
                let tab = world.open(session);
                let element = world.save_ref(session, &tab);
                for _ in 0..2 {
                    let clicked = world.click(session, &tab, &element);
                    assert_eq!(clicked.outcome, Outcome::Done, "{clicked:?}");
                }
                (session, tab)
            })
        })
        .collect();
    let tabs: Vec<(&str, TabId)> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    world.assert_idle();

    // Every CDP session carried only its owner's commands, and each tab got
    // exactly its own two clicks.
    let owner_of: HashMap<String, &str> = tabs
        .iter()
        .map(|(session, tab)| (world.cdp_session(tab), *session))
        .collect();
    assert_eq!(owner_of.len(), 4, "every tab has its own CDP session");
    for cdp in owner_of.keys() {
        assert_eq!(world.presses(cdp), 2, "clicks in {cdp}");
    }

    // Each session lists only its own tabs.
    for session in ["s1", "s2"] {
        let mut listed = tabs_of(&world.act(session, "tabs.list", json!({})));
        listed.sort();
        let mut own: Vec<String> = tabs
            .iter()
            .filter(|(s, _)| *s == session)
            .map(|(_, t)| t.0.clone())
            .collect();
        own.sort();
        assert_eq!(listed, own, "{session} sees exactly its tabs");
    }

    // A foreign tab is no_lease for every action, and nothing is sent for it.
    let foreign = &tabs.iter().find(|(s, _)| *s == "s1").unwrap().1;
    let before = world.sent().len();
    for (action, input) in [
        ("snapshot", json!({ "tab": foreign })),
        ("click", json!({ "tab": foreign, "ref": "e2" })),
        ("tabs.close", json!({ "tab": foreign })),
        ("read", json!({ "tab": foreign })),
    ] {
        let refused = world.act("s2", action, input);
        assert_eq!(
            refused.reason,
            Some(Reason::NoLease),
            "{action}: {refused:?}"
        );
        assert_eq!(refused.outcome, Outcome::Blocked);
    }
    assert_eq!(world.sent().len(), before);

    // Request ids are per session: the same id runs in both.
    let a = world.act_as("s1", "same-id", "tabs.list", json!({}));
    let b = world.act_as("s2", "same-id", "tabs.list", json!({}));
    assert_eq!(a.outcome, Outcome::Done);
    assert_eq!(b.outcome, Outcome::Done);
    assert_ne!(tabs_of(&a), tabs_of(&b));

    // Revoking one session in the middle of the other's work does not touch it.
    let s2_tab = tabs.iter().find(|(s, _)| *s == "s2").unwrap().1.clone();
    let s2_ref = world.save_ref("s2", &s2_tab);
    world.delay(
        "Input.dispatchMouseEvent:mousePressed",
        Duration::from_millis(300),
    );
    let mark = world.mark();
    let clicking = {
        let world = world.clone();
        let tab = s2_tab.clone();
        thread::spawn(move || world.click("s2", &tab, &s2_ref))
    };
    world.wait_sent(mark, |r| key(r) == "Input.dispatchMouseEvent:mousePressed");
    world.host.revoke("s1");
    let clicked = clicking.join().unwrap();
    assert_eq!(clicked.outcome, Outcome::Done, "{clicked:?}");
    world.release_knobs();
    assert_eq!(
        world.act("s1", "tabs.list", json!({})).reason,
        Some(Reason::NotGranted)
    );
    assert_eq!(tabs_of(&world.act("s2", "tabs.list", json!({}))).len(), 2);
    world.assert_idle();
}

#[test]
fn one_action_per_tab_and_the_connection_limit_hold_under_load() {
    let world = Arc::new(World::start(|s| s.limits.concurrent_actions = 2));
    world.grant("s1");
    world.grant("s2");
    let a = world.open("s1");
    let b = world.open("s2");
    let c = world.open("s2");
    let refs: HashMap<TabId, String> = [&a, &b, &c]
        .into_iter()
        .map(|t| {
            let owner = if *t == a { "s1" } else { "s2" };
            (t.clone(), world.save_ref(owner, t))
        })
        .collect();

    // Count actions in flight from the page's point of view.
    world.delay(
        "Input.dispatchMouseEvent:mouseReleased",
        Duration::from_millis(400),
    );
    let peak = Arc::new(AtomicU64::new(0));
    let sampling = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let sampler = {
        let world = world.clone();
        let peak = peak.clone();
        let sampling = sampling.clone();
        thread::spawn(move || {
            while sampling.load(Ordering::Acquire) {
                let now = world.host.policy().read().active_actions as u64;
                peak.fetch_max(now, Ordering::AcqRel);
                thread::sleep(Duration::from_millis(1));
            }
        })
    };
    let jobs: Vec<_> = [("s1", &a), ("s2", &b), ("s2", &c), ("s1", &a)]
        .into_iter()
        .map(|(session, tab)| {
            let world = world.clone();
            let tab = tab.clone();
            let element = refs[&tab].clone();
            thread::spawn(move || world.click(session, &tab, &element))
        })
        .collect();
    let results: Vec<ActionResult> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
    sampling.store(false, Ordering::Release);
    sampler.join().unwrap();
    // Everything ran (each waits its turn within BUSY_WAIT), never more
    // than two at once, and the two clicks on tab `a` were serialized.
    for result in &results {
        assert_eq!(result.outcome, Outcome::Done, "{result:?}");
    }
    assert!(peak.load(Ordering::Acquire) <= 2, "peak {peak:?}");
    assert_eq!(world.presses(&world.cdp_session(&a)), 2);
    let presses_a: Vec<Instant> = world
        .sent()
        .iter()
        .filter(|r| {
            r.session.as_deref() == Some(&world.cdp_session(&a))
                && r.method == "Input.dispatchMouseEvent"
        })
        .map(|r| r.at)
        .collect();
    assert_eq!(presses_a.len(), 6, "moved, pressed, released twice");
    world.assert_idle();

    // A tab held longer than BUSY_WAIT turns the next request into `busy`
    // without sending anything for it.
    world.release_knobs();
    world.hold("Input.dispatchMouseEvent:mouseReleased");
    let mark = world.mark();
    let stuck = {
        let world = world.clone();
        let tab = b.clone();
        let element = refs[&b].clone();
        thread::spawn(move || world.click("s2", &tab, &element))
    };
    world.wait_sent(mark, |r| key(r) == "Input.dispatchMouseEvent:mouseReleased");
    let before = world.sent().len();
    let busy = world.act("s2", "snapshot", json!({ "tab": b, "timeout_ms": 600 }));
    assert_eq!(busy.reason, Some(Reason::Busy), "{busy:?}");
    assert_eq!(busy.outcome, Outcome::Blocked);
    assert_eq!(world.sent().len(), before);
    // The held click ends `unknown` once its deadline passes (the release
    // may have happened), and the tab is usable again.
    world.release_knobs();
    let stuck = stuck.join().unwrap();
    assert_eq!(stuck.outcome, Outcome::Unknown, "{stuck:?}");
    assert_eq!(stuck.reason, Some(Reason::CdpTimeout));
    let again = world.act("s2", "snapshot", json!({ "tab": b }));
    assert_eq!(again.outcome, Outcome::Done, "{again:?}");
    world.assert_idle();
}

#[test]
fn tab_limits_hold_when_sessions_open_tabs_concurrently() {
    let world = Arc::new(World::start(|s| {
        s.limits.tabs_per_session = 2;
        s.limits.tabs_total = 3;
    }));
    world.grant("s1");
    world.grant("s2");
    // A slow createTarget widens the window between the limit check and the
    // lease insert.
    world.delay("Target.createTarget", Duration::from_millis(150));
    let jobs: Vec<_> = ["s1", "s1", "s1", "s1", "s2", "s2", "s2"]
        .into_iter()
        .map(|session| {
            let world = world.clone();
            thread::spawn(move || (session, world.act(session, "tabs.open", json!({}))))
        })
        .collect();
    let results: Vec<(&str, ActionResult)> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
    let done = |session: &str| {
        results
            .iter()
            .filter(|(s, r)| *s == session && r.outcome == Outcome::Done)
            .count()
    };
    for (_, result) in &results {
        if result.outcome != Outcome::Done {
            assert_eq!(result.reason, Some(Reason::TabLimit), "{result:?}");
            assert_eq!(result.outcome, Outcome::Blocked);
        }
    }
    assert!(done("s1") <= 2, "s1 opened {}", done("s1"));
    assert_eq!(done("s1") + done("s2"), 3, "total limit is 3");
    let created = world
        .sent()
        .iter()
        .filter(|r| r.method == "Target.createTarget")
        .count();
    assert_eq!(created, 3, "no tab was created over the limit");
    world.assert_idle();
}

// ---------------------------------------------------------------------------
// Browser crash, restart and reconnection (spec 14.1 rows 1, 2, 6, 9; 14.2)
// ---------------------------------------------------------------------------

#[test]
fn a_browser_crash_mid_action_is_unknown_and_reconnection_adopts_nothing() {
    let world = Arc::new(World::start(|_| {}));
    world.grant("s1");
    let tab = world.open("s1");
    let element = world.save_ref("s1", &tab);
    let cdp = world.cdp_session(&tab);
    assert_eq!(world.open_lease_rows(), 1);

    // The browser dies between mousePressed and mouseReleased.
    world.hold("Input.dispatchMouseEvent:mouseReleased");
    let mark = world.mark();
    let clicking = {
        let world = world.clone();
        let tab = tab.clone();
        let element = element.clone();
        thread::spawn(move || world.click("s1", &tab, &element))
    };
    world.wait_sent(mark, |r| key(r) == "Input.dispatchMouseEvent:mouseReleased");
    let epoch = world.host.policy().read().global_epoch;
    world.server.close_current();
    let clicked = clicking.join().unwrap();
    assert_eq!(clicked.outcome, Outcome::Unknown, "{clicked:?}");
    assert_eq!(clicked.reason, Some(Reason::LinkDown));
    assert_eq!(world.presses(&cdp), 1);
    world.wait_state(ConnectionState::Disconnected);
    world.release_knobs();
    {
        let state = world.host.policy().read();
        assert_ne!(state.global_epoch, epoch);
        assert_eq!(state.leases().count(), 0, "leases cleared");
        assert!(state.granted("s1"), "grants survive a browser crash");
    }
    world.assert_idle();
    assert!(
        wait_until(Duration::from_secs(2), || world.open_lease_rows() == 0),
        "lease rows closed with the link"
    );

    // The unknown result is final for its request id (spec 14.3): a retry
    // with the same id gets it back and touches nothing.
    let retried = world.act_as(
        "s1",
        &clicked.request_id,
        "click",
        json!({ "tab": tab, "ref": element }),
    );
    assert_eq!(retried.outcome, Outcome::Unknown, "{retried:?}");
    assert_eq!(retried.reason, Some(Reason::LinkDown));
    assert_eq!(world.server.connections(), 1, "the retry did not reconnect");

    // The next request reports the drop and starts one reconnection.
    let first = world.act("s1", "tabs.list", json!({}));
    assert_eq!(first.reason, Some(Reason::BrowserDisconnected), "{first:?}");
    world.wait_state(ConnectionState::Connected);
    assert_eq!(world.server.connections(), 2);
    // Nothing from before is adopted: the list is empty and the old id is gone.
    assert!(tabs_of(&world.act("s1", "tabs.list", json!({}))).is_empty());
    let old = world.act("s1", "snapshot", json!({ "tab": tab }));
    assert_eq!(old.reason, Some(Reason::NoLease), "{old:?}");
    let fresh = world.open("s1");
    assert_ne!(fresh, tab);
    assert_eq!(world.open_lease_rows(), 1);
    world.assert_idle();
}

#[test]
fn a_restarted_browser_on_a_new_port_is_found_again() {
    let world = World::start(|_| {});
    world.grant("s1");
    let _ = world.open("s1");
    // The user quits the browser: socket gone, file gone.
    std::fs::remove_file(world.dir.join(ACTIVE_PORT_FILE)).unwrap();
    world.server.close_current();
    world.wait_state(ConnectionState::Disconnected);
    let gone = world.act("s1", "tabs.open", json!({}));
    assert_eq!(gone.reason, Some(Reason::BrowserDisconnected));
    // Discovery runs and finds no file: the user has to turn debugging on
    // again, and the agent is told so; nothing loops.
    world.wait_state(ConnectionState::NeedsToggle);
    let needs = world.act("s1", "tabs.open", json!({}));
    assert_eq!(needs.reason, Some(Reason::BrowserNeedsSetup), "{needs:?}");
    // The browser comes back on another port; the next request reconnects there.
    let (restarted, _page) = fake_browser();
    write_port_file(&world.dir, &restarted);
    assert!(wait_until(Duration::from_secs(5), || {
        let _ = world.act("s1", "tabs.list", json!({}));
        world.host.connection().state == ConnectionState::Connected
    }));
    assert_eq!(restarted.connections(), 1);
    let opened = world.act("s1", "tabs.open", json!({}));
    assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
    assert!(restarted
        .received()
        .iter()
        .any(|r| r.method == "Target.createTarget"));
    world.host.set_toggle(false);
}

#[test]
fn after_a_drop_the_agent_triggers_at_most_one_consent_dialog() {
    let world = World::start(|_| {});
    world.host.set_consent_timeout(Duration::from_millis(300));
    world.grant("s1");
    // The browser now waits on a consent dialog nobody answers.
    let silent = FakeCdp::start_silent();
    write_port_file(&world.dir, &silent);
    world.server.close_current();
    world.wait_state(ConnectionState::Disconnected);
    let consents_before = world.event_count("consent_pending");
    for _ in 0..6 {
        let result = world.act("s1", "tabs.list", json!({}));
        assert_eq!(result.outcome, Outcome::Blocked, "{result:?}");
        thread::sleep(Duration::from_millis(150));
    }
    assert!(wait_until(Duration::from_secs(3), || {
        world.event_count("consent_timeout") == 1
    }));
    for _ in 0..4 {
        let _ = world.act("s1", "tabs.list", json!({}));
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_millis(500));
    assert_eq!(
        world.event_count("consent_pending") - consents_before,
        1,
        "one automatic attempt per drop"
    );
    assert!(world.event_count("auto_connect_skipped") >= 1);
    // The state still says why, and the agent is told to ask for Connect.
    assert_eq!(
        world.host.connection().state,
        ConnectionState::ConsentTimeout
    );
    let told = world.act("s1", "tabs.list", json!({}));
    assert!(told.message.contains("Connect"), "{told:?}");
    // A user click on Connect is always allowed and opens the next attempt.
    // (A discovery started by the last request may still be finishing.)
    assert!(wait_until(Duration::from_secs(2), || {
        world.host.connect();
        world.event_count("consent_pending") - consents_before == 2
    }));
}

#[test]
fn rapid_toggling_never_leaves_a_stale_attempt_in_charge() {
    let world = World::start(|_| {});
    // OFF→ON→OFF→ON as fast as a user can click: the last word wins, and an
    // attempt started before a flip neither reports its state nor keeps a socket.
    for round in 0..15 {
        world.host.set_toggle(false);
        world.host.set_toggle(true);
        if round % 3 == 0 {
            world.host.set_toggle(false);
            thread::sleep(Duration::from_millis(30));
            assert_eq!(world.host.connection().state, ConnectionState::Off);
            assert!(!world.host.link().is_open(), "round {round}");
            world.host.set_toggle(true);
        }
    }
    world.wait_state(ConnectionState::Connected);
    assert!(world.host.link().is_open());
    // OFF while the browser shows its consent dialog: the dialog is
    // abandoned, the state stays Off, and no socket survives.
    let silent = FakeCdp::start_silent();
    write_port_file(&world.dir, &silent);
    world.host.set_toggle(false);
    world.host.set_toggle(true);
    world.wait_state(ConnectionState::AwaitingConsent);
    world.host.set_toggle(false);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(world.host.connection().state, ConnectionState::Off);
    assert!(!world.host.link().is_open());
    assert!(!world.host.policy().read().toggle_on);
}

#[test]
fn a_hung_browser_times_out_pings_and_closes() {
    let world = Arc::new(World::start(|_| {}));
    world.grant("s1");
    let tab = world.open("s1");
    // Every command hangs from now on, including the ping.
    for method in [
        "Accessibility.getFullAXTree",
        "Browser.getVersion",
        "Page.createIsolatedWorld",
        "Runtime.callFunctionOn",
        "DOM.resolveNode",
    ] {
        world.hold(method);
    }
    let mark = world.mark();
    let shot = world.act("s1", "snapshot", json!({ "tab": tab, "timeout_ms": 500 }));
    assert_ne!(shot.outcome, Outcome::Done, "{shot:?}");
    assert!(
        matches!(
            shot.reason,
            Some(Reason::CdpTimeout | Reason::SnapshotTimeout | Reason::Timeout)
        ),
        "{shot:?}"
    );
    world.assert_idle();
    // The ping after the timeout goes unanswered: the link closes as a crash.
    world.wait_sent(mark, |r| r.method == "Browser.getVersion");
    assert!(wait_until(Duration::from_secs(10), || {
        world.host.connection().state == ConnectionState::Disconnected
    }));
    assert_eq!(world.host.link().status(), LinkStatus::Closed);
}

// ---------------------------------------------------------------------------
// Tab and lease cleanup (spec 7 "Životni vek", 14.1 rows 3, 5; T15)
// ---------------------------------------------------------------------------

#[test]
fn tabs_closed_under_the_agent_end_their_action_and_free_their_slot() {
    let world = Arc::new(World::start(|s| s.limits.tabs_per_session = 1));
    world.grant("s1");
    let tab = world.open("s1");
    let cdp = world.cdp_session(&tab);
    let target = world
        .host
        .policy()
        .read()
        .lease(&tab)
        .unwrap()
        .target_id
        .0
        .clone();
    // The limit is reached.
    let over = world.act("s1", "tabs.open", json!({}));
    assert_eq!(over.reason, Some(Reason::TabLimit));

    // T15: the user closes the tab while the agent waits on it.
    let mark = world.mark();
    let waiting = {
        let world = world.clone();
        let tab = tab.clone();
        thread::spawn(move || {
            world.act(
                "s1",
                "wait",
                json!({ "tab": tab, "for": "text", "value": "never", "timeout_ms": 5000 }),
            )
        })
    };
    world.wait_sent(mark, |r| {
        r.method == "Runtime.callFunctionOn" && r.session.as_deref() == Some(&cdp)
    });
    world.server.emit(
        "Target.detachedFromTarget",
        json!({ "sessionId": cdp, "targetId": target }),
        None,
    );
    let waited = waiting.join().unwrap();
    assert_eq!(waited.outcome, Outcome::Failed, "{waited:?}");
    assert_eq!(waited.reason, Some(Reason::TargetClosed));
    // Gone from the list after that report; no_lease from then on; slot free.
    assert!(tabs_of(&world.act("s1", "tabs.list", json!({}))).is_empty());
    let after = world.act("s1", "snapshot", json!({ "tab": tab }));
    assert_eq!(after.reason, Some(Reason::NoLease), "{after:?}");
    let second = world.open("s1");
    world.assert_idle();

    // A crashed renderer: the next action reports it, then the tab is gone.
    let cdp2 = world.cdp_session(&second);
    world
        .server
        .emit("Inspector.targetCrashed", json!({}), Some(&cdp2));
    assert!(wait_until(Duration::from_secs(2), || {
        world.host.policy().read().lease(&second).map(|l| l.state) == Some(TabState::Crashed)
    }));
    let crashed = world.act("s1", "snapshot", json!({ "tab": second }));
    assert_eq!(crashed.reason, Some(Reason::TargetClosed), "{crashed:?}");
    assert!(tabs_of(&world.act("s1", "tabs.list", json!({}))).is_empty());
    let third = world.open("s1");

    // The agent closes its own tab: one closeTarget, slot free again.
    let before = world.sent().len();
    let closed = world.act("s1", "tabs.close", json!({ "tab": third }));
    assert_eq!(closed.outcome, Outcome::Done, "{closed:?}");
    let closes: Vec<_> = world.sent()[before..]
        .iter()
        .filter(|r| r.method == "Target.closeTarget")
        .cloned()
        .collect();
    assert_eq!(closes.len(), 1);
    let again = world.act("s1", "tabs.close", json!({ "tab": third }));
    assert_eq!(again.reason, Some(Reason::NoLease), "{again:?}");
    let _fourth = world.open("s1");
    world.assert_idle();
    assert_eq!(
        world.open_lease_rows(),
        1,
        "only the live tab has an open row"
    );
}

#[test]
fn revocation_window_close_and_session_delete_release_exactly_their_tabs() {
    let world = World::start(|_| {});
    world.grant("s1");
    world.grant("s2");
    world
        .host
        .turn_started("w2", "second-window", "/other", Some(true));
    let s1_tab = world.open("s1");
    let s2_tabs = [world.open("s2"), world.open("s2")];
    let w2_tab = world.open("w2");
    assert_eq!(world.open_lease_rows(), 4);

    // Revoke s1: detach only its tab; the tab stays open; nobody else changes.
    let before = world.sent().len();
    world.host.revoke("s1");
    let tail: Vec<Received> = world.sent()[before..].to_vec();
    assert_eq!(tail.len(), 1, "{tail:?}");
    assert_eq!(tail[0].method, "Target.detachFromTarget");
    assert_eq!(
        tail[0].params["sessionId"],
        json!(world.cdp_session(&s1_tab))
    );
    assert_eq!(
        world.host.policy().read().lease(&s1_tab).unwrap().state,
        TabState::Revoked
    );
    // The revoked tab no longer counts against the limits and has no open row.
    assert_eq!(world.host.policy().read().live_tabs("s1"), 0);
    assert_eq!(world.open_lease_rows(), 3);
    // Granting again does not hand the old tab back (spec 14.3).
    world.grant("s1");
    let back = world.act("s1", "snapshot", json!({ "tab": s1_tab }));
    assert_eq!(back.reason, Some(Reason::NoLease), "{back:?}");
    assert!(tabs_of(&world.act("s1", "tabs.list", json!({}))).is_empty());

    // Closing the second app window releases w2's tab without closing it.
    let before = world.sent().len();
    world.host.window_closed("second-window");
    let tail: Vec<String> = world.sent()[before..]
        .iter()
        .map(|r| r.method.clone())
        .collect();
    assert_eq!(tail, vec!["Target.detachFromTarget"]);
    assert!(world.host.policy().read().session("w2").is_none());
    assert_eq!(
        world.host.policy().read().lease(&w2_tab).unwrap().state,
        TabState::Revoked
    );
    assert_eq!(world.open_lease_rows(), 2);

    // Deleting s2 closes both of its tabs and nothing else.
    let before = world.sent().len();
    world.host.session_deleted("s2");
    let closed: HashSet<String> = world.sent()[before..]
        .iter()
        .filter(|r| r.method == "Target.closeTarget")
        .map(|r| r.params["targetId"].as_str().unwrap().to_string())
        .collect();
    let expected: HashSet<String> = s2_tabs
        .iter()
        .map(|t| {
            world
                .host
                .policy()
                .read()
                .lease(t)
                .unwrap()
                .target_id
                .0
                .clone()
        })
        .collect();
    assert_eq!(closed, expected);
    assert_eq!(world.open_lease_rows(), 0);
    assert!(world
        .host
        .state()
        .tabs
        .iter()
        .all(|t| t.session_id != "s2" || t.tab.state == TabState::Closed));

    // OFF → ON: revoked leases vanish with the link, nothing is adopted, and
    // every session must be granted again.
    world.host.set_toggle(false);
    assert_eq!(world.host.policy().read().leases().count(), 0);
    world.host.set_toggle(true);
    world.wait_state(ConnectionState::Connected);
    assert_eq!(
        world.act("s1", "tabs.list", json!({})).reason,
        Some(Reason::NotGranted)
    );
    world.grant("s1");
    assert!(tabs_of(&world.act("s1", "tabs.list", json!({}))).is_empty());
    let _ = world.open("s1");
    world.assert_idle();
}

// ---------------------------------------------------------------------------
// OFF, revocation, turn end and transfer in the middle of an action (spec 6
// "Garancije", 16.3)
// ---------------------------------------------------------------------------

/// Start a click on `tab` whose `mousePressed` answer is delayed, and wait
/// until the press reached the browser. Returns the click's thread.
fn click_paused_after_press(
    world: &Arc<World>,
    session: &'static str,
    tab: &TabId,
) -> thread::JoinHandle<ActionResult> {
    let element = world.save_ref(session, tab);
    world.delay(
        "Input.dispatchMouseEvent:mousePressed",
        Duration::from_millis(400),
    );
    let mark = world.mark();
    let clicking = {
        let world = world.clone();
        let tab = tab.clone();
        thread::spawn(move || world.click(session, &tab, &element))
    };
    let cdp = world.cdp_session(tab);
    world.wait_sent(mark, |r| {
        key(r) == "Input.dispatchMouseEvent:mousePressed" && r.session.as_deref() == Some(&cdp)
    });
    clicking
}

#[test]
fn off_in_the_middle_of_a_click_sends_nothing_more_and_reports_unknown() {
    let world = Arc::new(World::start(|_| {}));
    world.grant("s1");
    let tab = world.open("s1");
    let cdp = world.cdp_session(&tab);
    world.hold("Input.dispatchMouseEvent:mouseReleased");
    let element = world.save_ref("s1", &tab);
    let mark = world.mark();
    let clicking = {
        let world = world.clone();
        let tab = tab.clone();
        let element = element.clone();
        thread::spawn(move || world.click("s1", &tab, &element))
    };
    world.wait_sent(mark, |r| key(r) == "Input.dispatchMouseEvent:mouseReleased");
    world.host.set_toggle(false);
    let revoked = world.revoked_at();
    let clicked = clicking.join().unwrap();
    assert_eq!(clicked.outcome, Outcome::Unknown, "{clicked:?}");
    assert_eq!(clicked.reason, Some(Reason::Revoked));
    assert!(clicked.message.contains("revoked"), "{clicked:?}");
    // After OFF only our detach left the process; no tab was closed.
    let after = world.written_after(revoked);
    assert!(
        after.iter().all(|m| m == "Target.detachFromTarget"),
        "{after:?}"
    );
    assert!(!world
        .sent()
        .iter()
        .any(|r| r.method == "Target.closeTarget"));
    assert_eq!(world.presses(&cdp), 1);
    assert_eq!(world.host.link().status(), LinkStatus::Closed);
    assert_eq!(world.open_lease_rows(), 0);
    world.assert_idle();
    // The result is final for its request id, even after ON.
    world.release_knobs();
    world.host.set_toggle(true);
    world.wait_state(ConnectionState::Connected);
    world.grant("s1");
    let before = world.sent().len();
    let cached = world.act_as(
        "s1",
        &clicked.request_id,
        "click",
        json!({ "tab": tab, "ref": element }),
    );
    assert_eq!(cached.outcome, Outcome::Unknown, "{cached:?}");
    assert_eq!(cached.reason, Some(Reason::Revoked));
    assert_eq!(world.sent().len(), before);
}

#[test]
fn revoke_turn_end_and_transfer_mid_action_stop_at_the_next_step() {
    let world = Arc::new(World::start(|_| {}));
    world.grant("s1");
    world.grant("s2");

    type Interrupt = fn(&World, &TabId);
    let cases: [(&str, Interrupt); 4] = [
        ("revoke", |w, _| w.host.revoke("s1")),
        ("turn end", |w, _| w.host.turn_finished("s1")),
        ("grant off", |w, _| w.host.set_grant("s1", "main", false)),
        ("transfer", |w, tab| {
            w.host.transfer(tab, "s1", "s2").unwrap()
        }),
    ];
    for (name, interrupt) in cases {
        world.release_knobs();
        world.grant("s1");
        let tab = world.open("s1");
        let cdp = world.cdp_session(&tab);
        let clicking = click_paused_after_press(&world, "s1", &tab);
        interrupt(&world, &tab);
        let revoked = world.revoked_at();
        let clicked = clicking.join().unwrap();
        assert_eq!(clicked.outcome, Outcome::Unknown, "{name}: {clicked:?}");
        assert_eq!(clicked.reason, Some(Reason::Revoked), "{name}: {clicked:?}");
        // The release never went out; at most our own detach did.
        assert!(
            !world
                .sent()
                .iter()
                .any(|r| key(r) == "Input.dispatchMouseEvent:mouseReleased"
                    && r.session.as_deref() == Some(&cdp)),
            "{name}: mouseReleased was sent"
        );
        let after = world.written_after(revoked);
        assert!(
            after.iter().all(|m| m == "Target.detachFromTarget"),
            "{name}: {after:?}"
        );
        world.assert_idle();
    }
    // After the transfer, s2 owns the tab but s1's refs are stale for it.
    let s2_tabs = tabs_of(&world.act("s2", "tabs.list", json!({})));
    assert_eq!(s2_tabs.len(), 1);
    let moved = TabId(s2_tabs[0].clone());
    let stale = world.act("s2", "click", json!({ "tab": moved, "ref": "e2" }));
    assert_eq!(stale.reason, Some(Reason::RefStale), "{stale:?}");
    let element = world.save_ref("s2", &moved);
    world.release_knobs();
    assert_eq!(world.click("s2", &moved, &element).outcome, Outcome::Done);
}

/// Spec 16.3 against the fake: a session clicks in a loop while the user
/// revokes it after a random delay. No command of the session is written
/// after the revocation, the page's click counter stops, and every request
/// gets exactly one result.
#[test]
fn revocation_race_through_the_whole_host() {
    let world = Arc::new(World::start(|_| {}));
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut random = move |below: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % below
    };
    for iteration in 0..12 {
        world.grant("s1");
        let tab = world.open("s1");
        let cdp = world.cdp_session(&tab);
        let element = world.save_ref("s1", &tab);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let clicker = {
            let world = world.clone();
            let tab = tab.clone();
            let stop = stop.clone();
            thread::spawn(move || {
                let mut results = Vec::new();
                while !stop.load(Ordering::Acquire) {
                    let id = request_id();
                    let result =
                        world.act_as("s1", &id, "click", json!({ "tab": tab, "ref": element }));
                    assert_eq!(result.request_id, id);
                    let finished = result.outcome == Outcome::Blocked;
                    results.push(result);
                    if finished {
                        break;
                    }
                }
                results
            })
        };
        thread::sleep(Duration::from_millis(random(250)));
        world.host.revoke("s1");
        let revoked = world.revoked_at();
        let presses_at_revoke = world.presses(&cdp);
        stop.store(true, Ordering::Release);
        let results = clicker.join().unwrap();
        let after = world.written_after(revoked);
        assert!(
            after.iter().all(|m| m == "Target.detachFromTarget"),
            "iteration {iteration}: {after:?}"
        );
        // A press already in flight at the revocation may still land; none after.
        thread::sleep(Duration::from_millis(50));
        assert!(
            world.presses(&cdp) <= presses_at_revoke + 1,
            "iteration {iteration}: clicks kept landing"
        );
        // Done clicks, then at most one interrupted click (unknown/revoked),
        // then refusals; never a done click after the revocation.
        let first_cut = results
            .iter()
            .position(|r| r.outcome != Outcome::Done)
            .unwrap_or(results.len());
        for result in &results[first_cut..] {
            match result.outcome {
                Outcome::Unknown => assert_eq!(result.reason, Some(Reason::Revoked)),
                Outcome::Blocked => assert!(
                    matches!(
                        result.reason,
                        Some(Reason::Revoked | Reason::NotGranted | Reason::NoLease)
                    ),
                    "iteration {iteration}: {result:?}"
                ),
                _ => panic!("iteration {iteration}: {result:?} after the revocation"),
            }
        }
        assert!(
            results[first_cut..]
                .iter()
                .filter(|r| r.outcome == Outcome::Unknown)
                .count()
                <= 1
        );
        world.assert_idle();
    }
}

// ---------------------------------------------------------------------------
// End to end: MCP server and CLI → control transport → host → semantic layer
// → fake CDP (spec 12, 16.1 "MCP")
// ---------------------------------------------------------------------------

/// A control endpoint for `world.host`, and the session's credentials.
fn control_for(world: &World, session: &str) -> Credentials {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let host = world.host.clone();
    thread::spawn(move || crate::control::serve_browser_for_tests(listener, host));
    Credentials::new(&endpoint, &world.host.token_for(session).unwrap()).unwrap()
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn call_tool(server: &mcp::Server, id: i64, name: &str, arguments: Value) -> Value {
    let reply = server
        .handle(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments },
        }))
        .expect("tools/call gets a reply");
    assert_eq!(reply["id"], json!(id));
    reply["result"].clone()
}

fn tool_text(result: &Value) -> String {
    result["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn mcp_tools_drive_the_fake_browser_through_the_control_transport() {
    let world = Arc::new(World::start(|_| {}));
    world.grant("s1");
    let creds = control_for(&world, "s1");
    let server = mcp::Server::new(Ok(creds), Box::new(Sink::default()));
    let init = server
        .handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}}))
        .unwrap();
    assert!(init["result"]["serverInfo"].is_object(), "{init}");

    let opened = call_tool(
        &server,
        2,
        "browser_tabs",
        json!({ "action": "open", "url": "https://example.com/" }),
    );
    assert_ne!(opened["isError"], json!(true), "{opened}");
    let tab = world.host.policy().read().tabs_for("s1")[0].tab.clone();
    assert!(tool_text(&opened).contains(&tab.0), "{opened}");

    let shot = call_tool(&server, 3, "browser_snapshot", json!({ "tab": tab }));
    let text = tool_text(&shot);
    assert!(text.contains("button \"Save\""), "{shot}");
    let element = save_ref_in(&text);
    let cdp = world.cdp_session(&tab);

    let clicked = call_tool(
        &server,
        4,
        "browser_click",
        json!({ "tab": tab, "ref": element }),
    );
    assert_ne!(clicked["isError"], json!(true), "{clicked}");
    assert_eq!(world.presses(&cdp), 1);

    // Agent mistakes come back as tool errors from the host, not protocol errors.
    let foreign = call_tool(
        &server,
        5,
        "browser_click",
        json!({ "tab": "t_00000000", "ref": element }),
    );
    assert_eq!(foreign["isError"], json!(true), "{foreign}");
    assert!(
        tool_text(&foreign).contains("reason: no_lease"),
        "{foreign}"
    );

    // OFF mid-click through MCP: the tool reports unknown/revoked.
    world.hold("Input.dispatchMouseEvent:mouseReleased");
    let pending = {
        let server = Arc::new(server);
        let mark = world.mark();
        let call = {
            let server = server.clone();
            let tab = tab.clone();
            let element = element.clone();
            thread::spawn(move || {
                call_tool(
                    &server,
                    6,
                    "browser_click",
                    json!({ "tab": tab, "ref": element }),
                )
            })
        };
        world.wait_sent(mark, |r| key(r) == "Input.dispatchMouseEvent:mouseReleased");
        world.host.set_toggle(false);
        let result = call.join().unwrap();
        (server, result)
    };
    let (server, interrupted) = pending;
    let summary = format!("{interrupted}");
    assert!(summary.contains("unknown"), "{summary}");
    assert!(summary.contains("revoked"), "{summary}");

    // While OFF every tool answers blocked/browser_off and nothing is sent.
    let before = world.sent().len();
    let off = call_tool(&server, 7, "browser_snapshot", json!({ "tab": tab }));
    assert_eq!(off["isError"], json!(true), "{off}");
    assert!(tool_text(&off).contains("reason: off"), "{off}");
    assert_eq!(world.sent().len(), before);
    world.release_knobs();
}

#[test]
fn the_cli_drives_the_fake_browser_and_maps_outcomes_to_exit_codes() {
    let world = World::start(|_| {});
    world.grant("s1");
    let creds = control_for(&world, "s1");

    let (out, code) = cli::execute_with(
        &["tabs.open", "--url", "https://example.com/"],
        creds.clone(),
    );
    assert_eq!(code, 0, "{out}");
    let opened: Value = serde_json::from_str(&out).unwrap();
    let tab = opened["data"]["tab"].as_str().unwrap().to_string();

    // A done snapshot prints only its text.
    let (text, code) = cli::execute_with(&["snapshot", "--tab", &tab], creds.clone());
    assert_eq!(code, 0, "{text}");
    let element = save_ref_in(&text);
    let (out, code) =
        cli::execute_with(&["click", "--tab", &tab, "--ref", &element], creds.clone());
    assert_eq!(code, 0, "{out}");
    assert_eq!(world.presses(&world.cdp_session(&TabId(tab.clone()))), 1);

    // blocked → 2, and nothing reaches the browser.
    let before = world.sent().len();
    let (out, code) = cli::execute_with(&["snapshot", "--tab", "t_00000000"], creds.clone());
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("no_lease"), "{out}");
    assert_eq!(world.sent().len(), before);

    // The same request id with the same input is answered from the cache.
    let (first, _) = cli::execute_with(
        &[
            "click",
            "--tab",
            &tab,
            "--ref",
            &element,
            "--request-id",
            "cli-retry",
        ],
        creds.clone(),
    );
    let presses = world.presses(&world.cdp_session(&TabId(tab.clone())));
    let (second, code) = cli::execute_with(
        &[
            "click",
            "--tab",
            &tab,
            "--ref",
            &element,
            "--request-id",
            "cli-retry",
        ],
        creds.clone(),
    );
    assert_eq!(code, 0, "{second}");
    assert_eq!(
        world.presses(&world.cdp_session(&TabId(tab.clone()))),
        presses
    );
    let first: Value = serde_json::from_str(&first).unwrap();
    let second: Value = serde_json::from_str(&second).unwrap();
    assert_eq!(first["outcome"], second["outcome"]);

    // Turn over: blocked/turn_inactive. Revoked token: blocked/revoked.
    world.host.turn_finished("s1");
    let (out, code) = cli::execute_with(&["tabs.list"], creds.clone());
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("turn_inactive"), "{out}");
    world.host.session_deleted("s1");
    let (out, code) = cli::execute_with(&["tabs.list"], creds);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("revoked"), "{out}");
}

// ---------------------------------------------------------------------------
// The MonoCode browser (managed mode) and ask-first access
// ---------------------------------------------------------------------------

fn created_in_background(world: &World) -> Vec<bool> {
    world
        .sent()
        .iter()
        .filter(|r| r.method == "Target.createTarget")
        .map(|r| r.params["background"] == json!(true))
        .collect()
}

#[test]
fn the_monocode_browser_starts_once_for_two_sessions_and_comes_back_after_a_crash() {
    let world = Arc::new(World::managed(AccessMode::Always));
    assert_eq!(
        world.server.connections(),
        0,
        "the switch alone connects nothing"
    );
    world.grant("s1");
    world.grant("s2");

    // Both sessions' first actions arrive while nothing runs: one connection
    // serves both, inside the same tool calls, and nobody is asked to consent.
    let opening: Vec<_> = ["s1", "s2"]
        .into_iter()
        .map(|s| world.spawn_act(s, "tabs.open", json!({ "url": "https://example.com/" })))
        .collect();
    let tabs: Vec<TabId> = opening
        .into_iter()
        .map(|handle| {
            let (opened, _) = handle.join().unwrap();
            assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
            TabId(opened.data.unwrap()["tab"].as_str().unwrap().to_string())
        })
        .collect();
    assert_eq!(world.server.connections(), 1);
    assert_eq!(world.event_count("consent_pending"), 0);
    assert!(world.host.connection().browser.unwrap().managed);
    // Our own browser shows what the agents do: tabs open in the foreground.
    assert_eq!(created_in_background(&world), vec![false, false]);
    for (session, tab) in [("s1", &tabs[0]), ("s2", &tabs[1])] {
        assert_eq!(
            tabs_of(&world.act(session, "tabs.list", json!({}))),
            vec![tab.0.clone()]
        );
    }

    // The browser crashes in the middle of s1's click.
    let element = world.save_ref("s1", &tabs[0]);
    world.hold("Input.dispatchMouseEvent:mouseReleased");
    let mark = world.mark();
    let clicking = {
        let world = world.clone();
        let tab = tabs[0].clone();
        thread::spawn(move || world.click("s1", &tab, &element))
    };
    world.wait_sent(mark, |r| key(r) == "Input.dispatchMouseEvent:mouseReleased");
    world.server.close_current();
    let clicked = clicking.join().unwrap();
    assert_eq!(clicked.outcome, Outcome::Unknown, "{clicked:?}");
    assert_eq!(clicked.reason, Some(Reason::LinkDown));
    // Not "disconnected, approve again": the MonoCode browser just waits for
    // the next action.
    world.wait_state(ConnectionState::Idle);
    world.release_knobs();
    world.assert_idle();
    // The rows follow the state event, which comes right after the state.
    assert!(wait_until(Duration::from_secs(2), || world
        .open_lease_rows()
        == 0));

    // The next action of either session reconnects and is served in the same
    // call; nothing from before the crash is adopted.
    let listed = world.act("s2", "tabs.list", json!({}));
    assert_eq!(listed.outcome, Outcome::Done, "{listed:?}");
    assert!(tabs_of(&listed).is_empty());
    assert_eq!(world.server.connections(), 2);
    let old = world.act("s1", "snapshot", json!({ "tab": tabs[0] }));
    assert_eq!(old.reason, Some(Reason::NoLease), "{old:?}");

    // The browser is closed and starts again on another port (a new process
    // in the same profile): the next action finds it there.
    let (restarted, _page) = fake_browser();
    write_port_file(&world.dir, &restarted);
    world.server.close_current();
    world.wait_state(ConnectionState::Idle);
    let opened = world.act("s1", "tabs.open", json!({}));
    assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
    assert_eq!(restarted.connections(), 1);
    assert_eq!(world.event_count("consent_pending"), 0);
    assert_eq!(world.open_lease_rows(), 1);
    world.host.set_toggle(false);
}

#[test]
fn off_stops_the_click_and_closes_the_monocode_browsers_pages() {
    let world = Arc::new(World::managed(AccessMode::Always));
    world.grant("s1");
    world.page.lock().unwrap().list_pages = true;
    let tab = world.open("s1");
    let cdp = world.cdp_session(&tab);
    let target = world.page.lock().unwrap().sessions[&cdp].clone();
    let element = world.save_ref("s1", &tab);
    world.hold("Input.dispatchMouseEvent:mouseReleased");
    let mark = world.mark();
    let clicking = {
        let world = world.clone();
        let tab = tab.clone();
        thread::spawn(move || world.click("s1", &tab, &element))
    };
    world.wait_sent(mark, |r| key(r) == "Input.dispatchMouseEvent:mouseReleased");
    world.host.set_toggle(false);
    let revoked = world.revoked_at();
    let clicked = clicking.join().unwrap();
    assert_eq!(clicked.outcome, Outcome::Unknown, "{clicked:?}");
    assert_eq!(clicked.reason, Some(Reason::Revoked));
    // After OFF nothing reached the page: only the browser-level cleanup.
    let after = world.written_after(revoked);
    assert!(
        after.iter().all(|m| matches!(
            m.as_str(),
            "Target.getTargets" | "Target.closeTarget" | "Target.detachFromTarget"
        )),
        "{after:?}"
    );
    assert_eq!(world.presses(&cdp), 1);
    // Every page of the MonoCode browser is closed, so it exits on its own;
    // workers are left alone.
    let closed: HashSet<String> = world
        .sent()
        .iter()
        .filter(|r| r.method == "Target.closeTarget")
        .map(|r| r.params["targetId"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(closed, HashSet::from([target, "U1".to_string()]));
    assert_eq!(world.host.link().status(), LinkStatus::Closed);
    assert_eq!(world.host.connection().state, ConnectionState::Off);
    assert!(
        !world.host.policy().read().settings.enabled,
        "OFF is remembered"
    );
    assert_eq!(world.open_lease_rows(), 0);
    world.assert_idle();

    // ON again starts nothing until an agent acts.
    world.release_knobs();
    world.host.set_toggle(true);
    world.wait_state(ConnectionState::Idle);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(world.server.connections(), 1);
    world.grant("s1");
    let opened = world.act("s1", "tabs.open", json!({}));
    assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
    assert_eq!(world.server.connections(), 2);
}

#[test]
fn off_on_a_hung_monocode_browser_keeps_to_the_off_budget() {
    let world = Arc::new(World::managed(AccessMode::Always));
    world.grant("s1");
    world.page.lock().unwrap().list_pages = true;
    let _ = world.open("s1");
    for method in [
        "Target.getTargets",
        "Target.closeTarget",
        "Target.detachFromTarget",
        "Browser.getVersion",
    ] {
        world.hold(method);
    }
    let started = Instant::now();
    world.host.set_toggle(false);
    let took = started.elapsed();
    // The switch is a UI command: a browser that stopped answering must not
    // hold it for the CDP command timeouts.
    assert!(took < Duration::from_secs(2), "OFF took {took:?}");
    assert_eq!(world.host.connection().state, ConnectionState::Off);
    assert_ne!(world.host.link().status(), LinkStatus::Open);
    world.assert_idle();
}

#[test]
fn an_access_request_nobody_answers_expires_without_touching_the_browser() {
    let world = Arc::new(World::managed(AccessMode::Ask));
    world.host.set_ask_wait(Duration::from_millis(800));
    world.host.turn_started("s1", "main", "/repo", None);
    world.host.turn_started("s2", "main", "/repo", None);

    // Two tool calls of s1 and one of s2 arrive without access.
    let first = world.spawn_act("s1", "tabs.list", json!({}));
    assert!(wait_until(Duration::from_secs(5), || world.requested("s1")));
    let second = world.spawn_act("s1", "tabs.open", json!({}));
    let other = world.spawn_act("s2", "tabs.list", json!({}));
    assert!(wait_until(Duration::from_secs(5), || world.requested("s2")));
    // One prompt per thread, not per action.
    assert_eq!(world.event_count("grant_requested"), 2);

    // The user answers s2 only.
    world.host.answer_request("s2", "main", true);
    let (allowed, _) = other.join().unwrap();
    assert_eq!(allowed.outcome, Outcome::Done, "{allowed:?}");
    assert_eq!(world.server.connections(), 1);

    // Nobody answers s1: both of its calls end with not_granted when the
    // wait runs out, and the prompt goes away.
    for handle in [first, second] {
        let (result, took) = handle.join().unwrap();
        assert_eq!(result.outcome, Outcome::Blocked, "{result:?}");
        assert_eq!(result.reason, Some(Reason::NotGranted));
        assert!(took < Duration::from_secs(3), "waited {took:?}");
    }
    assert!(!world.requested("s1"));
    assert_eq!(world.event_count("grant_request_expired"), 1);
    assert!(!world.host.policy().read().granted("s1"));
    // s1 sent nothing to the browser.
    assert!(!world
        .sent()
        .iter()
        .any(|r| r.method == "Target.createTarget"));
    world.assert_idle();

    // An unanswered prompt is not a "Not now": the next call asks again,
    // and the end of the turn withdraws it.
    let again = world.spawn_act("s1", "tabs.list", json!({}));
    assert!(wait_until(Duration::from_secs(5), || world.requested("s1")));
    world.host.turn_finished("s1");
    let (ended, took) = again.join().unwrap();
    assert_eq!(ended.reason, Some(Reason::NotGranted), "{ended:?}");
    assert!(took < Duration::from_millis(700), "waited {took:?}");
    assert!(!world.requested("s1"));
}

#[test]
fn an_access_request_warms_the_monocode_browser_up_but_sends_nothing_to_pages() {
    let world = Arc::new(World::managed(AccessMode::Ask));
    world.host.turn_started("s1", "main", "/repo", None);
    let waiting = world.spawn_act("s1", "tabs.open", json!({ "url": "https://example.com/" }));
    assert!(wait_until(Duration::from_secs(5), || world.requested("s1")));
    // The browser connects while the user decides, so an Allow late in the
    // wait does not add a cold start on top of it.
    world.wait_state(ConnectionState::Connected);
    assert!(world.requested("s1"), "still waiting for the answer");
    assert_eq!(world.server.connections(), 1);
    assert!(!world
        .sent()
        .iter()
        .any(|r| r.method == "Target.createTarget" || r.session.is_some()));

    // "Not now": the action ends without a tab; the warm browser stays
    // connected for the next thread that is allowed.
    world.host.answer_request("s1", "main", false);
    let (declined, _) = waiting.join().unwrap();
    assert_eq!(declined.reason, Some(Reason::NotGranted), "{declined:?}");
    assert!(!world
        .sent()
        .iter()
        .any(|r| r.method == "Target.createTarget"));
    world.grant("s2");
    let opened = world.act("s2", "tabs.open", json!({}));
    assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
    assert_eq!(world.server.connections(), 1);
    world.assert_idle();
}

#[test]
fn off_and_revocation_answer_a_waiting_request_at_once() {
    // The full 45 s wait: each case must end long before it.
    let world = Arc::new(World::managed(AccessMode::Ask));
    world.host.turn_started("s1", "main", "/repo", None);
    world.host.turn_started("s2", "main", "/repo", None);

    let waiting = world.spawn_act("s1", "tabs.list", json!({}));
    assert!(wait_until(Duration::from_secs(5), || world.requested("s1")));
    world.host.revoke("s1");
    let (revoked, took) = waiting.join().unwrap();
    assert_eq!(revoked.reason, Some(Reason::NotGranted), "{revoked:?}");
    assert!(took < Duration::from_secs(2), "waited {took:?}");
    assert!(!world.requested("s1"));

    let waiting = world.spawn_act("s2", "tabs.list", json!({}));
    assert!(wait_until(Duration::from_secs(5), || world.requested("s2")));
    world.host.set_toggle(false);
    let (off, took) = waiting.join().unwrap();
    // The agent learns the browser is off, not that its thread lacks access.
    assert_eq!(off.reason, Some(Reason::Off), "{off:?}");
    assert!(took < Duration::from_secs(2), "waited {took:?}");
    assert!(!world.requested("s2"));
    // A late "Allow" from a prompt still on screen connects nothing. (The
    // request itself already warmed the MonoCode browser up.)
    let before = world.server.connections();
    world.host.answer_request("s2", "main", true);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(world.server.connections(), before);
    assert_eq!(world.host.connection().state, ConnectionState::Off);
    world.assert_idle();
}

/// A stand-in for the MonoCode browser process: this test binary running the
/// ignored test below, which sleeps until it is killed.
fn browser_process() -> std::process::Child {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "browser::resilience::stand_in_browser_process",
            "--ignored",
            "--test-threads=1",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
#[ignore = "the child process browser_process() starts"]
fn stand_in_browser_process() {
    thread::sleep(Duration::from_secs(60));
}

/// A private MonoCode browser that is "running": its own profile folder,
/// where the fake listens, and a process that does not exit on its own.
fn private_browser(world: &World) -> PathBuf {
    let profile = std::env::temp_dir().join(format!("monocode-private-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&profile).unwrap();
    write_port_file(&profile, &world.server);
    world
        .host
        .adopt_managed_for_tests(browser_process(), profile.clone(), true);
    profile
}

#[test]
fn off_returns_at_once_and_the_private_browser_goes_away_in_the_background() {
    let world = Arc::new(World::launched(BrowserMode::Private, AccessMode::Always));
    let profile = private_browser(&world);
    world.grant("s1");
    world.page.lock().unwrap().list_pages = true;
    let _ = world.open("s1");
    assert!(world.host.connection().browser.unwrap().managed);

    // The process does not exit by itself: it is killed after the close
    // wait, and only then is the profile deleted. OFF does not wait for it.
    let started = Instant::now();
    world.host.set_toggle(false);
    let took = started.elapsed();
    assert!(took < Duration::from_secs(1), "OFF took {took:?}");
    assert_eq!(world.host.connection().state, ConnectionState::Off);
    assert!(world
        .sent()
        .iter()
        .any(|r| r.method == "Target.closeTarget" && r.params["targetId"] == "U1"));
    assert!(profile.exists(), "cleanup has not run yet");
    assert!(
        wait_until(Duration::from_secs(10), || !profile.exists()),
        "the private profile is deleted after the process is gone"
    );
}

#[test]
fn a_new_start_waits_for_the_previous_browser_to_be_gone() {
    let world = Arc::new(World::managed(AccessMode::Always));
    world
        .host
        .adopt_managed_for_tests(browser_process(), world.dir.clone(), false);
    world.grant("s1");
    let _ = world.open("s1");
    world.host.set_toggle(false);
    let off_at = Instant::now();
    // ON and an action right away: the old process still holds the profile,
    // so the new connection is made only once it is gone.
    world.host.set_toggle(true);
    world.grant("s1");
    let opened = world.act("s1", "tabs.open", json!({}));
    assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
    let waited = off_at.elapsed();
    assert!(
        waited >= Duration::from_millis(2500),
        "connected after {waited:?}, before the old browser was gone"
    );
    assert_eq!(world.server.connections(), 2);
}

#[test]
fn a_declined_request_closes_the_private_browser_nobody_may_use() {
    let world = Arc::new(World::launched(BrowserMode::Private, AccessMode::Ask));
    world.host.set_unused_grace(Duration::from_millis(400));
    let profile = private_browser(&world);
    world.page.lock().unwrap().list_pages = true;
    world.host.turn_started("s1", "main", "/repo", None);
    let waiting = world.spawn_act("s1", "tabs.list", json!({}));
    assert!(wait_until(Duration::from_secs(5), || world.requested("s1")));
    world.wait_state(ConnectionState::Connected);
    world.host.answer_request("s1", "main", false);
    let (declined, _) = waiting.join().unwrap();
    assert_eq!(declined.reason, Some(Reason::NotGranted), "{declined:?}");
    // Within the grace period the browser stays (the user may still allow
    // another thread).
    assert_eq!(world.host.connection().state, ConnectionState::Connected);
    // Then it closes: its pages, the link, the process and the profile.
    world.wait_state(ConnectionState::Idle);
    assert_eq!(world.event_count("managed_closed_unused"), 1);
    assert_eq!(world.host.link().status(), LinkStatus::Closed);
    assert!(world
        .sent()
        .iter()
        .any(|r| r.method == "Target.closeTarget" && r.params["targetId"] == "U1"));
    assert!(wait_until(Duration::from_secs(10), || !profile.exists()));
    assert!(world.host.policy().read().toggle_on, "the switch stays on");
}

#[test]
fn an_expired_request_closes_the_browser_only_when_no_thread_has_access() {
    let world = Arc::new(World::managed(AccessMode::Ask));
    world.host.set_ask_wait(Duration::from_millis(300));
    world.host.set_unused_grace(Duration::from_millis(300));
    world.grant("s2");
    let opened = world.act("s2", "tabs.open", json!({}));
    assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");

    // s1 asks and nobody answers, but s2 still has access: the browser stays.
    world.host.turn_started("s1", "main", "/repo", None);
    let expired = world.act("s1", "tabs.list", json!({}));
    assert_eq!(expired.reason, Some(Reason::NotGranted), "{expired:?}");
    thread::sleep(Duration::from_millis(1200));
    assert_eq!(world.host.connection().state, ConnectionState::Connected);
    assert_eq!(world.event_count("managed_closed_unused"), 0);

    // Once s2 loses access and s1's next request expires too, it closes.
    world.host.revoke("s2");
    let expired = world.act("s1", "tabs.list", json!({}));
    assert_eq!(expired.reason, Some(Reason::NotGranted), "{expired:?}");
    world.wait_state(ConnectionState::Idle);
    assert_eq!(world.event_count("managed_closed_unused"), 1);
    world.assert_idle();

    // The next allowed thread starts it again.
    world.grant("s2");
    let listed = world.act("s2", "tabs.list", json!({}));
    assert_eq!(listed.outcome, Outcome::Done, "{listed:?}");
    assert_eq!(world.server.connections(), 2);
}
