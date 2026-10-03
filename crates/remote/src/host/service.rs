//! Port of host/service.ts: the login service that keeps the host running,
//! as a launch agent on macOS, a systemd user service on Linux, or a Task
//! Scheduler task on Windows. The names match the TypeScript host's, so
//! installing this host replaces the Node host's service in place.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::control::{LifecycleAction, RunningHost, lifecycle};
use super::exec::{ExecOptions, exec};
use super::server::{home_dir, node_platform};
pub use super::windows::ServiceOptions;
use super::windows::{run_powershell, windows_task_script, windows_uninstall_script};

const LABEL: &str = "com.monocode.host";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ConnectionInfo {
    pub port: u16,
    pub pid: i64,
}

/// The running host's port, once it answers.
pub fn connection_info(directory: &Path) -> Result<ConnectionInfo, String> {
    let text = std::fs::read_to_string(directory.join("running.json"))
        .map_err(|error| error.to_string())?;
    let state: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let port = state
        .get("port")
        .and_then(serde_json::Value::as_u64)
        .filter(|port| (1..=65535).contains(port));
    let secret = state
        .get("secret")
        .and_then(serde_json::Value::as_str)
        .filter(|secret| {
            secret.len() == 43
                && secret
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        });
    let (Some(port), Some(secret)) = (port, secret) else {
        return Err("Invalid host state".into());
    };
    let pid = state
        .get("pid")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    let running = RunningHost {
        pid,
        port: port as u16,
        secret: secret.into(),
    };
    lifecycle(&running, LifecycleAction::Status).map_err(|_| "Host is not ready".to_string())?;
    Ok(ConnectionInfo {
        port: running.port,
        pid,
    })
}

fn xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn command_line(options: &ServiceOptions) -> Vec<String> {
    std::iter::once(options.program.executable.to_string_lossy().into_owned())
        .chain(options.program.serve_args(&options.directory, options.port))
        .collect()
}

pub fn launch_agent(options: &ServiceOptions, path: &str) -> String {
    let args: String = command_line(options)
        .iter()
        .map(|arg| format!("<string>{}</string>", xml(arg)))
        .collect();
    let log = xml(&options.directory.join("host.log").to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array>{args}</array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>ThrottleInterval</key><integer>10</integer>
<key>EnvironmentVariables</key><dict><key>PATH</key><string>{path}</string></dict>
<key>StandardOutPath</key><string>{log}</string>
<key>StandardErrorPath</key><string>{log}</string>
</dict></plist>
"#,
        path = xml(path),
    )
}

// systemd expands % specifiers in both settings; $ variables only in ExecStart.
fn unit_quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('\n', "\\n")
    )
}

pub fn systemd_unit(options: &ServiceOptions, path: &str) -> String {
    let exec_start = command_line(options)
        .iter()
        .map(|value| unit_quote(&value.replace('$', "$$")))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "[Unit]
Description=MonoCode Host
After=network.target

[Service]
ExecStart={exec_start}
Environment={}
Restart=on-failure
RestartSec=5
UMask=0077
KillMode=control-group
TimeoutStopSec=20

[Install]
WantedBy=default.target
",
        unit_quote(&format!("PATH={path}"))
    )
}

/// The current user's ID and name, as `os.userInfo()`.
#[cfg(unix)]
pub fn user_info() -> (u32, String) {
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: the buffers outlive the call and getpwuid_r writes within them.
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    let name = if status == 0 && !result.is_null() && !entry.pw_name.is_null() {
        // SAFETY: pw_name points into `buffer` as a NUL-terminated string.
        unsafe { std::ffi::CStr::from_ptr(entry.pw_name) }
            .to_string_lossy()
            .into_owned()
    } else {
        std::env::var("USER").unwrap_or_default()
    };
    (uid, name)
}

#[cfg(not(unix))]
pub fn user_info() -> (u32, String) {
    (0, std::env::var("USERNAME").unwrap_or_default())
}

// SSH sessions may lack the user bus variables systemctl --user needs.
fn systemd_environment(uid: u32) -> Vec<(OsString, OsString)> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{uid}"));
    let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .unwrap_or_else(|_| format!("unix:path={runtime}/bus"));
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(key, _)| key != "XDG_RUNTIME_DIR" && key != "DBUS_SESSION_BUS_ADDRESS")
        .collect();
    env.push(("XDG_RUNTIME_DIR".into(), runtime.into()));
    env.push(("DBUS_SESSION_BUS_ADDRESS".into(), bus.into()));
    env
}

