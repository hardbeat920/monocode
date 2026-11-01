//! Dev servers per checkout: which `localhost` ports the app of each
//! worktree runs on, so a thread's agent and its browser menu know where
//! *this* branch's app is when several branches run at once.
//!
//! MonoCode remembers every process it starts for a thread or a terminal
//! (the root) and the checkout it starts in. A listening port belongs to a
//! checkout when its process descends from one of those roots, shares a
//! root's process group (Unix), or has its working directory inside the
//! checkout (Linux and macOS, so a server started outside MonoCode counts
//! too). Only ports that answer HTTP are kept.

mod os;

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

/// One app a checkout serves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DevServer {
    pub port: u16,
    /// `http://localhost:<port>`.
    pub url: String,
    pub pid: u32,
    /// Executable name of the listening process (`node`, `python3`).
    pub process: String,
    /// The checkout (worktree root) the server belongs to.
    pub checkout: String,
    /// `/` answered with HTML: an app to open, not only an API.
    pub page: bool,
}

/// Spawn history kept for ancestry; a dead root still claims the orphans
/// whose parent pid names it.
const MAX_ROOTS: usize = 256;
/// How long one scan answers repeated questions (popover, turn start).
const SCAN_TTL: Duration = Duration::from_secs(2);
const PROBE_CONNECT: Duration = Duration::from_millis(300);
const PROBE_READ: Duration = Duration::from_millis(700);
const MAX_ANCESTRY: usize = 64;

struct Root {
    pid: u32,
    checkout: String,
    session: Option<String>,
    /// Seen dead once: from then on its pid may belong to someone else, so
    /// it only matches as the dead parent of an orphan.
    gone: bool,
}

static ROOTS: LazyLock<Mutex<Vec<Root>>> = LazyLock::new(|| Mutex::new(Vec::new()));
/// The last scan and when it ran.
type Scan = Option<(Instant, Vec<DevServer>)>;

static SCAN: LazyLock<Mutex<Scan>> = LazyLock::new(|| Mutex::new(None));

/// Record a process MonoCode started in `dir`: a thread's agent (`session`)
/// or a terminal (`None`).
pub fn note_spawn(pid: u32, dir: &Path, session: Option<&str>) {
    if pid == 0 {
        return;
    }
    let checkout = checkout_key(dir);
    let mut roots = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    roots.retain(|root| root.pid != pid);
    if let Some(session) = session {
        for root in roots.iter_mut() {
            if root.session.as_deref() == Some(session) {
                root.session = None;
            }
        }
    }
    roots.push(Root {
        pid,
        checkout,
        session: session.map(str::to_string),
        gone: false,
    });
    let excess = roots.len().saturating_sub(MAX_ROOTS);
    roots.drain(..excess);
}

/// The checkout of the thread's agent, if MonoCode started one this run.
pub fn session_checkout(session: &str) -> Option<String> {
    ROOTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .rev()
        .find(|root| root.session.as_deref() == Some(session))
        .map(|root| root.checkout.clone())
}

/// Dev servers of the checkout that holds `dir`.
pub fn for_dir(dir: &Path) -> Vec<DevServer> {
    of_checkout(&checkout_key(dir))
}

/// Dev servers of the checkout the thread's agent runs in; empty for a
/// session MonoCode did not start, without scanning.
pub fn for_session(session: &str) -> Vec<DevServer> {
    session_checkout(session)
        .map(|checkout| of_checkout(&checkout))
        .unwrap_or_default()
}

fn of_checkout(checkout: &str) -> Vec<DevServer> {
    scan()
        .into_iter()
        .filter(|server| server.checkout == checkout)
        .collect()
}

/// The lines a turn's `<monocode_browser>` block carries about the thread's
/// app; `dir` is the thread's working directory.
pub fn agent_note(dir: &Path, servers: &[DevServer]) -> String {
    let here = dir.display();
    if servers.is_empty() {
        return format!(
            "No dev server of this thread's worktree ({here}) is running. If you start one, start it in this worktree; listing your browser tabs then shows its address."
        );
    }
    let list = servers
        .iter()
        .map(|server| {
            let kind = if server.page { "web page" } else { "HTTP" };
            format!("{} ({}, {kind})", server.url, server.process)
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "This thread's worktree ({here}) serves its app at {list}. Check your changes there: servers on other ports may belong to other threads' branches."
    )
}

fn scan() -> Vec<DevServer> {
    let mut cache = SCAN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, servers)) = cache.as_ref() {
        if at.elapsed() < SCAN_TTL {
            return servers.clone();
        }
    }
    let servers = scan_now();
    *cache = Some((Instant::now(), servers.clone()));
    servers
}

