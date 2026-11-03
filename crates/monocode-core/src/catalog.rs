//! The same live model catalogs the React model picker queries. No inference.
mod claude;
use crate::{
    process,
    provider::{self, Kind},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    sync::{mpsc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

const CATALOG_BYTES: usize = 2 * 1024 * 1024;

/// Installed providers and their project-specific catalogs on a remote host.
/// The host owns discovery, credentials, catalog caching, and model defaults.
pub struct RemoteCatalog {
    pub providers: Vec<Kind>,
    pub models: std::collections::HashMap<Kind, Vec<Model>>,
    pub errors: std::collections::HashMap<Kind, String>,
}

pub fn remote_catalog(cwd: &str) -> Result<RemoteCatalog, String> {
    let remote = crate::remote::parse_remote_path(cwd).ok_or("Invalid remote project")?;
    let connections = crate::remote::default_connections()?;
    let machine = connections.machine_for_environment(&remote.environment_id)?;
    let descriptor = connections.request(&machine.id, "environment.describe", json!({"supportedProviders":Kind::ALL.into_iter().map(|kind|kind.id()).collect::<Vec<_>>()}))?;
    let providers: Vec<Kind> = descriptor["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().and_then(Kind::parse))
        .collect();
    let projects = connections.request(&machine.id, "projects.list", json!({}))?;
    let projects = projects
        .as_array()
        .ok_or("Invalid remote projects response")?;
    let mut project = projects.iter().find(|project| {
        project["cwd"].as_str().is_some_and(|path| {
            crate::files::path_key(path) == crate::files::path_key(&remote.host_path)
        })
    });
    if project.is_none() {
        for candidate in projects {
            let listed = connections.request(
                &machine.id,
                "git.worktrees",
                json!({"projectId":candidate["id"]}),
            );
            if listed
                .ok()
                .as_ref()
                .and_then(|value| value["worktrees"].as_array())
                .is_some_and(|trees| {
                    trees.iter().any(|tree| {
                        tree["missing"] != true
                            && tree["path"].as_str().is_some_and(|path| {
                                crate::files::path_key(path)
                                    == crate::files::path_key(&remote.host_path)
                            })
                    })
                })
            {
                project = Some(candidate);
                break;
            }
        }
    }
    let project = project.ok_or("Remote project is unavailable")?;
    let response = connections.request(
        &machine.id,
        "models.list",
        json!({"projectId":project["id"]}),
    )?;
    remote_catalog_response(&response, providers)
}

pub fn remote_models(cwd: &str, provider: &str) -> Result<Vec<Model>, String> {
    let kind = Kind::parse(provider).ok_or("Unknown remote provider")?;
    let mut catalog = remote_catalog(cwd)?;
    if !catalog.providers.contains(&kind) {
        return Err(format!(
            "{} CLI is not installed on this machine",
            kind.label()
        ));
    }
    if let Some(error) = catalog.errors.remove(&kind) {
        return Err(error);
    }
    catalog
        .models
        .remove(&kind)
        .filter(|models| !models.is_empty())
        .ok_or_else(|| format!("{} returned an empty model catalog", kind.label()))
}

fn remote_catalog_response(value: &Value, providers: Vec<Kind>) -> Result<RemoteCatalog, String> {
    if !value["models"].is_object() || !value["errors"].is_object() {
        return Err("Invalid remote model catalog".into());
    }
    let mut models = std::collections::HashMap::new();
    let mut errors = std::collections::HashMap::new();
    for kind in &providers {
        if let Some(error) = value["errors"][kind.id()].as_str() {
            errors.insert(*kind, error.into());
        }
        if let Some(rows) = value["models"][kind.id()].as_array() {
            let mut seen = std::collections::HashSet::new();
            let entries = rows
                .iter()
                .filter_map(|row| {
                    let id = row["id"].as_str()?;
                    if row["hidden"] == true || !seen.insert(id) {
                        return None;
                    }
                    let mut model = model(id, row["name"].as_str().unwrap_or(id));
                    model.default = row["isDefault"] == true || rows.first() == Some(row);
                    for setting in row["settings"].as_array().into_iter().flatten() {
                        let Some(id) = setting["id"].as_str() else {
                            continue;
                        };
                        let choices: Vec<(String, String)> = setting["options"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|choice| {
                                let value = choice["value"].as_str()?;
                                Some((
                                    value.into(),
                                    choice["label"].as_str().unwrap_or(value).into(),
                                ))
                            })
                            .collect();
                        let current = setting["value"].as_str().unwrap_or("");
                        match id {
                            "effort" | "reasoningEffort" => {
                                model.efforts =
                                    choices.iter().map(|(value, _)| value.clone()).collect();
                                model.default_effort =
                                    (!current.is_empty()).then(|| current.into());
                            }
                            "serviceTier" => {
                                model.tiers = choices
                                    .into_iter()
                                    .filter(|(value, _)| value != "default")
                                    .collect()
                            }
                            "fast" => model.fast = true,
                            "thinking" if setting["kind"] == "toggle" => model.thinking = true,
                            _ if !choices.is_empty() => model.controls.push(ModelControl {
                                id: id.into(),
                                name: setting["label"].as_str().unwrap_or(id).into(),
                                current: current.into(),
                                choices,
                            }),
                            _ => {}
                        }
                    }
                    Some(model)
                })
                .collect();
            models.insert(*kind, entries);
        }
    }
    Ok(RemoteCatalog {
        providers,
        models,
        errors,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    /// Concrete id behind a moving provider alias, retained for comparisons.
    #[serde(default)]
    pub alias_target: Option<String>,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    pub fast: bool,
    pub thinking: bool,
    pub default: bool,
    pub tiers: Vec<(String, String)>,
    #[serde(default)]
    pub controls: Vec<ModelControl>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelControl {
    pub id: String,
    pub name: String,
    pub current: String,
    pub choices: Vec<(String, String)>,
}
#[derive(Default)]
struct CatalogCache {
    entries: Vec<(Kind, crate::accounts::Profile, Vec<Model>, Instant)>,
    generation: std::collections::HashMap<(Kind, crate::accounts::Profile), u64>,
}
fn cache() -> &'static Mutex<CatalogCache> {
    static CACHE: OnceLock<Mutex<CatalogCache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
pub fn invalidate(kind: Kind, account: &crate::accounts::Profile) {
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    *cache.generation.entry((kind, account.clone())).or_default() += 1;
    cache
        .entries
        .retain(|(k, p, _, _)| *k != kind || p != account);
}
/// A model's catalog name from any account's cached catalog, without querying.
pub fn cached_name(kind: Kind, id: &str) -> Option<String> {
    let cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entries
        .iter()
        .filter(|(k, ..)| *k == kind)
        .find_map(|(_, _, rows, _)| rows.iter().find(|m| m.id == id).map(|m| m.name.clone()))
}
/// Account-scoped catalog for immediate display while a refresh runs.
pub fn cached_models_for(kind: Kind, account: &crate::accounts::Profile) -> Option<Vec<Model>> {
    cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entries
        .iter()
        .find(|(k, profile, _, _)| *k == kind && profile == account)
        .map(|(_, _, models, _)| models.clone())
}
pub fn models(kind: Kind) -> Result<Vec<Model>, String> {
    models_for(kind, &crate::accounts::Profile::default())
}
pub fn models_for(kind: Kind, account: &crate::accounts::Profile) -> Result<Vec<Model>, String> {
    let cache = cache();
    let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, _, rows, _)) = guard.entries.iter().find(|(k, profile, _, at)| {
        *k == kind && profile == account && at.elapsed() < Duration::from_secs(300)
    }) {
        return Ok(rows.clone());
    }
    let generation = guard
        .generation
        .get(&(kind, account.clone()))
        .copied()
        .unwrap_or(0);
    drop(guard);
    let value = if kind == Kind::Fx {
        cli_json(kind, &["models", "--json"])?
    } else if kind.is_acp() || kind.is_pi() {
        provider::catalog_for(kind, account.clone())?
    } else {
        query_provider(kind, "model/list", json!({"limit":100}), account)?
    };
    let rows = if kind == Kind::Codex {
        &value["result"]["data"]
    } else {
        &value["response"]["response"]["models"]
    };
    let mut models: Vec<_> = if kind == Kind::Fx {
        fx_models(&value)
    } else if kind == Kind::Devin {
        provider::devin::catalog_models(&value)
    } else if kind.is_acp() {
        acp_models(&value)
    } else if kind.is_pi() {
        pi_models(kind, &value)
    } else if kind == Kind::Claude {
        claude::models(rows)
    } else {
        rows.as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| {
                if row["hidden"] == true {
                    return None;
                }
                let id = if kind == Kind::Codex {
                    row["model"].as_str().or(row["id"].as_str())
                } else {
                    row["value"].as_str().or(row["id"].as_str())
                }?;
                let efforts: Vec<String> = if kind == Kind::Codex {
                    row["supportedReasoningEfforts"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|e| e["reasoningEffort"].as_str().map(str::to_owned))
                        .collect()
                } else {
                    row["supportedEffortLevels"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|e| e.as_str().map(str::to_owned))
                        .collect()
                };
                let tiers = row["serviceTiers"]
                    .as_array()
                    .filter(|v| !v.is_empty())
                    .or_else(|| row["additionalSpeedTiers"].as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|t| {
                        let id = t.as_str().or_else(|| t["id"].as_str())?;
                        if id == "default" {
                            return None;
                        }
                        Some((
                            id.to_owned(),
                            t["name"]
                                .as_str()
                                .unwrap_or(if id == "fast" { "Fast" } else { id })
                                .to_owned(),
                        ))
                    })
                    .collect();
                let thinking = row["supportsAdaptiveThinking"] == true;
                Some(Model {
                    id: id.into(),
                    name: row["displayName"].as_str().unwrap_or(id).into(),
                    alias_target: None,
                    default_effort: row["defaultReasoningEffort"]
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| efforts.iter().find(|e| *e == "high").cloned()),
                    efforts,
                    fast: row["supportsFastMode"] == true,
                    thinking,
                    default: row["isDefault"] == true,
                    tiers,
                    controls: Vec::new(),
                })
            })
            .collect()
    };
    if kind == Kind::Fx {
        if let Ok(status) = cli_json(kind, &["status", "--json"]) {
            if let Some(id) = status["model"].as_str() {
                if !models.iter().any(|m| m.id == id) {
                    models.insert(0, model(id, id));
                }
            }
        }
    }
    if models.is_empty() {
        return Err(format!("{} returned an empty model catalog", kind.label()));
    }
    let mut cache = cache.lock().unwrap();
    if cache
        .generation
        .get(&(kind, account.clone()))
        .copied()
        .unwrap_or(0)
        == generation
    {
        cache
            .entries
            .retain(|(k, profile, _, _)| *k != kind || profile != account);
        cache
            .entries
            .push((kind, account.clone(), models.clone(), Instant::now()));
    }
    Ok(models)
}

fn model(id: &str, name: &str) -> Model {
    Model {
        id: id.into(),
        name: name.into(),
        alias_target: None,
        efforts: Vec::new(),
        default_effort: None,
        fast: false,
        thinking: false,
        default: false,
        tiers: Vec::new(),
        controls: Vec::new(),
    }
}
fn leaves<'a>(values: &'a Value, out: &mut Vec<&'a Value>) {
    for value in values.as_array().into_iter().flatten() {
        if value["options"].is_array() {
            leaves(&value["options"], out);
        } else {
            out.push(value);
        }
    }
}
fn acp_models(value: &Value) -> Vec<Model> {
    let configs: Vec<_> = value["configOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    let option = configs.iter().find(|o| o["id"] == "model").or_else(|| {
        configs
            .iter()
            .find(|o| o["category"] == "model" && o["id"] != "provider")
    });
    let mut rows = Vec::new();
    if let Some(option) = option {
        leaves(&option["options"], &mut rows);
    }
    if rows.is_empty() {
        leaves(&value["models"]["availableModels"], &mut rows);
    }
    if rows.is_empty() {
        leaves(&value["models"], &mut rows);
    }
    if rows.is_empty() {
        leaves(&value["availableModels"], &mut rows);
    }
    let effort = configs.iter().find(|o| {
        o["id"] != "thinking"
            && o["type"] != "boolean"
            && (o["category"] == "thought_level"
                || matches!(
                    o["id"].as_str(),
                    Some(
                        "thought_level"
                            | "effort"
                            | "reasoning"
                            | "reasoning_effort"
                            | "thinking_level"
                    )
                ))
    });
    let mut choices = Vec::new();
    if let Some(effort) = effort {
        leaves(&effort["options"], &mut choices);
    }
    let efforts: Vec<String> = choices
        .iter()
        .filter_map(|o| o.as_str().or_else(|| o["value"].as_str()))
        .map(str::to_owned)
        .collect();
    let current = option
        .and_then(|o| o["currentValue"].as_str())
        .or_else(|| value["models"]["currentModelId"].as_str());
    let mut seen = std::collections::HashSet::new();
    rows.into_iter()
        .filter_map(|row| {
            let id = row
                .as_str()
                .or_else(|| row["value"].as_str())
                .or_else(|| row["modelId"].as_str())
                .or_else(|| row["id"].as_str())?;
            if !seen.insert(id.to_owned()) || row["hidden"] == true {
                return None;
            }
            let mut model = model(
                id,
                row["name"]
                    .as_str()
                    .or_else(|| row["label"].as_str())
                    .unwrap_or(id),
            );
            let row_config: Vec<_> = row["configOptions"]
                .as_array()
                .into_iter()
                .flatten()
                .collect();
            let row_effort = row_config.iter().find(|o| {
                o["id"] != "thinking"
                    && o["type"] != "boolean"
                    && (o["category"] == "thought_level"
                        || matches!(
                            o["id"].as_str(),
                            Some("reasoning" | "effort" | "reasoning_effort")
                        ))
            });
            let mut row_choices = Vec::new();
            if let Some(option) = row_effort {
                leaves(&option["options"], &mut row_choices);
            }
            model.efforts = if row_choices.is_empty() {
                efforts.clone()
            } else {
                row_choices
                    .iter()
                    .filter_map(|o| o.as_str().or_else(|| o["value"].as_str()))
                    .map(str::to_owned)
                    .collect()
            };
            model.default_effort = effort
                .and_then(|o| o["currentValue"].as_str())
                .map(str::to_owned);
            model.default_effort = row_effort
                .and_then(|o| o["currentValue"].as_str())
                .map(str::to_owned)
                .or(model.default_effort);
            model.fast = configs
                .iter()
                .chain(row_config.iter())
                .any(|o| o["id"] == "fast" || o["category"] == "fast");
            let options = if row_config.is_empty() {
                &configs
            } else {
                &row_config
            };
            model.controls = options
                .iter()
                .filter_map(|option| {
                    let id = option["id"].as_str()?;
                    if matches!(
                        id,
                        "model"
                            | "provider"
                            | "mode"
                            | "fast"
                            | "effort"
                            | "reasoning"
                            | "reasoning_effort"
                            | "thought_level"
                            | "thinking_level"
                    ) || matches!(option["category"].as_str(), Some("model" | "mode"))
                        || (option["category"] == "thought_level" && id != "thinking")
                    {
                        return None;
                    }
                    let mut choices = Vec::new();
                    leaves(&option["options"], &mut choices);
                    let choices: Vec<_> = if option["type"] == "boolean" {
                        vec![("false".into(), "Off".into()), ("true".into(), "On".into())]
                    } else {
                        choices
                            .into_iter()
                            .filter_map(|choice| {
                                let value = choice.as_str().or_else(|| choice["value"].as_str())?;
                                Some((
                                    value.into(),
                                    choice["name"]
                                        .as_str()
                                        .or_else(|| choice["label"].as_str())
                                        .unwrap_or(value)
                                        .into(),
                                ))
                            })
                            .collect()
                    };
                    if choices.is_empty() {
                        return None;
                    }
                    Some(ModelControl {
                        id: id.into(),
                        name: option["name"]
                            .as_str()
                            .or_else(|| option["label"].as_str())
                            .unwrap_or(id)
                            .into(),
                        current: option["currentValue"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| option["currentValue"].to_string()),
                        choices,
                    })
                })
                .collect();
            model.default = current == Some(id);
            Some(model)
        })
        .collect()
}
fn fx_models(value: &Value) -> Vec<Model> {
    value
        .as_array()
        .or_else(|| value["models"].as_array())
        .or_else(|| value["ids"].as_array())
        .or_else(|| value["data"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let id = row
                .as_str()
                .or_else(|| row["id"].as_str())
                .or_else(|| row["model"].as_str())?;
            Some(model(
                id,
                row["name"]
                    .as_str()
                    .or_else(|| row["displayName"].as_str())
                    .unwrap_or(id),
            ))
        })
        .collect()
}
/// Read-only CLI catalog probes bound output and clean up their processes,
/// with an absolute deadline even for noisy children.
fn cli_json(kind: Kind, args: &[&str]) -> Result<Value, String> {
    let binary =
        provider::resolve(kind).ok_or_else(|| format!("{} CLI not installed", kind.label()))?;
    let mut command = Command::new(binary);
    crate::scm::shell::apply_gui_env(&mut command);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    process::isolate(&mut command);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let _registration = process::register(child.id());
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take((CATALOG_BYTES + 1) as u64)
            .read_to_end(&mut bytes);
        let _ = tx.send(result.map(|_| bytes));
    });
    let result = (|| {
        let bytes = rx
            .recv_timeout(Duration::from_secs(20))
            .map_err(|_| "Model discovery timed out".to_string())?
            .map_err(|e| e.to_string())?;
        if bytes.len() > CATALOG_BYTES {
            return Err("Model catalog exceeded 2 MiB".into());
        }
        let text = String::from_utf8_lossy(&bytes);
        let start = text
            .find(['{', '['])
            .ok_or("Provider returned no JSON model catalog")?;
        serde_json::from_str(&text[start..]).map_err(|e| format!("Invalid model catalog: {e}"))
    })();
    process::terminate(&mut child);
    drop(rx);
    let _ = reader.join();
    result
}
fn pi_models(kind: Kind, value: &Value) -> Vec<Model> {
    let mut seen = std::collections::HashSet::new();
    let mut models: Vec<_> = value
        .get("models")
        .unwrap_or(value)
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let id = row["id"].as_str()?;
            let provider = row["provider"].as_str()?;
            let id = format!("{provider}/{id}");
            if !seen.insert(id.clone()) {
                return None;
            }
            let mut model = model(&id, row["name"].as_str().unwrap_or(&id));
            if row["reasoning"] == true {
                model.efforts = ["off", "minimal", "low", "medium", "high", "xhigh", "max"]
                    .map(str::to_owned)
                    .to_vec();
                model.default_effort = Some("medium".into());
            }
            model.fast = kind == Kind::Omp;
            Some(model)
        })
        .collect();
    models.sort_by(|a, b| a.name.cmp(&b.name));
    models
}

