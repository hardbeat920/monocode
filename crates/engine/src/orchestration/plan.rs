//! Port of the validators and prompts in
//! src/features/orchestration/model/orchestrationPlan.ts. The proposal types
//! and the card helpers (`proposalMarkdown`, `withOrchestrationProposal`,
//! `proposalBlock`, `restoreOrchestrationProposal`) live in
//! `monocode_core::orchestration`.

use std::collections::HashSet;
use std::future::Future;
use std::sync::LazyLock;

use monocode_core::orchestration::{
    OrchestrationChoice, OrchestrationProposal, OrchestrationProposalStatus, OrchestrationSettings,
    ProposedTask,
};
use monocode_core::paths::path_key;
use monocode_core::{Extra, HARNESSES, HarnessId, ModelSettings, js};
use regex::Regex;
use serde_json::{Map, Value};

use super::support::{is_drive_path, slice_tail};
use crate::submit::paths::is_equal_or_inside;

/// `required`: a trimmed, non-empty string of at most `max` UTF-16 units.
fn required(value: Option<&Value>, label: &str, max: usize) -> Result<String, String> {
    match value.and_then(Value::as_str) {
        Some(text) if !js::trim(text).is_empty() && js::len(text) <= max => {
            Ok(js::trim(text).to_string())
        }
        _ => Err(format!("Provide {label}")),
    }
}

/// `record`.
fn record(value: Option<&Value>) -> Result<&Map<String, Value>, String> {
    value
        .and_then(Value::as_object)
        .ok_or_else(|| "Expected an assignment object".to_string())
}

