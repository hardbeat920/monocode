//! Port of src/features/sessions/model/models.ts: the model catalog and the
//! pure resolve and default functions.
//!
//! The TypeScript kept live catalogs in module globals and read preferences
//! from localStorage. Here the live catalog is a `ModelCatalog` value, the
//! stored preferences are a `ModelPrefs` value, and the installer probe is a
//! `HarnessAvailability` value. `ModelEnv` bundles them with the per-project
//! overrides for the functions that need all four.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::LazyLock;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::block::ModelSettings;
use crate::harness::{HARNESSES, HarnessId};
use crate::project_providers::ProjectProviders;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettingChoice {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelSettingKind {
    #[serde(rename = "select")]
    Select,
    #[serde(rename = "toggle")]
    Toggle,
}

/// One provider setting a model exposes, such as reasoning effort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSetting {
    pub id: String,
    pub label: String,
    pub kind: ModelSettingKind,
    /// Default value.
    pub value: String,
    pub options: Vec<ModelSettingChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Upstream provider inside a multi-provider harness such as OpenCode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProvider {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModel {
    /// Catalog key, `harness:slug`.
    pub id: String,
    pub harness: HarnessId,
    pub name: String,
    /// Id passed to the CLI. `None` means the key minus the harness prefix.
    /// `Some("")` means omit `--model` and let the CLI choose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<ModelProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<Vec<ModelSetting>>,
    /// Context window, when the harness catalog reports one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<i64>,
}

impl AgentModel {
    pub fn new(id: &str, harness: HarnessId, name: &str) -> Self {
        Self {
            id: id.into(),
            harness,
            name: name.into(),
            native_id: None,
            provider: None,
            settings: None,
            context_window: None,
        }
    }

    pub fn with_native_id(mut self, native_id: &str) -> Self {
        self.native_id = Some(native_id.into());
        self
    }

    fn settings(&self) -> &[ModelSetting] {
        self.settings.as_deref().unwrap_or(&[])
    }

    /// `model.nativeId ?? nativeIdFrom(model.id)`.
    fn native_or_key(&self) -> String {
        self.native_id
            .clone()
            .unwrap_or_else(|| native_id_from(&self.id))
    }
}

fn select(id: &str, label: &str, value: &str, options: &[(&str, &str)]) -> ModelSetting {
    ModelSetting {
        id: id.into(),
        label: label.into(),
        kind: ModelSettingKind::Select,
        value: value.into(),
        options: options
            .iter()
            .map(|(value, label)| ModelSettingChoice {
                value: (*value).into(),
                label: (*label).into(),
            })
            .collect(),
        description: None,
    }
}

/// `MODELS`: the bundled fallback catalog, used until a CLI reports its own.
pub fn bundled_models() -> &'static [AgentModel] {
    static MODELS: LazyLock<Vec<AgentModel>> = LazyLock::new(|| {
        use HarnessId::*;
        let m = AgentModel::new;
        let mut grok_46 = m("grok:grok-4.6", Grok, "Grok 4.6").with_native_id("grok-4.6");
        grok_46.context_window = Some(500_000);
        grok_46.settings = Some(vec![select(
            "effort",
            "Reasoning",
            "high",
            &[
                ("xhigh", "Extra High"),
                ("high", "High"),
                ("medium", "Medium"),
                ("low", "Low"),
            ],
        )]);
        let mut grok_45 = m("grok:grok-4.5", Grok, "Grok 4.5").with_native_id("grok-4.5");
        grok_45.context_window = Some(500_000);
        grok_45.settings = Some(vec![select(
            "effort",
            "Reasoning",
            "high",
            &[("high", "High"), ("medium", "Medium"), ("low", "Low")],
        )]);
        vec![
            m("claude:sonnet-5", Claude, "Claude Sonnet 5").with_native_id("claude-sonnet-5"),
            m("claude:opus-5", Claude, "Claude Opus 5").with_native_id("claude-opus-5"),
            m("claude:opus-5-5", Claude, "Claude Opus 5.5").with_native_id("claude-opus-5-5"),
            m("claude:fable-5", Claude, "Claude Fable 5").with_native_id("claude-fable-5"),
            m("claude:opus-4.6", Claude, "Opus 4.6").with_native_id("claude-opus-4-6"),
            m("claude:sonnet-4.6", Claude, "Sonnet 4.6").with_native_id("claude-sonnet-4-6"),
            m("claude:haiku-4.5", Claude, "Haiku 4.5").with_native_id("claude-haiku-4-5"),
            m("claude:opus-4.5", Claude, "Opus 4.5").with_native_id("claude-opus-4-5"),
            m("cursor:composer-2.5", Cursor, "Composer 2.5").with_native_id("composer-2.5"),
            m("cursor:gpt-5.4", Cursor, "GPT-5.4").with_native_id("gpt-5.4"),
            m("cursor:claude-sonnet-4-6", Cursor, "Sonnet 4.6").with_native_id("claude-sonnet-4-6"),
            m("cursor:grok-4.6", Cursor, "Cursor Grok 4.6").with_native_id("grok-4.6"),
            grok_46,
            grok_45,
            m("opencode:glm-5", Opencode, "GLM 5"),
            m("opencode:minimax-m2.5", Opencode, "MiniMax M2.5"),
            m("opencode:kimi-k2.5", Opencode, "Kimi K2.5"),
            m("opencode:deepseek-v4-flash", Opencode, "DeepSeek V4 Flash"),
            m("opencode:qwen-3.5", Opencode, "Qwen 3.5"),
            m("opencode:grok-4.5", Opencode, "Grok 4.5"),
            m("opencode:claude-sonnet-4.6", Opencode, "Claude Sonnet 4.6"),
            m("opencode:gpt-5.4", Opencode, "GPT-5.4"),
            m("pi:default", Pi, "Default").with_native_id(""),
            m("omp:default", Omp, "Default").with_native_id(""),
            m("fx:zai/glm-5.2-fast", Fx, "GLM 5.2 Fast").with_native_id("zai/glm-5.2-fast"),
            m("hermes:default", Hermes, "Configured model").with_native_id(""),
            m("droid:default", Droid, "Configured model").with_native_id(""),
            m(
                "antigravity:gemini-3.8-flash-high",
                Antigravity,
                "Gemini 3.8 Flash (High)",
            )
            .with_native_id("gemini-3.8-flash-high"),
        ]
    });
    &MODELS
}

fn bundled_for(harness: HarnessId) -> &'static [AgentModel] {
    static BY_HARNESS: LazyLock<BTreeMap<HarnessId, Vec<AgentModel>>> = LazyLock::new(|| {
        let mut grouped: BTreeMap<HarnessId, Vec<AgentModel>> = BTreeMap::new();
        for model in bundled_models() {
            grouped
                .entry(model.harness)
                .or_default()
                .push(model.clone());
        }
        grouped
    });
    BY_HARNESS.get(&harness).map(Vec::as_slice).unwrap_or(&[])
}

fn bundled_by_id(id: &str) -> Option<&'static AgentModel> {
    bundled_models().iter().find(|model| model.id == id)
}

/// `DEFAULT_MODEL_ID`: the bundled default per provider. Codex has none.
pub fn bundled_default_model_id(harness: HarnessId) -> &'static str {
    match harness {
        HarnessId::Claude => "claude:sonnet-5",
        HarnessId::Codex => "",
        HarnessId::Cursor => "cursor:composer-2.5",
        HarnessId::Grok => "grok:grok-4.6",
        HarnessId::Opencode => "opencode:glm-5",
        HarnessId::Pi => "pi:default",
        HarnessId::Omp => "omp:default",
        HarnessId::Fx => "fx:zai/glm-5.2-fast",
        HarnessId::Hermes => "hermes:default",
        HarnessId::Droid => "droid:default",
        HarnessId::Antigravity => "antigravity:gemini-3.8-flash-high",
    }
}

/// The live catalogs CLIs reported, over the bundled fallback list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelCatalog {
    overlays: BTreeMap<HarnessId, Vec<AgentModel>>,
    overlay_defaults: BTreeMap<HarnessId, String>,
}

impl ModelCatalog {
    /// A catalog with only the bundled models.
    pub fn new() -> Self {
        Self::default()
    }

    /// `setHarnessModels`: replace a provider's list with its live catalog.
    /// An empty list is ignored.
    pub fn set_harness_models(&mut self, harness: HarnessId, models: Vec<AgentModel>) {
        if models.is_empty() {
            return;
        }
        let default_id = pick_default_id(harness, &models);
        self.overlays.insert(harness, models);
        self.overlay_defaults.insert(harness, default_id);
    }

