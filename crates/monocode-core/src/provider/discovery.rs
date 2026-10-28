//! Some unrelated programs also install `pi`, `omp`, `fx` or `grok`. As in the
//! legacy binary resolver, verify their CLI contract before marking them installed.
use super::*;
use std::sync::OnceLock;
type IdentityCache = HashMap<(Kind, PathBuf), Option<std::time::SystemTime>>;
pub(super) fn matches(kind: Kind, path: &Path) -> bool {
    if kind == Kind::Cursor {
        if path.components().any(|p| p.as_os_str() == ".grok") {
            return false;
        }
        if path.file_stem().is_some_and(|name| name == "agent") {
            // Inspect only one symlink hop, as in the Tauri resolver. Never
            // mistake Grok's unrelated `agent` shim for Cursor.
            return std::fs::read_link(path).ok().is_some_and(|target| {
                target
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("cursor-agent")
            });
        }
    }
    if !matches!(kind, Kind::Pi | Kind::Omp | Kind::Fx | Kind::Grok) {
        return true;
    }
    static CACHE: OnceLock<Mutex<IdentityCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (kind, path.to_path_buf());
    let modified = path.metadata().ok().and_then(|m| m.modified().ok());
    if cache.lock().unwrap().get(&key) == Some(&modified) {
        return true;
    }
    let mut command = Command::new(path);
    command
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::scm::shell::apply_gui_env(&mut command);
    process::isolate(&mut command);
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let _registration = process::register(child.id());
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.take(64 * 1024).read_to_string(&mut text);
        let _ = tx.send(text);
    });
    let text = rx.recv_timeout(Duration::from_secs(3)).unwrap_or_default();
    process::terminate(&mut child);
    drop(rx);
    let _ = reader.join();
    let valid = help_matches(kind, &text.to_lowercase());
    if valid {
        cache.lock().unwrap().insert(key, modified);
    }
    valid
}
fn help_matches(kind: Kind, text: &str) -> bool {
    match kind {
        Kind::Pi | Kind::Omp => text.contains("--mode") && text.contains("rpc"),
        Kind::Fx => {
            text.contains("acp") && (text.contains("fx.sh") || text.contains("coding agent"))
        }
        Kind::Grok => text.contains("agent") && (text.contains("x.ai") || text.contains("grok")),
        _ => true,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unrelated_binaries_with_the_same_name() {
        assert!(!help_matches(
            Kind::Fx,
            "fx: command-line json processing tool"
        ));
        assert!(help_matches(
            Kind::Fx,
            "fx: coding agent. acp stdio. https://fx.sh"
        ));
        assert!(!help_matches(
            Kind::Pi,
            "pi: print digits of pi --precision"
        ));
        assert!(help_matches(Kind::Pi, "pi --mode rpc"));
        assert!(help_matches(Kind::Grok, "grok build: x.ai agent stdio"));
    }
}
