use super::Profile;
use crate::{
    process,
    provider::{self, Kind},
};
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthAction {
    SignIn,
    SignOut,
}
pub fn arguments(kind: Kind, action: AuthAction) -> Option<&'static [&'static str]> {
    match (kind, action) {
        (Kind::Claude | Kind::Devin, AuthAction::SignIn) => Some(&["auth", "login"]),
        (Kind::Claude | Kind::Devin, AuthAction::SignOut) => Some(&["auth", "logout"]),
        (Kind::Codex | Kind::Cursor, AuthAction::SignIn) => Some(&["login"]),
        (Kind::Codex | Kind::Cursor, AuthAction::SignOut) => Some(&["logout"]),
        (Kind::Grok, AuthAction::SignIn) => Some(&["login", "--oauth"]),
        (Kind::Fx, AuthAction::SignIn) => Some(&["login", "vercel"]),
        _ => None,
    }
}
/// The CLI opens its browser and owns credential writes. Never relay its URLs,
/// OAuth codes or tokens to logs, clipboard, or the native account metadata.
pub fn authenticate(
    kind: Kind,
    profile: &Profile,
    action: AuthAction,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let args = arguments(kind, action)
        .ok_or("This provider does not offer a single browser sign-in flow")?;
    let binary =
        provider::resolve(kind).ok_or_else(|| format!("{} CLI not installed", kind.label()))?;
    let mut command = Command::new(binary);
    crate::scm::shell::apply_gui_env(&mut command);
    profile.apply(kind, &mut command)?;
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(home) = super::home() {
        command.current_dir(home);
    }
    process::isolate(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start {} sign-in: {e}", kind.label()))?;
    let _registration = process::register(child.id());
    let _account_process = super::track(child.id(), kind, profile);
    let mut stderr = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        // Keep draining the pipe even after the bounded diagnostic tail fills.
        while let Ok(n) = stderr.read(&mut buffer) {
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            if bytes.len() > 65_536 {
                bytes.drain(..bytes.len() - 65_536);
            }
        }
        let _ = tx.send(String::from_utf8_lossy(&bytes).into_owned());
    });
    let deadline = Instant::now()
        + Duration::from_secs(if action == AuthAction::SignIn {
            600
        } else {
            30
        });
    let result = loop {
        if cancel.load(Ordering::Relaxed) {
            break Err("Sign-in cancelled".into());
        }
        if Instant::now() >= deadline {
            break Err(format!(
                "{} sign-in timed out. Please try again.",
                kind.label()
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(_)) => {
                let detail = rx.recv_timeout(Duration::from_secs(1)).unwrap_or_default();
                let message = safe_detail(&detail);
                break Err(if message.is_empty() {
                    format!("{} sign-in exited unexpectedly", kind.label())
                } else {
                    message
                });
            }
            Err(e) => break Err(format!("Could not complete sign-in: {e}")),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
        }
    };
    process::terminate(&mut child);
    drop(rx);
    let _ = reader.join();
    result
}
fn safe_detail(value: &str) -> String {
    let last = value
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or("");
    let urls = regex::Regex::new(r"(?i)https?://\S+").unwrap();
    urls.replace_all(last, "sign-in link")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}
pub(crate) fn cli_output(kind: Kind, args: &[&str], profile: &Profile) -> Result<String, String> {
    let binary =
        provider::resolve(kind).ok_or_else(|| format!("{} CLI not found", kind.label()))?;
    let mut command = Command::new(binary);
    crate::scm::shell::apply_gui_env(&mut command);
    profile.apply(kind, &mut command)?;
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    process::isolate(&mut command);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let _registration = process::register(child.id());
    let _account_process = super::track(child.id(), kind, profile);
    let output = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = output
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let result = rx
        .recv_timeout(Duration::from_secs(15))
        .map_err(|_| "Provider request timed out".to_string())
        .and_then(|r| r.map_err(|e| e.to_string()))
        .and_then(|bytes| {
            if bytes.len() > 1_048_576 {
                Err("Provider output exceeded its limit".into())
            } else {
                Ok(String::from_utf8_lossy(&bytes).into_owned())
            }
        });
    process::terminate(&mut child);
    drop(rx);
    let _ = reader.join();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_auth_commands_and_error_details_match_the_provider_flow() {
        assert_eq!(
            arguments(Kind::Claude, AuthAction::SignIn),
            Some(["auth", "login"].as_slice())
        );
        assert_eq!(
            arguments(Kind::Codex, AuthAction::SignOut),
            Some(["logout"].as_slice())
        );
        assert_eq!(
            arguments(Kind::Grok, AuthAction::SignIn),
            Some(["login", "--oauth"].as_slice())
        );
        assert!(arguments(Kind::Pi, AuthAction::SignIn).is_none());
        assert_eq!(
            safe_detail("first line\n Complete at https://example.test/?code=private\n"),
            "Complete at sign-in link"
        );
        assert_eq!(safe_detail(&"a".repeat(300)).len(), 240);
    }
}