/// `stringList`.
fn string_list(value: Option<&Value>, label: &str, max: usize) -> Result<Vec<String>, String> {
    let Some(items) = value
        .and_then(Value::as_array)
        .filter(|items| items.len() <= max)
    else {
        return Err(format!("Invalid {label}"));
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in items {
        if item.as_str() == Some("") {
            continue;
        }
        let item = required(Some(item), label, 512)?;
        if seen.insert(item.clone()) {
            out.push(item);
        }
    }
    Ok(out)
}

/// `Number.isInteger(value)` for a JSON value.
pub(crate) fn js_integer(value: Option<&Value>) -> Option<i64> {
    let number = value?.as_f64()?;
    (number.is_finite() && number.fract() == 0.0).then_some(number as i64)
}

/// `scopePath`.
fn scope_path(value: &str) -> String {
    let slashed = value.replace('\\', "/");
    let prefix = if slashed.starts_with("//") {
        "//".to_string()
    } else if is_drive_path(&slashed) {
        slashed[..3].to_string()
    } else if slashed.starts_with('/') {
        "/".to_string()
    } else {
        String::new()
    };
    let parts: Vec<&str> = slashed[prefix.len()..]
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    let joined = format!("{prefix}{}", parts.join("/"));
    if joined.is_empty() {
        ".".into()
    } else {
        joined
    }
}

/// `/^[A-Za-z]:/`.
fn has_drive_letter(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// `projectRelativeScope`.
fn project_relative_scope(
    value: &str,
    cwd: Option<&str>,
    assignment_id: &str,
) -> Result<String, String> {
    let slashed = value.replace('\\', "/");
    if slashed.split('/').any(|part| part == "..") {
        return Err(format!(
            "Assignment \"{assignment_id}\" has invalid file scope \"{value}\". Use project-relative paths without '..', or '.' for the whole project"
        ));
    }
    let normalized = scope_path(value);
    if has_drive_letter(&normalized) && !is_drive_path(&normalized) {
        return Err(format!(
            "Assignment \"{assignment_id}\" has invalid file scope \"{value}\". Use project-relative paths, or '.' for the whole project"
        ));
    }
    let absolute = normalized.starts_with('/') || is_drive_path(&normalized);
    if !absolute {
        return Ok(normalized);
    }
    let Some(cwd) = cwd.filter(|cwd| is_equal_or_inside(&normalized, &scope_path(cwd))) else {
        return Err(format!(
            "Assignment \"{assignment_id}\" uses file scope \"{value}\" outside the selected checkout{}. Orchestration currently supports one checkout per run",
            cwd.map(|cwd| format!(" \"{cwd}\"")).unwrap_or_default()
        ));
    };
    let root = scope_path(cwd);
    if path_key(&normalized) == path_key(&root) {
        return Ok(".".into());
    }
    Ok(normalized
        .get(root.len()..)
        .unwrap_or_default()
        .trim_start_matches('/')
        .to_string())
}

/// `stringRecord`.
fn string_record(value: Option<&Value>, label: &str) -> Result<ModelSettings, String> {
    let input = record(value)?;
    if input.len() > 32 {
        return Err(format!("Invalid {label}"));
    }
    let mut out = ModelSettings::new();
    for (key, entry) in input {
        let key = required(Some(&Value::String(key.clone())), label, 128)?;
        let entry = required(Some(entry), label, 256)?;
        out.insert(key, entry);
    }
    Ok(out)
}

/// `validateOrchestrationSettings`.
pub fn validate_orchestration_settings(value: &Value) -> Result<OrchestrationSettings, String> {
    let input = record(Some(value))?;
    let max_workers = js_integer(input.get("maxWorkers"))
        .filter(|workers| (1..=4).contains(workers))
        .ok_or_else(|| "Choose 1 to 4 parallel workers".to_string())?;
    let Some(raw) = input
        .get("choices")
        .and_then(Value::as_array)
        .filter(|choices| !choices.is_empty() && choices.len() <= 4096)
    else {
        return Err(
            "No worker models are available, or the model catalog is too large. Check your connected harnesses."
                .into(),
        );
    };
    let mut choices: Vec<OrchestrationChoice> = Vec::new();
    let mut parsed = Vec::new();
    for value in raw {
        let item = record(Some(value))?;
        let harness = required(item.get("harness"), "a harness", 32)?;
        let harness = HarnessId::parse(&harness)
            .filter(|id| HARNESSES.contains(id))
            .ok_or_else(|| "Unknown worker harness".to_string())?;
        parsed.push(OrchestrationChoice {
            harness,
            model: required(item.get("model"), "a model", 256)?,
            name: required(item.get("name"), "a model name", 256)?,
            extra: Extra::new(),
        });
    }
    for choice in parsed {
        if !choices
            .iter()
            .any(|entry| entry.harness == choice.harness && entry.model == choice.model)
        {
            choices.push(choice);
        }
    }
    Ok(OrchestrationSettings {
        choices,
        max_workers,
        extra: Extra::new(),
    })
}

/// `validateOrchestrationSettings` for settings already in their typed shape.
pub fn validate_settings(
    settings: &OrchestrationSettings,
) -> Result<OrchestrationSettings, String> {
    validate_orchestration_settings(&serde_json::to_value(settings).unwrap_or(Value::Null))
}

static ASSIGNMENT_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]+$").expect("assignment id pattern"));

/// `validateProposedTasks`: validate the entire graph before enabling any
/// process or acquiring scopes.
pub fn validate_proposed_tasks(
    value: &Value,
    settings: &OrchestrationSettings,
    cwd: Option<&str>,
) -> Result<Vec<ProposedTask>, String> {
    let Some(items) = value
        .as_array()
        .filter(|items| !items.is_empty() && items.len() <= 40)
    else {
        return Err("Provide 1 to 40 assignments".into());
    };
    let mut tasks: Vec<ProposedTask> = Vec::new();
    for (index, value) in items.iter().enumerate() {
        let task = record(Some(value))?;
        let label = format!(
            "assignment {}{}",
            index + 1,
            task.get("id")
                .and_then(Value::as_str)
                .map(|id| format!(" ({id})"))
                .unwrap_or_default()
        );
        let model = required(task.get("model"), &format!("a model for {label}"), 256)?;
        // Models already identify their harness in our catalog. Recover an omitted
        // redundant field without another model call, but never override a choice.
        let mut harnesses: Vec<HarnessId> = Vec::new();
        for choice in settings
            .choices
            .iter()
            .filter(|choice| choice.model == model)
        {
            if !harnesses.contains(&choice.harness) {
                harnesses.push(choice.harness);
            }
        }
        let missing_harness = match task.get("harness") {
            None | Some(Value::Null) => true,
            Some(Value::String(text)) => text.is_empty(),
            Some(_) => false,
        };
        let recovered = (missing_harness && harnesses.len() == 1)
            .then(|| Value::String(harnesses[0].to_string()));
        let harness = required(
            recovered.as_ref().or(task.get("harness")),
            &format!(
                "a harness for {label}; use an exact harness/model pair from the available catalog"
            ),
            32,
        )?;
        let Some(harness_id) = settings
            .choices
            .iter()
            .find(|choice| choice.harness.to_string() == harness && choice.model == model)
            .map(|choice| choice.harness)
        else {
            return Err(format!(
                "The harness/model pair for {label} is outside the available catalog: {harness} / {model}"
            ));
        };
        let id = required(task.get("id"), "an assignment ID", 64)?;
        if !ASSIGNMENT_ID.is_match(&id) {
            return Err(
                "Assignment IDs must contain letters, numbers, underscores or hyphens".into(),
            );
        }
        let mut files: Vec<String> = Vec::new();
        for path in string_list(task.get("files"), "file scopes", 64)? {
            let scope = project_relative_scope(&path, cwd, &id)?;
            if !files.contains(&scope) {
                files.push(scope);
            }
        }
        if files.is_empty() {
            return Err(format!(
                "Assignment \"{id}\" has no file scopes. Use project-relative paths, or '.' for the whole project"
            ));
        }
        let title = required(task.get("title"), "a task title", 160)?;
        let prompt = required(task.get("prompt"), "task instructions", 30_000)?;
        let model_settings = match task.get("modelSettings") {
            None => None,
            Some(value) => Some(string_record(Some(value), "model settings")?),
        };
        let depends_on = match task.get("dependsOn") {
            None | Some(Value::Null) => Vec::new(),
            Some(value) => string_list(Some(value), "dependencies", 40)?,
        };
        tasks.push(ProposedTask {
            id,
            title,
            prompt,
            harness: harness_id,
            model,
            model_settings,
            files,
            depends_on,
            extra: Extra::new(),
        });
    }
    let ids: HashSet<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
    if ids.len() != tasks.len() {
        return Err("Assignment IDs must be unique".into());
    }
    let mut visited: HashSet<String> = HashSet::new();
    let mut visiting: HashSet<String> = HashSet::new();
    fn visit(
        id: &str,
        tasks: &[ProposedTask],
        ids: &HashSet<&str>,
        visited: &mut HashSet<String>,
        visiting: &mut HashSet<String>,
    ) -> Result<(), String> {
        if !ids.contains(id) {
            return Err("A dependency references an unknown assignment".into());
        }
        if visiting.contains(id) {
            return Err("Assignments have a dependency cycle".into());
        }
        if visited.contains(id) {
            return Ok(());
        }
        visiting.insert(id.to_string());
        if let Some(task) = tasks.iter().find(|task| task.id == id) {
            for dependency in &task.depends_on {
                visit(dependency, tasks, ids, visited, visiting)?;
            }
        }
        visiting.remove(id);
        visited.insert(id.to_string());
        Ok(())
    }
    for task in &tasks {
        visit(&task.id, &tasks, &ids, &mut visited, &mut visiting)?;
    }
    Ok(tasks)
}

/// `validateProposedTasks` for tasks already in their typed shape.
pub fn validate_tasks(
    tasks: &[ProposedTask],
    settings: &OrchestrationSettings,
    cwd: Option<&str>,
) -> Result<Vec<ProposedTask>, String> {
    validate_proposed_tasks(
        &serde_json::to_value(tasks).unwrap_or(Value::Null),
        settings,
        cwd,
    )
}

/// `orchestrationPlanningPrompt`.
pub fn orchestration_planning_prompt(
    request: &str,
    settings: &OrchestrationSettings,
    cwd: &str,
) -> String {
    [
        "Prepare an orchestration proposal for the user to review in MonoCode. Investigate and plan only: do not edit files, start workers, or invoke the MonoCode control CLI. No execution is authorized until the user confirms the assignment card.".to_string(),
        "You are the orchestrator: the user selected you in the composer model picker. Decide the task breakdown and choose each worker's harness and model from the available catalog below. Do not ask the user to assemble a team. They can change your choices in the card before confirming.".to_string(),
        "Keep planning efficient: inspect only what is needed to understand the request and relevant project conventions. Use the fewest useful tasks, with clear deliverables and acceptance checks. Do not create agents for trivial steps or duplicate investigation. Prefer a fast, economical model for straightforward work and a more capable model when complexity warrants it; do not invent model capabilities or prices. Reuse a suitable harness/model across tasks when that is sufficient. Explain your overall division of work briefly in the summary.".to_string(),
        "Use only exact harness/model pairs from the catalog. Give each task self-contained instructions and project-relative write scopes; directories own their descendants. Parallelize independent work with disjoint files. Serialize shared-file edits with dependencies and avoid concurrent repository-wide commands. Assign project-wide generation or final combined validation to a task with files [\".\"]. Workers must not commit, push, switch branches, or write outside the selected checkout. If the user requested Git or cross-checkout finalization, do not create a worker for it: the lead performs only those explicitly authorized final operations after every worker is reviewed, integrated, and the orchestration run is finished. All workers use app-managed isolated checkouts; do not ask them to create or switch worktrees.".to_string(),
        format!(
            "The exact checkout root is {}. Every files entry must be \".\" or a path relative to this root. For example, a discovered absolute path beneath this root must be returned without the root prefix. Never use an absolute path or '..'.",
            serde_json::to_string(cwd).unwrap_or_default()
        ),
        "Return your final proposal as one JSON object inside <monocode_proposal>...</monocode_proposal>. The app renders it as an editable card, so do not ask for approval in prose. No Markdown inside the JSON fields. Tasks may reference any task ID; the graph must be acyclic.".to_string(),
        "Schema: {\"title\":\"Short project title\",\"summary\":\"What you will do and how the work fits together\",\"tasks\":[{\"id\":\"task-1\",\"title\":\"Short task title\",\"prompt\":\"Self-contained instructions, constraints and checks\",\"harness\":\"exact harness ID\",\"model\":\"exact model ID\",\"files\":[\"src/feature\"],\"dependsOn\":[]}]}".to_string(),
        format!("Parallel worker limit: {}", settings.max_workers),
        format!(
            "<available_models>\n{}\n</available_models>",
            serde_json::to_string(&settings.choices).unwrap_or_default()
        ),
        format!("<user_request>\n{request}\n</user_request>"),
    ]
    .join("\n\n")
}

static TAGGED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<monocode_proposal>\s*([\s\S]*?)\s*</monocode_proposal>").expect("tag pattern")
});
static FENCED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"```(?:json)?\s*([\s\S]*?)```").expect("fence pattern"));

