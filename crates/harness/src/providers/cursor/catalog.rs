//! Port of src/integrations/harness/providers/cursor/cursorCatalog.ts: the
//! live Cursor model list, read over ACP (`cursor/list_available_models`,
//! then `session/new`) with `--list-models` as the fallback.

use std::sync::{Arc, LazyLock};

use anyhow::{Result, anyhow};
use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use monocode_core::harness::HarnessId;
use monocode_core::js;
use monocode_core::models::{AgentModel, ModelSetting, ModelSettingChoice, ModelSettingKind};
use parking_lot::Mutex;
use regex::Regex;
use serde_json::{Value, json};

use crate::core::acp::{AcpClient, AcpHandlers};
use crate::core::catalog::SharedCatalog;
use crate::core::child::{BinaryPathChoice, ChildHandlers, Children};
use crate::core::task::{self, SharedSpawner};

use super::protocol::{client_capabilities, js_string_or_empty};

const PROBE_ID: &str = "monocode-cursor-probe";
const DISCOVERY_TIMEOUT_MS: i64 = 15_000;
const REQUEST_TIMEOUT_MS: i64 = 12_000;

static ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").unwrap());
static LIST_ROW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\S+)\s+-\s+(.+)$").unwrap());
static DEFAULT_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*\(default\)\s*$").unwrap());
static VARIANT_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s+(Low|Medium|High|Extra High|Max|None|Minimal|Fast|Thinking)(\s+Fast)?$")
        .unwrap()
});

const EFFORT_TOKENS: [&str; 8] = [
    "extra-high",
    "xhigh",
    "minimal",
    "medium",
    "none",
    "low",
    "high",
    "max",
];

/// The first present field among `keys`, as a chain of `??`.
fn first_present<'a>(rec: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|key| rec.get(*key).filter(|value| !value.is_null()))
}

/// `String(a ?? b ?? "").trim()`.
fn field_text(rec: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    js::trim(&js_string_or_empty(first_present(rec, keys))).to_string()
}

fn cursor_model(native_id: String, name: String) -> AgentModel {
    let mut model = AgentModel::new(&format!("cursor:{native_id}"), HarnessId::Cursor, &name);
    model.native_id = Some(native_id);
    model
}

/// `modelsFromListAvailable`.
pub fn models_from_list_available(result: &Value) -> Vec<AgentModel> {
    let Some(Value::Array(models)) = result.get("models") else {
        return Vec::new();
    };
    unique_cursor_models(
        models
            .iter()
            .filter_map(|item| {
                let rec = item.as_object()?;
                let native_id = field_text(rec, &["value", "modelId", "id"]);
                let name = match first_present(rec, &["name"]) {
                    Some(name) => js::trim(&js_string_or_empty(Some(name))).to_string(),
                    None => native_id.clone(),
                };
                if native_id.is_empty() || name.is_empty() {
                    return None;
                }
                let mut model = cursor_model(native_id, name);
                model.settings = parse_config_options(rec.get("configOptions"));
                Some(model)
            })
            .collect(),
    )
}

/// `modelsFromSessionNew`.
pub fn models_from_session_new(result: &Value) -> Vec<AgentModel> {
    let rec = result.as_object();
    if let Some(Value::Array(available)) = rec
        .and_then(|rec| rec.get("models"))
        .and_then(|models| models.get("availableModels"))
        && !available.is_empty()
    {
        return unique_cursor_models(
            available
                .iter()
                .filter_map(|item| {
                    let model = item.as_object()?;
                    let native_id = field_text(model, &["modelId", "value"]);
                    let name = match first_present(model, &["name"]) {
                        Some(name) => js::trim(&js_string_or_empty(Some(name))).to_string(),
                        None => native_id.clone(),
                    };
                    if native_id.is_empty() || name.is_empty() {
                        return None;
                    }
                    Some(cursor_model(native_id, name))
                })
                .collect(),
        );
    }

    let options = match rec.and_then(|rec| rec.get("configOptions")) {
        Some(Value::Array(options)) => options.as_slice(),
        _ => &[],
    };
    for option in options {
        let config = option.as_object();
        let id = config
            .map(|config| js_string_or_empty(config.get("id")))
            .unwrap_or_default()
            .to_lowercase();
        let category = config
            .map(|config| js_string_or_empty(config.get("category")))
            .unwrap_or_default()
            .to_lowercase();
        if id != "model" && category != "model" {
            continue;
        }
        return unique_cursor_models(
            flatten_select_options(config.and_then(|config| config.get("options")))
                .into_iter()
                .map(|entry| {
                    let name = if entry.label.is_empty() {
                        entry.value.clone()
                    } else {
                        entry.label
                    };
                    cursor_model(entry.value, name)
                })
                .collect(),
        );
    }
    Vec::new()
}

