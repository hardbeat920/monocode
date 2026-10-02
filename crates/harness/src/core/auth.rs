//! Port of src/integrations/harness/core/auth.ts: run a provider's own login
//! flow. The child opens the browser and stores credentials; MonoCode only
//! watches its exit status.
//!
//! The TypeScript read the window label from Tauri and the home directory
//! from the fs plugin. Here the label is a parameter and the home directory
//! comes from the child backend.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures::FutureExt;
use futures::future::Shared;
use parking_lot::Mutex;
use regex::Regex;

use monocode_core::harness::HarnessId;
use monocode_core::js;

use super::child::{ChildAccount, ChildEvent, Children};
use super::provider_accounts::supports_provider_accounts;
use super::task::{self, BoxFuture};

pub use super::auth_support::{
    harness_login_args, is_harness_auth_error, latest_turn_needs_harness_login,
    supports_harness_login,
};

/// `LOGIN_TIMEOUT_MS`.
pub const LOGIN_TIMEOUT_MS: i64 = 10 * 60_000;
const LOGIN_CHILD_PREFIX: &str = "monocode-provider-login-";

fn safe_id(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// `loginChildId`.
pub fn login_child_id(window_label: &str, harness: HarnessId, account_id: Option<&str>) -> String {
    let window_label = if window_label.is_empty() {
        "main"
    } else {
        window_label
    };
    let legacy_id = format!("{LOGIN_CHILD_PREFIX}{}-{harness}", safe_id(window_label));
    match account_id {
        None | Some("default") | Some("") => legacy_id,
        Some(account_id) => format!("{legacy_id}-{}", safe_id(account_id)),
    }
}

/// `LOGIN_RESOLVERS`: the providers with a login command.
fn has_login_resolver(harness: HarnessId) -> bool {
    matches!(
        harness,
        HarnessId::Claude | HarnessId::Codex | HarnessId::Cursor | HarnessId::Grok | HarnessId::Fx
    )
}

static LINKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)https?://\S+").unwrap());

/// `safeLoginDetail`: the last stderr line, without links, at most 240 units.
fn safe_login_detail(value: &str) -> String {
    let without_links = LINKS.replace_all(value, "sign-in link");
    let collapsed = without_links
        .split(js::is_space)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if js::len(&collapsed) > 240 {
        format!("{}…", js::slice_prefix(&collapsed, 237))
    } else {
        collapsed
    }
}

type LoginRun = Shared<BoxFuture<'static, Result<(), String>>>;

/// Runs provider logins. Duplicate clicks share one run, so two OAuth flows
/// cannot race each other. One per window.
#[derive(Clone)]
pub struct HarnessLogin {
    children: Children,
    window_label: String,
    inflight: Arc<Mutex<HashMap<String, LoginRun>>>,
    timeout: Duration,
}

impl HarnessLogin {
    pub fn new(children: Children, window_label: &str) -> Self {
        Self {
            children,
            window_label: window_label.to_string(),
            inflight: Arc::default(),
            timeout: task::ms(LOGIN_TIMEOUT_MS),
        }
    }

