//! Port of src/integrations/harness/providers/codex/codexCatalog.ts: the
//! model list from `codex app-server`'s `model/list`.

use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use futures::FutureExt;
use futures::future::Shared;
use parking_lot::Mutex;
use serde_json::{Value, json};

use monocode_core::harness::HarnessId;
use monocode_core::models::{AgentModel, ModelSetting, ModelSettingChoice, ModelSettingKind};

use crate::core::catalog::SharedCatalog;
use crate::core::child::{ChildEvent, Children};
use crate::core::json_rpc::{JsonRpcClient, JsonRpcClientOptions, JsonRpcHandlers};
use crate::core::task::{BoxFuture, SharedSpawner, ms, timeout};

use super::json::{Record, as_record, string_field};

const PROBE_ID: &str = "monocode-codex-probe";
const DISCOVERY_TIMEOUT_MS: i64 = 15_000;
const REQUEST_TIMEOUT_MS: i64 = 12_000;

/// `refreshCodexCatalog` and `discoverCodexModels`. Clones share the
/// in-flight refresh.
#[derive(Clone)]
pub struct CodexCatalog {
    children: Children,
    spawner: SharedSpawner,
    catalog: SharedCatalog,
    inflight: Arc<Mutex<Option<Shared<BoxFuture<'static, ()>>>>>,
}

impl CodexCatalog {
    pub fn new(children: Children, spawner: SharedSpawner, catalog: SharedCatalog) -> Self {
        Self {
            children,
            spawner,
            catalog,
            inflight: Arc::new(Mutex::new(None)),
        }
    }

    /// `refreshCodexCatalog`: load the live model list into the shared
    /// catalog. Concurrent calls share one probe. Failures are logged.
    pub async fn refresh(&self) -> Result<()> {
        let refresh = {
            let mut inflight = self.inflight.lock();
            match inflight.as_ref() {
                Some(refresh) => refresh.clone(),
                None => {
                    let this = self.clone();
                    let refresh = async move {
                        match this.discover_codex_models(None).await {
                            Ok(models) if !models.is_empty() => {
                                this.catalog.set_harness_models(HarnessId::Codex, models)
                            }
                            Ok(_) => {}
                            Err(error) => log::debug!("[monocode] codex catalog {error:#}"),
                        }
                        *this.inflight.lock() = None;
                    }
                    .boxed()
                    .shared();
                    *inflight = Some(refresh.clone());
                    refresh
                }
            }
        };
        refresh.await;
        Ok(())
    }

    /// `discoverCodexModels`: start a probe app-server, check the account,
    /// and page through `model/list`.
    pub async fn discover_codex_models(
        &self,
        working_directory: Option<&str>,
    ) -> Result<Vec<AgentModel>> {
        let binary = self.children.resolve_codex_binary().await?;
        let cwd = match working_directory {
            Some(cwd) => cwd.to_string(),
            None => self.children.home_dir().await?,
        };
        let probe_id = format!("{PROBE_ID}-{}", uuid::Uuid::new_v4());
        let rpc_slot: Arc<Mutex<Option<JsonRpcClient>>> = Arc::new(Mutex::new(None));
        let handlers = {
            let slot = rpc_slot.clone();
            let spawner = self.spawner.clone();
            JsonRpcHandlers::default().on_request(move |id, _method, _params| {
                if let Some(rpc) = slot.lock().clone() {
                    spawner.spawn(Box::pin(async move {
                        let _ = rpc.respond(id, json!({})).await;
                    }));
                }
            })
        };
        let rpc = JsonRpcClient::new(
            &probe_id,
            Arc::new(self.children.clone()),
            handlers,
            JsonRpcClientOptions {
                include_jsonrpc: false,
                label: "codex-probe".into(),
                ..Default::default()
            },
        );
        *rpc_slot.lock() = Some(rpc.clone());

        let events = self.children.watch_child(&probe_id);
        {
            let rpc = rpc.clone();
            self.spawner.spawn(Box::pin(async move {
                while let Ok(event) = events.recv().await {
                    match event {
                        ChildEvent::Stdout(line) => rpc.push_line(&line),
                        ChildEvent::Stderr(_) => {}
                        ChildEvent::Exit(_) => rpc.close(Some("Codex probe exited")),
                    }
                }
            }));
        }

        let result = async {
            self.children
                .spawn_child(
                    &probe_id,
                    &binary.path,
                    vec!["app-server".into()],
                    &cwd,
                    None,
                    Some(HarnessId::Codex),
                )
                .await?;
            match timeout(ms(DISCOVERY_TIMEOUT_MS), probe(&rpc)).await {
                Some(result) => result,
                None => {
                    rpc.close(None);
                    Err(anyhow!("Codex model discovery timed out"))
                }
            }
        }
        .await;

        rpc.close(None);
        rpc_slot.lock().take();
        self.children.unwatch_child(&probe_id);
        let _ = self.children.kill_child(&probe_id).await;
        result
    }
}

