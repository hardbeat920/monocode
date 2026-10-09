// Wire routes, metadata and signed history adapted from cortexkit/antigravity-auth
// at 44eb8fa0ddd93e236942b9b36f9bb10fa2abde6d. See LICENSE (MIT).
use super::auth::now_ms;
use super::transport::{NativeError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone)]
pub struct Model {
    pub id: String,
    pub wire: String,
    pub budget: Option<i64>,
    pub context: u64,
    pub image: bool,
    pub claude: bool,
}

pub fn resolve_model(id: &str, variant: Option<&str>) -> Result<Model> {
    if variant.is_some_and(|v| !["low", "high"].contains(&v)) {
        return Err(NativeError::new(
            "model",
            "Unsupported Antigravity model variant.",
        ));
    }
    let id = id
        .strip_prefix("antigravity:")
        .unwrap_or(id)
        .strip_prefix("antigravity-")
        .unwrap_or(id.strip_prefix("antigravity:").unwrap_or(id));
    let mut base = id;
    let mut tier = variant;
    if variant.is_some() && (!id.starts_with("gemini-3") || id.contains("image")) {
        return Err(NativeError::new(
            "model",
            "This Antigravity model has no selectable thinking variant.",
        ));
    }
    if id.starts_with("gemini-3") && !id.contains("image") {
        for candidate in ["low", "medium", "high"] {
            if let Some(name) = id.strip_suffix(&format!("-{candidate}")) {
                base = name;
                if tier.is_none() {
                    tier = Some(candidate);
                }
                break;
            }
        }
    }
    if tier.is_some_and(|v| !["low", "medium", "high"].contains(&v)) {
        return Err(NativeError::new(
            "model",
            "Unsupported Antigravity model variant.",
        ));
    }
    let tier = tier.unwrap_or(if base.contains("pro") {
        "low"
    } else {
        "medium"
    });
    let (wire, budget, context, image, claude) = match base {
        "gemini-3.1-pro" => (
            if tier == "high" {
                "gemini-pro-agent".into()
            } else {
                "gemini-3.1-pro-low".into()
            },
            Some(if tier == "high" { 10001 } else { 1001 }),
            1048576,
            false,
            false,
        ),
        "gemini-3.5-flash" => (
            (match tier {
                "low" => "gemini-3.5-flash-extra-low",
                "high" => "gemini-3-flash-agent",
                _ => "gemini-3.5-flash-low",
            })
            .into(),
            Some(match tier {
                "low" => 1000,
                "high" => 10000,
                _ => 4000,
            }),
            1048576,
            false,
            false,
        ),
        "gemini-3.6-flash" | "gemini-3.7-flash" | "gemini-3.8-flash" => (
            format!("{base}-{tier}"),
            Some(match tier {
                "low" => 1000,
                "high" if base != "gemini-3.6-flash" => -1,
                "high" => 10000,
                _ => 4000,
            }),
            1048576,
            false,
            false,
        ),
        "claude-sonnet-4-6" | "claude-sonnet-4-6-thinking" => {
            ("claude-sonnet-4-6".into(), Some(1024), 250000, false, true)
        }
        "claude-opus-4-6-thinking" => (base.into(), Some(1024), 250000, false, true),
        "gpt-oss-120b" | "gpt-oss-120b-medium" => (
            "gpt-oss-120b-medium".into(),
            Some(8192),
            131072,
            false,
            false,
        ),
        "gemini-3.1-flash-image" => (base.into(), None, 66000, true, false),
        _ => {
            return Err(NativeError::new(
                "model",
                "This Antigravity model is not supported by the native backend.",
            ))
        }
    };
    Ok(Model {
        id: format!("antigravity:{id}"),
        wire,
        budget,
        context,
        image,
        claude,
    })
}