    /// Shorten the login timeout. Test seam.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// `loginHarness`. The run starts on the spawner at once, as the promise
    /// did, and a second call for the same provider and account joins it.
    pub fn login_harness(
        &self,
        harness: HarnessId,
        account_id: Option<&str>,
    ) -> BoxFuture<'static, Result<()>> {
        let key = format!("{harness}:{}", account_id.unwrap_or("default"));
        let run = {
            let mut inflight = self.inflight.lock();
            match inflight.get(&key) {
                Some(current) => current.clone(),
                None => {
                    let run = self.start(harness, account_id.map(str::to_string), key.clone());
                    inflight.insert(key, run.clone());
                    run
                }
            }
        };
        async move { run.await.map_err(|error| anyhow!(error)) }.boxed()
    }

    fn start(&self, harness: HarnessId, account_id: Option<String>, key: String) -> LoginRun {
        let (done_tx, done_rx) = futures::channel::oneshot::channel::<Result<(), String>>();
        let login = self.clone();
        let inflight = self.inflight.clone();
        let run: LoginRun = async move {
            done_rx
                .await
                .unwrap_or_else(|_| Err("Sign-in ended without a result".into()))
        }
        .boxed()
        .shared();
        let mine = run.clone();
        self.children.spawner().spawn(
            async move {
                let result = login
                    .run_harness_login(harness, account_id.as_deref())
                    .await
                    .map_err(|error| error.to_string());
                {
                    let mut inflight = inflight.lock();
                    if inflight.get(&key).is_some_and(|run| run.ptr_eq(&mine)) {
                        inflight.remove(&key);
                    }
                }
                let _ = done_tx.send(result);
            }
            .boxed(),
        );
        run
    }

    /// `runHarnessLogin`.
    async fn run_harness_login(&self, harness: HarnessId, account_id: Option<&str>) -> Result<()> {
        let title = harness.title();
        let Some(args) = harness_login_args(harness).filter(|_| has_login_resolver(harness)) else {
            return Err(anyhow!(
                "{title} does not offer a single browser sign-in flow."
            ));
        };

        let children = &self.children;
        let (resolved, cwd) =
            futures::try_join!(children.resolve_binary(harness), children.home_dir())?;
        let child_id = login_child_id(&self.window_label, harness, account_id);
        let _ = children.kill_child(&child_id).await;

        let events = children.watch_child(&child_id);
        let watch = async {
            let mut last_error = String::new();
            while let Ok(event) = events.recv().await {
                match event {
                    ChildEvent::Stdout(_) => {}
                    ChildEvent::Stderr(line) => {
                        let line = js::trim(&line);
                        if !line.is_empty() {
                            last_error = line.to_string();
                        }
                    }
                    ChildEvent::Exit(Some(0)) => return Ok(()),
                    ChildEvent::Exit(code) => {
                        let detail = safe_login_detail(&last_error);
                        if !detail.is_empty() {
                            return Err(anyhow!(detail));
                        }
                        let how = match code {
                            None => " unexpectedly".to_string(),
                            Some(code) => format!(" with code {code}"),
                        };
                        return Err(anyhow!("{title} sign-in exited{how}."));
                    }
                }
            }
            // The watch was replaced. The TypeScript promise waited for its
            // timeout in this case.
            futures::future::pending().await
        };

        let account = account_id
            .filter(|id| *id != "default" && supports_provider_accounts(harness))
            .map(|id| ChildAccount {
                provider: harness,
                id: id.to_string(),
            });
        let spawn = async {
            let spawned = children
                .spawn_child(
                    &child_id,
                    &resolved.path,
                    args.iter().map(|arg| arg.to_string()).collect(),
                    &cwd,
                    account,
                    Some(harness),
                )
                .await;
            match spawned {
                Ok(()) => futures::future::pending().await,
                Err(error) => Err(anyhow!("Could not start {title} sign-in: {error}")),
            }
        };
        let timed_out = async {
            task::sleep(self.timeout).await;
            // Keep the run in flight until the old child is gone, so a quick
            // retry under the same id is not killed by this late cleanup.
            let _ = children.kill_child(&child_id).await;
            Err(anyhow!("{title} sign-in timed out. Please try again."))
        };

        let result = smol::future::or(smol::future::or(watch, spawn), timed_out).await;
        children.unwatch_child(&child_id);
        result
    }

    /// `resetHarnessLoginState`. Test seam.
    pub fn reset_harness_login_state(&self) {
        self.inflight.lock().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::child::{ChildRouter, SpawnRequest};
    use crate::core::testing::{Call, Fake, children};
    use std::sync::Arc;

    fn fake() -> Fake {
        Fake {
            resolved: [
                (HarnessId::Claude, "/bin/claude".to_string()),
                (HarnessId::Codex, "/bin/codex".to_string()),
            ]
            .into(),
            ..Default::default()
        }
    }

    fn spawns(fake: &Fake) -> Vec<SpawnRequest> {
        fake.calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Spawn(request) => Some(request),
                _ => None,
            })
            .collect()
    }

    async fn wait_for_spawn(fake: &Fake) {
        task::timeout(Duration::from_secs(2), async {
            while spawns(fake).is_empty() {
                task::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("the login child was spawned");
    }

    fn exit(router: &Arc<ChildRouter>, child_id: &str, code: Option<i32>) {
        // The fake backend hands out pid 7.
        router.on_exit(child_id, code, 7);
    }

    #[test]
    fn launches_claudes_official_login_command_and_waits_for_success() {
        let (children, fake) = children(fake());
        let login = HarnessLogin::new(children.clone(), "test-window");
        smol::block_on(async {
            let run = login.login_harness(HarnessId::Claude, None);
            wait_for_spawn(&fake).await;
            assert_eq!(
                spawns(&fake),
                vec![SpawnRequest {
                    session_id: "monocode-provider-login-test-window-claude".into(),
                    command: "/bin/claude".into(),
                    args: vec!["auth".into(), "login".into()],
                    cwd: "/home/alice".into(),
                    account: None,
                    binary_provider: Some(HarnessId::Claude),
                    binary_path: None,
                }]
            );
            exit(
                children.router(),
                "monocode-provider-login-test-window-claude",
                Some(0),
            );
            run.await.unwrap();
        });
    }

    #[test]
    fn isolates_a_named_codex_account_during_sign_in() {
        let (children, fake) = children(fake());
        let login = HarnessLogin::new(children.clone(), "test-window");
        smol::block_on(async {
            let run = login.login_harness(HarnessId::Codex, Some("account-work"));
            wait_for_spawn(&fake).await;
            let spawn = spawns(&fake).remove(0);
            assert_eq!(
                spawn.session_id,
                "monocode-provider-login-test-window-codex-account-work"
            );
            assert_eq!(spawn.command, "/bin/codex");
            assert_eq!(spawn.args, vec!["login".to_string()]);
            assert_eq!(
                spawn.account,
                Some(ChildAccount {
                    provider: HarnessId::Codex,
                    id: "account-work".into()
                })
            );
            exit(children.router(), &spawn.session_id, Some(0));
            run.await.unwrap();
        });
    }

    #[test]
    fn deduplicates_repeated_login_clicks() {
        let (children, fake) = children(fake());
        let login = HarnessLogin::new(children.clone(), "test-window");
        smol::block_on(async {
            let first = login.login_harness(HarnessId::Codex, None);
            let second = login.login_harness(HarnessId::Codex, None);
            wait_for_spawn(&fake).await;
            exit(
                children.router(),
                "monocode-provider-login-test-window-codex",
                Some(0),
            );
            first.await.unwrap();
            second.await.unwrap();
            assert_eq!(spawns(&fake).len(), 1);
        });
    }

    #[test]
    fn does_not_invent_one_login_flow_for_multi_provider_harnesses() {
        let (children, fake) = children(fake());
        let login = HarnessLogin::new(children, "test-window");
        let error = smol::block_on(login.login_harness(HarnessId::Opencode, None)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not offer a single browser sign-in flow")
        );
        assert!(spawns(&fake).is_empty());
    }

    #[test]
    fn reports_the_last_stderr_line_without_links() {
        let (children, fake) = children(fake());
        let login = HarnessLogin::new(children.clone(), "w");
        smol::block_on(async {
            let run = login.login_harness(HarnessId::Claude, None);
            wait_for_spawn(&fake).await;
            let router = children.router();
            router.on_stderr(
                "monocode-provider-login-w-claude",
                "Open https://x.test/a?b=c   to sign in".into(),
            );
            router.on_stderr("monocode-provider-login-w-claude", "   ".into());
            exit(router, "monocode-provider-login-w-claude", Some(1));
            assert_eq!(
                run.await.unwrap_err().to_string(),
                "Open sign-in link to sign in"
            );
        });
        assert_eq!(
            login_child_id("", HarnessId::Fx, Some("a b")),
            "monocode-provider-login-main-fx-a-b"
        );
    }

    #[test]
    fn times_out_and_kills_the_child() {
        let (children, fake) = children(fake());
        let login = HarnessLogin::new(children, "w").with_timeout(Duration::from_millis(30));
        let error = smol::block_on(login.login_harness(HarnessId::Codex, None)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Codex sign-in timed out. Please try again."
        );
        let kills = fake
            .calls()
            .into_iter()
            .filter(
                |call| matches!(call, Call::Kill(id) if id == "monocode-provider-login-w-codex"),
            )
            .count();
        assert_eq!(kills, 2, "once before the spawn, once at the timeout");
    }
}