    /// `hasLiveCatalog`: a live CLI catalog has replaced the bundled list.
    pub fn has_live_catalog(&self, harness: HarnessId) -> bool {
        self.overlays.contains_key(&harness)
    }

    /// `resetHarnessModelOverlays`.
    pub fn reset_overlays(&mut self) {
        self.overlays.clear();
        self.overlay_defaults.clear();
    }

    /// `defaultModelId`.
    pub fn default_model_id(&self, harness: HarnessId) -> String {
        self.overlay_defaults
            .get(&harness)
            .cloned()
            .unwrap_or_else(|| bundled_default_model_id(harness).to_string())
    }

    /// `modelsFor`: the live list, else the bundled one.
    pub fn models_for(&self, harness: HarnessId) -> &[AgentModel] {
        match self.overlays.get(&harness) {
            Some(models) => models,
            None => bundled_for(harness),
        }
    }

    /// `allModels`, in `HARNESSES` order.
    pub fn all_models(&self) -> impl Iterator<Item = &AgentModel> {
        HARNESSES
            .into_iter()
            .flat_map(move |harness| self.models_for(harness).iter())
    }

    /// `findModel`: the first model with this key across all providers.
    pub fn find_model(&self, id: &str) -> Option<&AgentModel> {
        self.all_models().find(|model| model.id == id)
    }

    /// `lookupModel`: live list first, bundled list second, so a model the CLI
    /// stopped advertising keeps its full native id.
    fn lookup_model(&self, id: &str) -> Option<&AgentModel> {
        self.find_model(id).or_else(|| bundled_by_id(id))
    }

    /// `resolveModel`: the catalog entry a saved model id means for `harness`.
    pub fn resolve_model(&self, harness: HarnessId, id: Option<&str>) -> AgentModel {
        let available = self.models_for(harness);
        if let Some(id) = id.filter(|id| !id.is_empty()) {
            if let Some(exact) = self.find_model(id)
                && exact.harness == harness
            {
                return exact.clone();
            }
            let slug = native_id_from(id);
            if let Some(by_native) = available.iter().find(|model| model.native_or_key() == slug) {
                return by_native.clone();
            }
            // Keep moving Claude aliases as aliases until the CLI advertises them.
            if harness == HarnessId::Claude && matches!(slug.as_str(), "opus" | "sonnet" | "haiku")
            {
                return AgentModel {
                    id: format!("claude:{slug}"),
                    harness,
                    name: format!("Claude {}", capitalize(&slug)),
                    native_id: Some(slug),
                    provider: None,
                    settings: None,
                    context_window: None,
                };
            }
            // A versioned id may differ only by Claude's provider prefix. Do not
            // resolve it to a moving alias or another version via a prefix match.
            let comparable_slug = comparable_native_id(harness, &slug);
            let hits: Vec<&AgentModel> = available
                .iter()
                .filter(|model| {
                    let key = model.native_or_key();
                    let native = comparable_native_id(harness, &key);
                    native.starts_with(comparable_slug) || comparable_slug.starts_with(native)
                })
                .collect();
            if has_digit(comparable_slug) {
                if let Some(same) = hits.iter().find(|model| {
                    comparable_native_id(harness, &model.native_or_key()) == comparable_slug
                }) {
                    return (*same).clone();
                }
                if harness != HarnessId::Claude
                    && hits.len() == 1
                    && !has_digit(comparable_native_id(harness, &hits[0].native_or_key()))
                {
                    return hits[0].clone();
                }
            } else if let Some(first) = hits.first() {
                return (*first).clone();
            }
            if let Some(bundled) = bundled_by_id(id)
                && bundled.harness == harness
            {
                return bundled.clone();
            }
            // A saved concrete Claude version may be absent from both catalogs.
            // Keep the requested id so a new session does not silently switch models.
            let requested = crate::js::trim(id);
            if harness == HarnessId::Claude && is_concrete_claude_key(requested) {
                let native_id = native_id_for_unknown_key(requested);
                return AgentModel {
                    id: requested.to_string(),
                    harness,
                    name: native_id.clone(),
                    native_id: Some(native_id),
                    provider: None,
                    settings: None,
                    context_window: None,
                };
            }
        }
        // Codex has no built-in catalog. During startup, keep the saved model
        // until discovery finishes instead of borrowing another provider's model.
        if available.is_empty() {
            let requested = id.map(crate::js::trim).unwrap_or("");
            let model_id = if !requested.is_empty()
                && (!requested.contains(':') || requested.starts_with(&format!("{harness}:")))
            {
                requested.to_string()
            } else {
                String::new()
            };
            let native_id = native_id_from(&model_id);
            let name = if native_id.is_empty() {
                capitalize(harness.as_str())
            } else {
                codex_style_name(&native_id)
            };
            return AgentModel {
                id: model_id,
                harness,
                name,
                native_id: Some(native_id),
                provider: None,
                settings: None,
                context_window: None,
            };
        }
        let fallback_id = self.default_model_id(harness);
        let fallback = if fallback_id.is_empty() {
            None
        } else {
            self.find_model(&fallback_id)
        };
        fallback.unwrap_or(&available[0]).clone()
    }

    /// `modelContextWindow`: catalog-reported context window, when known.
    pub fn model_context_window(&self, id: &str) -> Option<i64> {
        self.find_model(id)?
            .context_window
            .filter(|window| *window > 0)
    }

    /// `nativeModelId` for a saved model key.
    pub fn native_model_id_for(&self, id: &str) -> String {
        match self.lookup_model(id) {
            Some(found) => native_model_id(found),
            None => native_id_for_unknown_key(id),
        }
    }

    /// `mergeModelSettings`: the model's defaults, overlaid with every current
    /// value the model supports.
    pub fn merge_model_settings(
        &self,
        model: &AgentModel,
        current: Option<&ModelSettings>,
    ) -> ModelSettings {
        if self.models_for(model.harness).is_empty() {
            return current.cloned().unwrap_or_default();
        }
        let mut next = default_model_settings(model);
        let Some(current) = current else {
            return next;
        };
        for setting in model.settings() {
            if let Some(value) = compatible_setting_value(setting, current.get(&setting.id)) {
                next.insert(setting.id.clone(), value);
            }
        }
        next
    }

    /// `preferredModelSettings`: the last chosen effort, fast mode, and so on,
    /// applied to any model that supports those values. `last_settings` is
    /// `ModelPrefs::last_model_settings`.
    pub fn preferred_model_settings(
        &self,
        model: &AgentModel,
        current: Option<&ModelSettings>,
        last_settings: &ModelSettings,
    ) -> ModelSettings {
        if self.models_for(model.harness).is_empty() {
            return current.cloned().unwrap_or_default();
        }
        let mut merged = current.cloned().unwrap_or_default();
        merged.extend(last_settings.iter().map(|(k, v)| (k.clone(), v.clone())));
        self.merge_model_settings(model, Some(&merged))
    }

    /// `encodeModelLaunchId`: compound launch id, such as
    /// `claude-opus-4-8[effort=high,fast=false]`.
    pub fn encode_model_launch_id(
        &self,
        model_id: &str,
        settings: Option<&ModelSettings>,
    ) -> String {
        let model = self.lookup_model(model_id);
        let native = match model {
            Some(model) => native_model_id(model),
            None => native_id_for_unknown_key(model_id),
        };
        let defs = model.map(AgentModel::settings).unwrap_or(&[]);
        if native.is_empty() || defs.is_empty() {
            return native;
        }
        let parts: Vec<String> = defs
            .iter()
            .map(|setting| {
                let value = settings
                    .and_then(|settings| settings.get(&setting.id))
                    .unwrap_or(&setting.value);
                format!("{}={}", setting.id, value)
            })
            .collect();
        format!("{native}[{}]", parts.join(","))
    }
}

/// `nativeModelId` for a catalog entry.
pub fn native_model_id(model: &AgentModel) -> String {
    claude_native_id(model.harness, &model.native_or_key())
}

/// `defaultModelSettings`.
pub fn default_model_settings(model: &AgentModel) -> ModelSettings {
    model
        .settings()
        .iter()
        .map(|setting| (setting.id.clone(), setting.value.clone()))
        .collect()
}

const EFFORT_SETTING_IDS: [&str; 5] = [
    "effort",
    "reasoning",
    "reasoningEffort",
    // Pi and OMP expose their reasoning level as a `thinking` select.
    "thinking",
    // OpenCode exposes reasoning levels as `variant`.
    "variant",
];

