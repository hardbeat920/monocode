//! Reviewable orchestration plans. Workers share a checkout; overlapping scopes
//! are serialized even when they have no explicit dependency.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Component, Path},
};

/// orchestrationSummary.ts: what the sidebar shows of a lead's run, kept after
/// it ends so the lead still lists its agents.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Summary {
    pub tasks: Vec<SummaryTask>,
    /// The run is still dispatching or waiting on its workers.
    pub live: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SummaryTask {
    pub session: Option<i64>,
    pub title: String,
    pub provider: String,
    pub model: Option<String>,
    /// queued, running, completed, failed or blocked.
    pub status: String,
}
impl SummaryTask {
    /// orchestrationTaskLabel, given whether the worker needs input.
    pub fn label(&self, summary: &Summary, needs_input: bool) -> &'static str {
        if needs_input {
            return "Needs input";
        }
        if !summary.live && matches!(self.status.as_str(), "running" | "queued") {
            return "Saved";
        }
        match self.status.as_str() {
            "queued" => "Queued",
            "running" => "Working",
            "completed" => "Done",
            "failed" => "Failed",
            "blocked" => "Needs review",
            _ => "Queued",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub files: Vec<String>,
    #[serde(default, alias = "dependsOn")]
    pub depends_on: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub title: String,
    pub summary: String,
    pub tasks: Vec<Task>,
}
pub const INSTRUCTIONS: &str = r#"Prepare an orchestration plan for the user's request. Inspect the project with read-only tools. Do not edit or launch workers yet. The user will review and start the plan in MonoCode.
Return a short explanation followed by exactly one JSON block between <monocode_orchestration> and </monocode_orchestration>.
Schema: {"title":"short title","summary":"approach","tasks":[{"id":"task-1","title":"assignment title","prompt":"complete self-contained instructions, validation and expected report","files":["src/path"],"depends_on":[]}]}
Use 1 to 12 assignments, unique ASCII IDs, exact project-relative file or directory scopes ('.' for the whole project), and dependency IDs. Independent assignments may run concurrently (up to four); overlapping scopes run serially. Each worker uses this session's provider, model and access policy. Include verification in the assignments. Never claim that workers have already started."#;

const OPEN: &str = "<monocode_orchestration>";
const CLOSE: &str = "</monocode_orchestration>";
const MAX_PLAN_BYTES: usize = 256 * 1024;
const MISSING: &str = "The response did not include a task plan. Select Retry plan to try again.";
const INCOMPLETE: &str =
    "The response ended before completing the task plan. Select Retry plan to try again.";

/// The dedicated provider plan channel can carry JSON without an envelope.
/// Assistant prose still needs an explicit, complete tagged plan.
pub fn parse_response(native: &str, assistant: &str) -> Result<Plan, String> {
    let native = native.trim();
    let fenced = native
        .split_once('\n')
        .filter(|(language, _)| matches!(language.trim_end(), "```" | "```json"));
    let result = if native.starts_with('{') {
        parse_json(native)
    } else if let Some((_, body)) = fenced {
        match body.rsplit_once('\n') {
            Some((body, "```")) => parse_json(body.trim()),
            _ => Err(INCOMPLETE.into()),
        }
    } else {
        parse(native)
    };
    result.or_else(|error| {
        parse(assistant).map_err(|fallback| if error == MISSING { fallback } else { error })
    })
}

pub fn parse(text: &str) -> Result<Plan, String> {
    let mut error = None;
    for (start, _) in text.rmatch_indices(OPEN) {
        match parse_tagged_body(&text[start + OPEN.len()..]) {
            Ok(plan) => return Ok(plan),
            Err(failure) => {
                error.get_or_insert(failure);
            }
        }
    }
    Err(error.unwrap_or_else(|| MISSING.into()))
}

fn parse_tagged_body(body: &str) -> Result<Plan, String> {
    // Parse exactly one JSON value before looking for the closing tag. A task
    // prompt can itself contain literal tags.
    let limit = body.floor_char_boundary(body.len().min(MAX_PLAN_BYTES + CLOSE.len()));
    let bounded = body[..limit].trim_start();
    let mut json = serde_json::Deserializer::from_str(bounded).into_iter::<Plan>();
    let result = json.next();
    let remaining = &bounded[json.byte_offset()..];
    match result {
        Some(Ok(plan)) if remaining.trim_start().starts_with(CLOSE) => {
            validate(&plan)?;
            Ok(plan)
        }
        Some(Ok(_)) if remaining.contains(CLOSE) => {
            Err("Invalid task plan: expected the closing tag after the JSON object.".into())
        }
        Some(Err(error)) if !error.is_eof() && bounded.contains(CLOSE) => {
            Err(format!("Invalid task plan: {error}"))
        }
        _ if body.len() > limit => Err("Task plan is too large.".into()),
        _ => Err(INCOMPLETE.into()),
    }
}

fn parse_json(body: &str) -> Result<Plan, String> {
    if body.len() > MAX_PLAN_BYTES {
        return Err("Task plan is too large.".into());
    }
    let plan: Plan = serde_json::from_str(body).map_err(|error| {
        if error.is_eof() {
            INCOMPLETE.into()
        } else {
            format!("Invalid task plan: {error}")
        }
    })?;
    validate(&plan)?;
    Ok(plan)
}
pub fn validate(plan: &Plan) -> Result<(), String> {
    if plan.title.trim().is_empty()
        || plan.title.len() > 160
        || plan.summary.len() > 4_000
        || plan.tasks.is_empty()
        || plan.tasks.len() > 12
    {
        return Err("Provide a title and 1–12 assignments.".into());
    }
    let mut ids = HashSet::new();
    for task in &plan.tasks {
        if task.id.is_empty()
            || task.id.len() > 64
            || !task
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || !ids.insert(task.id.as_str())
        {
            return Err(
                "Task IDs must be unique and contain only letters, numbers, '-' and '_'.".into(),
            );
        }
        if task.title.trim().is_empty()
            || task.title.len() > 160
            || task.prompt.trim().is_empty()
            || task.prompt.len() > 30_000
            || task.files.is_empty()
            || task.files.len() > 64
            || task.depends_on.len() > 12
        {
            return Err(format!(
                "Assignment {} needs a title, instructions and file scopes.",
                task.id
            ));
        }
        for file in &task.files {
            if file.trim().is_empty()
                || file.len() > 512
                || file.contains('\\')
                || file.contains(':')
                || Path::new(file).is_absolute()
                || Path::new(file).components().any(|c| {
                    matches!(
                        c,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
            {
                return Err(format!("Invalid file scope: {file}"));
            }
        }
    }
    let mut completed = HashSet::new();
    loop {
        let before = completed.len();
        for task in &plan.tasks {
            if task
                .depends_on
                .iter()
                .all(|id| completed.contains(id.as_str()))
            {
                completed.insert(task.id.as_str());
            }
        }
        if completed.len() == plan.tasks.len() {
            return Ok(());
        }
        if completed.len() == before {
            return Err("Task dependencies contain a cycle or unknown task ID.".into());
        }
    }
}
pub fn overlaps(a: &[String], b: &[String]) -> bool {
    a.iter().any(|a| {
        b.iter().any(|b| {
            a == "." || b == "." || Path::new(a).starts_with(b) || Path::new(b).starts_with(a)
        })
    })
}
pub fn worker_prompt(task: &Task, previous: &str) -> String {
    format!("You are completing assignment {} in a MonoCode orchestration.\n{}\n\n{}\n\nWrite only within these project-relative scopes: {}. Other agents may work in this checkout; preserve their changes and do not reset the repository. If you need a change outside your scopes, report the blocker. Run focused checks and report your changes, validation and any remaining issues.\n\nDependency reports:\n{}", task.id, task.title, task.prompt, task.files.join(", "), previous)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, deps: &[&str], file: &str) -> Task {
        Task {
            id: id.into(),
            title: id.into(),
            prompt: "Work".into(),
            files: vec![file.into()],
            depends_on: deps.iter().map(|s| (*s).into()).collect(),
        }
    }

    fn plan_json(title: &str, tasks: Vec<Task>) -> String {
        serde_json::to_string(&Plan {
            title: title.into(),
            summary: "Verify the changes.".into(),
            tasks,
        })
        .unwrap()
    }

    #[test]
    fn prefers_a_valid_native_plan_and_falls_back_to_tagged_assistant_text() {
        let native = plan_json("Native", vec![task("native", &[], "src")]);
        let assistant = plan_json("Assistant", vec![task("assistant", &[], "tests")]);
        let tagged = format!("Here is the plan.\n{OPEN}{assistant}{CLOSE}");
        for native in [native.clone(), format!("```json\n{native}\n```")] {
            assert_eq!(parse_response(&native, &tagged).unwrap().title, "Native");
        }
        let unsafe_scope = plan_json("Outside", vec![task("native", &[], "../src")]);
        for native in [
            "",
            "An ordinary Markdown plan",
            "{\"tasks\":{",
            &unsafe_scope,
        ] {
            assert_eq!(parse_response(native, &tagged).unwrap().title, "Assistant");
        }
        assert_eq!(
            parse_response(&unsafe_scope, "").unwrap_err(),
            "Invalid file scope: ../src"
        );
        assert_eq!(parse_response("", &assistant).unwrap_err(), MISSING);
    }

    #[test]
    fn recovers_a_complete_plan_after_an_unfinished_opening_tag() {
        let mut review = task("review", &["trace"], "tests");
        review.prompt = format!("Handle literal {OPEN} and {CLOSE} tags.");
        let json = plan_json("Complete", vec![task("trace", &[], "src"), review]);
        let text = format!("First draft: {OPEN}{{\"tasks\":[\nFinal: \\{OPEN}{json}{CLOSE}");
        let plan = parse_response("", &text).unwrap();
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[1].depends_on, ["trace"]);
        assert!(plan.tasks[1].prompt.contains(CLOSE));
    }

    #[test]
    fn rejects_a_truncated_plan_and_distinguishes_complete_invalid_json() {
        use crate::transcript::{Transcript, ROW_BYTES};

        let mut first = task("trace", &[], "src");
        first.prompt = "Read the docs. ".repeat(170);
        let json = plan_json(
            "Long plan",
            vec![first, task("review", &["trace"], "tests")],
        );
        let full = format!("\\{OPEN}{json}{CLOSE}");

        let mut transcript = Transcript::default();
        transcript.begin("assistant");
        transcript.append(&full);
        assert_eq!(transcript.rows[0].text.len(), ROW_BYTES);
        assert_eq!(
            parse_response("", &transcript.rows[0].text).unwrap_err(),
            INCOMPLETE
        );
        let restored = crate::response::prose(&transcript.rows);
        assert_eq!(parse_response("", &restored).unwrap().tasks.len(), 2);

        let invalid = format!("{OPEN}{{\"tasks\":{{}}}}{CLOSE}");
        assert!(parse_response("", &invalid)
            .unwrap_err()
            .starts_with("Invalid task plan"));
    }

    #[test]
    fn rejects_cycles_unknown_dependencies_and_scope_escapes() {
        let mut plan = Plan {
            title: "Work".into(),
            summary: String::new(),
            tasks: vec![task("a", &["b"], "src"), task("b", &["a"], "tests")],
        };
        assert!(validate(&plan).is_err());
        plan.tasks[1].depends_on.clear();
        assert!(validate(&plan).is_ok());
        plan.tasks[1].files = vec!["../elsewhere".into()];
        assert!(validate(&plan).is_err());
        assert!(overlaps(&["src".into()], &["src/main.rs".into()]));
        assert!(!overlaps(&["src".into()], &["src2/main.rs".into()]));
    }
}
