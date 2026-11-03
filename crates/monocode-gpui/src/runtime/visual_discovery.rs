//! Introduce CLI visual discovery once to contexts without native MCP tools.
use monocode_core::{provider::Kind, store::Store};
use std::path::Path;

const VERSION: u8 = 1;

pub(super) struct Discovery {
    key: String,
    executable: String,
}

impl Discovery {
    pub(super) fn prepare(
        store: &Store,
        session: i64,
        kind: Kind,
        account: &str,
        provider_id: Option<&str>,
        executable: &Path,
    ) -> Result<Option<Self>, String> {
        let fallback = matches!(kind, Kind::Pi | Kind::Omp | Kind::OpenCode)
            || (provider_id.is_some() && matches!(kind, Kind::Grok | Kind::Fx));
        if !fallback {
            return Ok(None);
        }
        let discovery = Self {
            key: format!("visual-discovery:{session}:{}:{account}", kind.id()),
            executable: executable.to_string_lossy().into_owned(),
        };
        if let Some(provider_id) = provider_id {
            let expected = discovery.receipt(provider_id);
            if store.setting(&discovery.key)?.as_deref() == Some(expected.as_str()) {
                return Ok(None);
            }
        }
        Ok(Some(discovery))
    }

    pub(super) fn prepend(&self, prompt: String) -> String {
        format!(
            "Optional native visuals are available; API reference: {} visual docs.\n\n{prompt}",
            super::shlex_quote(&self.executable),
        )
    }

    /// Call only after the introduction reached a successfully completed turn.
    /// A provider that has not supplied a context id remains eligible next time.
    pub(super) fn remember(&self, store: &Store, provider_id: Option<&str>) -> Result<(), String> {
        if let Some(provider_id) = provider_id.filter(|id| !id.trim().is_empty()) {
            store.save_setting(&self.key, &self.receipt(provider_id))?;
        }
        Ok(())
    }

    fn receipt(&self, provider_id: &str) -> String {
        serde_json::json!({
            "version": VERSION,
            "providerId": provider_id,
            "executable": self.executable,
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> (Fixture, Store) {
        let path = std::env::temp_dir().join(format!(
            "monocode-visual-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&path).unwrap();
        let store = Store::open_monocode(&path.join("sessions.db")).unwrap();
        (Fixture(path), store)
    }

    #[test]
    fn existing_fallback_contexts_get_discovery_and_only_success_suppresses_retries() {
        let (_fixture, store) = fixture();
        let executable = Path::new("/Applications/Mono Code.app/Contents/MacOS/monocode-gpui");
        for kind in [Kind::Pi, Kind::Omp, Kind::OpenCode, Kind::Grok, Kind::Fx] {
            let discovery =
                Discovery::prepare(&store, 7, kind, "default", Some("existing"), executable)
                    .unwrap()
                    .expect("Existing contexts still need an initial introduction");
            // An unsuccessful turn never calls remember; an unbound provider
            // also cannot establish a discovery receipt.
            discovery.remember(&store, None).unwrap();
            assert!(
                Discovery::prepare(&store, 7, kind, "default", Some("existing"), executable)
                    .unwrap()
                    .is_some()
            );
            discovery.remember(&store, Some("existing")).unwrap();
            assert!(
                Discovery::prepare(&store, 7, kind, "default", Some("existing"), executable)
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn new_contexts_accounts_executables_and_versions_receive_discovery() {
        let (_fixture, store) = fixture();
        let executable = Path::new("/old/monocode-gpui");
        for kind in [Kind::Pi, Kind::Omp, Kind::OpenCode] {
            let discovery =
                Discovery::prepare(&store, 8, kind, "default", Some("old-context"), executable)
                    .unwrap()
                    .unwrap();
            discovery.remember(&store, Some("old-context")).unwrap();
            assert!(
                Discovery::prepare(&store, 8, kind, "default", None, executable)
                    .unwrap()
                    .is_some()
            );
            assert!(
                Discovery::prepare(&store, 8, kind, "default", Some("new-context"), executable)
                    .unwrap()
                    .is_some()
            );
            assert!(
                Discovery::prepare(
                    &store,
                    8,
                    kind,
                    "other-account",
                    Some("old-context"),
                    executable
                )
                .unwrap()
                .is_some()
            );
            assert!(
                Discovery::prepare(
                    &store,
                    8,
                    kind,
                    "default",
                    Some("old-context"),
                    Path::new("/new/monocode-gpui")
                )
                .unwrap()
                .is_some()
            );
            assert!(
                Discovery::prepare(&store, 9, kind, "default", Some("old-context"), executable)
                    .unwrap()
                    .is_some()
            );
            let mut previous: serde_json::Value =
                serde_json::from_str(&discovery.receipt("old-context")).unwrap();
            previous["version"] = 0.into();
            store
                .save_setting(&discovery.key, &previous.to_string())
                .unwrap();
            assert!(
                Discovery::prepare(&store, 8, kind, "default", Some("old-context"), executable)
                    .unwrap()
                    .is_some()
            );
        }
    }

    #[test]
    fn actual_mcp_paths_never_receive_a_cli_hint() {
        let (_fixture, store) = fixture();
        let executable = Path::new("/app/monocode-gpui");
        for kind in [
            Kind::Codex,
            Kind::Claude,
            Kind::Cursor,
            Kind::Antigravity,
            Kind::Hermes,
            Kind::Devin,
        ] {
            for provider_id in [None, Some("existing")] {
                assert!(
                    Discovery::prepare(&store, 10, kind, "default", provider_id, executable)
                        .unwrap()
                        .is_none()
                );
            }
        }
        // Grok/fx advertise MCP configuration on new/load, but their resume
        // requests do not refresh mcpServers.
        for kind in [Kind::Grok, Kind::Fx] {
            assert!(
                Discovery::prepare(&store, 10, kind, "default", None, executable)
                    .unwrap()
                    .is_none()
            );
            assert!(
                Discovery::prepare(&store, 10, kind, "default", Some("existing"), executable)
                    .unwrap()
                    .is_some()
            );
        }
    }
}
