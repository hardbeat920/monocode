//! A title the host changes reaches open remote views, busy or not, and is never sent back.
use super::*;
use gpui::TestAppContext;
use monocode_core::store::Store;
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;

#[derive(Default)]
struct HostState {
    rows: Vec<Value>,
    requests: Vec<Value>,
}

struct FakeHost {
    client: Arc<monocode_core::remote::Connections>,
    environment: String,
    state: Arc<Mutex<HostState>>,
    stop: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<()>>,
    directory: std::path::PathBuf,
}

fn host_row(id: &str, title: &str) -> Value {
    json!({"id":id,"title":title,"harness":"codex","model":"codex:test","cwd":"/host/ws","updatedAt":1000,
        "status":"idle","archived":false,"pinned":false,"hasMessages":true})
}

impl FakeHost {
    /// Serves the project list only; every request is recorded.
    fn new(rows: Vec<Value>) -> Self {
        use std::io::{BufRead, Read, Write};
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // The clock ticks in microseconds here; parallel hosts need a distinct identity.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let environment = format!("remote-title-{}-{nonce}-{serial}", std::process::id());
        let directory = std::env::temp_dir().join(&environment);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(HostState {
            rows,
            ..Default::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (served, done, env) = (state.clone(), stop.clone(), environment.clone());
        let server = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("host accept: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':')
                        && key.eq_ignore_ascii_case("content-length")
                    {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let request: Value = serde_json::from_slice(&body).unwrap();
                let mut state = served.lock().unwrap();
                state.requests.push(request.clone());
                let body = match request["method"].as_str().unwrap_or_default() {
                    "environment.describe" => {
                        json!({"result":{"protocolVersion":1,"environmentId":env,
                        "name":"Fake","providers":["codex"],"capabilities":["sessions.sync"]}})
                    }
                    "sessions.list" => json!({"result":state.rows.clone()}),
                    other => json!({"error":format!("not served: {other}")}),
                }
                .to_string();
                drop(state);
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let client = Arc::new(monocode_core::remote::Connections::new(&directory));
        client.connect(String::new(), url, "a".repeat(43)).unwrap();
        Self {
            client,
            environment,
            state,
            stop,
            server: Some(server),
            directory,
        }
    }
    fn set(&self, id: &str, key: &str, value: Value) {
        for row in &mut self.state.lock().unwrap().rows {
            if row["id"] == id {
                row[key] = value.clone();
            }
        }
    }
    fn sent(&self, method: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|r| r["method"] == method)
            .count()
    }
}
impl Drop for FakeHost {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

struct Ui {
    window: gpui::WindowHandle<Shell>,
    chat: i64,
}

/// A Shell over one host conversation, open in a background tab of its project.
fn open(cx: &mut TestAppContext, host: &FakeHost) -> Ui {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let project = store
        .remember_remote_project(
            &host.environment,
            &json!({"id":"p","cwd":"/host/ws","name":"WS"}),
        )
        .unwrap();
    let values = host.state.lock().unwrap().rows.clone();
    let chat = store
        .register_remote_summary(&project, &values[0])
        .unwrap()
        .id;
    // The active tab is a local chat: an active remote chat would load through the
    // process-wide connections instead of this host.
    let local = store.create("codex", "/work/local").unwrap().id;
    let summaries = store.sessions().unwrap();
    let mut state = Workspace::default();
    state.add_project("/work/local");
    state.add_project(&project.key);
    state.open(chat);
    state.open(local);
    let saved = serde_json::to_string(&state).unwrap();
    let store = Arc::new(Mutex::new(store));
    let client = host.client.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| {
                let mut shell = Shell::new(store, summaries, project.key.clone(), Some(saved), cx);
                shell.remote = client;
                shell.workspace.project = project.key;
                shell.ensure_session(chat, cx);
                shell
            })
        })
        .unwrap()
    });
    cx.refresh().unwrap();
    cx.run_until_parked();
    let ui = Ui { window, chat };
    wait(cx, |cx| {
        read(&ui, cx, |s, _| s.sessions.contains_key(&chat))
    });
    ui
}

fn read<R>(
    ui: &Ui,
    cx: &mut TestAppContext,
    f: impl FnOnce(&mut Shell, &mut Context<Shell>) -> R,
) -> R {
    ui.window.update(cx, |shell, _, cx| f(shell, cx)).unwrap()
}