/// Runs a service manager command.
pub type Run<'a> =
    &'a dyn Fn(&str, &[&str], Option<&[(OsString, OsString)]>) -> Result<String, String>;

fn run_command(
    command: &str,
    args: &[&str],
    env: Option<&[(OsString, OsString)]>,
    timeout: Duration,
) -> Result<String, String> {
    exec(
        command,
        args,
        ExecOptions {
            env: env.map(<[_]>::to_vec),
            timeout,
            max_buffer: 128 * 1024,
            ..Default::default()
        },
    )
    .map(|output| output.stdout)
    .map_err(String::from)
}

/// Runs a PowerShell script.
pub type PowerShell<'a> = &'a dyn Fn(&str) -> Result<(), String>;

/// Stand-ins for the system, so tests never touch a real login service.
#[derive(Default)]
pub struct UninstallSystem<'a> {
    pub platform: Option<&'a str>,
    pub home: Option<PathBuf>,
    pub run: Option<Run<'a>>,
    pub powershell: Option<PowerShell<'a>>,
}

/// Removes the login service or scheduled task so the host no longer starts
/// automatically, stopping it where the service manager owns the process.
/// Never deletes the data directory: sessions, logs and device credentials
/// stay until the user removes them explicitly. Returns follow-up notes.
pub fn uninstall_service(system: UninstallSystem<'_>) -> Result<Vec<String>, String> {
    let platform = system.platform.unwrap_or(node_platform());
    let home = system.home.clone().unwrap_or_else(home_dir);
    let default_run = |command: &str, args: &[&str], env: Option<&[(OsString, OsString)]>| {
        run_command(command, args, env, Duration::from_secs(30))
    };
    let run: Run<'_> = system.run.unwrap_or(&default_run);
    match platform {
        "darwin" => {
            let (uid, _) = user_info();
            let service = format!("gui/{uid}/{LABEL}");
            let _ = run("launchctl", &["bootout", &service], None);
            // `bootout` returns while launchd is still stopping the host, and a
            // reinstall in that window finds the old job and never starts a new
            // one. Wait until it is gone; launchd kills a job after 20 seconds.
            for _ in 0..100 {
                if run("launchctl", &["print", &service], None).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            remove_if_present(
                &home
                    .join("Library/LaunchAgents")
                    .join(format!("{LABEL}.plist")),
            )?;
            Ok(Vec::new())
        }
        "linux" => {
            let (uid, username) = user_info();
            let env = systemd_environment(uid);
            let _ = run(
                "systemctl",
                &["--user", "disable", "--now", "monocode-host.service"],
                Some(&env),
            );
            remove_if_present(&home.join(".config/systemd/user/monocode-host.service"))?;
            let _ = run("systemctl", &["--user", "daemon-reload"], Some(&env));
            Ok(vec![format!(
                "Lingering is still enabled for {username}; other user services may rely on it. To turn it off: loginctl disable-linger {username}"
            )])
        }
        "win32" => {
            let script = windows_uninstall_script();
            match system.powershell {
                Some(powershell) => powershell(&script)?,
                None => {
                    run_powershell(&script)?;
                }
            }
            Ok(Vec::new())
        }
        _ => Err("MonoCode Host supports Windows, Linux and macOS".into()),
    }
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
        _ => Ok(()),
    }
}

fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    std::io::Write::write_all(
        &mut options.open(path).map_err(|error| error.to_string())?,
        contents.as_bytes(),
    )
    .map_err(|error| error.to_string())
}

