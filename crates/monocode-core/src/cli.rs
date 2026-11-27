//! Bounded CLI invocations for Settings management. Call only from workers.
use crate::provider::Kind;
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{OnceLock, RwLock},
    time::{Duration, Instant},
};
fn paths() -> &'static RwLock<HashMap<String, String>> {
    static PATHS: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
    PATHS.get_or_init(Default::default)
}
pub fn initialize(overrides: &HashMap<String, String>) {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        *paths().write().unwrap() = overrides.clone();
    });
}
pub fn configured(kind: Kind) -> Option<PathBuf> {
    paths()
        .read()
        .unwrap()
        .get(kind.id())
        .filter(|p| !p.trim().is_empty())
        .map(PathBuf::from)
}
pub fn output(
    binary: &Path,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let mut command = Command::new(binary);
    crate::scm::shell::apply_gui_env(&mut command);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::process::isolate(&mut command);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let _registration = crate::process::register(child.id());
    let read = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut result = Vec::new();
            let mut chunk = [0; 4096];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                result.extend_from_slice(&chunk[..n]);
                if result.len() > 65536 {
                    result.drain(..result.len() - 65536);
                }
            }
            String::from_utf8_lossy(&result).trim().to_owned()
        })
    };
    let stdout = read(Box::new(child.stdout.take().unwrap()));
    let stderr = read(Box::new(child.stderr.take().unwrap()));
    let deadline = Instant::now() + timeout;
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(e) => break Err(e.to_string()),
            Ok(None) if Instant::now() >= deadline => break Err("CLI command timed out".into()),
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
        }
    };
    crate::process::terminate(&mut child);
    let out = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();
    if result?.success() {
        Ok(out)
    } else {
        Err(format!("{err} {out}").trim().to_owned())
    }
}
#[derive(Clone, Debug)]
pub struct Inspection {
    pub path: PathBuf,
    pub version: String,
}
pub fn inspect(kind: Kind, path: Option<&str>) -> Result<Inspection, String> {
    let path = match path.filter(|s| !s.trim().is_empty()) {
        Some(p) => {
            let p = PathBuf::from(p);
            if !p.is_absolute() {
                return Err("Enter an absolute path to the CLI executable.".into());
            }
            p
        }
        None => crate::provider::resolve_auto(kind).ok_or("CLI could not be resolved")?,
    };
    if !path.is_file() {
        return Err("CLI executable does not exist.".into());
    }
    let cwd = crate::scm::shell::dirs_home()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    let version = output(&path, &["--version".into()], &cwd, Duration::from_secs(10))?;
    if kind == Kind::Codex
        && !regex::Regex::new(r"^codex-cli\s+\d+\.\d+\.\d+")
            .unwrap()
            .is_match(&version)
    {
        return Err("Codex CLI returned an invalid version.".into());
    }
    if kind == Kind::OpenCode {
        let v = regex::Regex::new(r"(?:^|\s)v?(\d+)\.(\d+)\.(\d+)").unwrap();
        let c = v
            .captures(&version)
            .ok_or("OpenCode CLI returned an invalid version.")?;
        let major = c[1].parse::<u32>().unwrap_or(0);
        let minor = c[2].parse::<u32>().unwrap_or(0);
        let patch = c[3].parse::<u32>().unwrap_or(0);
        if (major, minor, patch) < (1, 14, 19) {
            return Err(format!(
                "OpenCode v{major}.{minor}.{patch} is too old. Upgrade to v1.14.19 or newer."
            ));
        }
    }
    Ok(Inspection { path, version })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn validates_executable_and_provider_version_before_saving() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CLI with spaces");
        std::fs::write(&path, "#!/bin/sh\nprintf 'codex-cli 1.2.3\\n'\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            inspect(Kind::Codex, path.to_str()).unwrap().version,
            "codex-cli 1.2.3"
        );
        assert!(inspect(Kind::Codex, Some("relative/path"))
            .unwrap_err()
            .contains("absolute"));
        std::fs::write(&path, "#!/bin/sh\nprintf '1.10.3\\n'\n").unwrap();
        assert!(inspect(Kind::OpenCode, path.to_str())
            .unwrap_err()
            .contains("1.14.19"));
        assert!(inspect(Kind::Codex, path.to_str())
            .unwrap_err()
            .contains("invalid version"));
    }
}
