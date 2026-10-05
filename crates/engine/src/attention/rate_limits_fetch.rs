//! Port of src/features/providers/model/rateLimitsFetch.ts: one usage fetch
//! per provider. Claude, OpenCode Go, and Droid go through
//! `monocode_integrations` (the old Tauri commands) on the background
//! executor. Codex and Grok only report usage through their CLIs, so those
//! spawn a short-lived child through the harness `Children`.

use std::path::PathBuf;
use std::sync::{Arc, LazyLock, OnceLock};

use futures::FutureExt;
use futures::future::BoxFuture;
use gpui::BackgroundExecutor;
use monocode_core::HarnessId;
use monocode_harness::core::acp::{AcpClient, AcpHandlers};
use monocode_harness::core::child::{ChildAccount, ChildHandlers, Children};
use monocode_harness::core::json_rpc::{
    JsonRpcClient, JsonRpcClientOptions, JsonRpcHandlers, LineWriter,
};
use monocode_harness::core::task::{ms, timeout};
use monocode_harness::providers::grok::protocol::{
    GrokSpawnInput, grok_auth_method_id, grok_spawn_args,
};
use regex::Regex;
use serde_json::{Value, json};

use super::pi_usage::{PiUsageProvider, fetch_pi_usage};
use super::rate_limits::{
    ProviderRateLimits, RateLimitProvider, error_rate_limits, now_ms, parse_claude_oauth_usage,
    parse_codex_rate_limits, parse_droid_usage, parse_grok_billing, parse_opencode_go_usage,
    unavailable_rate_limits,
};

const USAGE_CHILD_ID: &str = "monocode-codex-usage";
const GROK_USAGE_CHILD_ID: &str = "monocode-grok-usage";
const DISCOVERY_TIMEOUT_MS: i64 = 15_000;
const REQUEST_TIMEOUT_MS: i64 = 12_000;

/// `CodexRateLimitResetOutcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexResetOutcome {
    Reset,
    NothingToReset,
    NoCredit,
    AlreadyRedeemed,
}

/// Where `RateLimits` gets its numbers. `NativeRateLimitFetcher` is the
/// real one; tests pass a fake.
pub trait RateLimitFetcher: Send + Sync {
    /// `fetchProviderRateLimits`. Never fails: problems come back as an
    /// `error` or `unavailable` snapshot.
    fn fetch(
        &self,
        provider: RateLimitProvider,
        account_id: &str,
    ) -> BoxFuture<'static, ProviderRateLimits>;

    /// `fetchPiUsage`.
    fn fetch_pi(&self, provider: PiUsageProvider) -> BoxFuture<'static, ProviderRateLimits>;

    /// `consumeCodexRateLimitResetCredit`.
    fn consume_codex_reset(
        &self,
        credit_id: Option<String>,
        account_id: &str,
    ) -> BoxFuture<'static, Result<CodexResetOutcome, String>>;
}

/// The status a `fetch_*_usage` command returned.
struct UsageFetch {
    status: String,
    body: Option<String>,
    error: Option<String>,
}