fn scan_now() -> Vec<DevServer> {
    let listeners = os::listeners();
    if listeners.is_empty() {
        return Vec::new();
    }
    let processes = os::processes();
    let roots: Vec<RootView> = {
        let mut roots = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
        for root in roots.iter_mut() {
            root.gone |= !processes.contains_key(&root.pid);
        }
        roots
            .iter()
            .map(|root| RootView {
                pid: root.pid,
                checkout: root.checkout.clone(),
                gone: root.gone,
            })
            .collect()
    };
    let unclaimed: HashSet<u32> = listeners
        .iter()
        .filter(|listener| claim(listener.pid, &processes, &roots).is_none())
        .map(|listener| listener.pid)
        .collect();
    let cwds = os::cwds(&unclaimed);
    let candidates = attribute(&listeners, &processes, &roots, |pid| {
        cwds.get(&pid).map(|cwd| checkout_key(cwd))
    });
    // Probe in parallel: a silent port costs one read timeout, not one each.
    let probes: Vec<_> = candidates
        .into_iter()
        .map(|candidate| {
            std::thread::spawn(move || {
                let page = probe(candidate.port, candidate.v6_only)?;
                Some((candidate, page))
            })
        })
        .collect();
    let mut servers: Vec<DevServer> = probes
        .into_iter()
        .filter_map(|probe| probe.join().ok().flatten())
        .map(|(candidate, page)| DevServer {
            port: candidate.port,
            url: format!("http://localhost:{}", candidate.port),
            pid: candidate.pid,
            process: candidate.process,
            checkout: candidate.checkout,
            page,
        })
        .collect();
    servers.sort_by_key(|server| (server.checkout.clone(), !server.page, server.port));
    servers
}

struct RootView {
    pid: u32,
    checkout: String,
    gone: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct Candidate {
    port: u16,
    pid: u32,
    v6_only: bool,
    process: String,
    checkout: String,
}

/// The checkout of the root that `pid` descends from or shares a process
/// group with.
fn claim(pid: u32, processes: &HashMap<u32, os::Proc>, roots: &[RootView]) -> Option<String> {
    let live = |pid: u32| roots.iter().find(|root| root.pid == pid && !root.gone);
    let mut current = pid;
    for _ in 0..MAX_ANCESTRY {
        let Some(process) = processes.get(&current) else {
            // A dead parent: its orphans still belong to the root it was.
            return roots
                .iter()
                .find(|root| root.pid == current && root.gone)
                .map(|root| root.checkout.clone());
        };
        // A group id is not reused while the group lives, so a dead
        // root's group still names it, unless a new process took its pid.
        if let Some(root) = process.pgid.and_then(|pgid| {
            roots
                .iter()
                .find(|root| root.pid == pgid && (!root.gone || !processes.contains_key(&pgid)))
        }) {
            return Some(root.checkout.clone());
        }
        if current != pid {
            if let Some(root) = live(current) {
                return Some(root.checkout.clone());
            }
        }
        if process.ppid == 0 || process.ppid == current {
            return None;
        }
        current = process.ppid;
    }
    None
}

/// Listeners that belong to a checkout, minus the ones that are not a
/// branch's app: MonoCode itself, the agents and shells it started, and
/// browsers (their remote debugging port answers HTTP too).
fn attribute(
    listeners: &[os::Listener],
    processes: &HashMap<u32, os::Proc>,
    roots: &[RootView],
    cwd_checkout: impl Fn(u32) -> Option<String>,
) -> Vec<Candidate> {
    let own = std::process::id();
    listeners
        .iter()
        .filter(|listener| listener.pid != own)
        .filter(|listener| {
            !roots
                .iter()
                .any(|root| root.pid == listener.pid && !root.gone)
        })
        .filter_map(|listener| {
            let process = processes
                .get(&listener.pid)
                .map(|process| process.name.clone())
                .unwrap_or_default();
            if is_browser(&process) {
                return None;
            }
            let checkout =
                claim(listener.pid, processes, roots).or_else(|| cwd_checkout(listener.pid))?;
            Some(Candidate {
                port: listener.port,
                pid: listener.pid,
                v6_only: listener.v6_only,
                process,
                checkout,
            })
        })
        .collect()
}

fn is_browser(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "chrome", "chromium", "msedge", "brave", "vivaldi", "opera", "firefox", "safari",
    ]
    .iter()
    .any(|browser| name.contains(browser))
        || name == "arc"
}