/// The body of `discoverCodexModels` under its timeout.
async fn probe(rpc: &JsonRpcClient) -> Result<Vec<AgentModel>> {
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

    let account = rpc
        .request_value("account/read", Some(json!({})), REQUEST_TIMEOUT_MS)
        .await
        .ok();
    if let Some(account) = account.as_ref().and_then(Value::as_object)
        && !account.get("account").is_some_and(truthy)
        && account.get("requiresOpenaiAuth").is_some_and(truthy)
    {
        bail!("Codex CLI is not authenticated. Run `codex login` and try again.");
    }

    list_all_models(rpc).await
}

/// JavaScript truthiness.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `listAllModels`: every page of `model/list`.
async fn list_all_models(rpc: &JsonRpcClient) -> Result<Vec<AgentModel>> {
    let mut rows: Vec<Value> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let params = match &cursor {
            Some(cursor) => json!({ "cursor": cursor }),
            None => json!({}),
        };
        let response = rpc
            .request_value("model/list", Some(params), REQUEST_TIMEOUT_MS)
            .await?;
        if let Some(page) = response.get("data").and_then(Value::as_array) {
            rows.extend(page.iter().cloned());
        }
        cursor = response
            .get("nextCursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    Ok(parse_codex_model_list(&rows))
}

/// `REASONING_LABELS`.
fn reasoning_label(value: &str) -> Option<&'static str> {
    Some(match value {
        "none" => "None",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra High",
        "max" => "Max",
        "ultra" => "Ultra",
        _ => return None,
    })
}

/// `parseCodexModelList`: visible models, one per native id, with the
/// app-server's default first.
pub fn parse_codex_model_list(data: &[Value]) -> Vec<AgentModel> {
    let models = data.iter().filter_map(parse_model).collect();
    order_default_first(unique_by_native(models), data)
}

fn native_id_of(rec: Option<&Record>) -> Option<&str> {
    string_field(rec, "model")
        .or_else(|| string_field(rec, "slug"))
        .or_else(|| string_field(rec, "id"))
}