/// `modelsFromListModelsOutput`: `cursor-agent --list-models` rows such as
/// `gpt-5-high - GPT-5 High (default)`.
pub fn models_from_list_models_output(stdout: &str) -> Vec<AgentModel> {
    let rows = stdout
        .split('\n')
        .filter_map(|raw| {
            let raw = raw.strip_suffix('\r').unwrap_or(raw);
            let cleaned = ANSI.replace_all(raw, "");
            let line = js::trim(&cleaned);
            let captures = LIST_ROW.captures(line)?;
            let id = captures[1].to_string();
            let name = js::trim(&DEFAULT_MARK.replace(&captures[2], "")).to_string();
            (!id.is_empty() && !name.is_empty()).then_some((id, name))
        })
        .collect::<Vec<_>>();
    group_cli_models(&rows)
}

#[derive(Debug, Clone)]
struct Variant {
    id: String,
    effort: Option<String>,
    fast: bool,
    thinking: bool,
}

struct Family {
    base: String,
    name: String,
    variants: Vec<Variant>,
}

fn toggle(id: &str, label: &str, on: bool, on_label: &str) -> ModelSetting {
    ModelSetting {
        id: id.into(),
        label: label.into(),
        kind: ModelSettingKind::Toggle,
        value: if on { "true" } else { "false" }.into(),
        options: vec![
            ModelSettingChoice {
                value: "false".into(),
                label: "Off".into(),
            },
            ModelSettingChoice {
                value: "true".into(),
                label: on_label.into(),
            },
        ],
        description: None,
    }
}

/// `groupCliModels`: fold `-high`, `-fast`, and `-thinking` variants into one
/// model with settings.
fn group_cli_models(rows: &[(String, String)]) -> Vec<AgentModel> {
    let mut families: Vec<Family> = Vec::new();
    for (id, name) in rows {
        let parsed = parse_cli_variant(id);
        let index = match families
            .iter()
            .position(|family| family.base == parsed.base)
        {
            Some(index) => index,
            None => {
                families.push(Family {
                    base: parsed.base.clone(),
                    name: name.clone(),
                    variants: Vec::new(),
                });
                families.len() - 1
            }
        };
        let family = &mut families[index];
        if *id == parsed.base || family.variants.is_empty() {
            family.name = strip_variant_words(name);
        }
        family.variants.push(Variant {
            id: id.clone(),
            effort: parsed.effort,
            fast: parsed.fast,
            thinking: parsed.thinking,
        });
    }

    families
        .into_iter()
        .map(|family| {
            let mut efforts: Vec<String> = Vec::new();
            for effort in family
                .variants
                .iter()
                .filter_map(|variant| variant.effort.clone())
            {
                if !efforts.contains(&effort) {
                    efforts.push(effort);
                }
            }
            let has_fast = family.variants.iter().any(|variant| variant.fast);
            let has_thinking = family.variants.iter().any(|variant| variant.thinking);
            let canonical = family
                .variants
                .iter()
                .find(|variant| variant.id == family.base)
                .or_else(|| family.variants.first());
            let mut settings = Vec::new();
            if efforts.len() > 1 {
                settings.push(ModelSetting {
                    id: effort_key_for(&family.base).into(),
                    label: "Effort".into(),
                    kind: ModelSettingKind::Select,
                    value: canonical
                        .and_then(|variant| variant.effort.clone())
                        .unwrap_or_else(|| efforts[0].clone()),
                    options: efforts
                        .iter()
                        .map(|value| ModelSettingChoice {
                            value: value.clone(),
                            label: effort_label(value),
                        })
                        .collect(),
                    description: None,
                });
            }
            if has_thinking {
                settings.push(toggle(
                    "thinking",
                    "Thinking",
                    canonical.is_some_and(|v| v.thinking),
                    "On",
                ));
            }
            if has_fast {
                settings.push(toggle(
                    "fast",
                    "Fast",
                    canonical.is_some_and(|v| v.fast),
                    "Fast",
                ));
            }
            let mut model = cursor_model(family.base, family.name);
            model.settings = (!settings.is_empty()).then_some(settings);
            model
        })
        .collect()
}

