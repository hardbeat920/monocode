//! Port of src/integrations/harness/providers/opencode/opencodeCatalog.ts:
//! the model catalog from `opencode models --verbose` and `opencode agent
//! list`.

use std::sync::{Arc, LazyLock};

use anyhow::{Result, bail};
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::Shared;
use monocode_core::harness::HarnessId;
use monocode_core::models::{
    AgentModel, ModelProvider, ModelSetting, ModelSettingChoice, ModelSettingKind,
};
use parking_lot::Mutex;
use regex::Regex;
use serde_json::Value;

use super::protocol::{
    MINIMUM_OPENCODE_VERSION, compare_semver, infer_default_agent, infer_default_variant,
    is_known_hidden_agent, locale_compare, open_code_variant_label, parse_open_code_version,
    sort_open_code_variants, title_case_slug,
};
use crate::core::catalog::SharedCatalog;
use crate::core::child::{BinaryPathChoice, Children};
use crate::core::task::{BoxFuture, SharedSpawner};

static SLUG_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\S+/\S+)\s*$").unwrap());
// JavaScript `.` stops at line terminators.
static AGENT_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([^\n\r\u{2028}\u{2029}]+)\s+\((\S+)\)\s*$").unwrap());

const PROVIDER_NAMES: [(&str, &str); 5] = [
    ("opencode", "OpenCode"),
    ("opencode-go", "OpenCode Go"),
    ("openai", "OpenAI"),
    ("xai", "xAI"),
    ("github-copilot", "GitHub Copilot"),
];

/// `ParsedProvider`. `models` keeps each model's JSON as parsed, in the
/// order `Object.entries` would list it.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedProvider {
    pub id: String,
    pub name: String,
    pub models: Vec<(String, Value)>,
}

/// What `parseModelsCliOutput` returns.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParsedModels {
    /// Insertion ordered, like the TypeScript `Map`.
    pub providers: Vec<ParsedProvider>,
    pub connected: Vec<String>,
}

/// `OpenCodeAgent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeAgent {
    pub name: String,
    pub mode: String,
    pub hidden: bool,
}

impl OpenCodeAgent {
    pub fn new(name: &str, mode: &str, hidden: bool) -> Self {
        Self {
            name: name.into(),
            mode: mode.into(),
            hidden,
        }
    }
}

/// `discoverOpenCodeModels`: run the CLI and read its catalog.
pub async fn discover_open_code_models(
    children: &Children,
    working_directory: Option<&str>,
) -> Result<Vec<AgentModel>> {
    let binary = children.resolve_open_code_binary().await?;
    let cwd = match working_directory {
        Some(cwd) => cwd.to_string(),
        None => children.home_dir().await?,
    };
    let exec = |args: &[&str]| {
        children.exec_child(
            &binary.path,
            args.iter().map(|arg| arg.to_string()).collect(),
            Some(&cwd),
            Some(HarnessId::Opencode),
            BinaryPathChoice::Runtime,
        )
    };
    let version_out = exec(&["--version"]).await?;
    let Some(version) = parse_open_code_version(&version_out) else {
        bail!(
            "Unable to determine OpenCode version. MonoCode requires v{MINIMUM_OPENCODE_VERSION} or newer."
        );
    };
    if compare_semver(&version, MINIMUM_OPENCODE_VERSION) < 0 {
        bail!("OpenCode v{version} is too old. Upgrade to v{MINIMUM_OPENCODE_VERSION} or newer.");
    }
    if super::v2::protocol::version(&version_out)? == super::v2::protocol::MajorVersion::Two {
        return super::v2::catalog::discover(children.clone(), &cwd, &Default::default()).await;
    }
    let models_out = exec(&["models", "--verbose"]).await?;
    let parsed = parse_models_cli_output(&models_out);
    let agents = match exec(&["agent", "list"]).await {
        Ok(agents_out) => parse_agent_list_cli_output(&agents_out),
        Err(error) => {
            log::debug!("[monocode] opencode agents {error:#}");
            Vec::new()
        }
    };
    Ok(flatten_open_code_models(&parsed, &agents))
}

