//! Port of src/integrations/harness/providers/claude/claudeCatalog.ts: the
//! bundled Claude model list, the `list_models` row mapping, and live
//! discovery through a probe process.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared};
use monocode_core::harness::HarnessId;
use monocode_core::js;
use monocode_core::models::{AgentModel, ModelSetting, ModelSettingChoice, ModelSettingKind};
use parking_lot::Mutex;
use regex::Regex;
use serde_json::{Value, json};

use crate::core::task::{SharedSpawner, timeout};

use super::io::SharedChildIo;
use super::protocol::{
    ClaudeSpawnOptions, MINIMUM_CLAUDE_FABLE_5_VERSION, MINIMUM_CLAUDE_OPUS_4_7_VERSION,
    MINIMUM_CLAUDE_OPUS_4_8_VERSION, MINIMUM_CLAUDE_OPUS_5_5_VERSION,
    MINIMUM_CLAUDE_OPUS_5_VERSION, MINIMUM_CLAUDE_SONNET_5_VERSION, Record, as_record,
    build_claude_spawn_args, build_control_request, compare_semver, is_claude_init_message,
    list_models_from_control_response, parse_claude_version, parse_control_response,
    parse_json_line, string_field,
};

fn choices(options: &[(&str, &str)]) -> Vec<ModelSettingChoice> {
    options
        .iter()
        .map(|(value, label)| ModelSettingChoice {
            value: (*value).into(),
            label: (*label).into(),
        })
        .collect()
}

fn setting(
    id: &str,
    label: &str,
    kind: ModelSettingKind,
    value: &str,
    options: &[(&str, &str)],
) -> ModelSetting {
    ModelSetting {
        id: id.into(),
        label: label.into(),
        kind,
        value: value.into(),
        options: choices(options),
        description: None,
    }
}

fn effort_low_to_ultrathink() -> ModelSetting {
    setting(
        "effort",
        "Reasoning",
        ModelSettingKind::Select,
        "high",
        &[
            ("low", "Low"),
            ("medium", "Medium"),
            ("high", "High"),
            ("max", "Max"),
            ("ultrathink", "Ultrathink"),
        ],
    )
}

fn effort_with_xhigh() -> ModelSetting {
    setting(
        "effort",
        "Reasoning",
        ModelSettingKind::Select,
        "high",
        &[
            ("low", "Low"),
            ("medium", "Medium"),
            ("high", "High"),
            ("xhigh", "Extra High"),
            ("max", "Max"),
            ("ultracode", "Ultracode"),
            ("ultrathink", "Ultrathink"),
        ],
    )
}

fn effort_opus_47() -> ModelSetting {
    setting(
        "effort",
        "Reasoning",
        ModelSettingKind::Select,
        "xhigh",
        &[
            ("low", "Low"),
            ("medium", "Medium"),
            ("high", "High"),
            ("xhigh", "Extra High"),
            ("max", "Max"),
            ("ultrathink", "Ultrathink"),
        ],
    )
}

fn effort_opus_45() -> ModelSetting {
    setting(
        "effort",
        "Reasoning",
        ModelSettingKind::Select,
        "high",
        &[
            ("low", "Low"),
            ("medium", "Medium"),
            ("high", "High"),
            ("max", "Max"),
        ],
    )
}

fn fast_mode() -> ModelSetting {
    setting(
        "fast",
        "Fast",
        ModelSettingKind::Toggle,
        "false",
        &[("true", "On"), ("false", "Off")],
    )
}

fn thinking() -> ModelSetting {
    setting(
        "thinking",
        "Thinking",
        ModelSettingKind::Toggle,
        "false",
        &[("true", "On"), ("false", "Off")],
    )
}

fn context_window() -> ModelSetting {
    setting(
        "context",
        "Context",
        ModelSettingKind::Select,
        "1m",
        &[("200k", "200k"), ("1m", "1M")],
    )
}