struct ParsedVariant {
    base: String,
    effort: Option<String>,
    fast: bool,
    thinking: bool,
}

/// `parseCliVariant`.
fn parse_cli_variant(id: &str) -> ParsedVariant {
    let mut rest = id;
    let mut fast = false;
    let mut thinking = false;
    let mut effort = None;
    if let Some(stripped) = rest.strip_suffix("-fast") {
        fast = true;
        rest = stripped;
    }
    if let Some(stripped) = rest.strip_suffix("-thinking") {
        thinking = true;
        rest = stripped;
    }
    for token in EFFORT_TOKENS {
        let suffix = format!("-{token}");
        if rest.ends_with(&suffix) && rest.len() > suffix.len() {
            effort = Some(token.to_string());
            rest = &rest[..rest.len() - suffix.len()];
            break;
        }
    }
    if let Some(stripped) = rest.strip_suffix("-thinking") {
        thinking = true;
        rest = stripped;
    }
    ParsedVariant {
        base: if rest.is_empty() {
            id.to_string()
        } else {
            rest.to_string()
        },
        effort,
        fast,
        thinking,
    }
}

/// `parseConfigOptions`: model settings from a model's ACP config options.
fn parse_config_options(raw: Option<&Value>) -> Option<Vec<ModelSetting>> {
    let Some(Value::Array(items)) = raw else {
        return None;
    };
    if items.is_empty() {
        return None;
    }
    let mut settings = Vec::new();
    for item in items {
        let Some(rec) = item.as_object() else {
            continue;
        };
        let id = field_text(rec, &["id", "configId"]);
        let category = field_text(rec, &["category"]).to_lowercase();
        if id.is_empty()
            || id == "mode"
            || id == "model"
            || category == "mode"
            || category == "model"
        {
            continue;
        }
        let label = match first_present(rec, &["name", "label"]) {
            Some(value) => js::trim(&js_string_or_empty(Some(value))).to_string(),
            None => id.clone(),
        };
        let label = if label.is_empty() { id.clone() } else { label };
        let description = rec
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string);
        let kind = match first_present(rec, &["type"]) {
            Some(value) => js_string_or_empty(Some(value)),
            None => "select".into(),
        };
        if kind == "boolean" {
            let current = rec.get("currentValue");
            let on = current == Some(&Value::Bool(true))
                || current.and_then(Value::as_str) == Some("true");
            let mut setting = toggle(&id, &label, on, "On");
            setting.description = description;
            settings.push(setting);
            continue;
        }
        let options = flatten_select_options(rec.get("options"));
        if options.is_empty() {
            continue;
        }
        let current = match rec.get("currentValue").filter(|value| !value.is_null()) {
            Some(value) => js_string_or_empty(Some(value)),
            None => options[0].value.clone(),
        };
        let lower: Vec<String> = options
            .iter()
            .map(|option| option.value.to_lowercase())
            .collect();
        let kind = if lower.iter().any(|value| value == "true")
            && lower.iter().any(|value| value == "false")
            && options.len() <= 2
        {
            ModelSettingKind::Toggle
        } else {
            ModelSettingKind::Select
        };
        settings.push(ModelSetting {
            id,
            label,
            kind,
            value: current,
            options,
            description,
        });
    }
    (!settings.is_empty()).then_some(settings)
}

