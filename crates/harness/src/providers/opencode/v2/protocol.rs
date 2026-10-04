use anyhow::{Result, bail};
use monocode_core::harness::RuntimeMode;
use serde_json::{Value, json};

use super::super::protocol::{
    MINIMUM_OPENCODE_VERSION, build_open_code_permission_rules, compare_semver,
    parse_open_code_model_slug, parse_open_code_version,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MajorVersion {
    One,
    Two,
}

pub fn version(output: &str) -> Result<MajorVersion> {
    let Some(version) = parse_open_code_version(output) else {
        bail!("Unable to determine OpenCode version");
    };
    match version.split('.').next() {
        Some("1") if compare_semver(&version, MINIMUM_OPENCODE_VERSION) >= 0 => {
            Ok(MajorVersion::One)
        }
        Some("2") => Ok(MajorVersion::Two),
        Some("1") => bail!("OpenCode {version} is older than {MINIMUM_OPENCODE_VERSION}"),
        _ => bail!("OpenCode {version} uses an unsupported protocol major version"),
    }
}

pub fn model_ref(model: &str, variant: Option<&str>) -> Result<Value> {
    let Some(model) =
        parse_open_code_model_slug(Some(model.strip_prefix("opencode:").unwrap_or(model)))
    else {
        bail!("Invalid OpenCode model identifier");
    };
    let mut model = json!({"providerID":model.provider_id,"id":model.model_id});
    if let Some(variant) = variant.filter(|value| !value.is_empty()) {
        model["variant"] = variant.into();
    }
    Ok(model)
}

pub fn permission_rules(mode: RuntimeMode) -> Value {
    Value::Array(build_open_code_permission_rules(mode).into_iter().map(|rule| json!({"action":rule.permission,"resource":rule.pattern,"effect":rule.action})).collect())
}