pub fn catalog() -> Vec<Value> {
    let definitions = [
        ("gemini-3.8-flash", "Gemini 3.8 Flash", 1048576),
        ("gemini-3.7-flash", "Gemini 3.7 Flash", 1048576),
        ("gemini-3.6-flash", "Gemini 3.6 Flash", 1048576),
        ("gemini-3.5-flash", "Gemini 3.5 Flash", 1048576),
        ("gemini-3.1-pro", "Gemini 3.1 Pro", 1048576),
        (
            "claude-sonnet-4-6-thinking",
            "Claude Sonnet 4.6 Thinking",
            250000,
        ),
        (
            "claude-opus-4-6-thinking",
            "Claude Opus 4.6 Thinking",
            250000,
        ),
        ("gemini-3.1-flash-image", "Gemini 3.1 Flash Image", 66000),
        ("gpt-oss-120b-medium", "GPT-OSS 120B Medium", 131072),
    ];
    definitions.into_iter().map(|(id, name, context)| {
        // Keep MonoCode's existing default id when the live catalog replaces
        // startup seeds. The route is still selected through the variant.
        let public_id = if id == "gemini-3.8-flash" { "gemini-3.8-flash-high" } else { id };
        let mut model = json!({"id":format!("antigravity:{public_id}"),"nativeId":id,"name":name,"harness":"antigravity","contextWindow":context});
        if id.starts_with("gemini-3") && !id.contains("image") {
            let choices = vec![json!({"value":"low","label":"Low"}), json!({"value":"high","label":"High"})];
            model["settings"] = json!([{"id":"thinking","label":"Thinking","kind":"select","value":if id.contains("pro") {"low"} else {"high"},"options":choices}]);
        }
        model
    }).collect()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HistoryItem {
    pub role: String,
    pub parts: Vec<Value>,
    pub model: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub id: String,
    pub cwd: String,
    pub email: String,
    pub trajectory: String,
    pub contents: Vec<HistoryItem>,
    pub last_execution: Option<String>,
    pub used_claude: bool,
    pub used_non_gemini: bool,
    pub last_request_ms: u64,
    // An intent persisted before a tool runs prevents execution on resume after
    // a crash. Unknown outcomes are returned as errors, never run a second time.
    pub tool_results: std::collections::HashMap<String, Value>,
}

impl History {
    pub fn new(cwd: &std::path::Path, email: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            cwd: cwd.to_string_lossy().into(),
            email,
            trajectory: uuid::Uuid::new_v4().to_string(),
            contents: Vec::new(),
            last_execution: None,
            used_claude: false,
            used_non_gemini: false,
            last_request_ms: 0,
            tool_results: Default::default(),
        }
    }
    pub fn filename(&self) -> String {
        format!("sessions/{}.dpapi", self.id)
    }
    pub fn contents_for(&self, model: &Model) -> Vec<Value> {
        let contents = self.contents.iter().filter_map(|item| {
            let same = item.model.as_deref().is_none_or(|old| old == model.wire);
            let mut parts = item.parts.clone();
            if !same {
                parts.retain(|p| p["thought"] != true);
                for part in &mut parts {
                    if let Some(part) = part.as_object_mut() {
                        part.remove("thoughtSignature");
                    }
                }
            }
            if !model.claude {
                // Unsigned cross-model Gemini function calls use the upstream
                // documented validator sentinel. Same-model signatures survive.
                for part in &mut parts {
                    if part.get("functionCall").is_some() && part.get("thoughtSignature").is_none()
                    {
                        part["thoughtSignature"] = json!("skip_thought_signature_validator");
                    }
                }
            }
            if parts.is_empty() {
                return None;
            }
            let role = if !same && parts.iter().all(|p| p.get("functionResponse").is_some()) {
                "user"
            } else {
                &item.role
            };
            Some(json!({"role":role,"parts":parts}))
        });
        let mut grouped: Vec<Value> = Vec::new();
        for content in contents {
            let responses = content["parts"]
                .as_array()
                .is_some_and(|parts| parts.iter().all(|p| p.get("functionResponse").is_some()));
            let merge = responses
                && grouped.last().is_some_and(|old| {
                    old["role"] == content["role"]
                        && old["parts"].as_array().is_some_and(|parts| {
                            parts.iter().all(|p| p.get("functionResponse").is_some())
                        })
                });
            if merge {
                grouped.last_mut().expect("response group")["parts"]
                    .as_array_mut()
                    .expect("parts")
                    .extend(content["parts"].as_array().expect("parts").iter().cloned());
            } else {
                grouped.push(content);
            }
        }
        grouped
    }
    pub fn request(&mut self, model: &Model, project: &str, tools: Value, plan: bool) -> Value {
        let contents = self.contents_for(model);
        let steps = contents.len()
            + contents
                .iter()
                .filter_map(|c| c["parts"].as_array())
                .flatten()
                .filter(|p| p.get("functionResponse").is_some())
                .count()
            + usize::from(self.last_execution.is_some());
        let steps = steps.max(1);
        self.used_claude |= model.claude;
        self.used_non_gemini |= model.claude || model.wire.starts_with("gpt-");
        let mut labels = json!({"last_step_index":steps.to_string(),"trajectory_id":self.trajectory,"used_claude":self.used_claude.to_string(),"used_claude_conservative":self.used_claude.to_string(),"used_non_gemini_model":self.used_non_gemini.to_string()});
        if let Some(id) = &self.last_execution {
            labels["last_execution_id"] = json!(id);
        }
        if let Some(value) = model_enum(&model.wire) {
            labels["model_enum"] = json!(value);
        }
        self.last_request_ms = now_ms().max(self.last_request_ms + 1);
        let request_id = format!(
            "agent/{}/{}/{}/{}",
            self.id,
            self.last_request_ms,
            self.trajectory,
            steps + 1
        );
        let mut instruction = if plan { "You are a coding assistant in plan mode. Only read, list and search files. Produce a plan without making changes or running commands." } else { "You are a coding assistant. Use the provided tools to inspect and change the user's workspace. Ask through the tool permission system before mutations when required. Tool parameters must exactly match the supplied schemas." }.to_string();
        if model.claude {
            instruction.push_str(" Interleaved thinking is enabled. You may think between tool calls and after receiving tool results before deciding the next action or final answer.");
        }
        let mut config = json!({"maxOutputTokens": if model.claude {32000} else if model.image {33000} else {32768}});
        if let Some(budget) = model.budget {
            config["thinkingConfig"] = if model.claude {
                json!({"include_thoughts":true,"thinking_budget":budget})
            } else {
                json!({"includeThoughts":true,"thinkingBudget":budget})
            };
        }
        if model.image {
            config["responseModalities"] = json!(["TEXT", "IMAGE"]);
        }
        let mut request =
            json!({"contents":contents,"systemInstruction":{"parts":[{"text":instruction}]}});
        if !model.image {
            request["tools"] = tools;
            request["toolConfig"] = json!({"functionCallingConfig":{"mode":"VALIDATED"}});
        }
        request["labels"] = labels;
        request["generationConfig"] = config;
        let uri = url::Url::from_directory_path(&self.cwd)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| self.cwd.clone());
        request["sessionId"] = json!(fnv1a(&uri));
        json!({"project":project,"requestId":request_id,"request":request,"model":model.wire,"userAgent":"antigravity","requestType":"agent"})
    }
}