/// `isEffortSettingId`.
pub fn is_effort_setting_id(id: &str) -> bool {
    EFFORT_SETTING_IDS.contains(&id)
}

/// `modelEffortSetting`: the select setting that controls reasoning effort.
pub fn model_effort_setting(model: &AgentModel) -> Option<&ModelSetting> {
    model.settings().iter().find(|setting| {
        setting.kind == ModelSettingKind::Select && is_effort_setting_id(&setting.id)
    })
}

/// `modelEffortLabel`.
pub fn model_effort_label(model: &AgentModel, values: Option<&ModelSettings>) -> Option<String> {
    let setting = model_effort_setting(model)?;
    let value = values
        .and_then(|values| values.get(&setting.id))
        .unwrap_or(&setting.value);
    Some(
        setting
            .options
            .iter()
            .find(|option| &option.value == value)
            .map(|option| option.label.clone())
            .unwrap_or_else(|| value.clone()),
    )
}

/// Cursor CLI uses `extra-high`; Claude uses `xhigh`.
fn setting_value_alias(value: &str) -> Option<&'static str> {
    match value {
        "extra-high" => Some("xhigh"),
        "xhigh" => Some("extra-high"),
        _ => None,
    }
}

fn compatible_setting_value(setting: &ModelSetting, value: Option<&String>) -> Option<String> {
    let value = value?;
    if setting.options.iter().any(|option| &option.value == value) {
        return Some(value.clone());
    }
    let alias = setting_value_alias(value)?;
    setting
        .options
        .iter()
        .any(|option| option.value == alias)
        .then(|| alias.to_string())
}

/// `nativeIdFrom`: the key minus its harness prefix and any `[settings]`.
fn native_id_from(id: &str) -> String {
    let trimmed = crate::js::trim(id);
    let slug = match trimmed.find(':') {
        Some(colon) => &trimmed[colon + 1..],
        None => trimmed,
    };
    match slug.find('[') {
        Some(bracket) => slug[..bracket].to_string(),
        None => slug.to_string(),
    }
}

/// `comparableNativeId`: match Claude ids with and without the provider prefix.
fn comparable_native_id(harness: HarnessId, id: &str) -> &str {
    if harness == HarnessId::Claude {
        id.strip_prefix("claude-").unwrap_or(id)
    } else {
        id
    }
}

fn has_digit(value: &str) -> bool {
    value.chars().any(|c| c.is_ascii_digit())
}

/// `/^claude:[a-z][a-z0-9-]*-\d/`.
fn is_concrete_claude_key(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("claude:") else {
        return false;
    };
    let bytes = rest.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_lowercase) {
        return false;
    }
    let prefix = bytes
        .iter()
        .take_while(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || **b == b'-')
        .count();
    (1..prefix).any(|i| bytes[i] == b'-' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
}

/// `nativeIdForUnknownKey`: last-resort native id for a saved key no catalog
/// knows. Every concrete Claude model is `claude-` plus a digit-bearing slug,
/// and picker keys can use dotted versions (`opus-4.8`) where the CLI expects
/// hyphens (`claude-opus-4-8`).
fn native_id_for_unknown_key(id: &str) -> String {
    let trimmed = crate::js::trim(id);
    let slug = native_id_from(trimmed);
    let harness = trimmed
        .find(':')
        .map(|colon| trimmed[..colon].to_lowercase())
        .unwrap_or_default();
    if harness != "claude" {
        return slug;
    }
    let chars: Vec<char> = slug.chars().collect();
    let dashed: String = chars
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if *c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
                '-'
            } else {
                *c
            }
        })
        .collect();
    claude_native_id(HarnessId::Claude, &dashed)
}

/// `claudeNativeId`: Claude's CLI rejects digit-bearing slugs without the
/// `claude-` prefix (`opus-5-5`), while bare aliases (`opus`) work.
fn claude_native_id(harness: HarnessId, native: &str) -> String {
    if harness != HarnessId::Claude || native.is_empty() || native.starts_with("claude-") {
        return native.to_string();
    }
    if has_digit(native) {
        format!("claude-{native}")
    } else {
        native.to_string()
    }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `nativeId.replace(/^gpt/i, "GPT").replace(/-([a-z])/g, ...)`.
fn codex_style_name(native_id: &str) -> String {
    let base = match native_id.get(..3) {
        Some(head) if head.eq_ignore_ascii_case("gpt") => format!("GPT{}", &native_id[3..]),
        _ => native_id.to_string(),
    };
    let chars: Vec<char> = base.chars().collect();
    let mut out = String::with_capacity(base.len());
    let mut i = 0;
    while i < chars.len() {
        out.push(chars[i]);
        if chars[i] == '-' && chars.get(i + 1).is_some_and(char::is_ascii_lowercase) {
            out.push(chars[i + 1].to_ascii_uppercase());
            i += 2;
            continue;
        }
        i += 1;
    }
    out
}

fn pick_default_id(harness: HarnessId, models: &[AgentModel]) -> String {
    let by_native = |native: &str| {
        models
            .iter()
            .find(|model| model.native_id.as_deref() == Some(native))
            .map(|model| model.id.clone())
    };
    let by_id = |id: &str| {
        models
            .iter()
            .find(|model| model.id == id)
            .map(|model| model.id.clone())
    };
    let first = || models.first().map(|model| model.id.clone());
    let bundled = bundled_default_model_id(harness);
    match harness {
        HarnessId::Claude => by_native("claude-sonnet-5")
            .or_else(|| by_native("sonnet"))
            .or_else(|| by_id(bundled))
            .or_else(first)
            .unwrap_or_else(|| bundled.to_string()),
        HarnessId::Cursor => by_native("composer-2.5")
            .or_else(|| {
                models
                    .iter()
                    .find(|model| matches!(model.native_id.as_deref(), Some("default" | "auto")))
                    .map(|model| model.id.clone())
            })
            .or_else(first)
            .unwrap_or_else(|| bundled.to_string()),
        HarnessId::Codex => first().unwrap_or_default(),
        HarnessId::Grok => by_native("grok-4.6")
            .or_else(|| by_id(bundled))
            .or_else(first)
            .unwrap_or_else(|| bundled.to_string()),
        HarnessId::Fx => [
            "zai/glm-5.2-fast",
            "zai/glm-5.2",
            "zai/glm-4.7-flash",
            "zai/glm-4.7",
            "openai/gpt-5.2",
        ]
        .into_iter()
        .find_map(by_native)
        .or_else(|| by_id(bundled))
        .or_else(first)
        .unwrap_or_else(|| bundled.to_string()),
        _ => by_id(bundled)
            .or_else(first)
            .unwrap_or_else(|| bundled.to_string()),
    }
}

/// `ModelPickerTab`: Favorites or one provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ModelPickerTab {
    #[default]
    Favorites,
    Harness(HarnessId),
}

impl ModelPickerTab {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelPickerTab::Favorites => "favorites",
            ModelPickerTab::Harness(id) => id.as_str(),
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value == "favorites" {
            return Some(ModelPickerTab::Favorites);
        }
        HarnessId::parse(value).map(ModelPickerTab::Harness)
    }
}

impl fmt::Display for ModelPickerTab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ModelPickerTab {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ModelPickerTab {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        ModelPickerTab::parse(&value)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown model picker tab {value}")))
    }
}

/// `modelPickerTabs`: Favorites, then every available provider.
pub fn model_picker_tabs(available: impl Fn(HarnessId) -> bool) -> Vec<ModelPickerTab> {
    std::iter::once(ModelPickerTab::Favorites)
        .chain(
            HARNESSES
                .into_iter()
                .filter(|id| available(*id))
                .map(ModelPickerTab::Harness),
        )
        .collect()
}

/// `coerceModelPickerTab`.
pub fn coerce_model_picker_tab(
    tab: ModelPickerTab,
    available: impl Fn(HarnessId) -> bool,
) -> ModelPickerTab {
    if model_picker_tabs(available).contains(&tab) {
        tab
    } else {
        ModelPickerTab::Favorites
    }
}

/// `stepModelPickerTab`: move left (`-1`) or right (`1`), wrapping.
pub fn step_model_picker_tab(
    tab: ModelPickerTab,
    delta: i32,
    available: impl Fn(HarnessId) -> bool,
) -> ModelPickerTab {
    let tabs = model_picker_tabs(available);
    if tabs.is_empty() {
        return tab;
    }
    let from = tabs.iter().position(|item| *item == tab).unwrap_or(0) as i64;
    let len = tabs.len() as i64;
    tabs[((from + delta as i64 + len) % len) as usize]
}

