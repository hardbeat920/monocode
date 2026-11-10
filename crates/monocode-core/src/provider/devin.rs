//! Supervised Devin sessions. Devin's ACP server has no mode that asks before
//! workspace edits, but its permission rules can: the child runs with a copy of
//! the user's config whose `ask` list also covers every write. `--config`
//! replaces the user config rather than layering on it, so the copy keeps the
//! user's own settings.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use jsonc::strip_jsonc_comments;
pub(crate) mod jsonc;
fn dirs_home() -> Option<String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
}

pub(crate) const ASK_EDITS_RULE: &str = "Write(**)";

/// The `--config` arguments for a supervised `devin acp` child.
pub(super) fn supervised_args() -> Result<Vec<String>, String> {
    let config = ask_before_edits(read_user_config()?.as_deref())?;
    let dir = std::env::var_os("MONOCODE_NATIVE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs_home().map(|home| PathBuf::from(home).join(".cache/monocode-native")))
        .ok_or("No user data directory available for Devin's supervised config")?
        .join("devin");
    let path = write_once(&dir, &config)?;
    Ok(vec!["--config".into(), path.to_string_lossy().into_owned()])
}

pub(super) fn api_key() -> Option<String> {
    let home = dirs_home()?;
    let mut candidates = Vec::new();
    for key in ["APPDATA", "XDG_DATA_HOME", "XDG_CONFIG_HOME"] {
        if let Some(dir) = std::env::var_os(key) {
            candidates.push(PathBuf::from(dir).join("devin/credentials.toml"));
        }
    }
    for path in [
        "AppData/Roaming/devin/credentials.toml",
        ".local/share/devin/credentials.toml",
        ".config/devin/credentials.toml",
    ] {
        candidates.push(Path::new(&home).join(path));
    }
    for path in candidates {
        if let Ok(text) = std::fs::read_to_string(path) {
            for key in ["windsurf_api_key", "devin_api_key", "api_key"] {
                for line in text.lines() {
                    if let Some((name, value)) = line.split_once('=') {
                        if name.trim() == key {
                            let value = value.trim();
                            if let Some(quote) =
                                value.chars().next().filter(|ch| *ch == '\"' || *ch == '\'')
                            {
                                if let Some(end) = value[1..].find(quote) {
                                    let value = &value[1..end + 1];
                                    if !value.trim().is_empty() {
                                        return Some(value.to_owned());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

pub(crate) const EFFORT_ORDER: [&str; 7] =
    ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

#[derive(Clone)]
pub(crate) struct Fusion {
    pub lead: String,
    pub lead_name: String,
    pub effort: Option<String>,
    pub sidekick: String,
    pub sidekick_name: String,
}
#[derive(Clone)]
pub(crate) struct Variant {
    pub value: String,
    pub name: String,
    pub effort: Option<String>,
    pub fast: bool,
    pub fusion: Option<Fusion>,
}
pub(crate) struct Family {
    pub key: String,
    pub name: String,
    pub variants: Vec<Variant>,
}

fn pattern(source: &'static str) -> &'static regex::Regex {
    static PATTERNS: std::sync::OnceLock<std::collections::HashMap<&'static str, regex::Regex>> =
        std::sync::OnceLock::new();
    PATTERNS.get_or_init(|| [
        r"(?i)\s+fast$",
        r"(?i)\s+\d+(?:\.\d+)?[KM]$",
        r"(?i)\s+no thinking$",
        r"(?i)^(.*\S)\s+(none|minimal|low|medium|high|xhigh|x-high|extra high|max)(?:\s+thinking)?$",
        "[^a-z0-9]+",
        r"(?i)^fusion\s*\((.*)\)\s*$",
    ].into_iter().map(|source| (source, regex::Regex::new(source).expect("static Devin model pattern"))).collect())
        .get(source).expect("known Devin model pattern")
}
fn variant_name(name: &str) -> (String, Option<String>, bool) {
    let mut rest = name.trim().to_owned();
    let fast_suffix = pattern(r"(?i)\s+fast$");
    let fast = fast_suffix.is_match(&rest);
    rest = fast_suffix.replace(&rest, "").into_owned();
    let context_pattern = pattern(r"(?i)\s+\d+(?:\.\d+)?[KM]$");
    let context = context_pattern
        .find(&rest)
        .map(|found| found.as_str().to_owned())
        .unwrap_or_default();
    rest.truncate(rest.len() - context.len());
    let no_thinking = pattern(r"(?i)\s+no thinking$");
    if no_thinking.is_match(&rest) {
        return (
            format!("{}{context}", no_thinking.replace(&rest, "")),
            Some("none".into()),
            fast,
        );
    }
    if let Some(found) = pattern(r"(?i)^(.*\S)\s+(none|minimal|low|medium|high|xhigh|x-high|extra high|max)(?:\s+thinking)?$").captures(&rest) {
        let effort = found[2].to_lowercase();
        return (format!("{}{context}", &found[1]), Some(if matches!(effort.as_str(), "x-high" | "extra high") {"xhigh".into()} else {effort}), fast);
    }
    (format!("{rest}{context}"), None, fast)
}
fn family_key(name: &str) -> String {
    pattern("[^a-z0-9]+")
        .replace_all(&name.to_lowercase(), "-")
        .trim_matches('-')
        .into()
}
fn id_effort(id: &str) -> Option<String> {
    id.rsplit_once('-')
        .map(|(_, effort)| effort.to_lowercase())
        .filter(|effort| EFFORT_ORDER.contains(&effort.as_str()))
}
fn fusion(value: &str, name: &str) -> Option<Fusion> {
    let (lead, sidekick) = value.strip_prefix("fusion-")?.split_once("-sidekick-")?;
    if lead.is_empty() || sidekick.is_empty() {
        return None;
    }
    let effort = id_effort(lead);
    let lead = effort
        .as_ref()
        .map(|effort| &lead[..lead.len() - effort.len() - 1])
        .unwrap_or(lead)
        .to_owned();
    let mut lead_name = lead.clone();
    let mut sidekick_name = sidekick.to_owned();
    if let Some(found) = pattern(r"(?i)^fusion\s*\((.*)\)\s*$").captures(name.trim()) {
        if let Some((left, right)) = found[1].split_once(" + ") {
            lead_name = variant_name(left.trim()).0;
            sidekick_name = right.trim().into();
        }
    }
    Some(Fusion {
        lead,
        lead_name,
        effort,
        sidekick: sidekick.into(),
        sidekick_name,
    })
}
pub(crate) fn model_choices(raw: &Value) -> Vec<(String, String)> {
    let options = raw.as_array().or_else(|| raw["configOptions"].as_array());
    let option = options
        .into_iter()
        .flatten()
        .find(|option| option["id"] == "model" || option["category"] == "model");
    let mut choices = Vec::new();
    if let Some(option) = option {
        flatten_choices(&option["options"], &mut choices);
    }
    choices
}
fn flatten_choices(raw: &Value, choices: &mut Vec<(String, String)>) {
    for choice in raw.as_array().into_iter().flatten() {
        if choice["options"].is_array() {
            flatten_choices(&choice["options"], choices);
        } else if let Some(value) = choice["value"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
        {
            choices.push((
                value.into(),
                choice["name"]
                    .as_str()
                    .or_else(|| choice["label"].as_str())
                    .unwrap_or(value)
                    .into(),
            ));
        }
    }
}
pub(crate) fn model_families(raw: &Value) -> Vec<Family> {
    let mut families: Vec<Family> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (value, name) in model_choices(raw) {
        if !seen.insert(value.clone()) {
            continue;
        }
        let fusion = fusion(&value, &name);
        let (base, effort, fast) = if fusion.is_some() {
            ("Fusion".into(), None, false)
        } else {
            variant_name(&name)
        };
        let effort = if fusion.is_some() {
            None
        } else {
            effort.or_else(|| id_effort(&value))
        };
        let key = if fusion.is_some() {
            "fusion".into()
        } else {
            family_key(&base)
        };
        let key = if key.is_empty() { value.clone() } else { key };
        let index = families
            .iter()
            .position(|family| family.key == key)
            .unwrap_or_else(|| {
                families.push(Family {
                    key,
                    name: base,
                    variants: Vec::new(),
                });
                families.len() - 1
            });
        families[index].variants.push(Variant {
            value,
            name,
            effort,
            fast,
            fusion,
        });
    }
    families
        .into_iter()
        .flat_map(|family| {
            let efforts = family
                .variants
                .iter()
                .filter(|variant| variant.effort.is_some())
                .count();
            let pairs: std::collections::HashSet<_> = family
                .variants
                .iter()
                .map(|variant| (&variant.effort, variant.fast))
                .collect();
            let clean = family
                .variants
                .iter()
                .any(|variant| variant.fusion.is_some())
                || family.variants.len() < 2
                || ((efforts == 0 || efforts == family.variants.len())
                    && pairs.len() == family.variants.len());
            if clean {
                vec![family]
            } else {
                family
                    .variants
                    .into_iter()
                    .map(|mut variant| {
                        variant.effort = None;
                        variant.fast = false;
                        Family {
                            key: variant.value.clone(),
                            name: variant.name.clone(),
                            variants: vec![variant],
                        }
                    })
                    .collect()
            }
        })
        .collect()
}
fn default_variant(family: &Family) -> &Variant {
    family
        .variants
        .iter()
        .find(|variant| !variant.fast)
        .unwrap_or(&family.variants[0])
}
/// Resolve the same family/effort/speed/lead/sidekick picker selections as Tauri.
pub(crate) fn model_value(
    raw: &Value,
    native: &str,
    settings: &crate::agent::Settings,
) -> (String, bool) {
    let families = model_families(raw);
    let effort = settings
        .model_settings
        .get("effort")
        .or(settings.effort.as_ref());
    let value = if let Some(family) = families.iter().find(|family| family.key == native) {
        let first = &family.variants[0];
        if let Some(fusion) = first.fusion.as_ref() {
            let lead = settings.model_settings.get("lead").unwrap_or(&fusion.lead);
            let mut pool: Vec<_> = family
                .variants
                .iter()
                .filter(|variant| {
                    variant
                        .fusion
                        .as_ref()
                        .is_some_and(|fusion| &fusion.lead == lead)
                })
                .collect();
            if pool.is_empty() {
                pool = family.variants.iter().collect();
            }
            let matching: Vec<_> = pool
                .iter()
                .copied()
                .filter(|variant| {
                    effort.is_some()
                        && variant
                            .fusion
                            .as_ref()
                            .and_then(|fusion| fusion.effort.as_ref())
                            == effort
                })
                .collect();
            if !matching.is_empty() {
                pool = matching;
            }
            let sidekick = settings
                .model_settings
                .get("sidekick")
                .unwrap_or(&fusion.sidekick);
            pool.iter()
                .find(|variant| {
                    variant
                        .fusion
                        .as_ref()
                        .is_some_and(|fusion| &fusion.sidekick == sidekick)
                })
                .copied()
                .unwrap_or(pool[0])
                .value
                .clone()
        } else if family.variants.len() < 2 {
            first.value.clone()
        } else {
            let fallback = default_variant(family);
            let effort = effort.or(fallback.effort.as_ref());
            let fast = settings
                .model_settings
                .get("fast")
                .map(|value| value == "true")
                .unwrap_or(settings.fast);
            family
                .variants
                .iter()
                .find(|variant| variant.effort.as_ref() == effort && variant.fast == fast)
                .or_else(|| {
                    family
                        .variants
                        .iter()
                        .find(|variant| variant.effort.as_ref() == effort && !variant.fast)
                })
                .unwrap_or(fallback)
                .value
                .clone()
        }
    } else {
        native.into()
    };
    let spent = families
        .iter()
        .find(|family| {
            family.key == native || family.variants.iter().any(|variant| variant.value == value)
        })
        .is_some_and(|family| {
            family
                .variants
                .iter()
                .any(|variant| variant.fusion.is_some())
                || (family.variants.len() > 1
                    && family
                        .variants
                        .iter()
                        .any(|variant| variant.effort.is_some()))
        });
    (value, spent)
}
struct ThoughtLevels {
    choices: Vec<(String, String)>,
    current: Option<String>,
}
fn thought(raw: &Value) -> Option<ThoughtLevels> {
    let options = raw.as_array().or_else(|| raw["configOptions"].as_array())?;
    let option = options
        .iter()
        .find(|option| option["category"] == "thought_level" || option["id"] == "thought_level")?;
    let mut choices = Vec::new();
    flatten_choices(&option["options"], &mut choices);
    (!choices.is_empty()).then(|| ThoughtLevels {
        choices,
        current: option["currentValue"].as_str().map(str::to_owned),
    })
}
fn effort_label(value: &str) -> &str {
    match value {
        "none" => "No thinking",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra High",
        "max" => "Max",
        _ => value,
    }
}
pub(crate) fn catalog_models(raw: &Value) -> Vec<crate::catalog::Model> {
    use crate::catalog::{Model, ModelControl};
    let current = raw["configOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|option| option["id"] == "model" || option["category"] == "model")
        .and_then(|option| option["currentValue"].as_str())
        .unwrap_or("");
    let mut families = model_families(raw);
    if let Some(index) = families.iter().position(|family| {
        family
            .variants
            .iter()
            .any(|variant| variant.value == current)
    }) {
        let current = families.remove(index);
        families.insert(0, current);
    }
    families
        .into_iter()
        .map(|family| {
            let fallback = default_variant(&family);
            let model_thought = raw["_monocodeThoughtLevels"]
                .get(&fallback.value)
                .map(thought)
                .unwrap_or_else(|| thought(raw));
            let fusion = family.variants.len() > 1 && fallback.fusion.is_some();
            let mut model = Model {
                alias_target: None,
                id: if family.variants.len() == 1 {
                    fallback.value.clone()
                } else {
                    family.key.clone()
                },
                name: family.name.clone(),
                efforts: Vec::new(),
                default_effort: None,
                fast: false,
                thinking: false,
                default: family
                    .variants
                    .iter()
                    .any(|variant| variant.value == current),
                tiers: Vec::new(),
                controls: Vec::new(),
            };
            if fusion {
                let first = fallback.fusion.as_ref().unwrap();
                let mut leads = Vec::new();
                let mut sidekicks = Vec::new();
                let mut per_lead: std::collections::HashMap<
                    String,
                    std::collections::HashSet<String>,
                > = std::collections::HashMap::new();
                for variant in &family.variants {
                    if let Some(fusion) = &variant.fusion {
                        if !leads.iter().any(|(id, _)| id == &fusion.lead) {
                            leads.push((fusion.lead.clone(), fusion.lead_name.clone()));
                        }
                        if !sidekicks.iter().any(|(id, _)| id == &fusion.sidekick) {
                            sidekicks.push((fusion.sidekick.clone(), fusion.sidekick_name.clone()));
                        }
                        if let Some(effort) = &fusion.effort {
                            per_lead
                                .entry(fusion.lead.clone())
                                .or_default()
                                .insert(effort.clone());
                        }
                    }
                }
                model.controls.push(ModelControl {
                    id: "lead".into(),
                    name: "Lead".into(),
                    current: first.lead.clone(),
                    choices: leads,
                });
                if per_lead.values().any(|efforts| efforts.len() > 1) {
                    model.efforts = EFFORT_ORDER
                        .into_iter()
                        .filter(|effort| {
                            family.variants.iter().any(|variant| {
                                variant
                                    .fusion
                                    .as_ref()
                                    .and_then(|fusion| fusion.effort.as_deref())
                                    == Some(*effort)
                            })
                        })
                        .map(str::to_owned)
                        .collect();
                    model.default_effort = first.effort.clone();
                }
                model.controls.push(ModelControl {
                    id: "sidekick".into(),
                    name: "Sidekick".into(),
                    current: first.sidekick.clone(),
                    choices: sidekicks,
                });
            } else {
                if family.variants.len() == 1 {
                    let parsed = variant_name(&fallback.name);
                    model.name = if model_thought.is_some()
                        && parsed.1.is_some()
                        && family_key(&parsed.0) == family.key
                    {
                        format!("{}{}", parsed.0, if parsed.2 { " Fast" } else { "" })
                    } else if parsed.1.is_none()
                        && fallback
                            .effort
                            .as_deref()
                            .is_some_and(|effort| effort != "none")
                    {
                        format!(
                            "{} {}",
                            fallback.name,
                            effort_label(fallback.effort.as_deref().unwrap())
                        )
                    } else {
                        fallback.name.clone()
                    };
                } else {
                    let efforts: Vec<_> = EFFORT_ORDER
                        .into_iter()
                        .filter(|effort| {
                            family
                                .variants
                                .iter()
                                .any(|variant| variant.effort.as_deref() == Some(*effort))
                        })
                        .map(str::to_owned)
                        .collect();
                    if efforts.len() > 1 {
                        model.efforts = efforts;
                        model.default_effort = fallback.effort.clone();
                    }
                    model.fast = family.variants.iter().any(|variant| variant.fast);
                }
                if model.efforts.is_empty() && fallback.fusion.is_none() {
                    if let Some(ThoughtLevels { choices, current }) = model_thought {
                        model.efforts = choices.into_iter().map(|(value, _)| value).collect();
                        model.default_effort = current.or_else(|| model.efforts.first().cloned());
                    }
                }
            }
            model
        })
        .collect()
}

/// The user config file Devin itself reads.
fn user_config_path() -> Option<PathBuf> {
    let env = |key: &str| {
        std::env::var_os(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let dir = if cfg!(windows) {
        env("APPDATA")
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| dirs_home().map(|home| PathBuf::from(home).join(".config")))
    }?;
    Some(dir.join("devin").join("config.json"))
}

fn read_user_config() -> Result<Option<String>, String> {
    let Some(path) = user_config_path() else {
        return Ok(None);
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Could not read {}: {error}", path.display())),
    }
}

/// The user's config with an `ask` rule for every write. Anything it cannot
/// extend safely is an error: Supervised must not start without the rule.
pub(crate) fn ask_before_edits(user: Option<&str>) -> Result<String, String> {
    const HELP: &str = "Supervised mode needs it to make Devin ask before edits.";
    let mut config = match user.map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => {
            let cleaned = strip_jsonc_comments(text).ok_or_else(|| {
                format!("Devin's config.json has an unterminated comment. {HELP}")
            })?;
            serde_json::from_str::<Value>(&cleaned)
                .map_err(|e| format!("Devin's config.json is not valid JSON ({e}). {HELP}"))?
        }
        None => json!({}),
    };
    let ask = config
        .as_object_mut()
        .ok_or_else(|| format!("Devin's config.json is not a JSON object. {HELP}"))?
        .entry("permissions")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("Devin's config.json `permissions` is not an object. {HELP}"))?
        .entry("ask")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| format!("Devin's config.json `permissions.ask` is not a list. {HELP}"))?;
    if !ask.iter().any(|rule| rule.as_str() == Some(ASK_EDITS_RULE)) {
        ask.push(json!(ASK_EDITS_RULE));
    }
    serde_json::to_string_pretty(&config).map_err(|e| e.to_string())
}

/// Named by content so a running child's file is never rewritten in place.
fn write_once(dir: &Path, contents: &str) -> Result<PathBuf, String> {
    let mut hasher = DefaultHasher::new();
    contents.hash(&mut hasher);
    let path = dir.join(format!("supervised-{:016x}.json", hasher.finish()));
    if path.is_file() {
        return Ok(path);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let temp = dir.join(format!(".{}-{stamp}.tmp", std::process::id()));
    // The copy can hold whatever the user keeps in config.json: owner-only.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&temp)
        .and_then(|mut file| file.write_all(contents.as_bytes()))
        .map_err(|e| format!("{}: {e}", temp.display()))?;
    if let Err(error) = std::fs::rename(&temp, &path) {
        let _ = std::fs::remove_file(&temp);
        // Another spawn wrote the same content first.
        if !path.is_file() {
            return Err(format!("{}: {error}", path.display()));
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask_rules(config: &str) -> Vec<String> {
        let value: Value = serde_json::from_str(config).unwrap();
        value["permissions"]["ask"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| rule.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn adds_the_write_rule_and_keeps_the_users_settings() {
        let merged = ask_before_edits(Some(
            r#"{"theme_mode":"dark","permissions":{"allow":["Exec(python3)"],"ask":["exec"]}}"#,
        ))
        .unwrap();
        let value: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["theme_mode"], "dark");
        assert_eq!(value["permissions"]["allow"], json!(["Exec(python3)"]));
        assert_eq!(ask_rules(&merged), vec!["exec", ASK_EDITS_RULE]);
        // Idempotent: the rule is not added twice.
        assert_eq!(
            ask_rules(&ask_before_edits(Some(&merged)).unwrap()),
            vec!["exec", ASK_EDITS_RULE]
        );
    }

    #[test]
    fn starts_from_an_empty_config() {
        assert_eq!(
            ask_rules(&ask_before_edits(None).unwrap()),
            vec![ASK_EDITS_RULE]
        );
        assert_eq!(
            ask_rules(&ask_before_edits(Some("  ")).unwrap()),
            vec![ASK_EDITS_RULE]
        );
    }

    #[test]
    fn refuses_configs_it_cannot_extend() {
        assert!(ask_before_edits(Some("{} /* unterminated")).is_err());
        assert!(ask_before_edits(Some("[]")).is_err());
        assert!(ask_before_edits(Some(r#"{"permissions":[]}"#)).is_err());
        assert!(ask_before_edits(Some(r#"{"permissions":{"ask":"Write(**)"}}"#)).is_err());
    }

    #[test]
    fn preserves_settings_and_permissions_in_commented_configs() {
        let user = r#"{
            // User-wide model and proxy settings.
            "agent": { "model": "swe-2-high" },
            "proxy": { "url": "https://example.invalid/a//b/*literal*/" },
            "label": "雪 \" // still a string /* not a comment */",
            "permissions": {
                "allow": ["Exec(git status)"], /* Keep existing grants. */
                "deny": ["Write(.env*)"],
                "ask": ["exec"]
            }
        } // A trailing comment is also valid."#;
        let merged = ask_before_edits(Some(user)).unwrap();
        let value: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["agent"]["model"], "swe-2-high");
        assert_eq!(
            value["proxy"]["url"],
            "https://example.invalid/a//b/*literal*/"
        );
        assert_eq!(
            value["label"],
            "雪 \" // still a string /* not a comment */"
        );
        assert_eq!(value["permissions"]["allow"], json!(["Exec(git status)"]));
        assert_eq!(value["permissions"]["deny"], json!(["Write(.env*)"]));
        assert_eq!(ask_rules(&merged), vec!["exec", ASK_EDITS_RULE]);
        assert_eq!(
            ask_rules(&ask_before_edits(Some("{ // comment\r\n}")).unwrap()),
            vec![ASK_EDITS_RULE]
        );
    }

    #[test]
    fn malformed_commented_configs_still_fail_closed() {
        for user in [
            "{} /* unterminated",
            r#"{"a": 1/* a comment cannot join number tokens */2}"#,
            r#"{"permissions": {"ask": /* still the wrong type */ "Write(**)"}}"#,
            r#"{"permissions": [/* still the wrong type */]}"#,
            r#"{"a": "unterminated // comment"#,
        ] {
            assert!(ask_before_edits(Some(user)).is_err(), "Accepted {user}");
        }
    }

    #[test]
    fn writes_each_config_once_by_content() {
        let dir =
            std::env::temp_dir().join(format!("monocode-devin-config-{}", std::process::id()));
        let first = write_once(&dir, "{}").unwrap();
        assert_eq!(write_once(&dir, "{}").unwrap(), first);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "{}");
        assert_ne!(write_once(&dir, "{\"a\":1}").unwrap(), first);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
