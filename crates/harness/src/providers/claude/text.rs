//! Port of src/integrations/harness/providers/claude/claudeText.ts: one
//! isolated Claude Code child for titles, commit messages, and side questions.
//!
//! The TypeScript kept `live` and a `turns` promise chain in module globals.
//! Here they are fields of [`ClaudeText`]; a futures mutex runs prompts one
//! at a time.

use std::sync::{Arc, LazyLock, Weak};
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures::channel::oneshot;
use monocode_core::block::{ModelSettings, TurnIntent};
use monocode_core::harness_event::HarnessEvent;
use monocode_core::js;
use monocode_core::models::AgentModel;
use parking_lot::Mutex;
use regex::Regex;
use serde::Serialize;

use crate::core::registry::{EventSink, TextPromptInput};
use crate::core::task::{AbortSignal, timeout};

use super::io::{SharedChildIo, claude_account};
use super::protocol::*;
use super::shared::{OrderedMap, is_agent_tool_name, join_stream_text};

/// `TEXT_CHILD_ID`.
pub const TEXT_CHILD_ID: &str = "monocode-claude-text";
/// `INIT_TIMEOUT_MS`.
const INIT_TIMEOUT: Duration = Duration::from_millis(8_000);
/// `REQUEST_TIMEOUT_MS`.
pub const REQUEST_TIMEOUT_MS: i64 = 45_000;
/// `TEXT_MODEL`.
pub const TEXT_MODEL: &str = "claude-haiku-4-5";

/// `TextSettings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSettings {
    key: String,
    launch_model: String,
    effort: Option<String>,
    prompt_effort: Option<String>,
    settings: ClaudeCliSettings,
    permission_mode: Option<ClaudePermissionMode>,
    max_turns: Option<i64>,
}

/// The `JSON.stringify({ effort, context, thinking, fast, readOnly })` key.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TextSettingsKey<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<&'a str>,
    thinking: bool,
    fast: bool,
    read_only: bool,
}

fn trimmed_setting<'a>(settings: Option<&'a ModelSettings>, key: &str) -> Option<&'a str> {
    settings
        .and_then(|settings| settings.get(key))
        .map(|value| js::trim(value))
        .filter(|value| !value.is_empty())
}

/// `textSettings`.
pub fn text_settings(
    model: &str,
    model_settings: Option<&ModelSettings>,
    intent: Option<TurnIntent>,
) -> TextSettings {
    let effort = trimmed_setting(model_settings, "effort");
    let context = trimmed_setting(model_settings, "context");
    let flag = |key: &str| {
        model_settings
            .and_then(|settings| settings.get(key))
            .map(String::as_str)
            == Some("true")
    };
    let thinking = flag("thinking");
    let fast = flag("fast");
    let read_only = intent == Some(TurnIntent::Plan);
    let mut settings = ClaudeCliSettings::default();
    if thinking {
        settings.always_thinking_enabled = Some(true);
    }
    if fast {
        settings.fast_mode = Some(true);
    }
    if is_claude_ultracode_effort(effort) {
        settings.ultracode = Some(true);
    }
    TextSettings {
        key: serde_json::to_string(&TextSettingsKey {
            effort,
            context,
            thinking,
            fast,
            read_only,
        })
        .unwrap_or_default(),
        launch_model: resolve_claude_api_model_id(model, context),
        effort: normalize_claude_cli_effort(effort, Some(model)),
        prompt_effort: effort.map(str::to_string),
        settings,
        permission_mode: read_only.then_some(ClaudePermissionMode::Plan),
        max_turns: read_only.then_some(1),
    }
}

static HAIKU: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)haiku").unwrap());

/// `pickTextModel`: the requested model, else the catalog's Haiku, else
/// `TEXT_MODEL`. `models` is `modelsFor("claude")`.
pub fn pick_text_model(requested: Option<&str>, models: &[AgentModel]) -> String {
    if let Some(selected) = requested
        .map(js::trim)
        .filter(|selected| !selected.is_empty())
    {
        return selected.to_string();
    }
    models
        .iter()
        .find(|model| {
            HAIKU.is_match(&format!(
                "{} {} {}",
                model.native_id.as_deref().unwrap_or(""),
                model.name,
                model.id
            ))
        })
        .and_then(|model| model.native_id.clone())
        .unwrap_or_else(|| TEXT_MODEL.into())
}