fn fnv1a(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    (hash as i64).to_string()
}

fn model_enum(wire: &str) -> Option<&'static str> {
    Some(match wire {
        "gemini-3.5-flash-extra-low" => "MODEL_PLACEHOLDER_M187",
        "gemini-3.5-flash-low" => "MODEL_PLACEHOLDER_M20",
        "gemini-3-flash-agent" => "MODEL_PLACEHOLDER_M84",
        "gemini-3.6-flash-low" => "MODEL_PLACEHOLDER_M73",
        "gemini-3.6-flash-medium" => "MODEL_PLACEHOLDER_M72",
        "gemini-3.6-flash-high" => "MODEL_PLACEHOLDER_M71",
        "gemini-3.7-flash-low" => "MODEL_PLACEHOLDER_M300",
        "gemini-3.7-flash-medium" => "MODEL_PLACEHOLDER_M299",
        "gemini-3.7-flash-high" => "MODEL_PLACEHOLDER_M298",
        "gemini-3.8-flash-low" => "MODEL_PLACEHOLDER_M320",
        "gemini-3.8-flash-medium" => "MODEL_PLACEHOLDER_M319",
        "gemini-3.8-flash-high" => "MODEL_PLACEHOLDER_M318",
        "gemini-3.1-pro-low" => "MODEL_PLACEHOLDER_M36",
        "gemini-pro-agent" => "MODEL_PLACEHOLDER_M16",
        "claude-sonnet-4-6" => "MODEL_PLACEHOLDER_M35",
        "claude-opus-4-6-thinking" => "MODEL_PLACEHOLDER_M26",
        "gemini-3.1-flash-image" => "MODEL_PLACEHOLDER_M21",
        "gpt-oss-120b-medium" => "MODEL_OPENAI_GPT_OSS_120B_MEDIUM",
        _ => return None,
    })
}

#[derive(Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Default)]
pub struct Generation {
    pub parts: Vec<Value>,
    pub calls: Vec<ToolCall>,
    pub finished: bool,
    pub usage: Option<Value>,
    pending_signature: Option<Value>,
}

