//! Ports of host/cli.test.ts and host/connect.test.ts. They start real,
//! detached host processes. Until the app binary exists, the host program
//! is this test binary, run through a wrapper script that selects
//! [`child_host`] and passes the host arguments in an environment variable.

use super::*;
use crate::host::network::parse_pairing_link;
use crate::host::runtime::RuntimeBundle;
use crate::host::server::HOST_VERSION;
use crate::host::server::tests::post;
use crate::host::test_backend::TestBackend;
use crate::remote::{Remote, remote_pair, remote_request};
use monocode_core::HarnessId;
use std::time::Instant;

const ARGS: &str = "MONOCODE_HOST_TEST_ARGS";
const WRAPPER: &str = "monocode-host-test";

/// Runs a host command in a child process started by the tests below.
#[test]
#[ignore = "run by the CLI tests as a separate host process"]
fn child_host() {
    let Ok(raw) = std::env::var(ARGS) else {
        return;
    };
    let args: Vec<String> = raw
        .split('\u{1f}')
        .filter(|arg| !arg.is_empty())
        .map(String::from)
        .collect();
    let runtime = RuntimeSource {
        source: std::env::temp_dir(),
        bundle: RuntimeBundle::native(WRAPPER),
        interpreter: None,
        args: Vec::new(),
    };
    let code = main(&args, &runtime, HOST_VERSION, |store| {
        Ok(TestBackend::new(store, vec![HarnessId::Codex]))
    });
    std::process::exit(code);
}