/// Bounded: a failed condition panics rather than hanging the suite.
fn wait(cx: &mut TestAppContext, mut done: impl FnMut(&mut TestAppContext) -> bool) {
    for _ in 0..3000 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("condition not reached");
}

fn refresh(ui: &Ui, cx: &mut TestAppContext) {
    let requests = read(ui, cx, |shell, _| shell.remote_refreshing);
    assert!(!requests, "a refresh is already in flight");
    read(ui, cx, |shell, cx| shell.refresh_remote_projects(cx));
    wait(cx, |cx| !read(ui, cx, |shell, _| shell.remote_refreshing));
}

/// Every title surface the shell derives from the live view.
fn surfaces(ui: &Ui, cx: &mut TestAppContext) -> [String; 3] {
    read(ui, cx, |shell, cx| {
        let tab = shell.workspace.tab_of(ui.chat).expect("open tab");
        let metadata = shell.session_metadata(ui.chat, cx);
        [
            metadata.summary.title.to_string(),
            metadata.display_title(),
            shell.tab_copy(tab, cx).headline,
        ]
    })
}

fn set_busy(ui: &Ui, cx: &mut TestAppContext, busy: bool) {
    read(ui, cx, |shell, cx| {
        shell.sessions[&ui.chat].update(cx, |view, _| {
            view.busy = busy;
            // A running turn owns these; an index must not rewind them.
            view.summary.updated_at = 5555;
        });
    });
}

#[gpui::test]
fn a_host_title_in_a_refresh_reaches_a_busy_view_without_a_rename(cx: &mut TestAppContext) {
    let host = FakeHost::new(vec![host_row("chat", "New chat")]);
    let ui = open(cx, &host);
    assert_eq!(surfaces(&ui, cx), ["New chat"; 3]);
    set_busy(&ui, cx, true);
    host.set("chat", "title", json!("Named by the host"));
    host.set("chat", "updatedAt", json!(9000));
    refresh(&ui, cx);
    assert_eq!(surfaces(&ui, cx), ["Named by the host"; 3]);
    read(&ui, cx, |shell, cx| {
        let view = shell.sessions[&ui.chat].read(cx);
        assert!(view.busy, "the title must not end the running turn");
        assert_eq!(view.summary.updated_at, 5555, "turn-owned fields are kept");
        assert_eq!(view.display_title(), "Named by the host");
    });
    // The turn ending does not need another index to show it.
    set_busy(&ui, cx, false);
    assert_eq!(surfaces(&ui, cx), ["Named by the host"; 3]);
    assert_eq!(
        host.sent("sessions.update"),
        0,
        "a host title is never sent back"
    );
    read(&ui, cx, |shell, _| {
        assert!(shell.error.is_none(), "{:?}", shell.error)
    });
}

#[gpui::test]
fn an_idle_view_still_takes_the_whole_host_summary(cx: &mut TestAppContext) {
    let host = FakeHost::new(vec![host_row("chat", "New chat")]);
    let ui = open(cx, &host);
    set_busy(&ui, cx, false);
    host.set("chat", "title", json!("Named by the host"));
    host.set("chat", "updatedAt", json!(9000));
    refresh(&ui, cx);
    assert_eq!(surfaces(&ui, cx), ["Named by the host"; 3]);
    read(&ui, cx, |shell, cx| {
        assert_eq!(shell.sessions[&ui.chat].read(cx).summary.updated_at, 9000);
    });
    assert_eq!(host.sent("sessions.update"), 0);
}

#[gpui::test]
fn a_busy_view_keeps_its_title_when_the_host_has_not_changed_it(cx: &mut TestAppContext) {
    let host = FakeHost::new(vec![host_row("chat", "New chat")]);
    let ui = open(cx, &host);
    set_busy(&ui, cx, true);
    refresh(&ui, cx);
    read(&ui, cx, |shell, cx| {
        let view = shell.sessions[&ui.chat].read(cx);
        assert_eq!(view.summary.title, "New chat");
        assert_eq!(view.summary.updated_at, 5555);
    });
    assert_eq!(host.sent("sessions.update"), 0);
}