/// `parseModelsCliOutput`: `provider/model` lines, each followed by the
/// model's JSON.
pub fn parse_models_cli_output(stdout: &str) -> ParsedModels {
    let mut parsed = ParsedModels::default();
    let mut current_slug: Option<String> = None;
    let mut json_lines: Vec<&str> = Vec::new();

    let mut flush_model = |slug: &mut Option<String>, lines: &mut Vec<&str>| {
        let (Some(current), false) = (slug.take(), lines.is_empty()) else {
            lines.clear();
            return;
        };
        let json = lines.join("\n");
        lines.clear();
        let json = json.trim();
        if json.is_empty() {
            return;
        }
        // Skip unparseable model JSON.
        let Ok(model) = serde_json::from_str::<Value>(json) else {
            return;
        };
        let Some(separator) = current.find('/').filter(|index| *index > 0) else {
            return;
        };
        let provider_id = &current[..separator];
        let model_id = &current[separator + 1..];
        let index = match parsed.providers.iter().position(|p| p.id == provider_id) {
            Some(index) => index,
            None => {
                parsed.providers.push(ParsedProvider {
                    id: provider_id.into(),
                    name: open_code_provider_name(provider_id),
                    models: Vec::new(),
                });
                parsed.providers.len() - 1
            }
        };
        let models = &mut parsed.providers[index].models;
        match models.iter_mut().find(|(id, _)| id == model_id) {
            Some(entry) => entry.1 = model,
            None => models.push((model_id.into(), model)),
        }
    };

    for line in stdout.split('\n') {
        let slug = if line.trim_start().starts_with('{') {
            None
        } else {
            SLUG_LINE_RE
                .captures(line)
                .and_then(|captures| captures.get(1))
        };
        if let Some(slug) = slug {
            flush_model(&mut current_slug, &mut json_lines);
            current_slug = Some(slug.as_str().to_string());
        } else if current_slug.is_some() {
            json_lines.push(line);
        }
    }
    flush_model(&mut current_slug, &mut json_lines);
    parsed.connected = parsed.providers.iter().map(|p| p.id.clone()).collect();
    parsed
}

/// `parseAgentListCliOutput`: `name (mode)` headers, each followed by the
/// agent's JSON.
pub fn parse_agent_list_cli_output(stdout: &str) -> Vec<OpenCodeAgent> {
    let mut agents = Vec::new();
    for line in stdout.split('\n') {
        if let Some(captures) = AGENT_HEADER_RE.captures(line) {
            let name = &captures[1];
            agents.push(OpenCodeAgent::new(
                name,
                &captures[2],
                is_known_hidden_agent(name),
            ));
        }
    }
    agents
}

/// `Object.keys` order: integer-like keys first in ascending order, then the
/// rest in insertion order.
fn object_key_order<T>(entries: &[(String, T)]) -> Vec<&(String, T)> {
    let index_of = |key: &str| -> Option<u32> {
        let value: u32 = key.parse().ok()?;
        (value != u32::MAX && value.to_string() == key).then_some(value)
    };
    let mut integers: Vec<(u32, &(String, T))> = entries
        .iter()
        .filter_map(|entry| index_of(&entry.0).map(|index| (index, entry)))
        .collect();
    integers.sort_by_key(|(index, _)| *index);
    integers
        .into_iter()
        .map(|(_, entry)| entry)
        .chain(entries.iter().filter(|entry| index_of(&entry.0).is_none()))
        .collect()
}

/// `flattenOpenCodeModels`: one catalog entry per connected provider model,
/// sorted by name.
pub fn flatten_open_code_models(
    parsed: &ParsedModels,
    agents: &[OpenCodeAgent],
) -> Vec<AgentModel> {
    let primary_agents: Vec<&OpenCodeAgent> = agents
        .iter()
        .filter(|agent| !agent.hidden && (agent.mode == "primary" || agent.mode == "all"))
        .collect();
    let mut models: Vec<AgentModel> = Vec::new();
    for provider in &parsed.providers {
        if !parsed.connected.contains(&provider.id) {
            continue;
        }
        for (model_id, model) in object_key_order(&provider.models) {
            let name = model
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| title_case_slug(model_id));
            let id = model.get("id").and_then(Value::as_str).unwrap_or(model_id);
            let native_id = format!("{}/{id}", provider.id);
            let context_window = model
                .get("limit")
                .and_then(|limit| limit.get("context"))
                .and_then(Value::as_f64)
                .filter(|window| *window > 0.0)
                .map(|window| window as i64);
            models.push(AgentModel {
                id: format!("opencode:{native_id}"),
                harness: HarnessId::Opencode,
                name,
                native_id: Some(native_id),
                provider: Some(ModelProvider {
                    id: provider.id.clone(),
                    name: provider.name.clone(),
                }),
                settings: open_code_model_settings(&provider.id, model, &primary_agents),
                context_window,
            });
        }
    }
    models.sort_by(|left, right| locale_compare(&left.name, &right.name));
    models
}

