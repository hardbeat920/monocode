//! Port of src/features/settings/model/mcpSettingsCache.ts: MCP discovery
//! shared by Settings and the composer pickers. Claude's slower health check
//! never delays the list.
//!
//! The TypeScript kept the cache in module maps. Here it is a
//! [`McpSettingsCache`] value whose requests run on a spawner, the way the
//! promises ran whether or not anyone awaited them.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared};
use monocode_harness::core::task::SharedSpawner;
use parking_lot::Mutex;

use super::mcp::{McpConnection, McpProvider, McpScope, parse_claude_mcp_list};

/// `McpServerRow`: a connection with its status text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerRow {
    pub connection: McpConnection,
    pub status: String,
}

/// `McpSettingsSnapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpSettingsSnapshot {
    pub servers: Vec<McpServerRow>,
    pub error: String,
    pub claude_error: String,
}

/// The two backend calls: `mcp_discover` and `claude_mcp_list`.
pub trait McpSources: Send + Sync {
    fn mcp_discover(&self, cwd: String) -> BoxFuture<'static, Result<Vec<McpConnection>, String>>;
    fn claude_mcp_list(&self, cwd: String) -> BoxFuture<'static, Result<String, String>>;
}

/// The real sources over the process crate, on smol's blocking pool.
pub struct ProcessMcpSources {
    pub host: monocode_process::harness::HarnessHost,
}

impl McpSources for ProcessMcpSources {
    fn mcp_discover(&self, cwd: String) -> BoxFuture<'static, Result<Vec<McpConnection>, String>> {
        smol::unblock(move || {
            let found = monocode_process::mcp::mcp_discover(cwd)?;
            serde_json::to_value(found)
                .and_then(serde_json::from_value)
                .map_err(|error| error.to_string())
        })
        .boxed()
    }

    fn claude_mcp_list(&self, cwd: String) -> BoxFuture<'static, Result<String, String>> {
        let host = self.host.clone();
        smol::unblock(move || monocode_process::harness::claude_mcp_list(&host, cwd)).boxed()
    }
}

type Listener = Arc<dyn Fn(&McpSettingsSnapshot) + Send + Sync>;
type Discovery = Shared<BoxFuture<'static, McpSettingsSnapshot>>;

#[derive(Default)]
struct State {
    snapshots: HashMap<String, McpSettingsSnapshot>,
    requests: HashMap<String, (u64, Discovery)>,
    health: HashSet<String>,
    listeners: HashMap<String, Vec<(u64, Listener)>>,
    next: u64,
}

struct Inner {
    sources: Arc<dyn McpSources>,
    spawner: SharedSpawner,
    state: Mutex<State>,
}

/// The MCP settings cache. Clones share one cache.
#[derive(Clone)]
pub struct McpSettingsCache {
    inner: Arc<Inner>,
}

impl McpSettingsCache {
    pub fn new(sources: Arc<dyn McpSources>, spawner: SharedSpawner) -> Self {
        Self {
            inner: Arc::new(Inner {
                sources,
                spawner,
                state: Mutex::default(),
            }),
        }
    }

    /// `getCachedMcpSettings`.
    pub fn get_cached_mcp_settings(&self, cwd: &str) -> Option<McpSettingsSnapshot> {
        self.inner.state.lock().snapshots.get(cwd).cloned()
    }

    /// `subscribeMcpSettings`. Returns an id for
    /// [`McpSettingsCache::unsubscribe`].
    pub fn subscribe_mcp_settings(
        &self,
        cwd: &str,
        listener: impl Fn(&McpSettingsSnapshot) + Send + Sync + 'static,
    ) -> u64 {
        let mut state = self.inner.state.lock();
        state.next += 1;
        let id = state.next;
        state
            .listeners
            .entry(cwd.to_string())
            .or_default()
            .push((id, Arc::new(listener)));
        id
    }

    pub fn unsubscribe(&self, cwd: &str, id: u64) {
        let mut state = self.inner.state.lock();
        if let Some(listeners) = state.listeners.get_mut(cwd) {
            listeners.retain(|(entry, _)| *entry != id);
            if listeners.is_empty() {
                state.listeners.remove(cwd);
            }
        }
    }