fn parse_model(raw: &Value) -> Option<AgentModel> {
    let rec = as_record(Some(raw))?;
    if rec.get("hidden") == Some(&Value::Bool(true)) {
        return None;
    }
    let native_id = native_id_of(Some(rec))?;
    let name = format_display_name(
        string_field(Some(rec), "displayName")
            .or_else(|| string_field(Some(rec), "name"))
            .unwrap_or(native_id),
    );
    let settings = parse_model_settings(rec);
    let mut model = AgentModel::new(&format!("codex:{native_id}"), HarnessId::Codex, &name)
        .with_native_id(native_id);
    if !settings.is_empty() {
        model.settings = Some(settings);
    }
    Some(model)
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

fn parse_model_settings(rec: &Record) -> Vec<ModelSetting> {
    let mut settings = Vec::new();
    let efforts: &[Value] = match rec.get("supportedReasoningEfforts") {
        Some(Value::Array(list)) => list,
        _ => &[],
    };
    let mut effort_options: Vec<ModelSettingChoice> = Vec::new();
    for entry in efforts {
        if let Value::String(entry) = entry {
            effort_options.push(ModelSettingChoice {
                value: entry.clone(),
                label: reasoning_label(entry).unwrap_or(entry).to_string(),
            });
            continue;
        }
        let row = as_record(Some(entry));
        let Some(value) = string_field(row, "reasoningEffort").or_else(|| string_field(row, "id"))
        else {
            continue;
        };
        effort_options.push(ModelSettingChoice {
            value: value.to_string(),
            label: reasoning_label(value)
                .or_else(|| string_field(row, "label"))
                .unwrap_or(value)
                .to_string(),
        });
    }
    let default_effort = string_field(Some(rec), "defaultReasoningEffort");
    if let Some(first) = effort_options.first() {
        let value = default_effort.unwrap_or(&first.value).to_string();
        settings.push(select(
            "reasoningEffort",
            "Reasoning",
            value,
            effort_options,
        ));
    }

    let tiers_raw: &[Value] = match (rec.get("serviceTiers"), rec.get("additionalSpeedTiers")) {
        (Some(Value::Array(list)), _) if !list.is_empty() => list,
        (_, Some(Value::Array(list))) => list,
        _ => &[],
    };
    let mut tier_options = vec![ModelSettingChoice {
        value: "default".into(),
        label: "Standard".into(),
    }];
    for entry in tiers_raw {
        if let Value::String(entry) = entry {
            if entry == "default" {
                continue;
            }
            tier_options.push(ModelSettingChoice {
                value: entry.clone(),
                label: if entry == "fast" {
                    "Fast".into()
                } else {
                    entry.clone()
                },
            });
            continue;
        }
        let row = as_record(Some(entry));
        let Some(id) = string_field(row, "id").filter(|id| *id != "default") else {
            continue;
        };
        tier_options.push(ModelSettingChoice {
            value: id.to_string(),
            label: string_field(row, "name").unwrap_or(id).to_string(),
        });
    }
    if tier_options.len() > 1 {
        let default_tier = string_field(Some(rec), "defaultServiceTier").unwrap_or("default");
        let value = if tier_options
            .iter()
            .any(|option| option.value == default_tier)
        {
            default_tier
        } else {
            "default"
        };
        settings.push(select(
            "serviceTier",
            "Service Tier",
            value.into(),
            tier_options,
        ));
    }

    settings
}

/// `formatDisplayName`: "gpt-5.6-luna" becomes "GPT-5.6-Luna".
fn format_display_name(name: &str) -> String {
    let name = match name.get(..3) {
        Some(prefix) if prefix.eq_ignore_ascii_case("gpt") => format!("GPT{}", &name[3..]),
        _ => name.to_string(),
    };
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '-'
            && let Some(next) = chars.peek().copied()
            && next.is_ascii_lowercase()
        {
            out.push(next.to_ascii_uppercase());
            chars.next();
        }
    }
    out
}

fn order_default_first(models: Vec<AgentModel>, rows: &[Value]) -> Vec<AgentModel> {
    if models.len() <= 1 {
        return models;
    }
    let default_row = rows
        .iter()
        .filter_map(|row| as_record(Some(row)))
        .find(|rec| rec.get("isDefault") == Some(&Value::Bool(true)));
    let Some(native_id) = native_id_of(default_row) else {
        return models;
    };
    let Some(index) = models
        .iter()
        .position(|model| model.native_id.as_deref() == Some(native_id))
    else {
        return models;
    };
    if index == 0 {
        return models;
    }
    let mut models = models;
    let chosen = models.remove(index);
    models.insert(0, chosen);
    models
}

fn unique_by_native(models: Vec<AgentModel>) -> Vec<AgentModel> {
    let mut seen: Vec<String> = Vec::new();
    models
        .into_iter()
        .filter(|model| match model.native_id.as_deref() {
            Some(native) if !native.is_empty() && !seen.iter().any(|s| s == native) => {
                seen.push(native.to_string());
                true
            }
            _ => false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_display_names_like_the_catalog() {
        assert_eq!(format_display_name("gpt-5.6-luna"), "GPT-5.6-Luna");
        assert_eq!(format_display_name("o3-mini"), "o3-Mini");
    }

    #[test]
    fn skips_hidden_and_duplicate_models() {
        let models = parse_codex_model_list(&[
            json!({ "model": "a", "hidden": true }),
            json!({ "model": "b" }),
            json!({ "slug": "b" }),
            json!({ "id": "c", "supportedReasoningEfforts": ["low", "xhigh"], "additionalSpeedTiers": ["fast"] }),
        ]);
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["codex:b", "codex:c"]
        );
        let settings = models[1].settings.as_ref().unwrap();
        assert_eq!(settings[0].value, "low");
        assert_eq!(settings[0].options[1].label, "Extra High");
        assert_eq!(settings[1].options[1].label, "Fast");
    }
}