/// `flattenSelectOptions`: select choices, with grouped options flattened.
fn flatten_select_options(raw: Option<&Value>) -> Vec<ModelSettingChoice> {
    let Some(Value::Array(entries)) = raw else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries {
        let Some(rec) = entry.as_object() else {
            continue;
        };
        if let Some(Value::String(value)) = rec.get("value") {
            let value = js::trim(value).to_string();
            if value.is_empty() {
                continue;
            }
            let label = match first_present(rec, &["name", "label"]) {
                Some(label) => js::trim(&js_string_or_empty(Some(label))).to_string(),
                None => value.clone(),
            };
            out.push(ModelSettingChoice {
                label: if label.is_empty() {
                    value.clone()
                } else {
                    label
                },
                value,
            });
            continue;
        }
        out.extend(flatten_select_options(rec.get("options")));
    }
    out
}

/// `uniqueCursorModels`: drop models without a native id and repeats.
fn unique_cursor_models(models: Vec<AgentModel>) -> Vec<AgentModel> {
    let mut seen = std::collections::HashSet::new();
    models
        .into_iter()
        .filter(|model| match model.native_id.as_deref() {
            Some(id) if !id.is_empty() => seen.insert(id.to_string()),
            _ => false,
        })
        .collect()
}

/// `stripVariantWords`.
fn strip_variant_words(name: &str) -> String {
    js::trim(&VARIANT_WORDS.replace(name, "")).to_string()
}

/// `effortKeyFor`.
fn effort_key_for(base: &str) -> &'static str {
    if base.starts_with("gpt-") || base.starts_with("kimi-") || base.starts_with("glm-") {
        "reasoning"
    } else {
        "effort"
    }
}

/// `effortLabel`.
fn effort_label(value: &str) -> String {
    match value {
        "none" => "None",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" | "extra-high" => "Extra High",
        "max" => "Max",
        other => other,
    }
    .to_string()
}

/// `refreshCursorCatalog`'s in-flight promise: concurrent refreshes share one
/// discovery.
#[derive(Default)]
pub struct CatalogRefresh {
    inflight: Mutex<Option<Shared<BoxFuture<'static, ()>>>>,
}

impl CatalogRefresh {
    /// `refreshCursorCatalog`. Failures only log; the last live list stays.
    pub fn refresh(
        self: &Arc<Self>,
        children: Children,
        spawner: SharedSpawner,
        catalog: SharedCatalog,
    ) -> Shared<BoxFuture<'static, ()>> {
        let mut inflight = self.inflight.lock();
        if let Some(existing) = inflight.as_ref() {
            return existing.clone();
        }
        let this = Arc::downgrade(self);
        let job_spawner = spawner.clone();
        let job: BoxFuture<'static, ()> = async move {
            match discover_cursor_models(&children, &job_spawner, None).await {
                Ok(models) if !models.is_empty() => {
                    catalog.set_harness_models(HarnessId::Cursor, models)
                }
                Ok(_) => {}
                Err(error) => log::debug!("[monocode] cursor catalog {error:#}"),
            }
            if let Some(this) = this.upgrade() {
                *this.inflight.lock() = None;
            }
        }
        .boxed();
        let shared = job.shared();
        *inflight = Some(shared.clone());
        drop(inflight);
        // Run it whether or not the caller polls, as a promise would.
        let detached = shared.clone();
        spawner.spawn(Box::pin(detached));
        shared
    }
}

/// `discoverCursorModels`: ACP first, then `--list-models`.
pub async fn discover_cursor_models(
    children: &Children,
    spawner: &SharedSpawner,
    working_directory: Option<&str>,
) -> Result<Vec<AgentModel>> {
    let from_acp = match discover_via_acp(children, spawner, working_directory).await {
        Ok(models) => models,
        Err(error) => {
            log::debug!("[monocode] cursor ACP catalog failed {error:#}");
            Vec::new()
        }
    };
    if !from_acp.is_empty() {
        return Ok(from_acp);
    }
    match discover_via_cli(children, working_directory).await {
        Ok(models) => Ok(models),
        Err(error) => {
            log::debug!("[monocode] cursor CLI catalog failed {error:#}");
            Ok(Vec::new())
        }
    }
}

