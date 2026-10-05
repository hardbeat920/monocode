use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use monocode_harness::core::child::ChildEvent;

use super::*;

const TEST_NAME: &str =
    "child_backend::tests::provider_guard_tests::unexpected_host_exit_stops_the_provider_group";
const ROOT_ENV: &str = "MONOCODE_TEST_PROVIDER_GUARD_ROOT";
const PROVIDER_ENV: &str = "MONOCODE_TEST_PROVIDER_GUARD_BINARY";

struct OwnedProcesses {
    root: PathBuf,
    provider: PathBuf,
    host: Option<Child>,
    sentinel: Child,
}

impl Drop for OwnedProcesses {
    fn drop(&mut self) {
        if let Some(host) = self.host.as_mut() {
            let _ = host.kill();
            let _ = host.wait();
        }
        for name in ["provider", "descendant"] {
            if let Some((pid, _)) = read_identity(&self.root, name) {
                let _ = Command::new(&self.provider)
                    .args(["stop", &pid.to_string()])
                    .status();
            }
        }
        let _ = self.sentinel.kill();
        let _ = self.sentinel.wait();
    }
}

fn read_identity(root: &Path, name: &str) -> Option<(u32, u32)> {
    let text = std::fs::read_to_string(root.join(format!("{name}.pid"))).ok()?;
    let mut fields = text.split_whitespace();
    Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
}