/// A folder holding the wrapper script, as the host's "bundle".
fn wrapper_runtime(folder: &Path) -> RuntimeSource {
    let script = folder.join(WRAPPER);
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n{ARGS}=\"$(printf '%s\\037' \"$@\")\" exec '{}' --exact host::cli::tests::child_host --ignored --nocapture\n",
            std::env::current_exe().unwrap().display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    RuntimeSource {
        source: folder.to_path_buf(),
        bundle: RuntimeBundle::native(WRAPPER),
        interpreter: None,
        args: Vec::new(),
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Host {
    _root: tempfile::TempDir,
    data: PathBuf,
    port: u16,
    runtime: RuntimeSource,
}

impl Host {
    fn new(prefix: &str) -> Self {
        let root = crate::host::store::tests::temporary(prefix);
        let data = root.path().join("data");
        std::fs::create_dir(&data).unwrap();
        let bundle = root.path().join("bundle");
        std::fs::create_dir(&bundle).unwrap();
        Self {
            runtime: wrapper_runtime(&bundle),
            data,
            port: free_port(),
            _root: root,
        }
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let mut all: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        all.extend([
            "--data-dir".into(),
            self.data.to_string_lossy().into_owned(),
            "--port".into(),
            self.port.to_string(),
        ]);
        let (mut out, stdout, _stderr) = Output::captured();
        run(
            &all,
            &self.runtime,
            HOST_VERSION,
            |store| Ok(TestBackend::new(store, vec![HarnessId::Codex])),
            &mut out,
        )
        .map(|()| stdout.text())
    }

    fn wait_stopped(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.data.join("running.json").exists() {
            assert!(Instant::now() < deadline, "the host did not stop");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if let Some(state) = read_running(&self.data) {
            if lifecycle(&state, LifecycleAction::Stop).is_err() {
                // SAFETY: signals only the test's own host process.
                #[cfg(unix)]
                unsafe {
                    libc::kill(state.pid as libc::pid_t, libc::SIGTERM);
                }
            }
            let deadline = Instant::now() + Duration::from_secs(10);
            while self.data.join("running.json").exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn starts_detached_authenticates_a_device_revokes_it_and_stops_independently_of_the_launcher() {
    let host = Host::new("monocode-cli-test-");
    // A stale legacy PID now belongs to this unrelated test process.
    std::fs::write(host.data.join("owner.lock"), std::process::id().to_string()).unwrap();
    assert!(host.run(&["start"]).unwrap().contains("Host started"));
    let status = host.run(&["status"]).unwrap();
    assert!(
        status.starts_with(&format!("Host {HOST_VERSION} is running (PID ")),
        "{status}"
    );
    assert!(
        host.run(&["serve"])
            .unwrap_err()
            .contains("A host already owns")
    );
    let before = host.run(&["connection-info"]).unwrap();
    // Connecting to an existing host must not install a service or restart it.
    assert_eq!(host.run(&["service", "install"]).unwrap(), before);
    let device: Value =
        serde_json::from_str(&host.run(&["pair", "--name", "Test laptop"]).unwrap()).unwrap();
    let describe = || {
        post(
            &format!("http://127.0.0.1:{}/rpc", host.port),
            &[(
                "Authorization",
                &format!("Bearer {}", device["token"].as_str().unwrap()),
            )],
            &json!({ "version": 1, "method": "environment.describe", "params": {} }).to_string(),
        )
    };
    let (code, described) = describe();
    assert_eq!(code, 200);
    assert_eq!(
        described["result"]["environmentId"],
        device["environmentId"]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&host.data), 0o700);
        assert_eq!(mode(&host.data.join("running.json")), 0o600);
    }
    let id = device["id"].as_str().unwrap();
    host.run(&["revoke", id]).unwrap();
    assert_eq!(host.run(&["revoke", id]).unwrap_err(), "Device not found");
    assert_eq!(describe().0, 401);
    assert!(host.run(&["stop"]).unwrap().contains("Host is stopping"));
    host.wait_stopped();
    assert!(host.run(&["status"]).unwrap().contains("Host is stopped"));
}

#[cfg(unix)]
#[test]
fn installs_pairs_over_pinned_tls_reuses_restarts_and_limits_a_host_to_loopback() {
    let host = Host::new("monocode-connect-test-");
    let port = host.port.to_string();
    // Tests never install a login service; `--no-service` also leaves an
    // existing one alone.
    let connect = |extra: &[&str]| -> Value {
        let mut args = vec!["connect", "--no-service", "--json", "--bind", "127.0.0.1"];
        args.extend_from_slice(extra);
        let text = host.run(&args).unwrap();
        serde_json::from_str(text.trim()).unwrap()
    };
    let status = || -> Value {
        serde_json::from_str(&host.run(&["connect", "status", "--json"]).unwrap()).unwrap()
    };
    let endpoint = format!("https://127.0.0.1:{port}");

    let first = connect(&[]);
    assert_eq!(first["port"], host.port);
    assert_eq!(first["service"], "detached");
    assert_eq!(first["endpoints"], json!([endpoint]));
    assert_eq!(first["version"], HOST_VERSION);
    let offer = parse_pairing_link(first["link"].as_str().unwrap()).unwrap();
    assert_eq!(
        offer.environment_id,
        first["environmentId"].as_str().unwrap()
    );
    assert_eq!(offer.fingerprint, first["fingerprint"].as_str().unwrap());
    assert_eq!(offer.endpoints, std::slice::from_ref(&endpoint));
    assert!(
        host.data
            .join("runtime")
            .join(HOST_VERSION)
            .join(WRAPPER)
            .exists()
    );
    assert!(host.data.join("bin").join("monocode-host").exists());

    // The desktop client pairs over the pinned certificate.
    let desktop = crate::host::store::tests::temporary("monocode-desktop-");
    let remote = Remote::new(desktop.path().to_path_buf());
    let machine = remote_pair(
        &remote,
        first["link"].as_str().unwrap().into(),
        String::new(),
    )
    .unwrap();
    let machine = serde_json::to_value(&machine).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    // A fresh client each time, so every call opens a new pinned connection.
    let describe = || {
        let remote = Remote::new(desktop.path().to_path_buf());
        remote_request(
            &remote,
            machine.clone(),
            "environment.describe".into(),
            json!({}),
        )
    };
    let described = describe().unwrap();
    assert_eq!(described["environmentId"], offer.environment_id.as_str());
    assert_eq!(described["hostVersion"], first["version"]);
    assert_eq!(described["endpoints"], json!([endpoint]));

    // Running connect again reuses the running host and issues a new link.
    let pid = status()["pid"].clone();
    let again = connect(&[]);
    assert_ne!(again["link"], first["link"]);
    assert_eq!(status()["pid"], pid);
    let mut wrong = parse_pairing_link(again["link"].as_str().unwrap()).unwrap();
    wrong.fingerprint = "A".repeat(43);
    let other = crate::host::store::tests::temporary("monocode-desktop-");
    let mismatch = remote_pair(
        &Remote::new(other.path().to_path_buf()),
        crate::host::network::pairing_link(&wrong),
        String::new(),
    )
    .err()
    .unwrap();
    assert!(mismatch.contains("different certificate"), "{mismatch}");

    let restarted = connect(&["--restart", "--yes"]);
    assert_eq!(restarted["fingerprint"], first["fingerprint"]);
    let after = status();
    assert_ne!(after["pid"], pid);
    assert_eq!(after["devices"].as_array().unwrap().len(), 1);
    assert_eq!(
        after["devices"][0]["name"],
        crate::remote_ssh::device_name().as_str()
    );
    assert!(describe().is_ok());

    assert!(
        host.run(&["connect", "disable"])
            .unwrap()
            .contains("Network access is off")
    );
    let disabled = status();
    assert_eq!(disabled["running"], true);
    assert_eq!(disabled["network"]["enabled"], false);
    assert_eq!(disabled["endpoints"], json!([]));
    assert_eq!(describe().unwrap()["endpoints"], json!([]));

    assert!(host.run(&["stop"]).unwrap().contains("Host is stopping"));
    host.wait_stopped();
}

#[test]
fn parses_options_and_rejects_bad_ports_and_commands() {
    let root = crate::host::store::tests::temporary("monocode-cli-args-");
    let data = root.path().join("data");
    let runtime = wrapper_runtime(root.path());
    let call = |args: &[&str]| {
        let mut all: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        all.extend(["--data-dir".into(), data.to_string_lossy().into_owned()]);
        let (mut out, stdout, _) = Output::captured();
        run(
            &all,
            &runtime,
            HOST_VERSION,
            |store| Ok(TestBackend::new(store, vec![])),
            &mut out,
        )
        .map(|()| stdout.text())
    };
    assert_eq!(call(&["--version"]).unwrap(), format!("{HOST_VERSION}\n"));
    assert!(call(&["help"]).unwrap().contains("connect pair [--json]"));
    assert_eq!(
        call(&["status", "--port", "0"]).unwrap_err(),
        "Invalid port"
    );
    assert_eq!(
        call(&["status", "--port", "x"]).unwrap_err(),
        "Invalid port"
    );
    assert_eq!(
        call(&["status", "--port"]).unwrap_err(),
        "Missing --port value"
    );
    assert_eq!(call(&["status"]).unwrap(), "Host is stopped\n");
    assert_eq!(
        call(&["nonsense"]).unwrap_err(),
        "Unknown command; run with --help"
    );
    assert_eq!(
        call(&["connect", "bogus"]).unwrap_err(),
        "Unknown connect command: bogus. Run with --help."
    );
    assert_eq!(
        call(&["service", "restart"]).unwrap_err(),
        "Use: service install, or service uninstall"
    );
    assert_eq!(
        call(&["revoke"]).unwrap_err(),
        "Provide a device ID to revoke"
    );
    let device: Value = serde_json::from_str(&call(&["pair", "--json"]).unwrap()).unwrap();
    assert_eq!(device["token"].as_str().unwrap().len(), 43);
    let devices: Value = serde_json::from_str(&call(&["devices"]).unwrap()).unwrap();
    assert_eq!(devices, json!([{ "id": device["id"], "name": "Desktop" }]));
    let report: Value =
        serde_json::from_str(&call(&["connect", "status", "--json"]).unwrap()).unwrap();
    assert_eq!(report["running"], false);
    assert_eq!(report["devices"].as_array().unwrap().len(), 1);
    assert_eq!(
        report["network"],
        json!({ "enabled": false, "bind": "0.0.0.0" })
    );
    assert!(
        call(&["connect", "pair"])
            .unwrap_err()
            .contains("is not running")
    );
}

#[test]
fn serves_in_process_answers_lifecycle_and_rebinds_on_network_changes() {
    let root = crate::host::store::tests::temporary("monocode-serve-");
    let directory = root.path().to_path_buf();
    let port = free_port();
    crate::host::network::write_network_settings(
        &directory,
        &NetworkSettings {
            enabled: true,
            bind: "192.0.2.1".into(),
        },
    )
    .unwrap();
    let owner = acquire_host_owner(&directory).unwrap();
    let store = Arc::new(HostStore::open(&directory.join("host.db")).unwrap());
    let handle = serve(
        HostOptions {
            directory: directory.clone(),
            port,
            version: "9.9.9".into(),
            owner,
        },
        TestBackend::new(store, vec![HarnessId::Claude]),
    )
    .unwrap();
    // The documentation address does not exist here, so the host serves
    // loopback and reports why.
    let status = handle.status();
    let network = status.network.clone().unwrap();
    assert!(network.enabled);
    assert!(network.error.is_some());
    assert_eq!(status.providers, Some(vec!["claude".to_string()]));
    let state = read_running(&directory).unwrap();
    assert_eq!(state.port, port);
    assert_eq!(
        lifecycle(&state, LifecycleAction::Status)
            .unwrap()
            .version
            .as_deref(),
        Some("9.9.9")
    );
    let wrong = crate::host::control::RunningHost {
        secret: "x".repeat(43),
        ..state.clone()
    };
    assert_eq!(
        lifecycle(&wrong, LifecycleAction::Status).unwrap_err(),
        "Could not verify the running host"
    );
    crate::host::network::write_network_settings(
        &directory,
        &NetworkSettings {
            enabled: true,
            bind: "127.0.0.1".into(),
        },
    )
    .unwrap();
    let applied = lifecycle(&state, LifecycleAction::Network).unwrap();
    assert_eq!(
        applied.network,
        Some(NetworkStatus {
            enabled: true,
            bind: "127.0.0.1".into(),
            error: None
        })
    );
    assert_eq!(handle.local_addr().unwrap().port(), port);
    // A second host cannot take the directory while this one runs.
    assert!(acquire_host_owner(&directory).is_err());
    assert_eq!(
        lifecycle(&state, LifecycleAction::Stop).unwrap().pid,
        Some(i64::from(std::process::id()))
    );
    handle.wait();
    assert!(!directory.join("running.json").exists());
    acquire_host_owner(&directory).unwrap();
}