/// `result.error?.trim() || fallback`.
fn reason(error: Option<String>, fallback: &str) -> String {
    error
        .map(|error| monocode_core::js::trim(&error).to_string())
        .filter(|error| !error.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

/// `fetchClaudeRateLimits`, over the `fetch_claude_usage` result.
fn claude_from(result: Result<UsageFetch, String>, now: i64) -> ProviderRateLimits {
    let provider = RateLimitProvider::Claude;
    let result = match result {
        Ok(result) => result,
        // The Tauri command rejected with a string, not an `Error`.
        Err(_) => return error_rate_limits(provider, "Claude usage unavailable", None, now),
    };
    if result.status == "ok"
        && let Some(body) = result.body.filter(|body| !body.is_empty())
    {
        return parse_claude_oauth_usage(&body, now);
    }
    if result.status == "unavailable" {
        return unavailable_rate_limits(
            provider,
            &reason(result.error, "Claude not signed in"),
            now,
        );
    }
    error_rate_limits(
        provider,
        &reason(result.error, "Claude usage unavailable"),
        None,
        now,
    )
}

/// `fetchOpencodeGoRateLimits`.
fn opencode_from(result: Result<UsageFetch, String>, now: i64) -> ProviderRateLimits {
    let provider = RateLimitProvider::Opencode;
    let result = match result {
        Ok(result) => result,
        Err(_) => return error_rate_limits(provider, "OpenCode Go usage unavailable", None, now),
    };
    if result.status == "ok"
        && let Some(body) = result.body.filter(|body| !body.is_empty())
    {
        let Ok(value) = serde_json::from_str::<Value>(&body) else {
            return error_rate_limits(provider, "OpenCode Go response was not JSON", None, now);
        };
        let parsed = parse_opencode_go_usage(&value, now);
        if parsed.has_window() {
            return parsed;
        }
        // A 200 with no usable windows is malformed: report an error so the
        // footer retries instead of sticking in "unavailable" forever.
        return error_rate_limits(
            provider,
            "OpenCode Go usage response was unexpected",
            None,
            now,
        );
    }
    if result.status == "unavailable" {
        return unavailable_rate_limits(
            provider,
            &reason(result.error, "OpenCode Go not connected"),
            now,
        );
    }
    error_rate_limits(
        provider,
        &reason(result.error, "OpenCode Go usage unavailable"),
        None,
        now,
    )
}

/// `fetchDroidRateLimits`.
fn droid_from(result: Result<UsageFetch, String>, now: i64) -> ProviderRateLimits {
    let provider = RateLimitProvider::Droid;
    let result = match result {
        Ok(result) => result,
        Err(_) => return error_rate_limits(provider, "Droid usage unavailable", None, now),
    };
    if result.status == "ok"
        && let Some(body) = result.body.filter(|body| !body.is_empty())
    {
        let Ok(value) = serde_json::from_str::<Value>(&body) else {
            return error_rate_limits(provider, "Droid usage response was not JSON", None, now);
        };
        let parsed = parse_droid_usage(&value, now);
        if parsed.has_window() {
            return parsed;
        }
        return unavailable_rate_limits(provider, "No Droid usage data", now);
    }
    if result.status == "unavailable" {
        return unavailable_rate_limits(
            provider,
            &reason(result.error, "Droid not signed in"),
            now,
        );
    }
    error_rate_limits(
        provider,
        &reason(result.error, "Droid usage unavailable"),
        None,
        now,
    )
}

static CODEX_SIGNED_OUT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)not signed in|chatgpt authentication required|not authenticated")
        .expect("valid regex")
});
static GROK_SIGNED_OUT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)authentication required|not authenticated|not signed in").expect("valid regex")
});
static CLI_MISSING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)ENOENT|not found|could not run").expect("valid regex"));

/// `fetchCodexRateLimits` after the probe returned.
pub(crate) fn codex_from(result: Result<Value, String>, now: i64) -> ProviderRateLimits {
    let provider = RateLimitProvider::Codex;
    match result {
        Ok(result) => {
            let parsed = parse_codex_rate_limits(&result, now);
            if parsed.has_window() || parsed.reset_credits.is_some() {
                return parsed;
            }
            if result.is_object() && parsed.session.is_none() && parsed.weekly.is_none() {
                return unavailable_rate_limits(provider, "No Codex usage data", now);
            }
            parsed
        }
        Err(message) => {
            if CODEX_SIGNED_OUT.is_match(&message) {
                return unavailable_rate_limits(provider, "Codex not signed in", now);
            }
            if CLI_MISSING.is_match(&message) {
                return unavailable_rate_limits(provider, "Codex CLI not found", now);
            }
            error_rate_limits(provider, &message, None, now)
        }
    }
}

/// `fetchGrokRateLimits` after the probe returned.
pub(crate) fn grok_from(result: Result<Value, String>, now: i64) -> ProviderRateLimits {
    let provider = RateLimitProvider::Grok;
    match result {
        Ok(result) => {
            let parsed = parse_grok_billing(&result, now);
            if parsed.weekly.is_some() || parsed.monthly.is_some() {
                return parsed;
            }
            unavailable_rate_limits(provider, "No Grok usage data", now)
        }
        Err(message) => {
            if GROK_SIGNED_OUT.is_match(&message) {
                return unavailable_rate_limits(provider, "Grok not signed in", now);
            }
            if CLI_MISSING.is_match(&message) {
                return unavailable_rate_limits(provider, "Grok Build CLI not found", now);
            }
            error_rate_limits(provider, &message, None, now)
        }
    }
}