/// Installs and starts the login service, then waits for the host to answer.
pub fn install_service(options: &ServiceOptions) -> Result<ConnectionInfo, String> {
    // Never replace a running host: connecting must not interrupt agent turns.
    if let Ok(info) = connection_info(&options.directory) {
        return Ok(info);
    }
    let home = home_dir();
    let mut folders: Vec<String> = Vec::new();
    for folder in [
        std::env::var("PATH").unwrap_or_default(),
        home.join(".local/bin").to_string_lossy().into_owned(),
        "/opt/homebrew/bin".into(),
        "/usr/local/bin".into(),
        "/usr/bin".into(),
        "/bin".into(),
    ] {
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    let path = folders.join(":");
    let run = |command: &str, args: &[&str], env: Option<&[(OsString, OsString)]>| {
        run_command(command, args, env, Duration::from_secs(15))
    };
    match node_platform() {
        "darwin" => {
            let (uid, _) = user_info();
            let domain = format!("gui/{uid}");
            if run("launchctl", &["print", &domain], None).is_err() {
                return Err("Sign in at the Mac's desktop once, then reconnect. MonoCode Host runs as a login service; keep the Mac signed in and awake.".into());
            }
            let folder = home.join("Library/LaunchAgents");
            let file = folder.join(format!("{LABEL}.plist"));
            std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
            let job = format!("{domain}/{LABEL}");
            if run("launchctl", &["print", &job], None).is_ok() {
                // A loaded service already has an owner and executable. Start it
                // without rewriting its configuration or sending a kill/restart.
                run("launchctl", &["kickstart", &job], None)?;
            } else {
                write_private(&file, &launch_agent(options, &path))?;
                // Right after `bootout`, as when connect replaces an older host,
                // launchd can refuse the same label until it finishes removing it.
                let file = file.to_string_lossy().into_owned();
                let mut attempt = 1;
                loop {
                    match run("launchctl", &["bootstrap", &domain, &file], None) {
                        Ok(_) => break,
                        Err(error) if attempt >= 10 => return Err(error),
                        Err(_) => {
                            attempt += 1;
                            std::thread::sleep(Duration::from_millis(500));
                        }
                    }
                }
            }
        }
        "linux" => {
            let (uid, username) = user_info();
            let env = systemd_environment(uid);
            let lingering = run(
                "loginctl",
                &["enable-linger", &username, "--no-ask-password"],
                Some(&env),
            )
            .and_then(|_| {
                run(
                    "loginctl",
                    &["show-user", &username, "--property=Linger", "--value"],
                    Some(&env),
                )
            });
            if lingering.map(|value| value.trim() == "yes") != Ok(true) {
                return Err(format!(
                    "This host needs systemd user services and lingering to keep sessions running after SSH disconnects. An administrator can enable it with: sudo loginctl enable-linger {username}"
                ));
            }
            let folder = home.join(".config/systemd/user");
            std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
            let file = folder.join("monocode-host.service");
            if !file.exists() {
                write_private(&file, &systemd_unit(options, &path))?;
            }
            run("systemctl", &["--user", "daemon-reload"], Some(&env))?;
            run(
                "systemctl",
                &["--user", "enable", "--now", "monocode-host.service"],
                Some(&env),
            )?;
        }
        "win32" => {
            run_powershell(&windows_task_script(
                options,
                &std::env::var("PATH").unwrap_or_default(),
            ))?;
        }
        _ => return Err("MonoCode Host supports Windows, Linux and macOS".into()),
    }
    // launchd waits up to its 10-second ThrottleInterval before starting a job
    // whose previous instance just exited, as when connect replaces a host.
    // One deadline bounds the wait, since each status check can itself take
    // 5 seconds to time out.
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Ok(info) = connection_info(&options.directory) {
            return Ok(info);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let log = options.directory.join("host.log");
    Err(if node_platform() == "win32" {
        format!(
            "The host task did not start. Sign in to the Windows desktop as the SSH user and keep that account signed in (locking is fine), then reconnect. Check Task Scheduler and {}.",
            log.display()
        )
    } else {
        format!(
            "The host service was installed but did not start. Check {}.",
            log.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::runtime::HostProgram;
    use std::sync::Mutex;

    #[test]
    fn keeps_paths_and_environment_content_from_injecting_service_configuration() {
        let options = ServiceOptions {
            directory: "/Users/a & b/%folder".into(),
            port: 3774,
            program: HostProgram {
                executable: "/runtime/a\"b/node".into(),
                args: vec!["/runtime/$name/host.mjs".into()],
            },
        };
        let plist = launch_agent(&options, "/bin:<test>&other");
        assert!(plist.contains("a&quot;b/node"));
        assert!(plist.contains("/bin:&lt;test&gt;&amp;other"));
        assert!(!plist.contains("<test>"));
        assert!(plist.contains("<string>serve</string><string>--data-dir</string><string>/Users/a &amp; b/%folder</string>"));
        let unit = systemd_unit(&options, "/bin:/a\n[Service]\nExecStart=/bad");
        assert_eq!(
            unit.lines()
                .filter(|line| line.starts_with("ExecStart="))
                .count(),
            1
        );
        assert!(unit.contains("%%folder"));
        assert!(unit.contains("$$name"));
        assert!(unit.contains("KillMode=control-group"));
    }

    #[cfg(unix)]
    #[test]
    fn removes_the_service_registration_but_keeps_host_data() {
        for platform in ["darwin", "linux"] {
            let home = crate::host::store::tests::temporary("monocode-service-test-");
            let service = if platform == "darwin" {
                home.path()
                    .join("Library/LaunchAgents/com.monocode.host.plist")
            } else {
                home.path()
                    .join(".config/systemd/user/monocode-host.service")
            };
            let data = home.path().join(".monocode-host/host.db");
            for file in [&service, &data] {
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(file, "existing").unwrap();
            }
            let calls = Mutex::new(Vec::<Vec<String>>::new());
            let run = |command: &str, args: &[&str], _env: Option<&[(OsString, OsString)]>| {
                calls.lock().unwrap().push(
                    std::iter::once(command.to_string())
                        .chain(args.iter().map(|arg| arg.to_string()))
                        .collect(),
                );
                // A service that is not loaded must not block cleanup.
                if ["bootout", "disable", "print"]
                    .iter()
                    .any(|arg| args.contains(arg))
                {
                    Err("not loaded".to_string())
                } else {
                    Ok(String::new())
                }
            };
            let notes = uninstall_service(UninstallSystem {
                platform: Some(platform),
                home: Some(home.path().to_path_buf()),
                run: Some(&run),
                powershell: None,
            })
            .unwrap();
            assert!(!service.exists(), "{platform}");
            assert_eq!(std::fs::read_to_string(&data).unwrap(), "existing");
            let calls = calls.into_inner().unwrap();
            if platform == "darwin" {
                assert_eq!(calls[0][..2], ["launchctl", "bootout"]);
            } else {
                assert!(calls.contains(&vec![
                    "systemctl".to_string(),
                    "--user".into(),
                    "disable".into(),
                    "--now".into(),
                    "monocode-host.service".into(),
                ]));
                assert!(notes.join("\n").contains("loginctl disable-linger"));
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn waits_for_launchd_to_finish_removing_the_host_before_returning() {
        let home = crate::host::store::tests::temporary("monocode-service-test-");
        // `bootout` returns while launchd still lists the stopping host.
        let listed = Mutex::new(2);
        let calls = Mutex::new(Vec::<String>::new());
        let run = |_command: &str, args: &[&str], _env: Option<&[(OsString, OsString)]>| {
            calls.lock().unwrap().push(args[0].to_string());
            let mut listed = listed.lock().unwrap();
            if args[0] == "print" {
                if *listed <= 0 {
                    return Err("Could not find service".to_string());
                }
                *listed -= 1;
            }
            Ok(String::new())
        };
        uninstall_service(UninstallSystem {
            platform: Some("darwin"),
            home: Some(home.path().to_path_buf()),
            run: Some(&run),
            powershell: None,
        })
        .unwrap();
        assert_eq!(
            calls.into_inner().unwrap(),
            ["bootout", "print", "print", "print"]
        );
    }

    #[test]
    fn unregisters_only_this_users_windows_task() {
        let scripts = Mutex::new(Vec::<String>::new());
        let powershell = |script: &str| {
            scripts.lock().unwrap().push(script.into());
            Ok(())
        };
        uninstall_service(UninstallSystem {
            platform: Some("win32"),
            powershell: Some(&powershell),
            ..Default::default()
        })
        .unwrap();
        let scripts = scripts.into_inner().unwrap();
        assert!(scripts[0].contains("\"MonoCode Host-$sid\""));
        assert!(scripts[0].contains("Unregister-ScheduledTask"));
        assert!(!scripts[0].contains("Remove-Item") && !scripts[0].contains(".monocode-host"));
    }

    #[test]
    fn connection_info_requires_a_valid_state_and_a_live_host() {
        let directory = crate::host::store::tests::temporary("monocode-service-test-");
        assert!(connection_info(directory.path()).is_err());
        std::fs::write(
            directory.path().join("running.json"),
            r#"{"pid":1,"port":1,"secret":"short"}"#,
        )
        .unwrap();
        assert_eq!(
            connection_info(directory.path()).unwrap_err(),
            "Invalid host state"
        );
        std::fs::write(
            directory.path().join("running.json"),
            format!(r#"{{"pid":1,"port":1,"secret":"{}"}}"#, "a".repeat(43)),
        )
        .unwrap();
        assert_eq!(
            connection_info(directory.path()).unwrap_err(),
            "Host is not ready"
        );
    }
}