fn send(out: &mut impl Write, v: Value) -> Result<(), String> {
    writeln!(out, "{v}")
        .and_then(|_| out.flush())
        .map_err(|e| e.to_string())
}
/// Read-only account probes use the same bounded, supervised app-server transport.
pub fn codex_request(
    account: &crate::accounts::Profile,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    query_provider(Kind::Codex, method, params, account).map(|value| value["result"].clone())
}
fn query_provider(
    kind: Kind,
    method: &str,
    params: Value,
    account: &crate::accounts::Profile,
) -> Result<Value, String> {
    let binary =
        provider::resolve(kind).ok_or_else(|| format!("{} CLI not installed", kind.label()))?;
    let mut command = Command::new(binary);
    crate::scm::shell::apply_gui_env(&mut command);
    account.apply(kind, &mut command)?;
    if kind == Kind::Codex {
        command.arg("app-server");
    } else {
        command.args([
            "--print",
            "--verbose",
            "--no-session-persistence",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--strict-mcp-config",
            "--mcp-config",
            "{\"mcpServers\":{}}",
            "--settings",
            "{\"disableAllHooks\":true}",
        ]);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    process::isolate(&mut command);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let _registration = process::register(child.id());
    let _account_process = crate::accounts::track(child.id(), kind, account);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let (tx, rx) = mpsc::sync_channel(8);
    let reader = thread::spawn(move || loop {
        let mut line = Vec::new();
        match stdout
            .by_ref()
            .take((CATALOG_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)
        {
            Ok(0) | Err(_) => break,
            Ok(_) if line.len() > CATALOG_BYTES => break,
            Ok(_) => {
                if tx.send(line).is_err() {
                    break;
                }
            }
        }
    });
    let result = (|| {
        if kind == Kind::Codex {
            send(
                &mut stdin,
                json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"monocode_native_catalog","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}}),
            )?;
        } else {
            send(
                &mut stdin,
                json!({"type":"control_request","request_id":"init-models","request":{"subtype":"initialize"}}),
            )?;
        }
        let deadline =
            Instant::now() + Duration::from_secs(if method == "model/list" { 15 } else { 20 });
        let mut request_deadline = Instant::now() + Duration::from_secs(12);
        let operation = if method == "model/list" {
            "model discovery"
        } else {
            "usage request"
        };
        let mut catalog_rows = Vec::new();
        let mut cursors = std::collections::HashSet::new();
        let mut claude_asked = false;
        loop {
            let receive_deadline = if kind == Kind::Codex && method == "model/list" {
                deadline.min(request_deadline)
            } else {
                deadline
            };
            let line = rx
                .recv_timeout(receive_deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| format!("{} {operation} timed out or exited", kind.label()))?;
            let Ok(mut v) = serde_json::from_slice::<Value>(&line) else {
                continue;
            };
            if kind == Kind::Codex {
                if v.get("method").is_some() && v.get("id").is_some() {
                    if v["method"] == "currentTime/read" {
                        send(
                            &mut stdin,
                            json!({"id":v["id"],"result":{"currentTimeAt":crate::time::now_ms()/1000}}),
                        )?;
                    } else {
                        send(
                            &mut stdin,
                            json!({"id":v["id"],"error":{"code":-32601,"message":"Unsupported method"}}),
                        )?;
                    }
                    continue;
                }
                if v["id"] == 1 {
                    if v.get("error").is_some_and(|error| !error.is_null()) {
                        return Err(v["error"]["message"]
                            .as_str()
                            .unwrap_or("Codex initialization failed")
                            .to_owned());
                    }
                    send(&mut stdin, json!({"method":"initialized"}))?;
                    send(&mut stdin, json!({"id":2,"method":method,"params":params}))?;
                    request_deadline = Instant::now() + Duration::from_secs(12);
                }
                if v["id"] == 2 {
                    if v.get("error").is_some_and(|error| !error.is_null()) {
                        return Err(v["error"].to_string());
                    }
                    if method == "model/list" {
                        catalog_rows.extend(
                            v["result"]["data"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .cloned(),
                        );
                        if let Some(cursor) =
                            v["result"]["nextCursor"].as_str().filter(|s| !s.is_empty())
                        {
                            if !cursors.insert(cursor.to_owned()) {
                                return Err("Codex model catalog repeated a page cursor".into());
                            }
                            let mut next = params.clone();
                            next["cursor"] = cursor.into();
                            send(&mut stdin, json!({"id":2,"method":method,"params":next}))?;
                            request_deadline = Instant::now() + Duration::from_secs(12);
                            continue;
                        }
                        v["result"]["data"] = catalog_rows.into();
                    }
                    return Ok(v);
                }
            } else {
                let initialized = v["response"]["request_id"] == "init-models"
                    && v["response"]["subtype"] == "success"
                    || v["type"] == "system"
                        && matches!(v["subtype"].as_str(), Some("init" | "initialized"));
                if initialized && !claude_asked {
                    claude_asked = true;
                    send(
                        &mut stdin,
                        json!({"type":"control_request","request_id":"list-models","request":{"subtype":"list_models"}}),
                    )?;
                }
                if v["response"]["request_id"] == "list-models" {
                    if v["response"]["subtype"] != "success" {
                        return Err(v["response"]["error"]
                            .as_str()
                            .unwrap_or("Claude model discovery failed")
                            .into());
                    }
                    return Ok(v);
                }
            }
        }
    })();
    process::terminate(&mut child);
    drop(rx);
    let _ = reader.join();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_catalogs_are_account_scoped_and_invalidated_together() {
        let account = crate::accounts::Profile {
            id: "catalog-cache-test".into(),
            root: None,
        };
        let other = crate::accounts::Profile {
            id: "catalog-cache-other".into(),
            root: None,
        };
        cache().lock().unwrap().entries.push((
            Kind::Claude,
            account.clone(),
            vec![model("opus", "Opus test")],
            Instant::now() - Duration::from_secs(301),
        ));
        assert_eq!(
            cached_models_for(Kind::Claude, &account).unwrap()[0].id,
            "opus"
        );
        assert!(cached_models_for(Kind::Claude, &other).is_none());
        assert!(cached_models_for(Kind::Codex, &account).is_none());
        invalidate(Kind::Claude, &account);
        assert!(cached_models_for(Kind::Claude, &account).is_none());
    }

    #[test]
    fn remote_catalog_keeps_host_ids_defaults_settings_and_provider_errors() {
        let catalog = remote_catalog_response(&json!({"models":{
            "codex":[{"id":"codex:gpt-test","nativeId":"gpt-test","harness":"codex","name":"Host model","settings":[
                {"id":"reasoningEffort","kind":"select","value":"high","options":[{"value":"low","label":"Low"},{"value":"high","label":"High"}]},
                {"id":"serviceTier","kind":"select","value":"default","options":[{"value":"default","label":"Standard"},{"value":"fast","label":"Fast"}]},
                {"id":"context","label":"Context window","kind":"select","value":"1m","options":[{"value":"1m","label":"1M"}]}]},
                {"id":"codex:gpt-test","name":"Duplicate"}],
            "claude":[{"id":"claude:opus","name":"Opus","settings":[{"id":"fast","kind":"toggle"},{"id":"thinking","kind":"toggle"}]}]
        },"errors":{"devin":"Sign in on the host"}}), vec![Kind::Codex,Kind::Claude,Kind::Devin]).unwrap();
        assert_eq!(catalog.models[&Kind::Codex].len(), 1);
        let model = &catalog.models[&Kind::Codex][0];
        assert_eq!(model.id, "codex:gpt-test");
        assert!(model.default);
        assert_eq!(model.efforts, ["low", "high"]);
        assert_eq!(model.default_effort.as_deref(), Some("high"));
        assert_eq!(model.tiers, [("fast".into(), "Fast".into())]);
        assert_eq!(model.controls[0].id, "context");
        assert_eq!(model.controls[0].current, "1m");
        assert!(catalog.models[&Kind::Claude][0].fast);
        assert!(catalog.models[&Kind::Claude][0].thinking);
        assert_eq!(catalog.errors[&Kind::Devin], "Sign in on the host");
        assert!(!catalog.models.contains_key(&Kind::Cursor));
    }
    #[test]
    fn acp_catalogs_accept_grouped_options_and_legacy_models() {
        let rows = acp_models(&json!({"configOptions":[
            {"id":"model","category":"model","currentValue":"a","options":[{"group":"Anthropic","options":[{"value":"a","name":"A"},{"value":"b","name":"B"}]}]},
            {"id":"thought_level","currentValue":"high","options":[{"value":"low"},{"value":"high"}]}
        ]}));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "A");
        assert!(rows[0].default);
        assert_eq!(rows[0].efforts, vec!["low", "high"]);
        let rows = acp_models(
            &json!({"models":{"currentModelId":"x","availableModels":[{"modelId":"x","name":"X"},{"modelId":"x","name":"Duplicate"}]}}),
        );
        assert_eq!(rows.len(), 1);
        assert!(rows[0].default);
    }
    #[test]
    fn pi_catalog_keeps_the_upstream_provider_in_model_ids() {
        let rows = pi_models(
            Kind::Omp,
            &json!({"models":[{"provider":"openai","id":"same","name":"A","reasoning":true},{"provider":"anthropic","id":"same","name":"B"}]}),
        );
        assert_eq!(rows[0].id, "openai/same");
        assert_eq!(rows[1].id, "anthropic/same");
        assert!(rows[0].efforts.contains(&"xhigh".into()));
        assert!(rows[1].efforts.is_empty());
        assert!(rows.iter().all(|m| m.fast));
    }
    #[test]
    fn parameterized_cursor_catalog_keeps_thinking_and_context_controls() {
        let rows = acp_models(
            &json!({"models":[{"value":"opus","name":"Opus","configOptions":[
                {"id":"thinking","category":"thought_level","name":"Thinking","currentValue":"false","options":[{"value":"false","name":"Off"},{"value":"true","name":"On"}]},
                {"id":"context","name":"Context window","currentValue":"300k","options":[{"value":"300k","name":"300K"},{"value":"1m","name":"1M"}]}
            ]}]}),
        );
        assert!(rows[0].efforts.is_empty());
        assert_eq!(rows[0].controls.len(), 2);
        assert_eq!(rows[0].controls[0].current, "false");
        assert_eq!(rows[0].controls[1].choices[1], ("1m".into(), "1M".into()));
    }
}