/// A probe's client, held so its request handler can answer, and dropped on
/// stop so the handler and the client do not keep each other alive.
type ClientCell = Arc<Mutex<Option<AcpClient>>>;

async fn discover_via_acp(
    children: &Children,
    spawner: &SharedSpawner,
    working_directory: Option<&str>,
) -> Result<Vec<AgentModel>> {
    let path = children.resolve_cursor_binary().await?.path;
    let cwd = match working_directory {
        Some(cwd) => cwd.to_string(),
        None => children.home_dir().await?,
    };
    let probe_id = format!("{PROBE_ID}-{}", uuid::Uuid::new_v4());
    let cell: ClientCell = Arc::new(Mutex::new(None));
    let handlers = AcpHandlers::default().on_request({
        let cell = cell.clone();
        let spawner = spawner.clone();
        move |id, _method, _params| {
            if let Some(acp) = cell.lock().clone() {
                spawner.spawn(Box::pin(async move {
                    let _ = acp.respond(id, json!({})).await;
                }));
            }
        }
    });
    let acp = AcpClient::new(&probe_id, Arc::new(children.clone()), handlers);
    *cell.lock() = Some(acp.clone());

    let stop = {
        let acp = acp.clone();
        let cell = cell.clone();
        let children = children.clone();
        let probe_id = probe_id.clone();
        move || {
            let acp = acp.clone();
            let cell = cell.clone();
            let children = children.clone();
            let probe_id = probe_id.clone();
            async move {
                acp.close(None);
                cell.lock().take();
                children.unwatch_child(&probe_id);
                let _ = children.kill_child(&probe_id).await;
            }
        }
    };

    children.watch_child_with(
        &probe_id,
        ChildHandlers {
            on_line: Box::new({
                let acp = acp.clone();
                move |line| acp.push_line(&line)
            }),
            on_exit: Box::new({
                let acp = acp.clone();
                move |_| acp.close(Some("Cursor probe exited"))
            }),
            on_stderr: None,
        },
    );

    let result = async {
        children
            .spawn_child(
                &probe_id,
                &path,
                vec!["acp".into()],
                &cwd,
                None,
                Some(HarnessId::Cursor),
            )
            .await?;
        let work = async {
            acp.request_value(
                "initialize",
                Some(json!({
                    "protocolVersion": 1,
                    "clientCapabilities": client_capabilities(),
                    "clientInfo": { "name": "monocode", "version": "0.1.0" },
                })),
                REQUEST_TIMEOUT_MS,
            )
            .await?;
            let _ = acp
                .request_value(
                    "authenticate",
                    Some(json!({ "methodId": "cursor_login" })),
                    REQUEST_TIMEOUT_MS,
                )
                .await;
            let listed = acp
                .request_value(
                    "cursor/list_available_models",
                    Some(json!({})),
                    REQUEST_TIMEOUT_MS,
                )
                .await?;
            let models = models_from_list_available(&listed);
            if !models.is_empty() {
                return Ok(models);
            }
            let created = acp
                .request_value(
                    "session/new",
                    Some(json!({ "cwd": cwd, "mcpServers": [] })),
                    REQUEST_TIMEOUT_MS,
                )
                .await?;
            Ok::<_, anyhow::Error>(models_from_session_new(&created))
        };
        match task::timeout(task::ms(DISCOVERY_TIMEOUT_MS), work).await {
            Some(result) => result,
            None => {
                stop().await;
                Err(anyhow!("Cursor model discovery timed out"))
            }
        }
    }
    .await;
    stop().await;
    result
}