fn parse_proposal(
    draft: &OrchestrationProposal,
    response: &str,
) -> Result<OrchestrationProposal, String> {
    let tagged = TAGGED
        .captures(response)
        .and_then(|found| found.get(1))
        .map(|found| found.as_str());
    let fenced = FENCED
        .captures(response)
        .and_then(|found| found.get(1))
        .map(|found| found.as_str());
    let raw = js::trim(tagged.or(fenced).unwrap_or(response));
    let parsed: Value = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    let input = record(Some(&parsed))?;
    let title = required(input.get("title"), "a proposal title", 160)?;
    let summary = required(input.get("summary"), "a proposal summary", 2000)?;
    let tasks = validate_proposed_tasks(
        input.get("tasks").unwrap_or(&Value::Null),
        &draft.settings,
        Some(draft.checkout_cwd.as_deref().unwrap_or(&draft.cwd)),
    )?;
    Ok(OrchestrationProposal {
        title,
        summary,
        tasks,
        status: OrchestrationProposalStatus::Ready,
        error: None,
        response: None,
        ..draft.clone()
    })
}

/// `completeOrchestrationProposal`: the lead's response as a ready card, or
/// the draft marked invalid with the reason.
pub fn complete_orchestration_proposal(
    draft: &OrchestrationProposal,
    response: &str,
    error: Option<&str>,
) -> OrchestrationProposal {
    let attempt = match error {
        Some(error) => Err(error.to_string()),
        None => parse_proposal(draft, response),
    };
    match attempt {
        Ok(ready) => ready,
        Err(reason) => OrchestrationProposal {
            status: OrchestrationProposalStatus::Invalid,
            response: Some(slice_tail(response, 200_000).to_string()),
            error: Some(match error {
                Some(error) => error.to_string(),
                None => format!("Could not prepare the assignment card: {reason}"),
            }),
            ..draft.clone()
        },
    }
}