/// `openCodeProviderName`: familiar names, and a readable fallback for
/// custom providers.
pub fn open_code_provider_name(provider_id: &str) -> String {
    PROVIDER_NAMES
        .iter()
        .find(|(id, _)| *id == provider_id)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| title_case_slug(provider_id))
}

fn select(id: &str, label: &str, value: String, options: Vec<ModelSettingChoice>) -> ModelSetting {
    ModelSetting {
        id: id.into(),
        label: label.into(),
        kind: ModelSettingKind::Select,
        value,
        options,
        description: None,
    }
}

fn open_code_model_settings(
    provider_id: &str,
    model: &Value,
    agents: &[&OpenCodeAgent],
) -> Option<Vec<ModelSetting>> {
    let mut settings = Vec::new();
    let variant_keys: Vec<(String, ())> = model
        .get("variants")
        .and_then(Value::as_object)
        .map(|variants| variants.keys().map(|key| (key.clone(), ())).collect())
        .unwrap_or_default();
    let keys: Vec<String> = object_key_order(&variant_keys)
        .into_iter()
        .map(|(key, _)| key.clone())
        .collect();
    let variant_values = sort_open_code_variants(&keys);
    if !variant_values.is_empty() {
        let default_variant = infer_default_variant(provider_id, &variant_values);
        let options: Vec<ModelSettingChoice> = variant_values
            .iter()
            .map(|value| ModelSettingChoice {
                value: value.clone(),
                label: open_code_variant_label(value),
            })
            .collect();
        let value = default_variant.unwrap_or_else(|| options[0].value.clone());
        settings.push(select("variant", "Variant", value, options));
    }
    if !agents.is_empty() {
        let default_agent = infer_default_agent(agents.iter().map(|agent| agent.name.as_str()));
        let options = agents
            .iter()
            .map(|agent| ModelSettingChoice {
                value: agent.name.clone(),
                label: title_case_slug(&agent.name),
            })
            .collect();
        let value = default_agent.unwrap_or_else(|| agents[0].name.clone());
        settings.push(select("agent", "Agent", value, options));
    }
    (!settings.is_empty()).then_some(settings)
}

type Inflight = Shared<BoxFuture<'static, ()>>;

/// `refreshOpenCodeCatalog` and its module-level `inflight` promise. One
/// refresh runs at a time; callers that arrive meanwhile share it.
pub struct CatalogRefresher {
    children: Children,
    catalog: SharedCatalog,
    spawner: SharedSpawner,
    inflight: Arc<Mutex<Option<Inflight>>>,
}

impl CatalogRefresher {
    pub fn new(children: Children, catalog: SharedCatalog, spawner: SharedSpawner) -> Self {
        Self {
            children,
            catalog,
            spawner,
            inflight: Arc::new(Mutex::new(None)),
        }
    }

    /// `refreshOpenCodeCatalog`. Failures are logged, as in TypeScript. The
    /// refresh runs on the spawner, so it finishes even if the caller stops
    /// waiting.
    pub fn refresh(&self) -> BoxFuture<'static, ()> {
        let mut inflight = self.inflight.lock();
        if let Some(running) = inflight.as_ref() {
            return running.clone().boxed();
        }
        let (done, finished) = oneshot::channel::<()>();
        let shared: Inflight = finished.map(|_| ()).boxed().shared();
        *inflight = Some(shared.clone());
        drop(inflight);

        let children = self.children.clone();
        let catalog = self.catalog.clone();
        let slot = self.inflight.clone();
        self.spawner.spawn(
            async move {
                match discover_open_code_models(&children, None).await {
                    Ok(models) => {
                        if !models.is_empty() {
                            catalog.set_harness_models(HarnessId::Opencode, models);
                        }
                    }
                    Err(error) => log::debug!("[monocode] opencode catalog {error:#}"),
                }
                *slot.lock() = None;
                let _ = done.send(());
            }
            .boxed(),
        );
        shared.boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn values(models: &[AgentModel], read: impl Fn(&AgentModel) -> Value) -> Vec<Value> {
        models.iter().map(read).collect()
    }

    // describe("OpenCode CLI inventory parsers")

