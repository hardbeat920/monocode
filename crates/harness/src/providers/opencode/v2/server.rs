use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use monocode_core::harness::HarnessId;
use serde_json::Value;

use super::client::Client;
use crate::core::child::{ChildEvent, ChildEvents, Children, SpawnRequest};
use crate::core::task::timeout;

/// Production uses the normal CLI profile. Tests can supply private XDG roots.
#[derive(Clone, Default)]
pub struct Options {
    pub environment: HashMap<String, String>,
}

impl std::fmt::Debug for Options {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenCodeV2Options")
            .field(
                "environment_names",
                &self.environment.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Options {
    pub fn isolated(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
        }
        let mut environment = HashMap::new();
        for (name, dir) in [
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_STATE_HOME", "state"),
            ("TMPDIR", "tmp"),
        ] {
            let path = root.join(dir);
            std::fs::create_dir_all(&path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
            }
            environment.insert(name.into(), path.to_string_lossy().into_owned());
        }
        environment.insert("OPENCODE_CONFIG_PROJECT_DISABLE".into(), "1".into());
        environment.insert("OPENCODE_CONFIG_CONTENT".into(), "{}".into());
        environment.insert(
            "OPENCODE_CONFIG_DIR".into(),
            root.join("config/opencode").to_string_lossy().into_owned(),
        );
        Ok(Self { environment })
    }
}

#[derive(Clone)]
pub struct Server {
    pub client: Client,
    pub events: ChildEvents,
    id: String,
    children: Children,
}

impl Server {
    pub async fn start(children: Children, directory: &str, options: &Options) -> Result<Self> {
        Self::start_for_session(children, directory, options, None).await
    }

    pub async fn start_for_session(
        children: Children,
        directory: &str,
        options: &Options,
        session: Option<&str>,
    ) -> Result<Self> {
        Self::start_inner(
            children,
            directory,
            options,
            session,
            Duration::from_secs(30),
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn start_with_timeout(
        children: Children,
        directory: &str,
        options: &Options,
        duration: Duration,
    ) -> Result<Self> {
        Self::start_inner(children, directory, options, None, duration).await
    }

    async fn start_inner(
        children: Children,
        directory: &str,
        options: &Options,
        session: Option<&str>,
        duration: Duration,
    ) -> Result<Self> {
        let binary = children.resolve_open_code_binary().await?;
        let port = children.free_harness_port().await?;
        let id = session
            .map(str::to_string)
            .unwrap_or_else(|| format!("monocode-opencode-v2-{}", uuid::Uuid::new_v4()));
        let password = uuid::Uuid::new_v4().to_string();
        let mut environment = options.environment.clone();
        environment.insert("OPENCODE_PASSWORD".into(), password.clone());
        environment.insert("OPENCODE_CLIENT".into(), "monocode".into());
        let events = children.watch_child(&id);
        let request = SpawnRequest {
            session_id: id.clone(),
            command: binary.path,
            args: vec![
                "serve".into(),
                "--stdio".into(),
                "--hostname=127.0.0.1".into(),
                format!("--port={port}"),
            ],
            cwd: directory.into(),
            binary_provider: Some(HarnessId::Opencode),
            environment,
            ..Default::default()
        };
        if let Err(error) = children.spawn_request(request).await {
            children.unwatch_child(&id);
            return Err(error);
        }
        let ready = timeout(duration, async {
            while let Ok(event) = events.recv().await {
                match event {
                    ChildEvent::Stdout(line) => {
                        let value: Value = match serde_json::from_str(&line) {
                            Ok(value) => value,
                            Err(_) => continue,
                        };
                        if let Some(url) = value.get("url").and_then(Value::as_str) {
                            return Client::new(url, directory, &password, children.clone());
                        }
                    }
                    ChildEvent::Stderr(_) => {}
                    ChildEvent::Exit(code) => {
                        bail!("OpenCode 2 server exited before startup with code {code:?}")
                    }
                }
            }
            Err(anyhow!("OpenCode 2 server closed before startup"))
        })
        .await;
        match ready {
            Some(Ok(client)) => Ok(Self {
                client,
                id,
                children,
                events,
            }),
            failure => {
                let _ = children.kill_child(&id).await;
                children.unwatch_child(&id);
                match failure {
                    Some(Err(error)) => Err(error),
                    _ => Err(anyhow!("OpenCode 2 server startup timed out")),
                }
            }
        }
    }

    pub fn at(&self, directory: &str) -> Client {
        let mut client = self.client.clone();
        client.directory = directory.into();
        client
    }

    pub async fn stop(&self) {
        let _ = self.children.kill_child(&self.id).await;
        self.children.unwatch_child(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::opencode::test_support::{FakeHost, wait_for};

    #[test]
    fn rejected_readiness_url_kills_and_unwatches_the_owned_child() {
        smol::block_on(async {
            let host = FakeHost::v2_startup(r#"{"url":"https://example.com:4096"}"#);
            let result = Server::start(host.children(), "/owned/work", &Options::default()).await;
            assert!(result.is_err());
            assert_eq!(host.kills(), vec![host.spawns()[0].session_id.clone()]);
            assert_eq!(host.watched_children(), 0);
            assert_eq!(host.watched_streams(), 0);
        });
    }

    #[test]
    fn startup_timeout_and_early_exit_release_the_owned_watch() {
        smol::block_on(async {
            let host = FakeHost::v2_startup("not a readiness line");
            let result = Server::start_with_timeout(
                host.children(),
                "/owned/work",
                &Options::default(),
                Duration::from_millis(5),
            )
            .await;
            assert!(
                result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("startup timed out")
            );
            assert_eq!(host.watched_children(), 0);
            assert_eq!(host.kills().len(), 1);

            let host = FakeHost::v2_startup("not a readiness line");
            let pending = {
                let children = host.children();
                smol::spawn(async move {
                    Server::start(children, "/owned/work", &Options::default()).await
                })
            };
            wait_for("owned startup child", || !host.spawns().is_empty()).await;
            host.exit(&host.spawns()[0].session_id, Some(7));
            assert!(
                pending
                    .await
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("code Some(7)")
            );
            assert_eq!(host.watched_children(), 0);
            assert_eq!(host.kills().len(), 1);
        });
    }

    #[test]
    fn stop_releases_watch_and_debug_never_contains_the_server_password() {
        smol::block_on(async {
            let host = FakeHost::v2();
            let server = Server::start(host.children(), "/owned/work", &Options::default())
                .await
                .unwrap();
            let spawned = &host.spawns()[0];
            assert_eq!(host.watched_children(), 1);
            assert!(!format!("{spawned:?}").contains(&spawned.environment["OPENCODE_PASSWORD"]));
            server.stop().await;
            assert_eq!(host.watched_children(), 0);
            assert_eq!(host.kills(), vec![spawned.session_id.clone()]);
        });
    }
}