/// `Some(page)` when the port answers HTTP; `page` when `/` is HTML. A
/// server that accepts but has not answered yet (a dev server compiling its
/// first page) counts as HTTP; one that answers something else does not.
fn probe(port: u16, v6_only: bool) -> Option<bool> {
    let address: SocketAddr = if v6_only {
        (std::net::Ipv6Addr::LOCALHOST, port).into()
    } else {
        (std::net::Ipv4Addr::LOCALHOST, port).into()
    };
    let mut stream = TcpStream::connect_timeout(&address, PROBE_CONNECT).ok()?;
    let _ = stream.set_write_timeout(Some(PROBE_CONNECT));
    let request = format!(
        "GET / HTTP/1.1\r\nHost: localhost:{port}\r\nAccept: text/html,*/*\r\nUser-Agent: MonoCode\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;
    let deadline = Instant::now() + PROBE_READ;
    let mut head = Vec::new();
    let mut buffer = [0u8; 2048];
    while head.len() < 8192 && !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            break;
        }
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => head.extend_from_slice(&buffer[..read]),
            Err(_) => break,
        }
    }
    classify(&head)
}

fn classify(head: &[u8]) -> Option<bool> {
    if head.is_empty() {
        return Some(false);
    }
    let text = String::from_utf8_lossy(head);
    if !text.starts_with("HTTP/") {
        return None;
    }
    let html = text.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("content-type")
                && value.to_ascii_lowercase().contains("text/html")
        })
    });
    Some(html)
}

/// The checkout that holds `dir`: the nearest directory with a `.git`
/// entry (a worktree has a `.git` file), else `dir` itself. Canonical, and
/// lowercase on Windows, so it compares as a key.
pub fn checkout_key(dir: &Path) -> String {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let root = dir
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
        .unwrap_or(dir);
    key(&root)
}