/// A provider and model the user picked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastModelChoice {
    pub harness: HarnessId,
    pub model: String,
}

/// localStorage keys the model preferences were stored under.
pub const FAVORITES_KEY: &str = "monocode.favoriteModels";
pub const MODEL_PICKER_TAB_KEY: &str = "monocode.modelPickerTab";
pub const HIDDEN_PICKER_PROVIDERS_KEY: &str = "monocode.hiddenPickerProviders";
pub const LAST_MODEL_KEY: &str = "monocode.lastModel";
pub const LAST_MODEL_SETTINGS_KEY: &str = "monocode.lastModelSettings";
pub const DEFAULT_MODELS_KEY: &str = "monocode.defaultModels";
pub const RECENT_MODELS_KEY: &str = "monocode.recentModels";
/// `RECENT_MODEL_LIMIT`.
pub const RECENT_MODEL_LIMIT: usize = 6;

/// How `save_last_model_settings` combines new values with stored ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveSettingsMode {
    /// New values win.
    #[default]
    Overwrite,
    /// Stored values win; only missing ones are recorded.
    Fill,
}

/// The model choices the TypeScript stored in localStorage. Each field names
/// the key it came from.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelPrefs {
    /// `FAVORITES_KEY`: favorite model keys.
    pub favorite_models: Vec<String>,
    /// `MODEL_PICKER_TAB_KEY`: the picker tab last shown.
    pub model_picker_tab: ModelPickerTab,
    /// `HIDDEN_PICKER_PROVIDERS_KEY`: providers hidden from the picker.
    pub hidden_picker_providers: Vec<HarnessId>,
    /// `LAST_MODEL_KEY`: the provider and model new sessions start with.
    pub last_model: Option<LastModelChoice>,
    /// `LAST_MODEL_SETTINGS_KEY`: last chosen effort, fast mode, and so on.
    pub last_model_settings: ModelSettings,
    /// `DEFAULT_MODELS_KEY`: the model picked per provider.
    pub default_models: BTreeMap<HarnessId, String>,
    /// `RECENT_MODELS_KEY`: most recent first, at most `RECENT_MODEL_LIMIT`.
    pub recent_models: Vec<LastModelChoice>,
}

fn parse_json(raw: Option<&str>) -> Option<Value> {
    serde_json::from_str(raw.filter(|raw| !raw.is_empty())?).ok()
}

fn parse_choice(value: &Value) -> Option<LastModelChoice> {
    let rec = value.as_object()?;
    let harness = HarnessId::parse(rec.get("harness")?.as_str()?)?;
    let model = rec.get("model")?.as_str()?.to_string();
    Some(LastModelChoice { harness, model })
}

