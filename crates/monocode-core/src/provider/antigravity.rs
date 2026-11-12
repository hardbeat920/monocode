//! Launch contracts for the standalone runtime and the CLI's built-in ACP server.
use super::*;

/// Recognize CLI names and Windows wrappers that require the ACP subcommand.
pub(super) fn is_cli(binary: &Path) -> bool {
    binary
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("agy")
                || (cfg!(windows)
                    && ["agy.exe", "agy.cmd", "agy.bat", "agy.com"]
                        .iter()
                        .any(|launcher| name.eq_ignore_ascii_case(launcher)))
        })
}

/// Select CLI arguments or the standalone Linux runtime's required uid flag.
pub(super) fn launch_args(binary: &Path) -> &'static [&'static str] {
    if is_cli(binary) {
        &["acp"]
    } else if cfg!(target_os = "linux") {
        &["--uid="]
    } else {
        &[]
    }
}

/// Keep CLI sessions in their workspace and standalone servers beside their resources.
pub(super) fn spawn_cwd<'a>(binary: &'a Path, cwd: &'a Path) -> &'a Path {
    if is_cli(binary) {
        cwd
    } else {
        binary.parent().unwrap_or(cwd)
    }
}

/// Append standard Windows server paths, then CLI fallbacks after every standalone candidate.
pub(super) fn extend_candidates(candidates: &mut Vec<PathBuf>, local: Option<&Path>) {
    if let Some(local) = local {
        candidates.push(local.join("agy-acp/agy_acp_server.exe"));
        candidates.push(local.join("agy/bin/agy_acp_server.exe"));
    }
    // Keep all standalone candidates ahead of the CLI: their login/config differs.
    let cli: Vec<_> = candidates
        .iter()
        .filter_map(|path| path.parent().map(|dir| dir.join("agy")))
        .collect();
    candidates.extend(cli);
    if let Some(local) = local {
        candidates.push(local.join("agy/bin/agy.exe"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create an executable discovery fixture without invoking any provider.
    fn fixture(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "ACP fixture; discovery must not execute it").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// Prefer an existing standalone runtime and use the CLI when it is removed.
    #[test]
    fn discovery_preserves_standalone_then_falls_back_to_cli() {
        let dir = tempfile::tempdir().unwrap();
        let server = dir.path().join(Kind::Antigravity.binary_name());
        let cli = dir
            .path()
            .join(if cfg!(windows) { "agy.exe" } else { "agy" });
        fixture(&server);
        fixture(&cli);
        let mut candidates = vec![server.clone()];
        extend_candidates(&mut candidates, None);
        assert_eq!(
            resolve_candidates(Kind::Antigravity, candidates.clone()),
            Some(server.clone())
        );
        std::fs::remove_file(server).unwrap();
        assert_eq!(resolve_candidates(Kind::Antigravity, candidates), Some(cli));
    }

    /// Find the Windows CLI and standalone runtime even when neither is on PATH.
    #[test]
    #[cfg(windows)]
    fn discovery_finds_official_windows_install_without_path() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("agy/bin/agy.exe");
        let server = dir.path().join("agy-acp/agy_acp_server.exe");
        fixture(&cli);
        let mut candidates = Vec::new();
        extend_candidates(&mut candidates, Some(dir.path()));
        assert_eq!(
            resolve_candidates(Kind::Antigravity, candidates.clone()),
            Some(cli)
        );
        fixture(&server);
        assert_eq!(
            resolve_candidates(Kind::Antigravity, candidates),
            Some(server)
        );
    }

    /// Verify CLI argv/workspace selection and standalone resource-directory behavior.
    #[test]
    fn launch_uses_workspace_for_cli_and_sibling_resources_for_standalone() {
        let cwd = Path::new("workspace");
        for name in ["agy", "AGY", "aGy"] {
            let cli = Path::new("tools").join(name);
            assert_eq!(launch_args(&cli), &["acp"]);
            assert_eq!(spawn_cwd(&cli, cwd), cwd);
        }
        for name in [
            "agy.exe", "AGY.EXE", "agy.cmd", "AGY.CMD", "agy.bat", "AGY.BAT", "agy.com", "AGY.COM",
        ] {
            let cli = Path::new("tools").join(name);
            assert_eq!(is_cli(&cli), cfg!(windows), "{name}");
            if cfg!(windows) {
                assert_eq!(launch_args(&cli), &["acp"], "{name}");
                assert_eq!(spawn_cwd(&cli, cwd), cwd, "{name}");
            }
        }
        let server = Path::new("tools").join(Kind::Antigravity.binary_name());
        assert_eq!(spawn_cwd(&server, cwd), Path::new("tools"));
        assert_eq!(
            launch_args(&server),
            if cfg!(target_os = "linux") {
                &["--uid="][..]
            } else {
                &[]
            }
        );
    }

    /// Opt in to authenticated catalog discovery without sending a model prompt.
    #[test]
    #[ignore = "requires an installed, signed-in Antigravity CLI; no model prompt is sent"]
    fn installed_cli_discovers_authenticated_catalog() {
        let binary = resolve(Kind::Antigravity).expect("Antigravity CLI must be installed");
        assert!(
            is_cli(&binary),
            "Select agy with MONOCODE_ANTIGRAVITY_BINARY"
        );
        let models = crate::catalog::models(Kind::Antigravity).unwrap();
        assert!(
            !models.is_empty(),
            "The signed-in CLI should advertise models"
        );
        eprintln!(
            "Resolved {} with {:?}; discovered {} models",
            binary.display(),
            launch_args(&binary),
            models.len()
        );
        for model in models {
            eprintln!("{}: {:?}", model.id, model.efforts);
        }
    }
}