/// `consumeCodexRateLimitResetCredit` after the probe returned.
pub(crate) fn reset_outcome_from(result: &Value) -> Result<CodexResetOutcome, String> {
    match result.get("outcome").and_then(Value::as_str) {
        Some("reset") => Ok(CodexResetOutcome::Reset),
        Some("nothingToReset") => Ok(CodexResetOutcome::NothingToReset),
        Some("noCredit") => Ok(CodexResetOutcome::NoCredit),
        Some("alreadyRedeemed") => Ok(CodexResetOutcome::AlreadyRedeemed),
        _ => Err("Codex returned an unknown reset result".into()),
    }
}

/// Codex usage requests over a short-lived `codex app-server` child.
///
/// Every probe reuses `USAGE_CHILD_ID` and kills whatever holds it first, so
/// probes for different accounts (footer, Settings, account picker) must not
/// overlap or they end each other. The lock runs them one at a time.
#[derive(Clone)]
pub struct CodexProbe {
    children: Children,
    lock: Arc<futures::lock::Mutex<()>>,
}

impl CodexProbe {
    pub fn new(children: Children) -> Self {
        Self {
            children,
            lock: Arc::default(),
        }
    }

    /// `requestCodexAccount`.
    pub async fn request(
        &self,
        method: &'static str,
        params: Value,
        account_id: String,
    ) -> Result<Value, String> {
        let _turn = self.lock.lock().await;
        let path = self
            .children
            .resolve_codex_binary()
            .await
            .map_err(|_| "Codex CLI not found".to_string())?
            .path;
        run_codex_account_request(self.children.clone(), path, method, params, account_id).await
    }
}

/// The real fetches.
pub struct NativeRateLimitFetcher {
    data_dir: PathBuf,
    executor: BackgroundExecutor,
    children: Option<Children>,
    codex: Option<CodexProbe>,
}

impl NativeRateLimitFetcher {
    /// `data_dir` holds the provider account profiles. Without `children`
    /// (no harness bridge yet), Codex and Grok report their CLI as missing.
    pub fn new(
        data_dir: PathBuf,
        executor: BackgroundExecutor,
        children: Option<Children>,
    ) -> Self {
        Self {
            data_dir,
            executor,
            codex: children.clone().map(CodexProbe::new),
            children,
        }
    }

    fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> BoxFuture<'static, T> {
        self.executor.spawn(async move { work() }).boxed()
    }

    fn request_codex_account(
        &self,
        method: &'static str,
        params: Value,
        account_id: String,
    ) -> BoxFuture<'static, Result<Value, String>> {
        let Some(codex) = self.codex.clone() else {
            return async { Err("Codex CLI not found".to_string()) }.boxed();
        };
        async move { codex.request(method, params, account_id).await }.boxed()
    }
}

fn home() -> String {
    monocode_platform::dirs_home().unwrap_or_else(|| "/".into())
}

/// Responds `{}` to every request the child sends, as the TypeScript did.
fn auto_respond(client: Arc<OnceLock<JsonRpcClient>>, children: &Children) -> JsonRpcHandlers {
    let spawner = children.spawner().clone();
    JsonRpcHandlers::default().on_request(move |id, _method, _params| {
        if let Some(rpc) = client.get().cloned() {
            spawner.spawn(
                async move {
                    let _ = rpc.respond(id, json!({})).await;
                }
                .boxed(),
            );
        }
    })
}