#[derive(Debug, Clone)]
struct InFlightTool {
    id: String,
    name: String,
    input: Record,
    partial_json: String,
    title: String,
}

/// `LiveText`.
struct LiveText {
    cwd: String,
    provider_account_id: Option<String>,
    model: String,
    settings_key: String,
    collecting: bool,
    output: String,
    closed: bool,
    ready: bool,
    turn: Option<oneshot::Sender<Result<(), String>>>,
    ready_done: Option<oneshot::Sender<()>>,
    on_event: Option<EventSink>,
    tools_by_index: std::collections::HashMap<i64, String>,
    tools_by_id: OrderedMap<InFlightTool>,
}

type TextCell = Arc<Mutex<LiveText>>;

struct TextInner {
    io: SharedChildIo,
    /// `modelsFor("claude")` over the app's catalog.
    models: Arc<dyn Fn() -> Vec<AgentModel> + Send + Sync>,
    live: Mutex<Option<TextCell>>,
    turns: futures::lock::Mutex<()>,
    init_timeout: Duration,
}

/// The isolated Claude Code text child.
#[derive(Clone)]
pub struct ClaudeText {
    inner: Arc<TextInner>,
}

impl ClaudeText {
    pub fn new(io: SharedChildIo, models: Arc<dyn Fn() -> Vec<AgentModel> + Send + Sync>) -> Self {
        Self::with_init_timeout(io, models, INIT_TIMEOUT)
    }