/// `orchestrationRepairPrompt`.
pub fn orchestration_repair_prompt(proposal: &OrchestrationProposal) -> String {
    [
        orchestration_planning_prompt(
            &proposal.request,
            &proposal.settings,
            proposal.checkout_cwd.as_deref().unwrap_or(&proposal.cwd),
        ),
        "Correct the previous proposal using the validation error below. Reuse your investigation and task breakdown; do not inspect the project again or run tools. Return only the corrected <monocode_proposal> JSON. Include an exact harness and model on every task. Do not execute any assignments.".to_string(),
        format!(
            "Validation error: {}",
            proposal
                .error
                .as_deref()
                .unwrap_or("The previous proposal was invalid")
        ),
        format!(
            "<previous_response>\n{}\n</previous_response>",
            proposal
                .response
                .as_deref()
                .map(|response| slice_tail(response, 60_000))
                .unwrap_or("")
        ),
    ]
    .join("\n\n")
}

/// `completeOrRepairOrchestrationProposal`: at most one corrective turn;
/// provider failures and cancellation never loop.
pub async fn complete_or_repair_orchestration_proposal<F, Fut>(
    draft: &OrchestrationProposal,
    response: &str,
    repair: F,
    can_repair: impl FnOnce() -> bool,
) -> Result<OrchestrationProposal, String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    let first = complete_orchestration_proposal(draft, response, None);
    if first.status != OrchestrationProposalStatus::Invalid || !can_repair() {
        return Ok(first);
    }
    let corrected = repair(orchestration_repair_prompt(&first)).await?;
    Ok(complete_orchestration_proposal(draft, &corrected, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use futures::executor::block_on;
    use monocode_core::orchestration::proposal_block;
    use serde_json::json;

    fn draft() -> OrchestrationProposal {
        serde_json::from_value(json!({
            "version": 1,
            "leadId": "lead",
            "cwd": "/repo",
            "request": "Build settings",
            "author": { "harness": "claude", "model": "claude:test", "name": "Lead" },
            "settings": {
                "choices": [{ "harness": "codex", "model": "codex:test", "name": "Worker" }],
                "maxWorkers": 2
            },
            "status": "planning",
            "title": "Planning",
            "summary": "",
            "tasks": []
        }))
        .unwrap()
    }

    fn task() -> Value {
        json!({
            "id": "ui",
            "title": "Settings UI",
            "prompt": "Build the view",
            "harness": "codex",
            "model": "codex:test",
            "files": ["src/settings"],
            "dependsOn": []
        })
    }

    fn with(base: Value, patch: Value) -> Value {
        let mut base = base;
        for (key, value) in patch.as_object().unwrap() {
            if value.is_null() {
                base.as_object_mut().unwrap().remove(key);
            } else {
                base[key] = value.clone();
            }
        }
        base
    }

    fn payload(tasks: Vec<Value>) -> Value {
        json!({ "title": "Settings", "summary": "Build the view, then validate", "tasks": tasks })
    }

    fn settings(choices: Value) -> OrchestrationSettings {
        serde_json::from_value(json!({ "choices": choices, "maxWorkers": 2 })).unwrap()
    }

    #[test]
    fn recovers_an_omitted_harness_from_an_exact_unambiguous_catalog_model_without_another_turn() {
        let repairs = RefCell::new(0);
        let result = block_on(complete_or_repair_orchestration_proposal(
            &draft(),
            &payload(vec![with(task(), json!({ "harness": null }))]).to_string(),
            |_| {
                *repairs.borrow_mut() += 1;
                async { Ok(String::new()) }
            },
            || true,
        ))
        .unwrap();
        assert_eq!(result.status, OrchestrationProposalStatus::Ready);
        assert_eq!(result.tasks[0].harness, HarnessId::Codex);
        assert_eq!(*repairs.borrow(), 0);
        assert_eq!(result.response, None);
    }

    #[test]
    fn never_guesses_ambiguous_harnesses_or_replaces_an_explicit_unavailable_choice() {
        let ambiguous = settings(json!([
            { "harness": "codex", "model": "shared", "name": "One" },
            { "harness": "opencode", "model": "shared", "name": "Two" }
        ]));
        let error = validate_proposed_tasks(
            &json!([with(task(), json!({ "model": "shared", "harness": null }))]),
            &ambiguous,
            None,
        )
        .unwrap_err();
        assert!(error.contains("a harness for assignment 1 (ui)"), "{error}");
        let error = validate_proposed_tasks(
            &json!([with(task(), json!({ "harness": "opencode" }))]),
            &draft().settings,
            None,
        )
        .unwrap_err();
        assert!(error.contains("outside the available catalog"));
        let error = validate_proposed_tasks(
            &json!([with(task(), json!({ "model": "unknown", "harness": null }))]),
            &ambiguous,
            None,
        )
        .unwrap_err();
        assert!(error.contains("available catalog"));
    }

    #[test]
    fn repairs_malformed_output_once_with_the_failed_response_exact_error_and_catalog() {
        let invalid = payload(vec![with(task(), json!({ "model": "unknown" }))]).to_string();
        let prompts = RefCell::new(Vec::new());
        let result = block_on(complete_or_repair_orchestration_proposal(
            &draft(),
            &invalid,
            |prompt| {
                prompts.borrow_mut().push(prompt);
                async { Ok(payload(vec![task()]).to_string()) }
            },
            || true,
        ))
        .unwrap();
        assert_eq!(result.status, OrchestrationProposalStatus::Ready);
        let prompts = prompts.borrow();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("assignment 1 (ui)"));
        assert!(prompts[0].contains(&invalid));
        assert!(prompts[0].contains("do not inspect the project again or run tools"));
        assert!(prompts[0].contains("\"model\":\"codex:test\""));
    }

    #[test]
    fn stops_after_one_unsuccessful_repair_and_retains_its_response_for_manual_retry() {
        let repairs = RefCell::new(0);
        let result = block_on(complete_or_repair_orchestration_proposal(
            &draft(),
            "Invalid",
            |_| {
                *repairs.borrow_mut() += 1;
                async { Ok("Still invalid".to_string()) }
            },
            || true,
        ))
        .unwrap();
        assert_eq!(*repairs.borrow(), 1);
        assert_eq!(result.status, OrchestrationProposalStatus::Invalid);
        assert!(result.tasks.is_empty());
        assert_eq!(result.response.as_deref(), Some("Still invalid"));
        assert!(orchestration_repair_prompt(&result).contains("Still invalid"));
    }

    #[test]
    fn does_not_spend_a_repair_turn_after_cancellation_or_retry_a_failed_provider() {
        let repairs = RefCell::new(0);
        let result = block_on(complete_or_repair_orchestration_proposal(
            &draft(),
            "Invalid",
            |_| {
                *repairs.borrow_mut() += 1;
                async { Err::<String, _>("Provider unavailable".to_string()) }
            },
            || false,
        ))
        .unwrap();
        assert_eq!(result.status, OrchestrationProposalStatus::Invalid);
        assert_eq!(*repairs.borrow(), 0);
        let failed = block_on(complete_or_repair_orchestration_proposal(
            &draft(),
            "Invalid",
            |_| {
                *repairs.borrow_mut() += 1;
                async { Err::<String, _>("Provider unavailable".to_string()) }
            },
            || true,
        ));
        assert_eq!(failed.unwrap_err(), "Provider unavailable");
        assert_eq!(*repairs.borrow(), 1);
    }

    #[test]
    fn prompts_the_current_lead_with_the_available_model_catalog_and_no_execution_authority() {
        let draft = draft();
        let prompt = orchestration_planning_prompt(&draft.request, &draft.settings, &draft.cwd);
        for needle in [
            "do not edit files, start workers",
            "\"model\":\"codex:test\"",
            "until the user confirms",
            "<monocode_proposal>",
            "fewest useful tasks",
            "Do not ask the user to assemble a team",
            "disjoint files",
            "acceptance checks",
            "exact checkout root is \"/repo\"",
            "returned without the root prefix",
        ] {
            assert!(prompt.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn turns_the_leads_structured_response_into_a_ready_card_without_changing_the_discovered_catalog()
     {
        let draft = draft();
        let mut body = payload(vec![task()]);
        body["settings"] = json!({ "choices": [] });
        let result = complete_orchestration_proposal(
            &draft,
            &format!("Commentary\n<monocode_proposal>{body}</monocode_proposal>"),
            None,
        );
        assert_eq!(result.status, OrchestrationProposalStatus::Ready);
        assert_eq!(
            serde_json::to_value(&result.tasks).unwrap(),
            json!([task()])
        );
        assert_eq!(result.settings, draft.settings);
        assert_eq!(result.author, draft.author);
    }

    #[test]
    fn validates_discovered_paths_against_the_proposals_concrete_worktree() {
        let mut worktree_draft = draft();
        worktree_draft.checkout_cwd = Some("/repo-worktrees/feature".into());
        let result = complete_orchestration_proposal(
            &worktree_draft,
            &payload(vec![with(
                task(),
                json!({ "files": ["/repo-worktrees/feature/src/settings"] }),
            )])
            .to_string(),
            None,
        );
        assert_eq!(result.status, OrchestrationProposalStatus::Ready);
        assert_eq!(result.tasks[0].files, vec!["src/settings".to_string()]);
        assert!(
            orchestration_repair_prompt(&worktree_draft)
                .contains("exact checkout root is \"/repo-worktrees/feature\"")
        );
    }

    #[test]
    fn accepts_fenced_json_and_rejects_prose_or_a_model_outside_the_available_catalog() {
        let draft = draft();
        assert_eq!(
            complete_orchestration_proposal(
                &draft,
                &format!("```json\n{}\n```", payload(vec![task()])),
                None
            )
            .status,
            OrchestrationProposalStatus::Ready
        );
        assert_eq!(
            complete_orchestration_proposal(&draft, "I will implement it now", None).status,
            OrchestrationProposalStatus::Invalid
        );
        let invalid = complete_orchestration_proposal(
            &draft,
            &payload(vec![with(task(), json!({ "model": "codex:unselected" }))]).to_string(),
            None,
        );
        assert_eq!(invalid.status, OrchestrationProposalStatus::Invalid);
        assert!(invalid.error.unwrap().contains("available catalog"));
        assert!(invalid.tasks.is_empty());
    }

    #[test]
    fn validates_cycles_unknown_dependencies_and_path_escapes_before_execution() {
        let settings = draft().settings;
        let check = |tasks: Value| validate_proposed_tasks(&tasks, &settings, None);
        assert!(
            check(json!([with(task(), json!({ "dependsOn": ["ui"] }))]))
                .unwrap_err()
                .contains("cycle")
        );
        assert!(
            check(json!([with(task(), json!({ "dependsOn": ["missing"] }))]))
                .unwrap_err()
                .contains("unknown assignment")
        );
        assert!(
            check(json!([with(task(), json!({ "files": ["../outside"] }))]))
                .unwrap_err()
                .contains("Assignment \"ui\" has invalid file scope \"../outside\"")
        );
        assert!(
            check(json!([task(), task()]))
                .unwrap_err()
                .contains("unique")
        );
        assert_eq!(
            check(json!([
                with(task(), json!({ "dependsOn": ["data"] })),
                with(task(), json!({ "id": "data" }))
            ]))
            .unwrap()
            .len(),
            2
        );
    }

    #[test]
    fn rebases_absolute_scopes_inside_the_checkout_and_identifies_outside_scopes() {
        let settings = draft().settings;
        assert_eq!(
            validate_proposed_tasks(
                &json!([with(
                    task(),
                    json!({ "files": ["/repo/src/settings", "src/settings", "/repo", "./src/shared"] })
                )]),
                &settings,
                Some("/repo"),
            )
            .unwrap()[0]
                .files,
            vec!["src/settings", ".", "src/shared"]
        );
        assert_eq!(
            validate_proposed_tasks(
                &json!([with(task(), json!({ "files": ["/repo-other/src"] }))]),
                &settings,
                Some("/repo"),
            )
            .unwrap_err(),
            "Assignment \"ui\" uses file scope \"/repo-other/src\" outside the selected checkout \"/repo\". Orchestration currently supports one checkout per run"
        );
    }

    #[test]
    fn rebases_windows_scopes_case_insensitively_with_portable_separators() {
        let settings = draft().settings;
        assert_eq!(
            validate_proposed_tasks(
                &json!([with(
                    task(),
                    json!({ "files": ["c:\\work\\repo\\src\\settings", "C:\\Work\\Repo"] })
                )]),
                &settings,
                Some("C:\\Work\\Repo"),
            )
            .unwrap()[0]
                .files,
            vec!["src/settings", "."]
        );
        assert!(
            validate_proposed_tasks(
                &json!([with(task(), json!({ "files": ["C:src\\settings"] }))]),
                &settings,
                Some("C:\\Work\\Repo"),
            )
            .unwrap_err()
            .contains("Assignment \"ui\" has invalid file scope \"C:src\\settings\"")
        );
    }

    #[test]
    fn keeps_an_explicitly_selected_worker_effort_and_rejects_malformed_settings() {
        let settings = draft().settings;
        let tasks = validate_proposed_tasks(
            &json!([with(
                task(),
                json!({ "modelSettings": { "reasoningEffort": "xhigh" } })
            )]),
            &settings,
            None,
        )
        .unwrap();
        assert_eq!(
            tasks[0].model_settings.as_ref().unwrap()["reasoningEffort"],
            "xhigh"
        );
        assert!(
            validate_proposed_tasks(
                &json!([with(
                    task(),
                    json!({ "modelSettings": { "reasoningEffort": 42 } })
                )]),
                &settings,
                None,
            )
            .unwrap_err()
            .contains("model settings")
        );
    }

    #[test]
    fn keeps_model_edits_and_assignments_through_persistence() {
        let mut proposal =
            complete_orchestration_proposal(&draft(), &payload(vec![task()]).to_string(), None);
        proposal.tasks[0].prompt = "User edited the instructions".into();
        let mut session =
            monocode_core::Session::blank("s", HarnessId::Claude, "claude:test", "/repo");
        session.blocks = vec![proposal_block("proposal", &proposal)];
        let saved = crate::runtime::session_store::sanitize_session_for_persist(&session);
        assert_eq!(
            saved.blocks[0]["orchestration"],
            serde_json::to_value(&proposal).unwrap()
        );
    }

    #[test]
    fn makes_interrupted_planning_non_executable_on_stop_and_reload() {
        let mut session =
            monocode_core::Session::blank("s", HarnessId::Claude, "claude:test", "/repo");
        session.busy = Some(true);
        session.blocks = vec![proposal_block("proposal", &draft())];
        let stopped = crate::runtime::reducer::stop_streaming(&session, 0);
        assert_eq!(
            stopped.blocks[0].orchestration.as_ref().unwrap().status,
            OrchestrationProposalStatus::Invalid
        );
        let saved = crate::runtime::session_store::sanitize_session_for_persist(&session);
        assert_eq!(saved.blocks[0]["orchestration"]["status"], "invalid");
    }

    #[test]
    fn validates_settings_and_drops_repeated_choices() {
        let settings = validate_orchestration_settings(&json!({
            "maxWorkers": 3,
            "choices": [
                { "harness": "codex", "model": "a", "name": "A" },
                { "harness": "codex", "model": "a", "name": "Again" },
                { "harness": "claude", "model": "a", "name": "B" }
            ]
        }))
        .unwrap();
        assert_eq!(settings.max_workers, 3);
        assert_eq!(settings.choices.len(), 2);
        assert_eq!(settings.choices[0].name, "A");
        assert_eq!(
            validate_orchestration_settings(&json!({ "maxWorkers": 5, "choices": [] }))
                .unwrap_err(),
            "Choose 1 to 4 parallel workers"
        );
        assert!(
            validate_orchestration_settings(&json!({ "maxWorkers": 2, "choices": [] }))
                .unwrap_err()
                .starts_with("No worker models are available")
        );
        assert_eq!(
            validate_orchestration_settings(&json!({
                "maxWorkers": 2,
                "choices": [{ "harness": "nope", "model": "a", "name": "A" }]
            }))
            .unwrap_err(),
            "Unknown worker harness"
        );
    }
}