/// `runCodexAccountRequest`.
async fn run_codex_account_request(
    children: Children,
    path: String,
    method: &'static str,
    params: Value,
    account_id: String,
) -> Result<Value, String> {
    let cell = Arc::new(OnceLock::new());
    let writer: Arc<dyn LineWriter> = Arc::new(children.clone());
    let rpc = JsonRpcClient::new(
        USAGE_CHILD_ID,
        writer,
        auto_respond(cell.clone(), &children),
        JsonRpcClientOptions {
            include_jsonrpc: false,
            label: "codex-usage".into(),
            ..Default::default()
        },
    );
    let _ = cell.set(rpc.clone());
    let stop = {
        let rpc = rpc.clone();
        let children = children.clone();
        move || async move {
            rpc.close(None);
            children.unwatch_child(USAGE_CHILD_ID);
            let _ = children.kill_child(USAGE_CHILD_ID).await;
        }
    };
    let _ = children.kill_child(USAGE_CHILD_ID).await;
    {
        let on_line = rpc.clone();
        let on_exit = rpc.clone();
        children.watch_child_with(
            USAGE_CHILD_ID,
            ChildHandlers {
                on_line: Box::new(move |line| on_line.push_line(&line)),
                on_exit: Box::new(move |_| on_exit.close(Some("Codex usage probe exited"))),
                on_stderr: None,
            },
        );
    }
    let result = async {
        children
            .spawn_child(
                USAGE_CHILD_ID,
                &path,
                vec!["app-server".into()],
                &home(),
                Some(ChildAccount {
                    provider: HarnessId::Codex,
                    id: account_id,
                }),
                Some(HarnessId::Codex),
            )
            .await
            .map_err(|error| error.to_string())?;
        let work = async {
            rpc.request_value(
                "initialize",
                Some(json!({
                    "clientInfo": { "name": "monocode", "title": "MonoCode", "version": "0.1.0" },
                    "capabilities": { "experimentalApi": true },
                })),
                REQUEST_TIMEOUT_MS,
            )
            .await?;
            rpc.notify("initialized", None).await?;
            rpc.request_value(method, Some(params), REQUEST_TIMEOUT_MS)
                .await
        };
        match timeout(ms(DISCOVERY_TIMEOUT_MS), work).await {
            Some(result) => result.map_err(|error| error.to_string()),
            None => Err("Codex usage probe timed out".to_string()),
        }
    }
    .await;
    stop().await;
    result
}

/// `requestGrokBilling`.
async fn request_grok_billing(children: Children, path: String) -> Result<Value, String> {
    let cell: Arc<OnceLock<AcpClient>> = Arc::new(OnceLock::new());
    let writer: Arc<dyn LineWriter> = Arc::new(children.clone());
    let responder = cell.clone();
    let spawner = children.spawner().clone();
    let acp = AcpClient::new(
        GROK_USAGE_CHILD_ID,
        writer,
        AcpHandlers::default().on_request(move |id, _method, _params| {
            if let Some(acp) = responder.get().cloned() {
                spawner.spawn(
                    async move {
                        let _ = acp.respond(id, json!({})).await;
                    }
                    .boxed(),
                );
            }
        }),
    );
    let _ = cell.set(acp.clone());
    let stop = {
        let acp = acp.clone();
        let children = children.clone();
        move || async move {
            acp.close(None);
            children.unwatch_child(GROK_USAGE_CHILD_ID);
            let _ = children.kill_child(GROK_USAGE_CHILD_ID).await;
        }
    };
    let _ = children.kill_child(GROK_USAGE_CHILD_ID).await;
    {
        let on_line = acp.clone();
        let on_exit = acp.clone();
        children.watch_child_with(
            GROK_USAGE_CHILD_ID,
            ChildHandlers {
                on_line: Box::new(move |line| on_line.push_line(&line)),
                on_exit: Box::new(move |_| on_exit.close(Some("Grok usage probe exited"))),
                on_stderr: None,
            },
        );
    }
    let result = async {
        let args = grok_spawn_args(GrokSpawnInput {
            model: "",
            effort: None,
            full_access: false,
            plan: false,
        });
        children
            .spawn_child(GROK_USAGE_CHILD_ID, &path, args, &home(), None, None)
            .await
            .map_err(|error| error.to_string())?;
        let work = async {
            let init = acp
                .request_value(
                    "initialize",
                    Some(json!({
                        "protocolVersion": 1,
                        "clientCapabilities": {
                            "fs": { "readTextFile": false, "writeTextFile": false },
                            "terminal": false,
                        },
                        "clientInfo": { "name": "monocode", "version": "0.1.0" },
                    })),
                    REQUEST_TIMEOUT_MS,
                )
                .await?;
            if let Some(method_id) = grok_auth_method_id(&init) {
                let _ = acp
                    .request_value(
                        "authenticate",
                        Some(json!({ "methodId": method_id, "_meta": { "headless": true } })),
                        REQUEST_TIMEOUT_MS,
                    )
                    .await;
            }
            acp.request_value("_x.ai/billing", Some(json!({})), REQUEST_TIMEOUT_MS)
                .await
        };
        match timeout(ms(DISCOVERY_TIMEOUT_MS), work).await {
            Some(result) => result.map_err(|error| error.to_string()),
            None => Err("Grok usage probe timed out".to_string()),
        }
    }
    .await;
    stop().await;
    result
}