    pub fn with_init_timeout(
        io: SharedChildIo,
        models: Arc<dyn Fn() -> Vec<AgentModel> + Send + Sync>,
        init_timeout: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(TextInner {
                io,
                models,
                live: Mutex::new(None),
                turns: futures::lock::Mutex::new(()),
                init_timeout,
            }),
        }
    }

    fn pick_model(&self, requested: Option<&str>) -> String {
        pick_text_model(requested, &(self.inner.models)())
    }

    /// `stopClaudeTextPrompt`.
    pub async fn stop(&self) -> Result<()> {
        self.drop_live().await;
        Ok(())
    }

    /// `warmupClaudeText`.
    pub async fn warmup(&self, cwd: &str) -> Result<()> {
        if cwd.is_empty() || cwd == "~" {
            return Ok(());
        }
        let _turns = self.inner.turns.lock().await;
        let _ = self.ensure_live(cwd, None, None, None).await;
        Ok(())
    }

    /// `runClaudeTextPrompt`.
    pub async fn run(&self, input: TextPromptInput) -> Result<String> {
        let _turns = self.inner.turns.lock().await;
        self.prompt_on_live(input).await
    }

    /// `promptOnLive`.
    async fn prompt_on_live(&self, input: TextPromptInput) -> Result<String> {
        throw_if_aborted(input.signal.as_ref())?;
        let model = self.pick_model(input.model.as_deref());
        let settings = text_settings(&model, input.model_settings.as_ref(), input.intent);
        let session = self
            .ensure_live(
                &input.cwd,
                input.provider_account_id.as_deref(),
                Some(&model),
                Some(settings.clone()),
            )
            .await?;
        throw_if_aborted(input.signal.as_ref())?;
        let done = {
            let mut live = session.lock();
            live.output.clear();
            live.collecting = true;
            live.on_event = input.on_event.clone();
            live.tools_by_index.clear();
            live.tools_by_id.clear();
            let (resolve, done) = oneshot::channel();
            live.turn = Some(resolve);
            done
        };
        let wait = std::time::Duration::from_millis(
            input.timeout_ms.unwrap_or(REQUEST_TIMEOUT_MS).max(0) as u64,
        );

        let result = async {
            let message =
                build_claude_user_message(&input.prompt, &[], settings.prompt_effort.as_deref())
                    .map_err(|error| anyhow!(error))?;
            self.inner
                .io
                .write_child(TEXT_CHILD_ID, serde_json::to_string(&message)?)
                .await?;
            let turn = async {
                match done.await {
                    Ok(result) => result.map_err(|error| anyhow!(error)),
                    // The resolver went away without a verdict.
                    Err(_) => Err(anyhow!("Claude text generator stopped")),
                }
            };
            let aborted = abort_wait(input.signal.clone(), Arc::downgrade(&session));
            match timeout(wait, smol::future::or(turn, aborted)).await {
                Some(outcome) => outcome?,
                None => return Err(anyhow!("Claude text generation timed out")),
            }
            let output = js::trim(&session.lock().output).to_string();
            if output.is_empty() {
                return Err(anyhow!("Claude returned empty output."));
            }
            Ok(output)
        }
        .await;

        {
            let mut live = session.lock();
            live.collecting = false;
            live.turn = None;
        }
        // The TypeScript dropped the child after every prompt, success or not.
        self.drop_live().await;
        result
    }

    /// `ensureLive`.
    async fn ensure_live(
        &self,
        cwd: &str,
        provider_account_id: Option<&str>,
        requested_model: Option<&str>,
        requested_settings: Option<TextSettings>,
    ) -> Result<TextCell> {
        let model = self.pick_model(requested_model);
        let settings = requested_settings.unwrap_or_else(|| text_settings(&model, None, None));
        if let Some(live) = self.inner.live.lock().clone() {
            let state = live.lock();
            if !state.closed
                && state.cwd == cwd
                && state.provider_account_id.as_deref() == provider_account_id
                && state.model == model
                && state.settings_key == settings.key
            {
                drop(state);
                return Ok(live);
            }
        }
        self.drop_live().await;
        self.start_live(cwd, provider_account_id, model, settings)
            .await
    }

    /// `startLive`.
    async fn start_live(
        &self,
        cwd: &str,
        provider_account_id: Option<&str>,
        model: String,
        settings: TextSettings,
    ) -> Result<TextCell> {
        let path = self.inner.io.resolve_claude_binary().await?;
        let session: TextCell = Arc::new(Mutex::new(LiveText {
            cwd: cwd.to_string(),
            provider_account_id: provider_account_id.map(str::to_string),
            model,
            settings_key: settings.key.clone(),
            collecting: false,
            output: String::new(),
            closed: false,
            ready: false,
            turn: None,
            ready_done: None,
            on_event: None,
            tools_by_index: Default::default(),
            tools_by_id: OrderedMap::new(),
        }));

        let on_line = {
            let session = session.clone();
            Arc::new(move |line: String| handle_line(&session, &line))
        };
        let on_exit = {
            let session = Arc::downgrade(&session);
            let inner = Arc::downgrade(&self.inner);
            Arc::new(move |_code: Option<i64>| {
                let Some(session) = session.upgrade() else {
                    return;
                };
                if let Some(inner) = inner.upgrade() {
                    let mut current = inner.live.lock();
                    if current
                        .as_ref()
                        .is_some_and(|live| Arc::ptr_eq(live, &session))
                    {
                        *current = None;
                    }
                }
                let mut live = session.lock();
                live.closed = true;
                if let Some(turn) = live.turn.take() {
                    let _ = turn.send(Err("Claude text generator exited".into()));
                }
                if let Some(ready) = live.ready_done.take() {
                    let _ = ready.send(());
                }
            })
        };
        self.inner.io.watch_child(TEXT_CHILD_ID, on_line, on_exit);

        let started = async {
            self.inner
                .io
                .spawn_child(
                    TEXT_CHILD_ID,
                    &path,
                    build_claude_spawn_args(&ClaudeSpawnOptions {
                        isolated: true,
                        model: Some(settings.launch_model.clone()),
                        effort: settings.effort.clone(),
                        settings: Some(settings.settings.clone()),
                        permission_mode: settings.permission_mode,
                        max_turns: settings.max_turns,
                        ..Default::default()
                    }),
                    cwd,
                    Some(claude_account(provider_account_id)),
                )
                .await?;
            *self.inner.live.lock() = Some(session.clone());
            wait_for_ready(&session, self.inner.init_timeout).await
        };
        match started.await {
            Ok(()) => Ok(session),
            Err(error) => {
                session.lock().closed = true;
                self.inner.io.unwatch_child(TEXT_CHILD_ID);
                let _ = self.inner.io.kill_child(TEXT_CHILD_ID).await;
                Err(error)
            }
        }
    }

    /// `dropLive`.
    async fn drop_live(&self) {
        let current = self.inner.live.lock().take();
        if let Some(current) = current {
            let mut live = current.lock();
            live.closed = true;
            if let Some(ready) = live.ready_done.take() {
                let _ = ready.send(());
            }
            if let Some(turn) = live.turn.take() {
                let _ = turn.send(Err("Claude text generator stopped".into()));
            }
        }
        self.inner.io.unwatch_child(TEXT_CHILD_ID);
        let _ = self.inner.io.kill_child(TEXT_CHILD_ID).await;
    }
}