async fn discover_via_cli(
    children: &Children,
    working_directory: Option<&str>,
) -> Result<Vec<AgentModel>> {
    let path = children.resolve_cursor_binary().await?.path;
    let cwd = match working_directory {
        Some(cwd) => cwd.to_string(),
        None => children.home_dir().await?,
    };
    let stdout = children
        .exec_child(
            &path,
            vec!["--list-models".into()],
            Some(&cwd),
            Some(HarnessId::Cursor),
            BinaryPathChoice::Runtime,
        )
        .await?;
    Ok(models_from_list_models_output(&stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_list_available_models_with_settings() {
        let models = models_from_list_available(&json!({ "models": [
            { "value": "composer-2.5", "name": "Composer 2.5", "configOptions": [
                { "id": "mode", "options": [{ "value": "agent" }] },
                { "id": "fast", "name": "Fast", "type": "boolean", "currentValue": true },
                { "id": "effort", "name": "Effort", "currentValue": "high", "options": [
                    { "value": "low", "name": "Low" }, { "group": "g", "options": [{ "value": "high", "name": "High" }] }
                ] },
                { "id": "toggle", "options": [{ "value": "true" }, { "value": "false" }] }
            ] },
            { "modelId": "composer-2.5", "name": "Duplicate" },
            { "id": "gpt-5" },
            { "name": "No id" }
        ] }));
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "cursor:composer-2.5");
        let settings = models[0].settings.as_ref().unwrap();
        assert_eq!(settings.len(), 3);
        assert_eq!(settings[0].kind, ModelSettingKind::Toggle);
        assert_eq!(settings[0].value, "true");
        assert_eq!(settings[1].options.len(), 2);
        assert_eq!(settings[1].value, "high");
        assert_eq!(settings[2].kind, ModelSettingKind::Toggle);
        assert_eq!(models[1].name, "gpt-5");
        assert_eq!(models[1].settings, None);
    }

    #[test]
    fn reads_session_new_models_then_config_options() {
        let models = models_from_session_new(&json!({ "models": { "availableModels": [
            { "modelId": "auto", "name": "Auto" }, { "value": "sonnet-4" }
        ] } }));
        assert_eq!(
            models.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            ["Auto", "sonnet-4"]
        );
        let models = models_from_session_new(&json!({ "configOptions": [
            { "id": "mode" },
            { "id": "Picker", "category": "MODEL", "options": [{ "value": "m1", "name": "M1" }, { "value": "m2" }] }
        ] }));
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["cursor:m1", "cursor:m2"]
        );
        assert!(models_from_session_new(&Value::Null).is_empty());
    }

    #[test]
    fn groups_cli_variants_into_settings() {
        let stdout = "\x1b[1mAvailable models\x1b[0m\r\n\
            gpt-5-low - GPT-5 Low\n\
            gpt-5 - GPT-5 (default)\n\
            gpt-5-high - GPT-5 High\n\
            gpt-5-high-fast - GPT-5 High Fast\n\
            sonnet-4-thinking - Sonnet 4 Thinking\n\
            sonnet-4 - Sonnet 4\n\
            not a row\n";
        let models = models_from_list_models_output(stdout);
        assert_eq!(models.len(), 2);
        let gpt = &models[0];
        assert_eq!(
            (gpt.id.as_str(), gpt.name.as_str()),
            ("cursor:gpt-5", "GPT-5")
        );
        let settings = gpt.settings.as_ref().unwrap();
        assert_eq!(settings[0].id, "reasoning");
        assert_eq!(settings[0].value, "low");
        assert_eq!(
            settings[0]
                .options
                .iter()
                .map(|o| o.label.as_str())
                .collect::<Vec<_>>(),
            ["Low", "High"]
        );
        assert_eq!(settings[1].id, "fast");
        assert_eq!(settings[1].value, "false");
        let sonnet = &models[1];
        assert_eq!(sonnet.name, "Sonnet 4");
        let settings = sonnet.settings.as_ref().unwrap();
        assert_eq!(
            (settings[0].id.as_str(), settings[0].value.as_str()),
            ("thinking", "false")
        );
    }

    #[test]
    fn parses_variant_suffixes() {
        let parsed = parse_cli_variant("claude-4-opus-high-thinking-fast");
        assert_eq!(parsed.base, "claude-4-opus");
        assert_eq!(parsed.effort.as_deref(), Some("high"));
        assert!(parsed.fast && parsed.thinking);
        assert_eq!(parse_cli_variant("high").base, "high");
        assert_eq!(parse_cli_variant("-fast").base, "-fast");
    }
}