impl Generation {
    pub fn chunk(&mut self, raw: Value, emit: &mut impl FnMut(Value)) -> Result<()> {
        let response = raw.get("response").unwrap_or(&raw);
        if let Some(error) = raw.get("error").or_else(|| response.get("error")) {
            return Err(match error["code"].as_u64() {
                Some(429) => NativeError::new(
                    "quota",
                    "Antigravity quota is exhausted. Wait before retrying.",
                ),
                Some(401) => NativeError::auth(),
                Some(403) => NativeError::new(
                    "ineligible",
                    "This Google account is not eligible for Antigravity.",
                ),
                _ => NativeError::new("protocol", "Antigravity returned an error in its stream."),
            });
        }
        let candidate = &response["candidates"][0];
        if let Some(reason) = candidate["finishReason"].as_str() {
            self.finished = true;
            if !["STOP", "MAX_TOKENS"].contains(&reason) {
                return Err(NativeError::new(
                    "model",
                    "Antigravity blocked the generated response.",
                ));
            }
        }
        if let Some(usage) = response.get("usageMetadata") {
            self.usage = Some(usage.clone());
        }
        if let Some(parts) = candidate["content"]["parts"].as_array() {
            for part in parts {
                let mut part = part.clone();
                if part.get("thoughtSignature").is_some()
                    && part.get("functionCall").is_none()
                    && part["text"].as_str().is_none_or(str::is_empty)
                {
                    if part["thought"] != true {
                        if let Some(previous) = self
                            .parts
                            .last_mut()
                            .filter(|p| p["text"].is_string() && p["thought"] != true)
                        {
                            previous["thoughtSignature"] = part["thoughtSignature"].clone();
                            continue;
                        }
                    }
                    // Empty thought parts sign the next function-call batch.
                    // A signature-only part can precede text too. Hold it until
                    // the next block (or attach a trailing signature on finish).
                    self.pending_signature = part.get("thoughtSignature").cloned();
                    continue;
                }
                if part.get("functionCall").is_some()
                    || (part["thought"] != true
                        && part["text"].as_str().is_some_and(|t| !t.is_empty()))
                {
                    let pending = self.pending_signature.take();
                    if part.get("thoughtSignature").is_none() {
                        if let Some(signature) = pending {
                            part["thoughtSignature"] = signature;
                        }
                    }
                }
                if let Some(text) = part["text"].as_str() {
                    if text.is_empty() {
                        continue;
                    }
                    emit(
                        json!({"type":if part["thought"] == true {"reasoning.delta"} else {"message.delta"},"text":text}),
                    );
                    let merge = self.parts.last().is_some_and(|old| {
                        old["text"].is_string()
                            && (old["thought"] == true) == (part["thought"] == true)
                            && (part.get("thoughtSignature").is_none()
                                || old.get("thoughtSignature").is_none()
                                || old["thoughtSignature"] == part["thoughtSignature"])
                    });
                    if merge {
                        let old = self.parts.last_mut().expect("last text part");
                        let combined =
                            format!("{}{text}", old["text"].as_str().unwrap_or_default());
                        old["text"] = json!(combined);
                        if let Some(signature) = part.get("thoughtSignature") {
                            old["thoughtSignature"] = signature.clone();
                        }
                    } else {
                        self.parts.push(part);
                    }
                } else if let Some(call) = part.get_mut("functionCall") {
                    let name = call["name"]
                        .as_str()
                        .ok_or_else(|| {
                            NativeError::new("protocol", "Invalid Antigravity tool call.")
                        })?
                        .to_string();
                    let id = call["id"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                    call["id"] = json!(id);
                    let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
                    if !args.is_object() {
                        return Err(NativeError::new(
                            "protocol",
                            "Invalid Antigravity tool arguments.",
                        ));
                    }
                    if let Some(previous) = self.calls.iter_mut().find(|c| c.id == id) {
                        previous.args = args;
                        if let Some(old) = self
                            .parts
                            .iter_mut()
                            .find(|p| p["functionCall"]["id"] == id)
                        {
                            *old = part;
                        }
                    } else {
                        self.calls.push(ToolCall { id, name, args });
                        self.parts.push(part);
                    }
                } else if let Some(image) = part.get("inlineData") {
                    emit(
                        json!({"type":"image.generated","itemId":uuid::Uuid::new_v4().to_string(),"name":"Antigravity image","data":image["data"],"alt":"Generated by Antigravity"}),
                    );
                    self.parts.push(part);
                }
            }
        }
        if self.finished {
            if let (Some(signature), Some(previous)) =
                (self.pending_signature.take(), self.parts.last_mut())
            {
                previous["thoughtSignature"] = signature;
            }
        }
        Ok(())
    }
    pub fn complete(&self) -> Result<()> {
        if !self.finished || self.parts.is_empty() {
            return Err(NativeError::new(
                "network",
                "Antigravity generation ended before completion. Please retry.",
            ));
        }
        Ok(())
    }
}