    #[test]
    fn parses_models_verbose_output() {
        let stdout = [
            "opencode/glm-5",
            r#"{"id":"glm-5","name":"GLM 5","variants":{"high":{},"medium":{}}}"#,
            "anthropic/claude-sonnet-4-6",
            r#"{"id":"claude-sonnet-4-6","name":"Claude Sonnet 4.6","variants":{"high":{}}}"#,
            "",
        ]
        .join("\n");
        let parsed = parse_models_cli_output(&stdout);
        let models = flatten_open_code_models(
            &parsed,
            &[
                OpenCodeAgent::new("build", "primary", false),
                OpenCodeAgent::new("plan", "primary", false),
                OpenCodeAgent::new("title", "primary", true),
            ],
        );
        assert_eq!(
            values(&models, |m| json!(m.native_id)),
            vec![
                json!("anthropic/claude-sonnet-4-6"),
                json!("opencode/glm-5")
            ]
        );
        assert_eq!(
            values(&models, |m| serde_json::to_value(&m.provider).unwrap()),
            vec![
                json!({ "id": "anthropic", "name": "Anthropic" }),
                json!({ "id": "opencode", "name": "OpenCode" }),
            ]
        );
        assert!(
            models[1]
                .settings
                .as_ref()
                .unwrap()
                .iter()
                .any(|setting| setting.id == "variant")
        );
        let agent = models[0]
            .settings
            .as_ref()
            .unwrap()
            .iter()
            .find(|setting| setting.id == "agent")
            .unwrap();
        assert_eq!(agent.value, "build");
        assert_eq!(agent.options.len(), 2);
        assert_eq!(models[0].id, "opencode:anthropic/claude-sonnet-4-6");
    }

    #[test]
    fn parses_agent_list_headers() {
        let agents = parse_agent_list_cli_output(
            &["build (primary)", "{}", "compaction (primary)", "{}"].join("\n"),
        );
        assert_eq!(
            agents,
            vec![
                OpenCodeAgent::new("build", "primary", false),
                OpenCodeAgent::new("compaction", "primary", true),
            ]
        );
    }

    #[test]
    fn sorts_variant_options_and_labels_xhigh_as_extra_high() {
        let parsed = parse_models_cli_output(
            &[
                "some-cloud/spark-1",
                r#"{"id":"spark-1","name":"Spark 1","variants":{"high":{},"minimal":{},"xhigh":{},"low":{},"medium":{}}}"#,
                "",
            ]
            .join("\n"),
        );
        let models = flatten_open_code_models(&parsed, &[]);
        let variant = models[0]
            .settings
            .as_ref()
            .unwrap()
            .iter()
            .find(|setting| setting.id == "variant")
            .unwrap();
        assert_eq!(
            variant
                .options
                .iter()
                .map(|o| o.value.as_str())
                .collect::<Vec<_>>(),
            ["minimal", "low", "medium", "high", "xhigh"]
        );
        assert_eq!(
            variant
                .options
                .iter()
                .map(|o| o.label.as_str())
                .collect::<Vec<_>>(),
            ["Minimal", "Low", "Medium", "High", "Extra High"]
        );
        assert_eq!(variant.value, "medium");
    }

    #[test]
    fn uses_familiar_provider_names_and_readable_custom_provider_fallbacks() {
        assert_eq!(open_code_provider_name("opencode-go"), "OpenCode Go");
        assert_eq!(open_code_provider_name("openai"), "OpenAI");
        assert_eq!(open_code_provider_name("acme-cloud"), "Acme Cloud");
    }

    // describe("flattenOpenCodeModels context window")

    #[test]
    fn carries_limit_context_onto_the_catalog_entry() {
        let parsed = ParsedModels {
            providers: vec![ParsedProvider {
                id: "opencode".into(),
                name: "opencode".into(),
                models: vec![(
                    "big-pickle".into(),
                    json!({
                        "id": "big-pickle",
                        "name": "Big Pickle",
                        "limit": { "context": 200_000, "output": 32_000 },
                    }),
                )],
            }],
            connected: vec!["opencode".into()],
        };
        let models = flatten_open_code_models(&parsed, &[]);
        assert_eq!(models[0].context_window, Some(200_000));
        assert_eq!(models[0].settings, None);
    }

    #[test]
    fn skips_unparseable_json_and_falls_back_to_a_title_cased_name() {
        let parsed = parse_models_cli_output(
            &[
                "acme/broken",
                "{not json",
                "acme/fast-model",
                "{",
                r#"  "id": "fast-model""#,
                "}",
            ]
            .join("\n"),
        );
        let models = flatten_open_code_models(&parsed, &[]);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Fast Model");
        assert_eq!(models[0].native_id.as_deref(), Some("acme/fast-model"));
    }

    #[test]
    fn lists_integer_like_keys_first_like_object_entries() {
        let entries = vec![
            ("b".to_string(), ()),
            ("2".to_string(), ()),
            ("a".to_string(), ()),
            ("1".to_string(), ()),
            ("01".to_string(), ()),
        ];
        let order: Vec<&str> = object_key_order(&entries)
            .into_iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(order, ["1", "2", "b", "a", "01"]);
    }
}