impl RateLimitFetcher for NativeRateLimitFetcher {
    fn fetch(
        &self,
        provider: RateLimitProvider,
        account_id: &str,
    ) -> BoxFuture<'static, ProviderRateLimits> {
        match provider {
            RateLimitProvider::Claude => {
                let data_dir = self.data_dir.clone();
                let account_id = account_id.to_string();
                self.blocking(move || {
                    let result = monocode_integrations::rate_limits::fetch_claude_usage(
                        &data_dir,
                        Some(account_id),
                    )
                    .map(|fetch| UsageFetch {
                        status: fetch.status,
                        body: fetch.body,
                        error: fetch.error,
                    });
                    claude_from(result, now_ms())
                })
            }
            RateLimitProvider::Opencode => self.blocking(|| {
                let result =
                    monocode_integrations::rate_limits::fetch_opencode_go_usage().map(|fetch| {
                        UsageFetch {
                            status: fetch.status,
                            body: fetch.body,
                            error: fetch.error,
                        }
                    });
                opencode_from(result, now_ms())
            }),
            RateLimitProvider::Droid => self.blocking(|| {
                let result = monocode_integrations::rate_limits::fetch_droid_usage().map(|fetch| {
                    UsageFetch {
                        status: fetch.status,
                        body: fetch.body,
                        error: fetch.error,
                    }
                });
                droid_from(result, now_ms())
            }),
            RateLimitProvider::Codex => {
                let request = self.request_codex_account(
                    "account/rateLimits/read",
                    json!({}),
                    account_id.to_string(),
                );
                async move { codex_from(request.await, now_ms()) }.boxed()
            }
            RateLimitProvider::Grok => {
                let Some(children) = self.children.clone() else {
                    return async {
                        unavailable_rate_limits(
                            RateLimitProvider::Grok,
                            "Grok Build CLI not found",
                            now_ms(),
                        )
                    }
                    .boxed();
                };
                async move {
                    let path = match children.resolve_grok_binary().await {
                        Ok(binary) => binary.path,
                        Err(_) => {
                            return unavailable_rate_limits(
                                RateLimitProvider::Grok,
                                "Grok Build CLI not found",
                                now_ms(),
                            );
                        }
                    };
                    grok_from(request_grok_billing(children, path).await, now_ms())
                }
                .boxed()
            }
        }
    }

    fn fetch_pi(&self, provider: PiUsageProvider) -> BoxFuture<'static, ProviderRateLimits> {
        self.blocking(move || fetch_pi_usage(provider, now_ms()))
    }

    fn consume_codex_reset(
        &self,
        credit_id: Option<String>,
        account_id: &str,
    ) -> BoxFuture<'static, Result<CodexResetOutcome, String>> {
        let mut params = json!({ "idempotencyKey": uuid::Uuid::new_v4().to_string() });
        if let Some(credit_id) = credit_id {
            params["creditId"] = Value::String(credit_id);
        }
        let request = self.request_codex_account(
            "account/rateLimitResetCredit/consume",
            params,
            account_id.to_string(),
        );
        async move { reset_outcome_from(&request.await?) }.boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::rate_limits::RateLimitStatus;

    const NOW: i64 = 1_790_000_000_000;

    fn fetch(status: &str, body: Option<&str>, error: Option<&str>) -> Result<UsageFetch, String> {
        Ok(UsageFetch {
            status: status.into(),
            body: body.map(str::to_string),
            error: error.map(str::to_string),
        })
    }

    #[test]
    fn claude_reads_the_command_result() {
        let ok = claude_from(
            fetch("ok", Some(r#"{"five_hour":{"utilization":10}}"#), None),
            NOW,
        );
        assert_eq!(ok.status, RateLimitStatus::Ok);
        assert_eq!(ok.session.unwrap().used_percent, 10.0);
        let signed_out = claude_from(fetch("unavailable", None, Some("  ")), NOW);
        assert_eq!(signed_out.status, RateLimitStatus::Unavailable);
        assert_eq!(signed_out.error.as_deref(), Some("Claude not signed in"));
        let failed = claude_from(Err("boom".into()), NOW);
        assert_eq!(failed.error.as_deref(), Some("Claude usage unavailable"));
    }

    #[test]
    fn opencode_reports_an_empty_200_as_an_error_so_the_footer_retries() {
        let empty = opencode_from(fetch("ok", Some("{}"), None), NOW);
        assert_eq!(empty.status, RateLimitStatus::Error);
        assert_eq!(
            empty.error.as_deref(),
            Some("OpenCode Go usage response was unexpected")
        );
        let garbage = opencode_from(fetch("ok", Some("nope"), None), NOW);
        assert_eq!(
            garbage.error.as_deref(),
            Some("OpenCode Go response was not JSON")
        );
        let offline = opencode_from(fetch("unavailable", None, None), NOW);
        assert_eq!(offline.error.as_deref(), Some("OpenCode Go not connected"));
    }

    #[test]
    fn droid_reports_an_empty_payload_as_unavailable() {
        let empty = droid_from(fetch("ok", Some("{}"), None), NOW);
        assert_eq!(empty.status, RateLimitStatus::Unavailable);
        assert_eq!(empty.error.as_deref(), Some("No Droid usage data"));
        let failed = droid_from(fetch("error", None, Some("HTTP 500")), NOW);
        assert_eq!(failed.error.as_deref(), Some("HTTP 500"));
    }

    #[test]
    fn codex_maps_probe_failures_to_sign_in_and_missing_cli() {
        assert_eq!(
            codex_from(Err("ChatGPT authentication required".into()), NOW)
                .error
                .as_deref(),
            Some("Codex not signed in")
        );
        assert_eq!(
            codex_from(Err("spawn failed: ENOENT".into()), NOW)
                .error
                .as_deref(),
            Some("Codex CLI not found")
        );
        let other = codex_from(Err("Codex usage probe timed out".into()), NOW);
        assert_eq!(other.status, RateLimitStatus::Error);
        let empty = codex_from(Ok(json!({ "rateLimits": {} })), NOW);
        assert_eq!(empty.error.as_deref(), Some("No Codex usage data"));
        let credits_only = codex_from(
            Ok(json!({ "rateLimitResetCredits": { "availableCount": 1 } })),
            NOW,
        );
        assert_eq!(credits_only.status, RateLimitStatus::Ok);
    }

    #[test]
    fn grok_maps_probe_failures_and_empty_billing() {
        assert_eq!(
            grok_from(Err("Not signed in".into()), NOW).error.as_deref(),
            Some("Grok not signed in")
        );
        assert_eq!(
            grok_from(Err("could not run grok".into()), NOW)
                .error
                .as_deref(),
            Some("Grok Build CLI not found")
        );
        assert_eq!(
            grok_from(Ok(json!({})), NOW).error.as_deref(),
            Some("No Grok usage data")
        );
    }

    /// A `codex app-server` stand-in: answers every request, holding the
    /// usage read for 5 ms so overlapping probes would show.
    struct FakeCodex {
        router: Arc<monocode_harness::core::child::ChildRouter>,
        state: parking_lot::Mutex<(i32, i32, Vec<String>)>,
    }

    use monocode_harness::core::child::{
        ChildBackend, ChildFuture, ExecRequest, HttpRequest, HttpResponse, ResolvedHarnessBinary,
        SpawnRequest,
    };

    fn unsupported<T: Send + 'static>() -> ChildFuture<T> {
        async { Err("unsupported".to_string()) }.boxed()
    }

    impl ChildBackend for FakeCodex {
        fn spawn(&self, request: SpawnRequest) -> ChildFuture<u32> {
            let mut state = self.state.lock();
            state.0 += 1;
            state.1 = state.1.max(state.0);
            state.2.push(
                request
                    .account
                    .map(|account| account.id)
                    .unwrap_or_default(),
            );
            async { Ok(1) }.boxed()
        }
        fn write(&self, session_id: String, line: String) -> ChildFuture<()> {
            let router = self.router.clone();
            async move {
                let message: Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
                let (Some(id), Some(method)) = (message.get("id").cloned(), message.get("method").and_then(Value::as_str))
                else {
                    return Ok(());
                };
                let result = if method == "account/rateLimits/read" {
                    smol::Timer::after(std::time::Duration::from_millis(5)).await;
                    json!({ "rateLimits": { "primary": { "usedPercent": 10, "windowDurationMins": 300 } } })
                } else {
                    json!({})
                };
                router.on_stdout(&session_id, json!({ "id": id, "result": result }).to_string());
                Ok(())
            }
            .boxed()
        }
        fn kill(&self, _session_id: String) -> ChildFuture<()> {
            let mut state = self.state.lock();
            state.0 = (state.0 - 1).max(0);
            async { Ok(()) }.boxed()
        }
        fn kill_all(&self) -> ChildFuture<()> {
            async { Ok(()) }.boxed()
        }
        fn runtime_binary_path(&self, _provider: HarnessId) -> Option<String> {
            None
        }
        fn resolve_default(&self, _provider: HarnessId) -> ChildFuture<ResolvedHarnessBinary> {
            async {
                Ok(ResolvedHarnessBinary {
                    path: "/bin/codex".into(),
                    args: None,
                })
            }
            .boxed()
        }
        fn resolve_configured(
            &self,
            _provider: HarnessId,
            _binary_path: String,
        ) -> ChildFuture<ResolvedHarnessBinary> {
            unsupported()
        }
        fn exec(&self, _request: ExecRequest) -> ChildFuture<String> {
            unsupported()
        }
        fn free_port(&self) -> ChildFuture<u16> {
            unsupported()
        }
        fn http(&self, _request: HttpRequest) -> ChildFuture<HttpResponse> {
            unsupported()
        }
        fn sse_open(
            &self,
            _session_id: String,
            _url: String,
            _headers: Option<std::collections::HashMap<String, String>>,
        ) -> ChildFuture<()> {
            unsupported()
        }
        fn sse_close(&self, _session_id: String) -> ChildFuture<()> {
            unsupported()
        }
        fn read_text_file(&self, _path: String) -> ChildFuture<String> {
            unsupported()
        }
        fn update_cli(
            &self,
            _command: String,
            _provider: HarnessId,
            _binary_path: Option<String>,
        ) -> ChildFuture<()> {
            unsupported()
        }
        fn home_dir(&self) -> ChildFuture<String> {
            async { Ok("/home/test".to_string()) }.boxed()
        }
    }

    #[test]
    fn runs_usage_probes_for_different_accounts_one_at_a_time() {
        let router = Arc::new(monocode_harness::core::child::ChildRouter::new());
        let backend = Arc::new(FakeCodex {
            router: router.clone(),
            state: parking_lot::Mutex::new((0, 0, Vec::new())),
        });
        let children = Children::new(
            backend.clone(),
            router,
            Arc::new(monocode_harness::core::task::SmolSpawner),
        );
        let probe = CodexProbe::new(children);
        let results = smol::block_on(futures::future::join_all(
            ["default", "account-work", "account-personal"].map(|account| {
                let probe = probe.clone();
                async move {
                    codex_from(
                        probe
                            .request("account/rateLimits/read", json!({}), account.into())
                            .await,
                        NOW,
                    )
                }
            }),
        ));
        let state = backend.state.lock();
        assert_eq!(state.1, 1);
        assert_eq!(state.2, ["default", "account-work", "account-personal"]);
        let used: Vec<f64> = results
            .iter()
            .map(|result| result.session.unwrap().used_percent)
            .collect();
        assert_eq!(used, [10.0, 10.0, 10.0]);
    }

    #[test]
    fn reads_the_reset_outcome() {
        assert_eq!(
            reset_outcome_from(&json!({ "outcome": "noCredit" })),
            Ok(CodexResetOutcome::NoCredit)
        );
        assert!(reset_outcome_from(&json!({})).is_err());
    }
}