/// `parseStringRecord`.
fn parse_string_record(value: &Value) -> ModelSettings {
    value
        .as_object()
        .map(|rec| {
            rec.iter()
                .filter_map(|(key, entry)| Some((key.clone(), entry.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

impl ModelPrefs {
    /// Read every model preference from the old localStorage values.
    pub fn from_local_storage(get: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            favorite_models: Self::parse_favorite_models(get(FAVORITES_KEY).as_deref()),
            model_picker_tab: Self::parse_model_picker_tab(get(MODEL_PICKER_TAB_KEY).as_deref()),
            hidden_picker_providers: Self::parse_hidden_picker_providers(
                get(HIDDEN_PICKER_PROVIDERS_KEY).as_deref(),
            ),
            last_model: Self::parse_last_model_choice(get(LAST_MODEL_KEY).as_deref()),
            last_model_settings: Self::parse_last_model_settings(
                get(LAST_MODEL_SETTINGS_KEY).as_deref(),
            ),
            default_models: Self::parse_default_models(get(DEFAULT_MODELS_KEY).as_deref()),
            recent_models: Self::parse_recent_model_choices(get(RECENT_MODELS_KEY).as_deref()),
        }
    }

    /// `loadFavoriteModels`.
    pub fn parse_favorite_models(raw: Option<&str>) -> Vec<String> {
        parse_json(raw)
            .and_then(|value| {
                value.as_array().map(|list| {
                    list.iter()
                        .filter_map(|id| id.as_str().map(str::to_string))
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    /// `loadModelPickerTab`.
    pub fn parse_model_picker_tab(raw: Option<&str>) -> ModelPickerTab {
        raw.and_then(ModelPickerTab::parse).unwrap_or_default()
    }

    /// `loadHiddenPickerProviders`.
    pub fn parse_hidden_picker_providers(raw: Option<&str>) -> Vec<HarnessId> {
        parse_json(raw)
            .and_then(|value| {
                value.as_array().map(|list| {
                    list.iter()
                        .filter_map(|id| id.as_str().and_then(HarnessId::parse))
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    /// `loadLastModelChoice`.
    pub fn parse_last_model_choice(raw: Option<&str>) -> Option<LastModelChoice> {
        parse_choice(&parse_json(raw)?)
    }

    /// `loadLastModelSettings`.
    pub fn parse_last_model_settings(raw: Option<&str>) -> ModelSettings {
        parse_json(raw)
            .map(|value| parse_string_record(&value))
            .unwrap_or_default()
    }

    /// `loadDefaultModels`.
    pub fn parse_default_models(raw: Option<&str>) -> BTreeMap<HarnessId, String> {
        let Some(value) = parse_json(raw) else {
            return BTreeMap::new();
        };
        value
            .as_object()
            .map(|rec| {
                rec.iter()
                    .filter_map(|(key, model)| {
                        let model = model.as_str().filter(|model| !model.is_empty())?;
                        Some((HarnessId::parse(key)?, model.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `loadRecentModelChoices`: unique choices, at most `RECENT_MODEL_LIMIT`.
    pub fn parse_recent_model_choices(raw: Option<&str>) -> Vec<LastModelChoice> {
        let Some(Value::Array(items)) = parse_json(raw) else {
            return Vec::new();
        };
        let mut choices: Vec<LastModelChoice> = Vec::new();
        for choice in items.iter().filter_map(parse_choice) {
            if choices.contains(&choice) {
                continue;
            }
            choices.push(choice);
            if choices.len() == RECENT_MODEL_LIMIT {
                break;
            }
        }
        choices
    }

    /// `saveLastModelSettings`.
    pub fn save_last_model_settings(&mut self, settings: &ModelSettings, mode: SaveSettingsMode) {
        for (key, value) in settings {
            if mode == SaveSettingsMode::Fill && self.last_model_settings.contains_key(key) {
                continue;
            }
            self.last_model_settings.insert(key.clone(), value.clone());
        }
    }

    /// `isPickerProviderVisible`.
    pub fn is_picker_provider_visible(&self, id: HarnessId) -> bool {
        !self.hidden_picker_providers.contains(&id)
    }

    /// `savePickerProviderVisible`.
    pub fn set_picker_provider_visible(&mut self, id: HarnessId, visible: bool) {
        let mut hidden: Vec<HarnessId> = Vec::new();
        for item in &self.hidden_picker_providers {
            if !hidden.contains(item) {
                hidden.push(*item);
            }
        }
        if visible {
            hidden.retain(|item| *item != id);
        } else if !hidden.contains(&id) {
            hidden.push(id);
        }
        self.hidden_picker_providers = hidden;
    }

    /// `saveDefaultModel`. An empty model reads back as unset, so it is removed.
    pub fn save_default_model(&mut self, harness: HarnessId, model: &str) {
        if model.is_empty() {
            self.default_models.remove(&harness);
        } else {
            self.default_models.insert(harness, model.to_string());
        }
    }

    /// `saveLastModelChoice`: also remembers the model for its provider.
    pub fn save_last_model_choice(&mut self, harness: HarnessId, model: &str) {
        self.save_default_model(harness, model);
        self.last_model = Some(LastModelChoice {
            harness,
            model: model.to_string(),
        });
    }

    /// `saveRecentModelChoice`: move a choice to the front of the recent list.
    pub fn save_recent_model_choice(
        &mut self,
        harness: HarnessId,
        model: &str,
    ) -> &[LastModelChoice] {
        let choice = LastModelChoice {
            harness,
            model: model.to_string(),
        };
        self.recent_models.retain(|item| *item != choice);
        self.recent_models.insert(0, choice);
        self.recent_models.truncate(RECENT_MODEL_LIMIT);
        &self.recent_models
    }

    /// `showProviderInModelPicker`: installed providers the user has not
    /// hidden. Before the first probe every provider shows, so the tab strip
    /// does not collapse to Favorites and then jump.
    pub fn show_provider_in_model_picker(
        &self,
        id: HarnessId,
        installed: bool,
        probed: bool,
    ) -> bool {
        if !self.is_picker_provider_visible(id) {
            return false;
        }
        !probed || installed
    }
}

/// Port of availabilityState.ts: which CLIs the installer probe found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HarnessAvailability {
    pub installed: BTreeSet<HarnessId>,
    /// The probe has finished at least once (`probedAt > 0`).
    pub probed: bool,
}

impl HarnessAvailability {
    /// `isHarnessAvailable`.
    pub fn is_available(&self, id: HarnessId) -> bool {
        self.installed.contains(&id)
    }
}

/// Everything the default-model functions read: the catalog, the stored
/// preferences, the installer probe, and the per-project overrides.
#[derive(Debug, Clone, Copy)]
pub struct ModelEnv<'a> {
    pub catalog: &'a ModelCatalog,
    pub prefs: &'a ModelPrefs,
    pub availability: &'a HarnessAvailability,
    pub projects: &'a ProjectProviders,
}

impl<'a> ModelEnv<'a> {
    /// `preferredModelId`: the user's model for a provider, else the catalog default.
    pub fn preferred_model_id(&self, harness: HarnessId) -> String {
        if let Some(saved) = self
            .prefs
            .default_models
            .get(&harness)
            .filter(|m| !m.is_empty())
        {
            return saved.clone();
        }
        if let Some(last) = &self.prefs.last_model
            && last.harness == harness
        {
            return last.model.clone();
        }
        self.catalog.default_model_id(harness)
    }

    /// `preferredModelSettings` with the stored last settings.
    pub fn preferred_model_settings(
        &self,
        model: &AgentModel,
        current: Option<&ModelSettings>,
    ) -> ModelSettings {
        self.catalog
            .preferred_model_settings(model, current, &self.prefs.last_model_settings)
    }

    /// `firstEnabledHarness`: `preferred` unless the project hides it, in
    /// which case the first provider the project allows. Falls back to
    /// `preferred` when a project hides everything.
    pub fn first_enabled_harness(&self, cwd: Option<&str>, preferred: HarnessId) -> HarnessId {
        let hidden = self.projects.load(cwd).hidden.unwrap_or_default();
        let enabled = |id: HarnessId| {
            !hidden.contains(&id)
                && self.prefs.show_provider_in_model_picker(
                    id,
                    self.availability.is_available(id),
                    self.availability.probed,
                )
        };
        if enabled(preferred) {
            return preferred;
        }
        HARNESSES
            .into_iter()
            .find(|id| enabled(*id))
            .unwrap_or(preferred)
    }

    /// `defaultSessionChoice`: the provider and model new conversations start with.
    pub fn default_session_choice(&self, cwd: Option<&str>) -> LastModelChoice {
        let project = self.projects.load(cwd);
        let preferred = project
            .default_harness
            .or(self.prefs.last_model.as_ref().map(|last| last.harness))
            .unwrap_or(HarnessId::Cursor);
        let harness = self.first_enabled_harness(cwd, preferred);
        let model = project
            .models
            .as_ref()
            .and_then(|models| models.get(&harness).cloned())
            .or_else(|| {
                (project.default_harness == Some(harness))
                    .then(|| project.default_model.clone())
                    .flatten()
            })
            .unwrap_or_else(|| self.preferred_model_id(harness));
        LastModelChoice { harness, model }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(values: &[(&str, &str)]) -> Vec<ModelSettingChoice> {
        values
            .iter()
            .map(|(value, label)| ModelSettingChoice {
                value: (*value).into(),
                label: (*label).into(),
            })
            .collect()
    }

    fn opus() -> AgentModel {
        let mut model = AgentModel::new("claude:opus-5", HarnessId::Claude, "Opus 5");
        model.settings = Some(vec![
            select(
                "effort",
                "Reasoning",
                "high",
                &[("high", "High"), ("xhigh", "Extra High"), ("max", "Max")],
            ),
            ModelSetting {
                id: "fast".into(),
                label: "Fast".into(),
                kind: ModelSettingKind::Toggle,
                value: "false".into(),
                options: choice(&[("true", "On"), ("false", "Off")]),
                description: None,
            },
        ]);
        model
    }

    fn haiku() -> AgentModel {
        let mut model = AgentModel::new("claude:haiku-4.5", HarnessId::Claude, "Haiku 4.5");
        model.settings = Some(vec![ModelSetting {
            id: "thinking".into(),
            label: "Thinking".into(),
            kind: ModelSettingKind::Toggle,
            value: "false".into(),
            options: choice(&[("true", "On"), ("false", "Off")]),
            description: None,
        }]);
        model
    }

    fn settings(pairs: &[(&str, &str)]) -> ModelSettings {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn live(id: &str, name: &str, native: &str) -> AgentModel {
        AgentModel::new(id, HarnessId::Claude, name).with_native_id(native)
    }

    struct Fixture {
        catalog: ModelCatalog,
        prefs: ModelPrefs,
        availability: HarnessAvailability,
        projects: ProjectProviders,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                catalog: ModelCatalog::new(),
                prefs: ModelPrefs::default(),
                availability: HarnessAvailability::default(),
                projects: ProjectProviders::default(),
            }
        }

        fn env(&self) -> ModelEnv<'_> {
            ModelEnv {
                catalog: &self.catalog,
                prefs: &self.prefs,
                availability: &self.availability,
                projects: &self.projects,
            }
        }
    }

    // model settings memory
    #[test]
    fn keeps_valid_current_values_when_merging_onto_a_model() {
        let catalog = ModelCatalog::new();
        assert_eq!(
            catalog.merge_model_settings(
                &opus(),
                Some(&settings(&[("effort", "xhigh"), ("fast", "true")]))
            ),
            settings(&[("effort", "xhigh"), ("fast", "true")])
        );
    }

    #[test]
    fn drops_values_the_new_model_does_not_support() {
        let catalog = ModelCatalog::new();
        assert_eq!(
            catalog.merge_model_settings(
                &haiku(),
                Some(&settings(&[("effort", "xhigh"), ("fast", "true")]))
            ),
            settings(&[("thinking", "false")])
        );
    }

    #[test]
    fn maps_extra_high_onto_claudes_xhigh() {
        let catalog = ModelCatalog::new();
        assert_eq!(
            catalog.merge_model_settings(&opus(), Some(&settings(&[("effort", "extra-high")]))),
            settings(&[("effort", "xhigh"), ("fast", "false")])
        );
    }

    #[test]
    fn remembers_extra_high_and_fast_across_models_that_support_them() {
        let mut f = Fixture::new();
        f.prefs.save_last_model_settings(
            &settings(&[("effort", "xhigh"), ("fast", "true")]),
            SaveSettingsMode::Overwrite,
        );
        assert_eq!(
            f.env().preferred_model_settings(&opus(), None),
            settings(&[("effort", "xhigh"), ("fast", "true")])
        );
        assert_eq!(
            f.env().preferred_model_settings(&haiku(), None),
            settings(&[("thinking", "false")])
        );
    }

    #[test]
    fn merges_newly_saved_settings_into_previously_stored_ones() {
        let mut prefs = ModelPrefs::default();
        prefs.save_last_model_settings(
            &settings(&[("effort", "xhigh"), ("fast", "true")]),
            SaveSettingsMode::Overwrite,
        );
        prefs.save_last_model_settings(
            &settings(&[("thinking", "true")]),
            SaveSettingsMode::Overwrite,
        );
        assert_eq!(
            prefs.last_model_settings,
            settings(&[("effort", "xhigh"), ("fast", "true"), ("thinking", "true")])
        );
    }

    #[test]
    fn applies_stored_preferences_over_a_sessions_current_values() {
        let mut f = Fixture::new();
        f.prefs.save_last_model_settings(
            &settings(&[("effort", "xhigh"), ("fast", "true")]),
            SaveSettingsMode::Overwrite,
        );
        assert_eq!(
            f.env().preferred_model_settings(
                &opus(),
                Some(&settings(&[("effort", "high"), ("fast", "false")]))
            ),
            settings(&[("effort", "xhigh"), ("fast", "true")])
        );
    }

    #[test]
    fn fill_mode_keeps_stored_preferences_when_the_session_still_has_defaults() {
        let mut prefs = ModelPrefs::default();
        prefs.save_last_model_settings(
            &settings(&[("effort", "xhigh"), ("fast", "true")]),
            SaveSettingsMode::Overwrite,
        );
        prefs.save_last_model_settings(
            &settings(&[("effort", "high"), ("fast", "false")]),
            SaveSettingsMode::Fill,
        );
        assert_eq!(
            prefs.last_model_settings,
            settings(&[("effort", "xhigh"), ("fast", "true")])
        );
    }

    #[test]
    fn fill_mode_records_session_values_that_have_not_been_stored_yet() {
        let mut prefs = ModelPrefs::default();
        prefs.save_last_model_settings(&settings(&[("effort", "xhigh")]), SaveSettingsMode::Fill);
        assert_eq!(prefs.last_model_settings, settings(&[("effort", "xhigh")]));
    }

    #[test]
    fn uses_the_current_session_when_nothing_has_been_stored_yet() {
        let f = Fixture::new();
        assert_eq!(
            f.env().preferred_model_settings(
                &opus(),
                Some(&settings(&[("effort", "xhigh"), ("fast", "true")]))
            ),
            settings(&[("effort", "xhigh"), ("fast", "true")])
        );
    }

    #[test]
    fn treats_opencode_variant_as_the_effort_setting() {
        let mut model = AgentModel::new(
            "opencode:some-cloud/spark-1",
            HarnessId::Opencode,
            "Spark 1",
        )
        .with_native_id("some-cloud/spark-1");
        model.settings = Some(vec![select(
            "variant",
            "Variant",
            "medium",
            &[
                ("minimal", "Minimal"),
                ("low", "Low"),
                ("medium", "Medium"),
                ("high", "High"),
                ("xhigh", "Extra High"),
            ],
        )]);
        assert_eq!(
            model_effort_setting(&model).map(|s| s.id.as_str()),
            Some("variant")
        );
        assert_eq!(
            ModelCatalog::new()
                .merge_model_settings(&model, Some(&settings(&[("variant", "high")]))),
            settings(&[("variant", "high")])
        );
        assert_eq!(model_effort_label(&model, None).as_deref(), Some("Medium"));
    }

    // provider defaults
    #[test]
    fn remembers_a_model_per_provider_without_changing_the_default_provider() {
        let mut f = Fixture::new();
        f.prefs
            .save_last_model_choice(HarnessId::Cursor, "cursor:grok-4.6");
        f.prefs
            .save_default_model(HarnessId::Claude, "claude:opus-5");
        f.prefs
            .save_default_model(HarnessId::Opencode, "opencode:glm-5");
        assert_eq!(
            f.prefs.last_model,
            Some(LastModelChoice {
                harness: HarnessId::Cursor,
                model: "cursor:grok-4.6".into()
            })
        );
        assert_eq!(
            f.prefs.default_models,
            BTreeMap::from([
                (HarnessId::Cursor, "cursor:grok-4.6".to_string()),
                (HarnessId::Claude, "claude:opus-5".to_string()),
                (HarnessId::Opencode, "opencode:glm-5".to_string()),
            ])
        );
        assert_eq!(
            f.env().preferred_model_id(HarnessId::Claude),
            "claude:opus-5"
        );
        assert_eq!(
            f.env().preferred_model_id(HarnessId::Cursor),
            "cursor:grok-4.6"
        );
    }

    #[test]
    fn falls_back_to_last_model_for_the_default_provider_when_no_map_exists() {
        let mut f = Fixture::new();
        f.prefs.last_model = ModelPrefs::parse_last_model_choice(Some(
            r#"{"harness":"cursor","model":"cursor:grok-4.6"}"#,
        ));
        assert_eq!(
            f.env().preferred_model_id(HarnessId::Cursor),
            "cursor:grok-4.6"
        );
        assert_eq!(
            f.env().preferred_model_id(HarnessId::Claude),
            f.catalog.default_model_id(HarnessId::Claude)
        );
    }

    #[test]
    fn uses_the_saved_default_provider_and_its_model_for_new_sessions() {
        let mut f = Fixture::new();
        f.prefs
            .save_last_model_choice(HarnessId::Claude, "claude:opus-5");
        assert_eq!(
            f.env().default_session_choice(None),
            LastModelChoice {
                harness: HarnessId::Claude,
                model: "claude:opus-5".into()
            }
        );
    }

    #[test]
    fn keeps_catalog_defaults_when_nothing_is_saved() {
        let f = Fixture::new();
        assert_eq!(
            f.env().default_session_choice(None),
            LastModelChoice {
                harness: HarnessId::Cursor,
                model: f.catalog.default_model_id(HarnessId::Cursor),
            }
        );
    }

    #[test]
    fn swaps_a_hidden_default_provider_for_the_first_enabled_one() {
        let mut f = Fixture::new();
        f.prefs
            .save_last_model_choice(HarnessId::Claude, "claude:opus-5");
        f.projects
            .set_project_provider_hidden("/repo/a", HarnessId::Claude, true);
        assert_eq!(
            f.env().default_session_choice(Some("/repo/a")),
            LastModelChoice {
                harness: HarnessId::Codex,
                model: f.catalog.default_model_id(HarnessId::Codex),
            }
        );
        assert_eq!(
            f.env().default_session_choice(Some("/repo/b")),
            LastModelChoice {
                harness: HarnessId::Claude,
                model: "claude:opus-5".into()
            }
        );
    }

    #[test]
    fn uses_a_projects_own_provider_and_model_when_set() {
        let mut f = Fixture::new();
        f.prefs
            .save_last_model_choice(HarnessId::Claude, "claude:opus-5");
        f.projects.set_project_default_provider(
            "/repo/a",
            HarnessId::Cursor,
            "cursor:composer-2.5",
        );
        assert_eq!(
            f.env().default_session_choice(Some("/repo/a")),
            LastModelChoice {
                harness: HarnessId::Cursor,
                model: "cursor:composer-2.5".into()
            }
        );
        assert_eq!(
            f.env().default_session_choice(Some("/repo/b")),
            LastModelChoice {
                harness: HarnessId::Claude,
                model: "claude:opus-5".into()
            }
        );
    }

    #[test]
    fn keeps_a_provider_the_project_still_allows() {
        let mut f = Fixture::new();
        f.projects
            .set_project_provider_hidden("/repo/a", HarnessId::Cursor, true);
        assert_eq!(
            f.env()
                .first_enabled_harness(Some("/repo/a"), HarnessId::Claude),
            HarnessId::Claude
        );
        assert_eq!(
            f.env()
                .first_enabled_harness(Some("/repo/a"), HarnessId::Cursor),
            HarnessId::Claude
        );
    }

    #[test]
    fn skips_providers_the_probe_did_not_find() {
        let mut f = Fixture::new();
        f.availability.probed = true;
        f.availability.installed.insert(HarnessId::Grok);
        assert_eq!(
            f.env().first_enabled_harness(None, HarnessId::Claude),
            HarnessId::Grok
        );
        f.availability.installed.clear();
        assert_eq!(
            f.env().first_enabled_harness(None, HarnessId::Claude),
            HarnessId::Claude
        );
    }

    #[test]
    fn keeps_the_six_most_recently_used_unique_models() {
        use HarnessId::*;
        let mut prefs = ModelPrefs::default();
        for (harness, model) in [
            (Claude, "claude:opus-5"),
            (Cursor, "cursor:composer-2.5"),
            (Grok, "grok:grok-4.6"),
            (Opencode, "opencode:glm-5"),
            (Pi, "pi:default"),
            (Omp, "omp:default"),
            (Fx, "fx:zai/glm-5.2-fast"),
            (Cursor, "cursor:composer-2.5"),
        ] {
            prefs.save_recent_model_choice(harness, model);
        }
        let expected: Vec<LastModelChoice> = [
            (Cursor, "cursor:composer-2.5"),
            (Fx, "fx:zai/glm-5.2-fast"),
            (Omp, "omp:default"),
            (Pi, "pi:default"),
            (Opencode, "opencode:glm-5"),
            (Grok, "grok:grok-4.6"),
        ]
        .into_iter()
        .map(|(harness, model)| LastModelChoice {
            harness,
            model: model.into(),
        })
        .collect();
        assert_eq!(prefs.recent_models, expected);
        let raw = serde_json::to_string(&prefs.recent_models).unwrap();
        assert_eq!(ModelPrefs::parse_recent_model_choices(Some(&raw)), expected);
    }

    #[test]
    fn parses_old_local_storage_values() {
        let stored = BTreeMap::from([
            (FAVORITES_KEY, r#"["claude:opus-5", 3]"#),
            (MODEL_PICKER_TAB_KEY, "grok"),
            (HIDDEN_PICKER_PROVIDERS_KEY, r#"["pi","bogus"]"#),
            (
                LAST_MODEL_KEY,
                r#"{"harness":"claude","model":"claude:opus-5"}"#,
            ),
            (LAST_MODEL_SETTINGS_KEY, r#"{"effort":"high","n":1}"#),
            (
                DEFAULT_MODELS_KEY,
                r#"{"claude":"claude:opus-5","codex":"","nope":"x"}"#,
            ),
            (RECENT_MODELS_KEY, "not json"),
        ]);
        let prefs = ModelPrefs::from_local_storage(|key| stored.get(key).map(|v| v.to_string()));
        assert_eq!(prefs.favorite_models, ["claude:opus-5"]);
        assert_eq!(
            prefs.model_picker_tab,
            ModelPickerTab::Harness(HarnessId::Grok)
        );
        assert_eq!(prefs.hidden_picker_providers, [HarnessId::Pi]);
        assert_eq!(prefs.last_model_settings, settings(&[("effort", "high")]));
        assert_eq!(prefs.default_models.len(), 1);
        assert!(prefs.recent_models.is_empty());
        assert_eq!(
            ModelPrefs::parse_model_picker_tab(Some("bogus")),
            ModelPickerTab::Favorites
        );
    }

    // model picker tabs
    fn available(id: HarnessId) -> bool {
        matches!(id, HarnessId::Claude | HarnessId::Fx | HarnessId::Cursor)
    }

    #[test]
    fn starts_with_favorites_then_installed_providers() {
        use ModelPickerTab::*;
        assert_eq!(
            model_picker_tabs(available),
            [
                Favorites,
                Harness(HarnessId::Claude),
                Harness(HarnessId::Cursor),
                Harness(HarnessId::Fx)
            ]
        );
    }

    #[test]
    fn wraps_left_and_right_across_favorites_and_providers() {
        use ModelPickerTab::*;
        assert_eq!(
            step_model_picker_tab(Favorites, 1, available),
            Harness(HarnessId::Claude)
        );
        assert_eq!(
            step_model_picker_tab(Harness(HarnessId::Claude), 1, available),
            Harness(HarnessId::Cursor)
        );
        assert_eq!(
            step_model_picker_tab(Harness(HarnessId::Fx), 1, available),
            Favorites
        );
        assert_eq!(
            step_model_picker_tab(Favorites, -1, available),
            Harness(HarnessId::Fx)
        );
    }

    #[test]
    fn treats_an_unavailable_current_tab_as_the_start_of_the_list() {
        use ModelPickerTab::*;
        assert_eq!(
            step_model_picker_tab(Harness(HarnessId::Pi), 1, available),
            Harness(HarnessId::Claude)
        );
    }

    #[test]
    fn falls_back_to_favorites_when_the_current_tab_is_hidden() {
        use ModelPickerTab::*;
        assert_eq!(
            coerce_model_picker_tab(Harness(HarnessId::Pi), available),
            Favorites
        );
        assert_eq!(
            coerce_model_picker_tab(Harness(HarnessId::Cursor), available),
            Harness(HarnessId::Cursor)
        );
        assert_eq!(coerce_model_picker_tab(Favorites, available), Favorites);
    }

    // picker provider visibility
    #[test]
    fn shows_every_provider_until_the_user_hides_one() {
        let mut prefs = ModelPrefs::default();
        assert!(prefs.hidden_picker_providers.is_empty());
        assert!(prefs.is_picker_provider_visible(HarnessId::Pi));
        prefs.set_picker_provider_visible(HarnessId::Pi, false);
        prefs.set_picker_provider_visible(HarnessId::Omp, false);
        assert!(!prefs.is_picker_provider_visible(HarnessId::Pi));
        assert!(!prefs.is_picker_provider_visible(HarnessId::Omp));
        assert!(prefs.is_picker_provider_visible(HarnessId::Claude));
        assert_eq!(
            prefs.hidden_picker_providers,
            [HarnessId::Pi, HarnessId::Omp]
        );
        prefs.set_picker_provider_visible(HarnessId::Pi, true);
        assert!(prefs.is_picker_provider_visible(HarnessId::Pi));
        assert_eq!(prefs.hidden_picker_providers, [HarnessId::Omp]);
    }

    #[test]
    fn omits_hidden_providers_even_before_an_install_probe() {
        let mut prefs = ModelPrefs::default();
        prefs.set_picker_provider_visible(HarnessId::Fx, false);
        assert!(!prefs.show_provider_in_model_picker(HarnessId::Fx, true, false));
        assert!(prefs.show_provider_in_model_picker(HarnessId::Claude, true, false));
    }

    #[test]
    fn omits_uninstalled_providers_after_the_probe_keeps_them_before() {
        let prefs = ModelPrefs::default();
        assert!(prefs.show_provider_in_model_picker(HarnessId::Pi, false, false));
        assert!(!prefs.show_provider_in_model_picker(HarnessId::Pi, false, true));
        assert!(prefs.show_provider_in_model_picker(HarnessId::Pi, true, true));
    }

    // live catalog overlays
    #[test]
    fn retains_a_saved_codex_model_and_settings_before_its_catalog_loads() {
        let catalog = ModelCatalog::new();
        let model = catalog.resolve_model(HarnessId::Codex, Some("codex:gpt-5.6-sol"));
        assert_eq!(model.id, "codex:gpt-5.6-sol");
        assert_eq!(model.harness, HarnessId::Codex);
        assert_eq!(model.name, "GPT-5.6-Sol");
        assert_eq!(model.native_id.as_deref(), Some("gpt-5.6-sol"));
        let current = settings(&[("reasoningEffort", "high"), ("serviceTier", "priority")]);
        assert_eq!(
            catalog.merge_model_settings(&model, Some(&current)),
            current
        );
        let blank = catalog.resolve_model(HarnessId::Codex, None);
        assert_eq!(blank.id, "");
        assert_eq!(blank.name, "Codex");
    }

    #[test]
    fn is_empty_until_a_cli_catalog_replaces_the_fallback_list() {
        let mut catalog = ModelCatalog::new();
        assert!(!catalog.has_live_catalog(HarnessId::Pi));
        catalog.set_harness_models(
            HarnessId::Pi,
            vec![
                AgentModel::new("pi:opus", HarnessId::Pi, "Opus").with_native_id("anthropic/opus"),
            ],
        );
        assert!(catalog.has_live_catalog(HarnessId::Pi));
        assert!(!catalog.has_live_catalog(HarnessId::Omp));
    }

    #[test]
    fn keeps_saved_claude_versions_distinct_from_a_live_alias() {
        let mut catalog = ModelCatalog::new();
        catalog.set_harness_models(
            HarnessId::Claude,
            vec![
                live("claude:sonnet", "Sonnet 5", "sonnet"),
                live("claude:opus", "Opus 5", "opus"),
            ],
        );
        assert_eq!(
            catalog
                .resolve_model(HarnessId::Claude, Some("claude:opus-5"))
                .id,
            "claude:opus-5"
        );
        let saved = catalog.resolve_model(HarnessId::Claude, Some("claude:opus-5-5"));
        assert_eq!(saved.id, "claude:opus-5-5");
        assert_eq!(native_model_id(&saved), "claude-opus-5-5");

        // A relaunch starts with the built-in catalog until discovery completes.
        catalog.reset_overlays();
        let alias = catalog.resolve_model(HarnessId::Claude, Some("claude:opus"));
        assert_eq!(alias.id, "claude:opus");
        assert_eq!(native_model_id(&alias), "opus");
    }

    #[test]
    fn keeps_opus_5_5_when_the_live_catalog_drops_it() {
        let mut catalog = ModelCatalog::new();
        catalog.set_harness_models(
            HarnessId::Claude,
            vec![
                live("claude:opus", "Opus", "opus"),
                live("claude:opus-5", "Opus 5", "claude-opus-5"),
                live("claude:sonnet", "Sonnet", "sonnet"),
            ],
        );
        assert_eq!(
            catalog
                .resolve_model(HarnessId::Claude, Some("claude:opus-5-5"))
                .id,
            "claude:opus-5-5"
        );
        assert_eq!(
            catalog.native_model_id_for("claude:opus-5-5"),
            "claude-opus-5-5"
        );
        assert_eq!(
            catalog.encode_model_launch_id("claude:opus-5-5", None),
            "claude-opus-5-5"
        );
    }

    #[test]
    fn prefers_the_bundled_versioned_model_over_a_singleton_fuzzy_match() {
        let mut catalog = ModelCatalog::new();
        catalog.set_harness_models(
            HarnessId::Claude,
            vec![live("claude:opus-5", "Opus 5", "claude-opus-5")],
        );
        assert_eq!(
            catalog
                .resolve_model(HarnessId::Claude, Some("claude:opus-5-5"))
                .id,
            "claude:opus-5-5"
        );
    }

    #[test]
    fn keeps_a_saved_claude_version_missing_from_both_catalogs() {
        let mut catalog = ModelCatalog::new();
        catalog.set_harness_models(
            HarnessId::Claude,
            vec![
                live("claude:sonnet", "Sonnet", "sonnet"),
                live("claude:opus-5", "Opus 5", "claude-opus-5"),
            ],
        );
        let model = catalog.resolve_model(HarnessId::Claude, Some("claude:opus-5-6"));
        assert_eq!(model.id, "claude:opus-5-6");
        assert_eq!(model.harness, HarnessId::Claude);
        assert_eq!(model.native_id.as_deref(), Some("claude-opus-5-6"));
        assert_eq!(native_model_id(&model), "claude-opus-5-6");

        let dotted = catalog.resolve_model(HarnessId::Claude, Some("claude:opus-4.8"));
        assert_eq!(dotted.id, "claude:opus-4.8");
        assert_eq!(dotted.native_id.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(
            catalog.native_model_id_for("claude:opus-4.8"),
            "claude-opus-4-8"
        );
    }

    #[test]
    fn prefixes_a_short_live_catalog_native_id_before_launching() {
        let mut catalog = ModelCatalog::new();
        catalog.set_harness_models(
            HarnessId::Claude,
            vec![live("claude:opus-5-5", "Opus 5.5", "opus-5-5")],
        );
        assert_eq!(
            catalog.native_model_id_for("claude:opus-5-5"),
            "claude-opus-5-5"
        );
        assert_eq!(
            native_model_id(&live("claude:opus-5-5", "Opus 5.5", "opus-5-5")),
            "claude-opus-5-5"
        );
    }

    #[test]
    fn rebuilds_a_claude_native_id_for_a_key_no_list_knows() {
        let catalog = ModelCatalog::new();
        assert_eq!(
            catalog.native_model_id_for("claude:opus-4-8"),
            "claude-opus-4-8"
        );
        assert_eq!(
            catalog.native_model_id_for("claude:opus-4-7"),
            "claude-opus-4-7"
        );
        assert_eq!(
            catalog.native_model_id_for("claude:haiku-4-5"),
            "claude-haiku-4-5"
        );
        assert_eq!(
            catalog.native_model_id_for("claude:opus-6"),
            "claude-opus-6"
        );
    }

    #[test]
    fn leaves_a_bare_claude_alias_and_other_providers_alone() {
        let catalog = ModelCatalog::new();
        assert_eq!(catalog.native_model_id_for("claude:opus"), "opus");
        assert_eq!(catalog.native_model_id_for("claude:sonnet"), "sonnet");
        assert_eq!(
            catalog.native_model_id_for("codex:gpt-5.6-unreleased"),
            "gpt-5.6-unreleased"
        );
    }

    #[test]
    fn keeps_a_lone_fuzzy_match_for_a_versioned_id() {
        let mut catalog = ModelCatalog::new();
        catalog.set_harness_models(
            HarnessId::Claude,
            vec![live("claude:opus-4-6", "Opus 4.6", "claude-opus-4-6")],
        );
        assert_eq!(
            catalog
                .resolve_model(HarnessId::Claude, Some("claude-opus-4-6"))
                .native_id
                .as_deref(),
            Some("claude-opus-4-6")
        );
    }

    #[test]
    fn keeps_a_live_opus_5_5_id_on_opus_5_5_across_relaunch() {
        let mut catalog = ModelCatalog::new();
        catalog.set_harness_models(
            HarnessId::Claude,
            vec![live(
                "claude:opus-5-5",
                "Claude Opus 5.5",
                "claude-opus-5-5",
            )],
        );
        assert_eq!(
            catalog
                .resolve_model(HarnessId::Claude, Some("claude:opus-5-5"))
                .native_id
                .as_deref(),
            Some("claude-opus-5-5")
        );
        catalog.reset_overlays();
        assert_eq!(
            catalog
                .resolve_model(HarnessId::Claude, Some("claude:opus-5-5"))
                .id,
            "claude:opus-5-5"
        );
        assert_eq!(
            catalog
                .resolve_model(HarnessId::Claude, Some("claude:opus-5"))
                .id,
            "claude:opus-5"
        );
        assert_eq!(
            catalog
                .resolve_model(HarnessId::Claude, Some("claude:opus"))
                .id,
            "claude:opus"
        );
    }

    #[test]
    fn keeps_a_saved_alias_when_live_discovery_lists_only_versions() {
        let mut catalog = ModelCatalog::new();
        catalog.set_harness_models(
            HarnessId::Claude,
            vec![
                live("claude:opus-5-20260101", "Opus 5", "claude-opus-5-20260101"),
                live("claude:opus-5-5", "Opus 5.5", "claude-opus-5-5"),
            ],
        );
        let alias = catalog.resolve_model(HarnessId::Claude, Some("claude:opus"));
        assert_eq!(alias.id, "claude:opus");
        assert_eq!(native_model_id(&alias), "opus");
    }

    /// A bundled entry may omit `nativeId`, which means the key minus the
    /// harness prefix. An explicit `""` means omit `--model`.
    fn expected_native(model: &AgentModel) -> String {
        match &model.native_id {
            Some(native) => native.clone(),
            None => match model.id.find(':') {
                Some(colon) => model.id[colon + 1..].to_string(),
                None => model.id.clone(),
            },
        }
    }

    fn bundled_by_harness() -> BTreeMap<HarnessId, Vec<AgentModel>> {
        let mut grouped: BTreeMap<HarnessId, Vec<AgentModel>> = BTreeMap::new();
        for model in bundled_models() {
            grouped
                .entry(model.harness)
                .or_default()
                .push(model.clone());
        }
        grouped
    }

    // every bundled model resolves to its own native id
    #[test]
    fn with_no_live_catalog() {
        let catalog = ModelCatalog::new();
        let wrong: Vec<String> = bundled_models()
            .iter()
            .filter(|model| catalog.native_model_id_for(&model.id) != expected_native(model))
            .map(|model| model.id.clone())
            .collect();
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    #[test]
    fn with_a_live_catalog_that_dropped_the_model() {
        let mut wrong = Vec::new();
        for (harness, models) in bundled_by_harness() {
            let mut catalog = ModelCatalog::new();
            catalog.set_harness_models(harness, vec![models[0].clone()]);
            for model in &models {
                if catalog.native_model_id_for(&model.id) != expected_native(model) {
                    wrong.push(model.id.clone());
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    #[test]
    fn resolve_model_never_returns_a_model_from_another_harness() {
        let mut wrong = Vec::new();
        for (harness, models) in bundled_by_harness() {
            let mut catalog = ModelCatalog::new();
            catalog.set_harness_models(harness, vec![models[0].clone()]);
            for model in &models {
                let resolved = catalog.resolve_model(harness, Some(&model.id));
                if resolved.harness != harness {
                    wrong.push(format!("{} resolved to {}", model.id, resolved.id));
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    #[test]
    fn encode_model_launch_id_never_drops_a_provider_prefix() {
        let mut wrong = Vec::new();
        for (harness, models) in bundled_by_harness() {
            let mut catalog = ModelCatalog::new();
            catalog.set_harness_models(harness, vec![models[0].clone()]);
            for model in &models {
                let launch = catalog
                    .encode_model_launch_id(&model.id, Some(&settings(&[("effort", "high")])));
                let base = launch.split('[').next().unwrap_or_default().to_string();
                if base != expected_native(model) {
                    wrong.push(format!("{} got {base}", model.id));
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    #[test]
    fn encodes_settings_into_the_launch_id() {
        let catalog = ModelCatalog::new();
        assert_eq!(
            catalog.encode_model_launch_id("grok:grok-4.6", Some(&settings(&[("effort", "low")]))),
            "grok-4.6[effort=low]"
        );
        assert_eq!(
            catalog.encode_model_launch_id("grok:grok-4.5", None),
            "grok-4.5[effort=high]"
        );
        assert_eq!(catalog.model_context_window("grok:grok-4.6"), Some(500_000));
    }
}