fn key(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    };
    let text = text.strip_prefix("//?/").unwrap_or(&text).to_string();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::os::{
        parse_lsof_listen, parse_proc_net_tcp, parse_proc_stat, parse_ps, Listener, Proc,
    };
    use super::*;

    fn proc(ppid: u32, pgid: Option<u32>, name: &str) -> Proc {
        Proc {
            ppid,
            pgid,
            name: name.into(),
        }
    }

    fn root(pid: u32, checkout: &str, gone: bool) -> RootView {
        RootView {
            pid,
            checkout: checkout.into(),
            gone,
        }
    }

    fn listen(port: u16, pid: u32) -> Listener {
        Listener {
            port,
            pid,
            v6_only: false,
        }
    }

    #[test]
    fn a_server_belongs_to_the_checkout_whose_agent_started_it() {
        // agent 100 (wt-a) → bash 101 → npm 102 → node 103 :5173
        // agent 200 (wt-b) → bash 201 → node 203 :5174
        let processes = HashMap::from([
            (100, proc(1, Some(100), "claude")),
            (101, proc(100, Some(101), "bash")),
            (102, proc(101, Some(101), "npm")),
            (103, proc(102, Some(101), "node")),
            (200, proc(1, Some(200), "codex")),
            (201, proc(200, Some(201), "bash")),
            (203, proc(201, Some(201), "node")),
        ]);
        let roots = [root(100, "/wt-a", false), root(200, "/wt-b", false)];
        let found = attribute(
            &[listen(5173, 103), listen(5174, 203)],
            &processes,
            &roots,
            |_| None,
        );
        assert_eq!(
            found
                .iter()
                .map(|c| (c.port, c.checkout.as_str(), c.process.as_str()))
                .collect::<Vec<_>>(),
            [(5173, "/wt-a", "node"), (5174, "/wt-b", "node")]
        );
    }

    #[test]
    fn an_orphan_keeps_its_checkout_through_the_dead_parent_or_its_group() {
        // Windows: the shell that started node exited; node's parent pid
        // still names the dead agent.
        let processes = HashMap::from([(301, proc(300, None, "node"))]);
        let roots = [root(300, "/wt-c", true)];
        let found = attribute(&[listen(3000, 301)], &processes, &roots, |_| None);
        assert_eq!(found[0].checkout, "/wt-c");

        // Unix: reparented to init after the agent exited, but still in the
        // agent's process group.
        let processes = HashMap::from([
            (1, proc(0, Some(1), "init")),
            (401, proc(1, Some(400), "node")),
        ]);
        let roots = [root(400, "/wt-d", true)];
        let found = attribute(&[listen(4000, 401)], &processes, &roots, |_| None);
        assert_eq!(found[0].checkout, "/wt-d");
    }

    #[test]
    fn a_reused_pid_of_a_dead_root_claims_nothing_alive() {
        // Root 500 died; an unrelated process now has pid 500.
        let processes = HashMap::from([
            (500, proc(1, Some(500), "systemd-thing")),
            (501, proc(500, Some(500), "node")),
        ]);
        let roots = [root(500, "/wt-e", true)];
        assert!(attribute(&[listen(5000, 501)], &processes, &roots, |_| None).is_empty());
    }

    #[test]
    fn agents_browsers_and_strangers_are_not_apps_but_a_server_in_the_checkout_is() {
        let processes = HashMap::from([
            (600, proc(1, Some(600), "opencode")),
            (601, proc(600, Some(600), "chrome")),
            (602, proc(1, Some(602), "postgres")),
            (603, proc(1, Some(603), "node")),
        ]);
        let roots = [root(600, "/wt-f", false)];
        let found = attribute(
            &[
                listen(4096, 600),
                listen(9222, 601),
                listen(5432, 602),
                listen(8080, 603),
            ],
            &processes,
            &roots,
            |pid| (pid == 603).then(|| "/wt-f".to_string()),
        );
        assert_eq!(
            found.iter().map(|c| c.port).collect::<Vec<_>>(),
            [8080],
            "the agent's own port, the browser and a server outside any checkout are left out"
        );
    }

    #[test]
    fn proc_net_tcp_keeps_loopback_and_wildcard_listeners() {
        let v4 = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 11111 1 0000000000000000 100 0 0 10 0
   1: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 22222 1 0000000000000000 100 0 0 10 0
   2: 0F02000A:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 33333 1 0000000000000000 100 0 0 10 0
   3: 0100007F:1436 0100007F:9C40 01 00000000:00000000 00:00000000 00000000  1000        0 44444 1 0000000000000000 100 0 0 10 0";
        assert_eq!(
            parse_proc_net_tcp(v4, false),
            [(11111, 5173), (22222, 8080)],
            "a LAN-only bind and an established connection are skipped"
        );
        let v6 = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:1436 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 55555 1 0000000000000000 100 0 0 10 0
   1: 00000000000000000000000000000000:0BB8 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 66666 1 0000000000000000 100 0 0 10 0";
        assert_eq!(parse_proc_net_tcp(v6, true), [(55555, 5174), (66666, 3000)]);
    }

    #[test]
    fn proc_stat_survives_odd_process_names() {
        let stat = "4242 (node (dev) x) S 4100 4000 4000 0 -1 4194560 1 0 0 0";
        assert_eq!(
            parse_proc_stat(stat),
            Some(proc(4100, Some(4000), "node (dev) x"))
        );
    }

    #[test]
    fn lsof_and_ps_output_parse() {
        let lsof = "p812\nn*:5173\np813\nn127.0.0.1:3000\nn[::1]:3000\np814\nn192.168.1.4:80\np815\nn[::1]:4321\n";
        assert_eq!(
            parse_lsof_listen(lsof),
            [
                Listener {
                    port: 3000,
                    pid: 813,
                    v6_only: false
                },
                Listener {
                    port: 4321,
                    pid: 815,
                    v6_only: true
                },
                Listener {
                    port: 5173,
                    pid: 812,
                    v6_only: false
                },
            ]
        );
        let ps = "  812   700   700 /usr/local/bin/node\n  700     1   700 /Applications/Visual Studio Code.app/x\n";
        let table = parse_ps(ps);
        assert_eq!(table[&812], proc(700, Some(700), "node"));
        assert_eq!(table[&700].name, "x");
    }

    #[test]
    fn probe_answers_tell_pages_from_apis_and_non_http() {
        assert_eq!(
            classify(b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n"),
            Some(true)
        );
        assert_eq!(
            classify(b"HTTP/1.1 404 Not Found\r\ncontent-type: application/json\r\n\r\n"),
            Some(false)
        );
        assert_eq!(classify(b""), Some(false), "accepted, still compiling");
        assert_eq!(classify(b"-ERR unknown command\r\n"), None);
    }

    #[test]
    fn the_agent_note_names_the_worktree_and_its_app() {
        let dir = Path::new("/work/wt-a");
        assert!(
            agent_note(dir, &[]).contains("No dev server of this thread's worktree (/work/wt-a)")
        );
        let note = agent_note(
            dir,
            &[DevServer {
                port: 5174,
                url: "http://localhost:5174".into(),
                pid: 9,
                process: "node".into(),
                checkout: "/work/wt-a".into(),
                page: true,
            }],
        );
        assert!(note.contains("serves its app at http://localhost:5174 (node, web page)"));
    }

    /// The whole path on this machine: a process MonoCode starts in a
    /// worktree runs a child that serves HTML on a free port.
    #[cfg(unix)]
    #[test]
    fn a_real_child_server_is_found_for_its_worktree_and_not_for_another() {
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("python3 missing; skipped");
            return;
        }
        let base = std::env::temp_dir().join(format!("monocode-dev-{}", uuid::Uuid::new_v4()));
        let mine = base.join("wt-mine");
        let other = base.join("wt-other");
        for dir in [&mine, &other] {
            std::fs::create_dir_all(dir.join("src")).unwrap();
            std::fs::write(dir.join(".git"), "gitdir: elsewhere\n").unwrap();
        }
        let port_file = base.join("port");
        let server = format!(
            r#"
import http.server, socketserver
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200); self.send_header("Content-Type", "text/html"); self.end_headers(); self.wfile.write(b"<p>hi</p>")
    def log_message(self, *a): pass
s = socketserver.TCPServer(("127.0.0.1", 0), H)
open({port:?}, "w").write(str(s.server_address[1]))
s.serve_forever()
"#,
            port = port_file.display().to_string()
        );
        // A shell stands in for the agent: the server is its grandchild.
        let mut agent = std::process::Command::new("sh")
            .arg("-c")
            .arg("python3 -c \"$0\"; :")
            .arg(&server)
            .current_dir(mine.join("src"))
            .spawn()
            .unwrap();
        note_spawn(agent.id(), &mine.join("src"), Some("dev-test-session"));
        let deadline = Instant::now() + Duration::from_secs(10);
        let port: u16 = loop {
            if let Some(port) = std::fs::read_to_string(&port_file)
                .ok()
                .and_then(|text| text.trim().parse().ok())
            {
                break port;
            }
            assert!(Instant::now() < deadline, "the test server did not start");
            std::thread::sleep(Duration::from_millis(50));
        };
        let found = scan_now();
        let _ = agent.kill();
        let _ = std::process::Command::new("pkill")
            .args(["-f", &port_file.display().to_string()])
            .status();
        let _ = agent.wait();
        let _ = std::fs::remove_dir_all(&base);

        let server = found
            .iter()
            .find(|server| server.port == port)
            .expect("the child's server is found");
        assert_eq!(server.checkout, checkout_key(&mine));
        assert!(server.page, "it serves HTML");
        assert_eq!(server.url, format!("http://localhost:{port}"));
        assert_ne!(server.checkout, key(&other));
        assert_eq!(
            session_checkout("dev-test-session").as_deref(),
            Some(server.checkout.as_str())
        );
    }
}