fn probe(provider: &Path, mode: &str, pid: u32) -> bool {
    Command::new(provider)
        .args([mode, &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn wait_until(condition: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    true
}

fn write_marker(root: &Path, name: &str, bytes: &[u8]) {
    let staging = root.join(format!("{name}.tmp"));
    std::fs::write(&staging, bytes).unwrap();
    std::fs::rename(staging, root.join(name)).unwrap();
}

fn compiled_provider(root: &Path) -> PathBuf {
    let source = root.join("provider_fixture.rs");
    let provider = root.join(if cfg!(windows) { "codex.exe" } else { "codex" });
    std::fs::write(&source, include_str!("provider_guard_fixture.rs")).unwrap();
    let mut rustc = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    rustc.args([
        "--edition=2024",
        "--crate-name=monocode_guard_fixture",
        "-C",
        "debuginfo=0",
    ]);
    #[cfg(windows)]
    rustc.args(["-C", "target-feature=+crt-static"]);
    let output = rustc.arg(source).arg("-o").arg(&provider).output().unwrap();
    assert!(
        output.status.success(),
        "native provider fixture compilation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    provider
}

fn run_host_fixture(root: PathBuf, provider: PathBuf) {
    let runtime = HostRuntime::new(2);
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let (children, _backend) = host_children(
        data,
        HashMap::from([(HarnessId::Codex, provider.clone())]),
        runtime.spawner(),
    );
    let events = children.watch_child("guard-contract");
    smol::block_on(children.spawn_child(
        "guard-contract",
        &provider.to_string_lossy(),
        vec!["provider".into(), root.to_string_lossy().into_owned()],
        &root.to_string_lossy(),
        None,
        Some(HarnessId::Codex),
    ))
    .unwrap();
    smol::block_on(children.write_child("guard-contract", "ping")).unwrap();
    smol::block_on(async {
        loop {
            match events.recv().await.unwrap() {
                ChildEvent::Stdout(line) if line == "pong" => break,
                ChildEvent::Exit(_) => panic!("the provider exited before its protocol reply"),
                _ => {}
            }
        }
    });
    write_marker(&root, "host-ready", b"pong\n");
    while !root.join("host-action").is_file() {
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(std::fs::read(root.join("host-action")).unwrap(), b"quit\n");
    smol::block_on(children.write_child("guard-contract", "quit")).unwrap();
    smol::block_on(async {
        loop {
            if let ChildEvent::Exit(code) = events.recv().await.unwrap() {
                assert_eq!(code, Some(0));
                break;
            }
        }
    });
    write_marker(&root, "host-completed", b"0\n");
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn start_host(root: &Path, provider: &Path) -> Child {
    let output = File::create(root.join("host-output.log")).unwrap();
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(ROOT_ENV, root)
        .env(PROVIDER_ENV, provider)
        .stdin(Stdio::null())
        .stdout(output.try_clone().unwrap())
        .stderr(output)
        .spawn()
        .unwrap()
}

fn start_sentinel(root: &Path, provider: &Path) -> Child {
    let mut command = Command::new(provider);
    command
        .arg("sentinel")
        .arg(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().unwrap()
}

#[cfg(unix)]
fn guardian_pids(host: u32, provider: u32) -> Vec<u32> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,pgid="])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let fields = line
                .split_whitespace()
                .filter_map(|field| field.parse::<u32>().ok())
                .collect::<Vec<_>>();
            (fields.len() == 3
                && fields[1] == host
                && fields[0] == fields[2]
                && fields[0] != provider)
                .then(|| fields[0])
        })
        .collect()
}

/// The host subprocess enters through this same compiled test executable.
/// Its provider and descendant are a separate native executable, without Node.
#[test]
fn unexpected_host_exit_stops_the_provider_group() {
    if let Some(root) = std::env::var_os(ROOT_ENV) {
        run_host_fixture(root.into(), std::env::var_os(PROVIDER_ENV).unwrap().into());
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let provider = compiled_provider(root.path());
    let sentinel = start_sentinel(root.path(), &provider);
    let host = start_host(root.path(), &provider);
    let mut owned = OwnedProcesses {
        root: root.path().to_owned(),
        provider: provider.clone(),
        host: Some(host),
        sentinel,
    };
    assert!(
        wait_until(|| root.path().join("host-ready").is_file()
            && read_identity(root.path(), "provider").is_some()
            && read_identity(root.path(), "descendant").is_some()
            && read_identity(root.path(), "sentinel").is_some()),
        "the actual headless spawn must complete its protocol reply: {}",
        std::fs::read_to_string(root.path().join("host-output.log")).unwrap()
    );
    assert_eq!(
        std::fs::read(root.path().join("host-ready")).unwrap(),
        b"pong\n"
    );
    let (provider_pid, group) = read_identity(root.path(), "provider").unwrap();
    let (descendant, descendant_group) = read_identity(root.path(), "descendant").unwrap();
    let (sentinel, sentinel_group) = read_identity(root.path(), "sentinel").unwrap();
    assert_ne!(provider_pid, descendant);
    assert_eq!(sentinel, owned.sentinel.id());
    assert!(probe(&provider, "alive", provider_pid));
    assert!(probe(&provider, "alive", descendant));
    #[cfg(unix)]
    {
        assert_eq!(group, provider_pid, "the provider must lead its own group");
        assert_eq!(descendant_group, group);
        assert_ne!(sentinel_group, group);
    }
    #[cfg(windows)]
    let _ = (group, descendant_group, sentinel_group);
    #[cfg(unix)]
    let guardians = guardian_pids(owned.host.as_ref().unwrap().id(), provider_pid);
    let host = owned.host.as_mut().unwrap();
    host.kill().unwrap();
    let status = host.wait().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(9), "the host must die without Drop");
    }
    #[cfg(windows)]
    assert!(!status.success(), "the host must die without Drop");
    assert!(
        wait_until(
            || !probe(&provider, "alive", provider_pid) && !probe(&provider, "alive", descendant)
        ),
        "hard host exit left provider PID {provider_pid} or descendant PID {descendant} alive"
    );
    #[cfg(unix)]
    {
        assert!(
            !probe(&provider, "group-alive", group),
            "the owned provider group {group} must be gone"
        );
        assert!(wait_until(|| guardians
            .iter()
            .all(|pid| !probe(&provider, "alive", *pid))));
    }
    assert!(
        probe(&provider, "alive", sentinel),
        "the unrelated native child must survive"
    );
}

#[cfg(unix)]
#[test]
fn ordinary_provider_exit_reaps_the_descendant_and_guard() {
    let root = tempfile::tempdir().unwrap();
    let provider = compiled_provider(root.path());
    let sentinel = start_sentinel(root.path(), &provider);
    let host = start_host(root.path(), &provider);
    let mut owned = OwnedProcesses {
        root: root.path().to_owned(),
        provider: provider.clone(),
        host: Some(host),
        sentinel,
    };
    assert!(wait_until(|| root.path().join("host-ready").is_file()
        && read_identity(root.path(), "descendant").is_some()));
    let (provider_pid, group) = read_identity(root.path(), "provider").unwrap();
    let (descendant, descendant_group) = read_identity(root.path(), "descendant").unwrap();
    assert_eq!(group, provider_pid);
    assert_eq!(descendant_group, group);
    assert!(wait_until(|| guardian_pids(
        owned.host.as_ref().unwrap().id(),
        provider_pid
    )
    .len()
        == 1));
    let guardians = guardian_pids(owned.host.as_ref().unwrap().id(), provider_pid);
    assert_eq!(
        guardians.len(),
        1,
        "the native provider must have one guardian"
    );
    write_marker(root.path(), "host-action", b"quit\n");
    assert!(wait_until(|| root.path().join("host-completed").is_file()));
    assert_eq!(
        std::fs::read(root.path().join("host-completed")).unwrap(),
        b"0\n"
    );
    assert!(wait_until(|| !probe(&provider, "alive", provider_pid)
        && !probe(&provider, "alive", descendant)
        && guardians.iter().all(|pid| !probe(&provider, "alive", *pid))));
    assert!(!probe(&provider, "group-alive", group));
    assert!(owned.host.as_mut().unwrap().try_wait().unwrap().is_none());
    assert!(probe(&provider, "alive", owned.sentinel.id()));
}