fn throw_if_aborted(signal: Option<&AbortSignal>) -> Result<()> {
    match signal {
        Some(signal) => signal.throw_if_aborted(),
        None => Ok(()),
    }
}

/// The abort side of the TypeScript race: fail the turn, then reject.
async fn abort_wait(signal: Option<AbortSignal>, session: Weak<Mutex<LiveText>>) -> Result<()> {
    let Some(signal) = signal else {
        return futures::future::pending().await;
    };
    signal.aborted().await;
    if let Some(session) = session.upgrade()
        && let Some(turn) = session.lock().turn.take()
    {
        let _ = turn.send(Err("By-the-way request cancelled".into()));
    }
    Err(anyhow!("By-the-way request cancelled"))
}

/// `waitForReady`: resolves when the child reports it is ready, or after the
/// timeout either way. Fails when the child closed first.
async fn wait_for_ready(session: &TextCell, wait: Duration) -> Result<()> {
    let ready = {
        let mut live = session.lock();
        if live.ready {
            return Ok(());
        }
        let (done, ready) = oneshot::channel();
        live.ready_done = Some(done);
        ready
    };
    if timeout(wait, ready).await.is_none() {
        session.lock().ready_done = None;
        return Ok(());
    }
    if session.lock().closed {
        return Err(anyhow!("Claude text generator exited"));
    }
    Ok(())
}

/// `handleLine`. The event a line produces reaches the sink after the
/// session unlocks, so the sink may call back into the runner.
fn handle_line(session: &TextCell, line: &str) {
    let Some(rec) = parse_json_line(line) else {
        return;
    };
    let delivery = {
        let mut live = session.lock();
        if is_claude_init_message(&rec) {
            live.ready = true;
            if let Some(ready) = live.ready_done.take() {
                let _ = ready.send(());
            }
        }
        if !live.collecting {
            return;
        }
        match string_field(Some(&rec), "type") {
            Some("assistant") => {
                let snapshot = assistant_text_blocks(&rec).join("");
                if !snapshot.is_empty() {
                    live.output = join_stream_text(&live.output, &snapshot);
                }
                None
            }
            Some("stream_event") => handle_stream_event(&mut live, &rec)
                .and_then(|event| Some((live.on_event.clone()?, event))),
            Some("result") => {
                let result = turn_status_from_result(&rec);
                if let Some(turn) = live.turn.take() {
                    let _ = turn.send(if result.status == ClaudeTurnStatus::Failed {
                        Err(result.error.unwrap_or_else(|| "Claude turn failed".into()))
                    } else {
                        Ok(())
                    });
                }
                None
            }
            _ => None,
        }
    };
    if let Some((sink, event)) = delivery {
        sink(event);
    }
}

fn agent_model(name: &str, input: &Record) -> Option<String> {
    if !is_agent_tool_name(name) {
        return None;
    }
    string_field(Some(input), "model").map(str::to_string)
}