/// Models Claude Code runs with a 1M context window from the bare model id:
/// `native_1m` in its model catalog in 2.1.280 and 2.1.285. A `[1m]` suffix
/// changes nothing for them and no model id holds them to 200k, so they offer
/// no Context choice. Other models run at 200k and offer 1M only when Claude
/// Code lists their `[1m]` variant.
const NATIVE_1M_MODELS: [&str; 10] = [
    "claude-fable-5",
    "claude-fable-5-1",
    "claude-mythos-5",
    "claude-mythos-5-1",
    "claude-opus-4-7",
    "claude-opus-4-8",
    "claude-opus-5",
    "claude-opus-5-5",
    "claude-sonnet-5",
    "claude-sonnet-5-5",
];

fn catalog_model(id: &str, name: &str, native_id: &str, settings: Vec<ModelSetting>) -> AgentModel {
    let mut model = AgentModel::new(id, HarnessId::Claude, name).with_native_id(native_id);
    model.settings = Some(settings);
    model
}

/// `CLAUDE_MODEL_CATALOG`: the fallback catalog when `list_models` is
/// unavailable. It offers no Context choice: without a listed `[1m]` variant
/// there is no sign the account can use 1M.
pub fn claude_model_catalog() -> &'static [AgentModel] {
    static CATALOG: LazyLock<Vec<AgentModel>> = LazyLock::new(|| {
        vec![
            catalog_model(
                "claude:fable-5",
                "Claude Fable 5",
                "claude-fable-5",
                vec![effort_with_xhigh()],
            ),
            catalog_model(
                "claude:opus-5",
                "Claude Opus 5",
                "claude-opus-5",
                vec![effort_with_xhigh(), fast_mode()],
            ),
            catalog_model(
                "claude:opus-5-5",
                "Claude Opus 5.5",
                "claude-opus-5-5",
                vec![effort_with_xhigh(), fast_mode()],
            ),
            catalog_model(
                "claude:sonnet-5",
                "Claude Sonnet 5",
                "claude-sonnet-5",
                vec![effort_with_xhigh()],
            ),
            catalog_model(
                "claude:opus-4.8",
                "Claude Opus 4.8",
                "claude-opus-4-8",
                vec![effort_with_xhigh(), fast_mode()],
            ),
            catalog_model(
                "claude:opus-4.7",
                "Claude Opus 4.7",
                "claude-opus-4-7",
                vec![effort_opus_47(), fast_mode()],
            ),
            catalog_model(
                "claude:opus-4.6",
                "Claude Opus 4.6",
                "claude-opus-4-6",
                vec![effort_low_to_ultrathink(), fast_mode()],
            ),
            catalog_model(
                "claude:sonnet-4.6",
                "Claude Sonnet 4.6",
                "claude-sonnet-4-6",
                vec![effort_low_to_ultrathink()],
            ),
            catalog_model(
                "claude:opus-4.5",
                "Claude Opus 4.5",
                "claude-opus-4-5",
                vec![effort_opus_45(), fast_mode()],
            ),
            catalog_model(
                "claude:haiku-4.5",
                "Claude Haiku 4.5",
                "claude-haiku-4-5",
                vec![thinking()],
            ),
        ]
    });
    &CATALOG
}

pub(crate) const PROBE_ID: &str = "monocode-claude-probe";
pub(crate) const LIST_MODELS_REQUEST_ID: &str = "monocode_list_models";
pub(crate) const INIT_REQUEST_ID: &str = "monocode_init";
pub(crate) const DISCOVERY_TIMEOUT_MS: i64 = 15_000;

fn effort_label(level: &str) -> Option<&'static str> {
    Some(match level {
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra High",
        "max" => "Max",
        _ => return None,
    })
}

