use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use monocode_core::models::AgentModel;
use serde_json::{Map, Value};

use super::super::catalog::{
    OpenCodeAgent, ParsedModels, ParsedProvider, flatten_open_code_models, open_code_provider_name,
};
use super::client::Client;
use super::server::{Options, Server};
use crate::core::child::Children;

pub async fn discover(
    children: Children,
    directory: &str,
    options: &Options,
) -> Result<Vec<AgentModel>> {
    let server = Server::start(children, directory, options).await?;
    let result = read(&server.client).await;
    server.stop().await;
    result
}

pub async fn read(client: &Client) -> Result<Vec<AgentModel>> {
    // OpenCode's own ACP client waits for catalog metadata to settle.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let models = client.models().await?;
        let agents = client.agents().await?;
        let result = flatten(&models, &agents)?;
        if !result.is_empty() || Instant::now() >= deadline {
            return Ok(result);
        }
        smol::Timer::after(Duration::from_millis(100)).await;
    }
}

pub fn flatten(models: &Value, agents: &Value) -> Result<Vec<AgentModel>> {
    let models = models
        .as_array()
        .ok_or_else(|| anyhow!("OpenCode 2 returned invalid model metadata"))?;
    let agents = agents
        .as_array()
        .ok_or_else(|| anyhow!("OpenCode 2 returned invalid agent metadata"))?;
    let mut parsed = ParsedModels::default();
    for model in models {
        if model.get("enabled").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let (Some(provider), Some(id)) = (
            model.get("providerID").and_then(Value::as_str),
            model.get("id").and_then(Value::as_str),
        ) else {
            continue;
        };
        let position = if let Some(index) = parsed
            .providers
            .iter()
            .position(|entry| entry.id == provider)
        {
            index
        } else {
            parsed.providers.push(ParsedProvider {
                id: provider.into(),
                name: open_code_provider_name(provider),
                models: Vec::new(),
            });
            parsed.connected.push(provider.into());
            parsed.providers.len() - 1
        };
        let mut model = model.clone();
        let variants: Map<String, Value> = model
            .get("variants")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|variant| {
                variant
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|id| (id.to_string(), variant.clone()))
            })
            .collect();
        model["variants"] = variants.into();
        parsed.providers[position].models.push((id.into(), model));
    }
    let agents: Vec<_> = agents
        .iter()
        .filter_map(|agent| {
            Some(OpenCodeAgent::new(
                agent.get("id")?.as_str()?,
                agent.get("mode")?.as_str()?,
                agent
                    .get("hidden")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ))
        })
        .collect();
    Ok(flatten_open_code_models(&parsed, &agents))
}