/// `handleStreamEvent`, returning the event to deliver.
fn handle_stream_event(live: &mut LiveText, rec: &Record) -> Option<HarnessEvent> {
    if let Some(delta) = stream_delta_from_event(rec) {
        return Some(match delta.kind {
            ClaudeDeltaKind::Assistant => {
                live.output = join_stream_text(&live.output, &delta.text);
                HarnessEvent::MessageDelta { text: delta.text }
            }
            ClaudeDeltaKind::Reasoning => HarnessEvent::ReasoningDelta { text: delta.text },
        });
    }

    if let Some(started) = tool_start_from_event(rec) {
        let tool = InFlightTool {
            title: tool_title(&started.name, &started.input),
            id: started.id,
            name: started.name,
            input: started.input,
            partial_json: String::new(),
        };
        if started.index >= 0 {
            live.tools_by_index.insert(started.index, tool.id.clone());
        }
        live.tools_by_id.set(tool.id.clone(), tool.clone());
        return Some(HarnessEvent::ToolStarted {
            agent_model: agent_model(&tool.name, &tool.input),
            call_id: tool.id.clone(),
            title: tool.title.clone(),
            kind: Some(tool_kind_from_name(&tool.name)),
            status: Some(
                if is_agent_tool_name(&tool.name) {
                    "in_progress"
                } else {
                    "pending"
                }
                .into(),
            ),
            background: None,
            preview: preview_from_tool(&tool.name, &tool.input, None),
            paths: None,
        });
    }

    let json_delta = input_json_delta_from_event(rec)?;
    let tool_id = live.tools_by_index.get(&json_delta.index).cloned()?;
    let tool = live.tools_by_id.get_mut(&tool_id)?;
    tool.partial_json.push_str(&json_delta.partial);
    let parsed = try_parse_json_record(&tool.partial_json)?;
    tool.input = parsed.clone();
    tool.title = tool_title(&tool.name, &parsed);
    let tool = tool.clone();
    Some(HarnessEvent::ToolUpdated {
        agent_model: agent_model(&tool.name, &tool.input),
        call_id: tool.id.clone(),
        title: Some(tool.title.clone()),
        kind: Some(tool_kind_from_name(&tool.name)),
        status: Some("pending".into()),
        detail: Some(summarize_tool_request(&tool.name, &parsed)),
        preview: preview_from_tool(&tool.name, &parsed, None),
        paths: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::harness::HarnessId;

    #[test]
    fn picks_the_catalog_haiku_unless_a_model_is_requested() {
        let haiku =
            AgentModel::new("claude:haiku", HarnessId::Claude, "Haiku 4.5").with_native_id("haiku");
        let opus = AgentModel::new("claude:opus", HarnessId::Claude, "Opus").with_native_id("opus");
        assert_eq!(
            pick_text_model(Some(" opus "), std::slice::from_ref(&haiku)),
            "opus"
        );
        assert_eq!(pick_text_model(None, &[opus.clone(), haiku]), "haiku");
        assert_eq!(pick_text_model(None, &[opus]), TEXT_MODEL);
    }

    #[test]
    fn read_only_text_prompts_run_one_plan_mode_turn() {
        let mut settings = ModelSettings::new();
        settings.insert("effort".into(), " ultracode ".into());
        settings.insert("context".into(), "1m".into());
        let text = text_settings("claude-opus-5", Some(&settings), Some(TurnIntent::Plan));
        assert_eq!(text.launch_model, "claude-opus-5[1m]");
        assert_eq!(text.effort.as_deref(), Some("xhigh"));
        assert_eq!(text.prompt_effort.as_deref(), Some("ultracode"));
        assert_eq!(text.settings.ultracode, Some(true));
        assert_eq!(text.permission_mode, Some(ClaudePermissionMode::Plan));
        assert_eq!(text.max_turns, Some(1));
        assert_eq!(
            text.key,
            r#"{"effort":"ultracode","context":"1m","thinking":false,"fast":false,"readOnly":true}"#
        );
    }
}