/// `modelsFromClaudeListModels`: map a `list_models` payload into the picker
/// catalog.
pub fn models_from_claude_list_models(raw: &Value) -> Vec<AgentModel> {
    let rows: &[Value] = match raw {
        Value::Array(rows) => rows,
        _ => as_record(raw)
            .and_then(|rec| rec.get("models"))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    };
    let mut models: Vec<AgentModel> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for item in rows {
        let Some(model) = model_from_list_row(item) else {
            continue;
        };
        let key = model.native_id.clone().unwrap_or_else(|| model.id.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        models.push(model);
    }
    models
}

static DATE_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-\d{8}$").unwrap());

fn model_from_list_row(raw: &Value) -> Option<AgentModel> {
    let rec = as_record(raw)?;
    if rec.get("disabled") == Some(&Value::Bool(true)) {
        return None;
    }
    let value = string_field(Some(rec), "value").unwrap_or("");
    if value.is_empty() || value == "default" || value.starts_with("cc-update-required") {
        return None;
    }
    let resolved = string_field(Some(rec), "resolvedModel").unwrap_or("");
    let from_value = split_claude_model_value(value);
    let from_resolved = split_claude_model_value(resolved);
    let native_id = claude_launch_id(&from_value.id, &from_resolved.id);
    if native_id.is_empty() {
        return None;
    }

    let display_name = string_field(Some(rec), "displayName").unwrap_or("");
    let description = string_field(Some(rec), "description").unwrap_or("");
    let name = picker_name(display_name, description, &native_id, &from_resolved.id);
    let base = if from_resolved.id.is_empty() {
        native_id.as_str()
    } else {
        from_resolved.id.as_str()
    };
    let native_1m = NATIVE_1M_MODELS.contains(&DATE_SUFFIX.replace(base, "").as_ref());
    let settings = settings_from_list_row(
        rec,
        !native_1m && (from_value.context_1m || from_resolved.context_1m),
    );

    let mut model = AgentModel::new(&claude_catalog_id(&native_id), HarnessId::Claude, &name)
        .with_native_id(&native_id);
    if !settings.is_empty() {
        model.settings = Some(settings);
    }
    Some(model)
}

fn settings_from_list_row(rec: &Record, context_1m: bool) -> Vec<ModelSetting> {
    let mut settings = Vec::new();
    let levels = advertised_effort_levels(rec);
    if rec.get("supportsEffort") == Some(&Value::Bool(true)) || !levels.is_empty() {
        settings.push(effort_setting(&levels));
    } else if rec.get("supportsAdaptiveThinking") == Some(&Value::Bool(true)) {
        settings.push(thinking());
    }
    if rec.get("supportsFastMode") == Some(&Value::Bool(true)) {
        settings.push(fast_mode());
    }
    if context_1m {
        settings.push(context_window());
    }
    settings
}

fn advertised_effort_levels(rec: &Record) -> Vec<String> {
    rec.get("supportedEffortLevels")
        .and_then(Value::as_array)
        .map(|levels| {
            levels
                .iter()
                .filter_map(Value::as_str)
                .filter(|level| !js::trim(level).is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn effort_setting(levels: &[String]) -> ModelSetting {
    let known: Vec<&str> = levels
        .iter()
        .map(String::as_str)
        .filter(|level| effort_label(level).is_some())
        .collect();
    let values = if known.is_empty() {
        vec!["low", "medium", "high", "max"]
    } else {
        known
    };
    let mut options: Vec<ModelSettingChoice> = values
        .into_iter()
        .map(|value| ModelSettingChoice {
            value: value.into(),
            label: effort_label(value).unwrap_or(value).into(),
        })
        .collect();
    if options.iter().any(|option| option.value == "xhigh") {
        options.push(ModelSettingChoice {
            value: "ultracode".into(),
            label: "Ultracode".into(),
        });
    }
    options.push(ModelSettingChoice {
        value: "ultrathink".into(),
        label: "Ultrathink".into(),
    });
    let default_value = if options.iter().any(|option| option.value == "high") {
        "high".to_string()
    } else {
        options
            .first()
            .map(|option| option.value.clone())
            .unwrap_or_else(|| "high".into())
    };
    ModelSetting {
        id: "effort".into(),
        label: "Reasoning".into(),
        kind: ModelSettingKind::Select,
        value: default_value,
        options,
        description: None,
    }
}

fn picker_name(
    display_name: &str,
    description: &str,
    fallback: &str,
    resolved_model: &str,
) -> String {
    let name = js::trim(display_name);
    let head = js::trim(description.split('·').next().unwrap_or(""));
    let mut picked = if !name.is_empty() {
        name
    } else if !head.is_empty() {
        head
    } else {
        fallback
    };
    if !head.is_empty()
        && !name.is_empty()
        && head.to_lowercase().starts_with(&name.to_lowercase())
        && js::len(head) > js::len(name)
    {
        picked = head;
    }
    qualify_claude_alias_name(picked, resolved_model)
}

static CATALOG_VERSION_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s+v?\d+(?:[.-]\d+)*").unwrap());

/// `qualifyClaudeAliasName`: Claude's live catalog can name a moving alias
/// only as "Opus" while also reporting its concrete target as
/// `claude-opus-5-5`. Keep the alias for CLI launches, but include the
/// resolved version in the label so model releases do not silently look like
/// the previous generation.
fn qualify_claude_alias_name(name: &str, resolved_model: &str) -> String {
    let Some(resolved) = resolved_claude_model_name(resolved_model) else {
        return name.to_string();
    };
    let pattern = format!(
        r"(?i)^(Claude\s+)?({})(.*)$",
        regex::escape(&resolved.family)
    );
    let Ok(family) = Regex::new(&pattern) else {
        return name.to_string();
    };
    let Some(found) = family.captures(name) else {
        return name.to_string();
    };
    let suffix = found.get(3).map_or("", |m| m.as_str());
    // A catalog-supplied version is more authoritative than our interpretation
    // of the concrete id. Otherwise, enrich any generic alias variation.
    if CATALOG_VERSION_SUFFIX.is_match(suffix) {
        return name.to_string();
    }
    format!(
        "{}{} {}{}",
        found.get(1).map_or("", |m| m.as_str()),
        found.get(2).map_or("", |m| m.as_str()),
        resolved.version,
        suffix
    )
}

struct ResolvedName {
    family: String,
    version: String,
}

static VERSION_PART: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+(?:\.\d+)*$").unwrap());
static DATE_PART: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{8}$").unwrap());

fn is_version_part(part: &str) -> bool {
    VERSION_PART.is_match(part) && !DATE_PART.is_match(part)
}

fn resolved_claude_model_name(model: &str) -> Option<ResolvedName> {
    let id = split_claude_model_value(model).id;
    if !id.to_lowercase().starts_with("claude-") {
        return None;
    }
    let parts: Vec<&str> = id["claude-".len()..].split('-').collect();
    let version_start = parts.iter().position(|part| is_version_part(part))?;
    if version_start == 0 {
        return None;
    }
    let mut version: Vec<&str> = Vec::new();
    for part in &parts[version_start..] {
        if !is_version_part(part) {
            break;
        }
        version.extend(part.split('.'));
    }
    if version.is_empty() {
        return None;
    }
    let family = parts[..version_start]
        .iter()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    format!("{}{}", first.to_uppercase(), chars.as_str().to_lowercase())
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    Some(ResolvedName {
        family,
        version: version.join("."),
    })
}

struct SplitValue {
    id: String,
    context_1m: bool,
}

static ONE_M_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(.*)\[1m\]$").unwrap());

fn split_claude_model_value(value: &str) -> SplitValue {
    let trimmed = js::trim(value);
    if let Some(head) = ONE_M_SUFFIX
        .captures(trimmed)
        .and_then(|captures| captures.get(1))
        .map(|m| js::trim(m.as_str()))
        .filter(|head| !head.is_empty())
    {
        return SplitValue {
            id: head.to_string(),
            context_1m: true,
        };
    }
    SplitValue {
        id: trimmed.to_string(),
        context_1m: false,
    }
}

fn claude_catalog_id(native_id: &str) -> String {
    let slug = native_id.strip_prefix("claude-").unwrap_or(native_id);
    format!("claude:{slug}")
}

/// `claudeLaunchId`: the `--model` argument for a `list_models` row.
///
/// Claude advertises family aliases (`opus`) that must stay bare, and concrete
/// ids that need the `claude-` prefix. A versioned `value` of `opus-5-5` is
/// not a valid CLI model name; prefer `resolvedModel` when it is the full id,
/// otherwise restore the prefix.
fn claude_launch_id(value_id: &str, resolved_id: &str) -> String {
    let native_id = if value_id.is_empty() {
        resolved_id
    } else {
        value_id
    };
    if native_id.is_empty() {
        return String::new();
    }
    if native_id.starts_with("claude-") || !native_id.chars().any(|c| c.is_ascii_digit()) {
        return native_id.to_string();
    }
    if resolved_id.starts_with("claude-") {
        resolved_id.to_string()
    } else {
        format!("claude-{native_id}")
    }
}

/// `modelsForClaudeVersion`: the bundled catalog, without models this CLI
/// version is too old to run. A missing version hides every gated model.
pub fn models_for_claude_version(version: Option<&str>) -> Vec<AgentModel> {
    let at_least =
        |minimum: &str| version.is_some_and(|version| compare_semver(version, minimum) >= 0);
    claude_model_catalog()
        .iter()
        .filter(|model| match model.native_id.as_deref().unwrap_or("") {
            "claude-opus-5-5" => at_least(MINIMUM_CLAUDE_OPUS_5_5_VERSION),
            "claude-opus-5" => at_least(MINIMUM_CLAUDE_OPUS_5_VERSION),
            "claude-sonnet-5" => at_least(MINIMUM_CLAUDE_SONNET_5_VERSION),
            "claude-fable-5" => at_least(MINIMUM_CLAUDE_FABLE_5_VERSION),
            "claude-opus-4-8" => at_least(MINIMUM_CLAUDE_OPUS_4_8_VERSION),
            "claude-opus-4-7" => at_least(MINIMUM_CLAUDE_OPUS_4_7_VERSION),
            _ => true,
        })
        .cloned()
        .collect()
}

/// Live discovery: a `list_models` probe, with `--version` as the fallback.
#[derive(Clone)]
pub struct ClaudeCatalog {
    inner: Arc<CatalogInner>,
}

struct CatalogInner {
    io: SharedChildIo,
    spawner: SharedSpawner,
    /// `setHarnessModels("claude", models)` on the app's catalog.
    set_models: Arc<dyn Fn(Vec<AgentModel>) + Send + Sync>,
    inflight: Mutex<Option<Shared<BoxFuture<'static, ()>>>>,
    timeout: Duration,
}

impl ClaudeCatalog {
    pub fn new(
        io: SharedChildIo,
        spawner: SharedSpawner,
        set_models: Arc<dyn Fn(Vec<AgentModel>) + Send + Sync>,
    ) -> Self {
        Self {
            inner: Arc::new(CatalogInner {
                io,
                spawner,
                set_models,
                inflight: Mutex::new(None),
                timeout: Duration::from_millis(DISCOVERY_TIMEOUT_MS as u64),
            }),
        }
    }

    /// `refreshClaudeCatalog`. Calls made while one runs share it. Failures
    /// are logged, not returned.
    pub fn refresh(&self) -> Shared<BoxFuture<'static, ()>> {
        let mut inflight = self.inner.inflight.lock();
        if let Some(running) = inflight.as_ref() {
            return running.clone();
        }
        let catalog = self.clone();
        let run = async move {
            match catalog.discover(None).await {
                Ok(models) if !models.is_empty() => (catalog.inner.set_models)(models),
                Ok(_) => {}
                Err(error) => log::debug!("[monocode] claude catalog {error:#}"),
            }
            *catalog.inner.inflight.lock() = None;
        }
        .boxed()
        .shared();
        *inflight = Some(run.clone());
        run
    }

    /// `discoverClaudeModels`.
    pub async fn discover(&self, working_directory: Option<&str>) -> Result<Vec<AgentModel>> {
        let listed = match self.discover_via_list_models(working_directory).await {
            Ok(models) => models,
            Err(error) => {
                log::debug!("[monocode] claude list_models catalog failed {error:#}");
                Vec::new()
            }
        };
        if !listed.is_empty() {
            return Ok(listed);
        }
        self.discover_via_version(working_directory).await
    }

    async fn cwd(&self, working_directory: Option<&str>) -> Result<String> {
        match working_directory {
            Some(cwd) => Ok(cwd.to_string()),
            None => self.inner.io.home_dir().await,
        }
    }

    /// `discoverViaListModels`.
    async fn discover_via_list_models(
        &self,
        working_directory: Option<&str>,
    ) -> Result<Vec<AgentModel>> {
        let io = self.inner.io.clone();
        let path = io.resolve_claude_binary().await?;
        let cwd = self.cwd(working_directory).await?;
        let session_id = uuid::Uuid::new_v4().to_string();
        let probe_id = format!("{PROBE_ID}-{session_id}");

        let (settle, pending) = oneshot::channel::<Result<Vec<AgentModel>>>();
        let settle = Arc::new(Mutex::new(Some(settle)));
        let finish = {
            let settle = settle.clone();
            move |result: Result<Vec<AgentModel>>| {
                if let Some(settle) = settle.lock().take() {
                    let _ = settle.send(result);
                }
            }
        };
        let finish = Arc::new(finish);

        let asked = Arc::new(AtomicBool::new(false));
        let ask = {
            let io = io.clone();
            let probe_id = probe_id.clone();
            let spawner = self.inner.spawner.clone();
            let finish = finish.clone();
            move || {
                if asked.swap(true, Ordering::SeqCst) {
                    return;
                }
                let line = serde_json::to_string(&build_control_request(
                    LIST_MODELS_REQUEST_ID,
                    json!({ "subtype": "list_models" }),
                ))
                .unwrap_or_default();
                let write = io.write_child(&probe_id, line);
                let finish = finish.clone();
                spawner.spawn(
                    async move {
                        if let Err(error) = write.await {
                            finish(Err(error));
                        }
                    }
                    .boxed(),
                );
            }
        };

        let on_line = {
            let finish = finish.clone();
            Arc::new(move |line: String| {
                let Some(rec) = parse_json_line(&line) else {
                    return;
                };
                if is_claude_init_message(&rec) {
                    ask();
                }
                if parse_control_response(&rec)
                    .is_some_and(|init| init.ok && init.request_id == INIT_REQUEST_ID)
                {
                    ask();
                }
                if let Some(rows) = list_models_from_control_response(&rec, LIST_MODELS_REQUEST_ID)
                {
                    finish(Ok(models_from_claude_list_models(&Value::Array(rows))));
                }
            })
        };
        let on_exit = {
            let finish = finish.clone();
            Arc::new(move |_code: Option<i64>| {
                finish(Err(anyhow!("Claude Code catalog probe exited")))
            })
        };
        io.watch_child(&probe_id, on_line, on_exit);

        let result = async {
            io.spawn_child(
                &probe_id,
                &path,
                build_claude_spawn_args(&ClaudeSpawnOptions {
                    isolated: true,
                    session_id: Some(session_id.clone()),
                    ..Default::default()
                }),
                &cwd,
                None,
            )
            .await?;
            io.write_child(
                &probe_id,
                serde_json::to_string(&build_control_request(
                    INIT_REQUEST_ID,
                    json!({ "subtype": "initialize" }),
                ))?,
            )
            .await?;
            match timeout(self.inner.timeout, pending).await {
                Some(Ok(result)) => result,
                Some(Err(_)) => Err(anyhow!("Claude Code catalog probe exited")),
                None => Err(anyhow!("Claude Code catalog probe timed out")),
            }
        }
        .await;
        io.unwatch_child(&probe_id);
        let _ = io.kill_child(&probe_id).await;
        result
    }

    /// `discoverViaVersion`.
    async fn discover_via_version(
        &self,
        working_directory: Option<&str>,
    ) -> Result<Vec<AgentModel>> {
        let path = self.inner.io.resolve_claude_binary().await?;
        let cwd = self.cwd(working_directory).await?;
        let output = self
            .inner
            .io
            .exec_child(&path, vec!["--version".into()], Some(&cwd))
            .await?;
        Ok(models_for_claude_version(
            parse_claude_version(&output).as_deref(),
        ))
    }
}