    fn publish(&self, cwd: &str, snapshot: McpSettingsSnapshot) {
        let listeners: Vec<Listener> = {
            let mut state = self.inner.state.lock();
            state.snapshots.insert(cwd.to_string(), snapshot.clone());
            state
                .listeners
                .get(cwd)
                .map(|listeners| {
                    listeners
                        .iter()
                        .map(|(_, listener)| listener.clone())
                        .collect()
                })
                .unwrap_or_default()
        };
        for listener in listeners {
            listener(&snapshot);
        }
    }

    fn current_request(&self, cwd: &str) -> Option<u64> {
        self.inner.state.lock().requests.get(cwd).map(|(id, _)| *id)
    }

    /// `loadMcpSettings`: discovery is shared across settings and pickers;
    /// health never delays the list. Pass `claude_health: false` for pickers
    /// that do not need Claude's status.
    pub fn load_mcp_settings(
        &self,
        cwd: &str,
        force: bool,
        claude_health: bool,
    ) -> BoxFuture<'static, McpSettingsSnapshot> {
        let (id, discovery) = {
            let mut state = self.inner.state.lock();
            let existing = state
                .requests
                .get(cwd)
                .filter(|_| !force)
                .map(|(id, discovery)| (*id, discovery.clone()));
            match existing {
                Some(existing) => existing,
                None => {
                    state.next += 1;
                    let id = state.next;
                    let (done, result) = oneshot::channel();
                    let discovery: Discovery = result
                        .map(|snapshot| snapshot.unwrap_or_default())
                        .boxed()
                        .shared();
                    state
                        .requests
                        .insert(cwd.to_string(), (id, discovery.clone()));
                    drop(state);
                    let cache = self.clone();
                    let key = cwd.to_string();
                    self.inner.spawner.spawn(
                        async move {
                            let snapshot = cache.fetch_mcp_settings(&key).await;
                            if cache.current_request(&key) == Some(id) {
                                cache.inner.state.lock().health.remove(&key);
                                cache.publish(&key, snapshot.clone());
                            }
                            let _ = done.send(snapshot);
                        }
                        .boxed(),
                    );
                    (id, discovery)
                }
            }
        };
        let cache = self.clone();
        let key = cwd.to_string();
        async move {
            let snapshot = discovery.await;
            if claude_health && snapshot.error.is_empty() && cache.current_request(&key) == Some(id)
            {
                cache.load_claude_health(&key, id);
            }
            cache.get_cached_mcp_settings(&key).unwrap_or(snapshot)
        }
        .boxed()
    }

    async fn fetch_mcp_settings(&self, cwd: &str) -> McpSettingsSnapshot {
        match self.inner.sources.mcp_discover(cwd.to_string()).await {
            Ok(configured) => McpSettingsSnapshot {
                servers: configured
                    .into_iter()
                    .map(|connection| {
                        let status = if connection.enabled == Some(false) {
                            "Disabled"
                        } else {
                            "Configured"
                        };
                        McpServerRow {
                            connection,
                            status: status.into(),
                        }
                    })
                    .collect(),
                error: String::new(),
                claude_error: String::new(),
            },
            Err(cause) => McpSettingsSnapshot {
                servers: Vec::new(),
                error: cause,
                claude_error: String::new(),
            },
        }
    }

    fn load_claude_health(&self, cwd: &str, discovery: u64) {
        if !self.inner.state.lock().health.insert(cwd.to_string()) {
            return;
        }
        let cache = self.clone();
        let key = cwd.to_string();
        let request = self.inner.sources.claude_mcp_list(cwd.to_string());
        self.inner.spawner.spawn(
            async move {
                let output = request.await;
                if cache.current_request(&key) != Some(discovery) {
                    return;
                }
                let snapshot = cache.get_cached_mcp_settings(&key).unwrap_or_default();
                match output {
                    Ok(output) => {
                        // `new Map(...)`: a repeated name keeps its first position
                        // and its last status.
                        let mut health: Vec<(String, String)> = Vec::new();
                        for server in parse_claude_mcp_list(&output) {
                            match health.iter_mut().find(|(name, _)| *name == server.name) {
                                Some(entry) => entry.1 = server.status,
                                None => health.push((server.name, server.status)),
                            }
                        }
                        let lookup: HashMap<&str, &str> = health
                            .iter()
                            .map(|(name, status)| (name.as_str(), status.as_str()))
                            .collect();
                        let mut servers: Vec<McpServerRow> = snapshot
                            .servers
                            .iter()
                            .map(|server| {
                                let status = if server.connection.enabled == Some(false) {
                                    "Disabled".to_string()
                                } else if server.connection.provider == McpProvider::Claude {
                                    lookup.get(server.connection.name.as_str()).map_or_else(
                                        || "Configured".to_string(),
                                        |status| status.to_string(),
                                    )
                                } else {
                                    server.status.clone()
                                };
                                McpServerRow {
                                    connection: server.connection.clone(),
                                    status,
                                }
                            })
                            .collect();
                        // Claude can supply connections that are not stored in a
                        // local config file.
                        for (name, status) in health {
                            if servers.iter().any(|server| {
                                server.connection.provider == McpProvider::Claude
                                    && server.connection.name == name
                            }) {
                                continue;
                            }
                            servers.push(McpServerRow {
                                connection: McpConnection {
                                    provider: McpProvider::Claude,
                                    name,
                                    scope: McpScope::Local,
                                    config_path: String::new(),
                                    transport: String::new(),
                                    enabled: None,
                                },
                                status,
                            });
                        }
                        cache.publish(
                            &key,
                            McpSettingsSnapshot {
                                servers,
                                claude_error: String::new(),
                                ..snapshot
                            },
                        );
                    }
                    Err(cause) => cache.publish(
                        &key,
                        McpSettingsSnapshot {
                            claude_error: cause,
                            ..snapshot
                        },
                    ),
                }
            }
            .boxed(),
        );
    }

    /// `clearMcpSettingsCache`.
    pub fn clear_mcp_settings_cache(&self) {
        let mut state = self.inner.state.lock();
        state.snapshots.clear();
        state.requests.clear();
        state.health.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use monocode_harness::core::task::SmolSpawner;

    struct FakeSources {
        configured: Mutex<Vec<McpConnection>>,
        health: Mutex<VecDeque<oneshot::Receiver<Result<String, String>>>>,
        health_now: Mutex<Option<Result<String, String>>>,
        calls: AtomicUsize,
    }

    impl McpSources for FakeSources {
        fn mcp_discover(
            &self,
            _cwd: String,
        ) -> BoxFuture<'static, Result<Vec<McpConnection>, String>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            futures::future::ready(Ok(self.configured.lock().clone())).boxed()
        }

        fn claude_mcp_list(&self, _cwd: String) -> BoxFuture<'static, Result<String, String>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(now) = self.health_now.lock().clone() {
                return futures::future::ready(now).boxed();
            }
            let receiver = self.health.lock().pop_front().expect("health reply");
            receiver
                .map(|reply| reply.unwrap_or(Err("dropped".into())))
                .boxed()
        }
    }

    fn docs(enabled: Option<bool>) -> McpConnection {
        McpConnection {
            provider: McpProvider::Claude,
            name: "docs".into(),
            scope: McpScope::Project,
            config_path: "/repo/.mcp.json".into(),
            transport: "stdio".into(),
            enabled,
        }
    }

    fn setup(
        health_now: Option<Result<String, String>>,
        enabled: Option<bool>,
    ) -> (Arc<FakeSources>, McpSettingsCache) {
        let sources = Arc::new(FakeSources {
            configured: Mutex::new(vec![docs(enabled)]),
            health: Mutex::default(),
            health_now: Mutex::new(health_now),
            calls: AtomicUsize::new(0),
        });
        let cache = McpSettingsCache::new(sources.clone(), Arc::new(SmolSpawner));
        (sources, cache)
    }

    fn deferred_health(sources: &FakeSources) -> oneshot::Sender<Result<String, String>> {
        let (sender, receiver) = oneshot::channel();
        sources.health.lock().push_back(receiver);
        sender
    }

    /// `vi.waitFor`.
    fn wait_for(check: impl Fn() -> bool) {
        for _ in 0..200 {
            if check() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("condition never held");
    }

    #[test]
    fn publishes_discovery_before_slow_health_and_shares_both_requests_across_consumers() {
        let (sources, cache) = setup(None, None);
        let resolve = deferred_health(&sources);
        let changes: Arc<Mutex<Vec<McpSettingsSnapshot>>> = Arc::default();
        let sink = changes.clone();
        let id = cache
            .subscribe_mcp_settings("/repo", move |snapshot| sink.lock().push(snapshot.clone()));
        smol::block_on(async {
            let (picker, settings) = futures::join!(
                cache.load_mcp_settings("/repo", false, false),
                cache.load_mcp_settings("/repo", false, true)
            );
            assert_eq!(picker.servers[0].status, "Configured");
            assert_eq!(settings.servers[0].connection.name, "docs");
            assert_eq!(sources.calls.load(Ordering::SeqCst), 2);
            cache.load_mcp_settings("/repo", false, true).await;
            assert_eq!(sources.calls.load(Ordering::SeqCst), 2);
        });
        resolve
            .send(Ok(
                "docs: local - Connected\nremote: https://example.com - Needs authentication"
                    .into(),
            ))
            .unwrap();
        wait_for(|| {
            cache
                .get_cached_mcp_settings("/repo")
                .is_some_and(|snapshot| snapshot.servers.len() == 2)
        });
        assert_eq!(
            changes.lock().last().unwrap().servers[0].status,
            "Connected"
        );
        cache.unsubscribe("/repo", id);
    }

    #[test]
    fn loads_non_claude_pickers_without_running_claude_and_lets_a_later_claude_consumer_request_health()
     {
        let (sources, cache) = setup(Some(Ok("docs: local - Connected".into())), None);
        smol::block_on(async {
            cache.load_mcp_settings("/repo", false, false).await;
            cache.load_mcp_settings("/repo", false, false).await;
            assert_eq!(sources.calls.load(Ordering::SeqCst), 1);
            cache.load_mcp_settings("/repo", false, true).await;
        });
        wait_for(|| {
            cache
                .get_cached_mcp_settings("/repo")
                .is_some_and(|snapshot| snapshot.servers[0].status == "Connected")
        });
        assert_eq!(sources.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn ignores_health_from_a_request_superseded_by_refresh() {
        let (sources, cache) = setup(None, None);
        let first = deferred_health(&sources);
        let second = deferred_health(&sources);
        smol::block_on(async {
            cache.load_mcp_settings("/repo", false, true).await;
            cache.load_mcp_settings("/repo", true, true).await;
        });
        second.send(Ok("docs: local - Connected".into())).unwrap();
        wait_for(|| {
            cache
                .get_cached_mcp_settings("/repo")
                .is_some_and(|snapshot| snapshot.servers[0].status == "Connected")
        });
        first
            .send(Ok("docs: local - Failed\nstale: local - Connected".into()))
            .unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let snapshot = cache.get_cached_mcp_settings("/repo").unwrap();
        let names: Vec<&str> = snapshot
            .servers
            .iter()
            .map(|row| row.connection.name.as_str())
            .collect();
        assert_eq!(names, ["docs"]);
        assert_eq!(snapshot.servers[0].status, "Connected");
    }

    #[test]
    fn keeps_configured_rows_when_health_fails_and_preserves_disabled_status() {
        let (_sources, cache) = setup(Some(Err("Error: Health unavailable".into())), Some(false));
        smol::block_on(cache.load_mcp_settings("/repo", false, true));
        wait_for(|| {
            cache
                .get_cached_mcp_settings("/repo")
                .is_some_and(|snapshot| snapshot.claude_error.contains("Health unavailable"))
        });
        let snapshot = cache.get_cached_mcp_settings("/repo").unwrap();
        assert_eq!(snapshot.servers[0].status, "Disabled");
        assert_eq!(snapshot.error, "");
    }
}
