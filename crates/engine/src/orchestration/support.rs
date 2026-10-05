//! Port of the module-level helpers in
//! src/features/orchestration/model/orchestration.ts: scope keys and overlap,
//! the worker and lead prompts, the shell quoting, and the input checks every
//! control action runs.

use std::collections::HashSet;
use std::sync::LazyLock;

use monocode_core::js;
use monocode_core::paths::path_key;
use monocode_core::user_question::{QuestionAnswers, UserQuestion};
use regex::Regex;
use serde_json::{Map, Value};

/// `orchestrationPathKey`: one comparison form for scopes returned by Rust and
/// paths reported by a harness. Windows canonicalize uses the extended
/// `\\?\D:\...` form while providers usually report `D:\...`; both must
/// describe the same location.
pub fn orchestration_path_key(value: &str) -> String {
    let mut slashed = value.replace('\\', "/");
    let lower = slashed.to_ascii_lowercase();
    if lower.starts_with("//?/unc/") {
        slashed = format!("//{}", &slashed[8..]);
    } else if lower.len() >= 7
        && lower.starts_with("//?/")
        && lower.as_bytes()[4].is_ascii_alphabetic()
        && &lower.as_bytes()[5..7] == b":/"
    {
        slashed = slashed[4..].to_string();
    }
    let prefix = if slashed.starts_with("//") {
        "//".to_string()
    } else if is_drive_path(&slashed) {
        slashed[..3].to_string()
    } else if slashed.starts_with('/') {
        "/".to_string()
    } else {
        String::new()
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in slashed[prefix.len()..].split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
        } else {
            parts.push(part);
        }
    }
    let joined = format!("{prefix}{}", parts.join("/"));
    path_key(if joined.is_empty() { "." } else { &joined })
}

/// `/^[A-Za-z]:\//`.
pub(crate) fn is_drive_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

/// `scopeContains`.
pub fn scope_contains(scope: &str, path: &str) -> bool {
    path == scope
        || path.starts_with(&if scope.ends_with('/') {
            scope.to_string()
        } else {
            format!("{scope}/")
        })
}

/// `scopesOverlap`.
pub fn scopes_overlap(a: &[String], b: &[String]) -> bool {
    a.iter().any(|left| {
        let left_key = orchestration_path_key(left);
        b.iter().any(|right| {
            let right_key = orchestration_path_key(right);
            scope_contains(&left_key, &right_key) || scope_contains(&right_key, &left_key)
        })
    })
}

/// `sameCheckout`.
pub fn same_checkout(a: &str, b: &str) -> bool {
    path_key(&a.replace('\\', "/")) == path_key(&b.replace('\\', "/"))
}

/// `recoveryTurn`.
pub fn recovery_turn(reason: &str) -> String {
    format!(
        "Continue the existing assignment from its retained worker checkout. The previous turn was stopped because the orchestration run was interrupted: {reason}\n\nInspect the current files and prior conversation before acting. Preserve completed work, do not repeat destructive or external operations, remain inside the assigned write scope, run the remaining focused checks, and report what was already done versus what you completed now."
    )
}

static ASSIGNMENT_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:\r?\n[ \t]*)*<monocode_assignment\b[^>]*>[\s\S]*?</monocode_assignment>")
        .expect("assignment pattern")
});

/// `workerTurnPrompt`: the prompt the worker receives, including the
/// envelope the transcript hides.
pub fn worker_turn_prompt(prompt: &str, files: &[String], scratch_dir: Option<&str>) -> String {
    let scratch = match scratch_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => format!(
            " Temporary helpers and test output may be written in your private scratch directory: {}. TMPDIR, TMP and TEMP point there. Use this directory for scratch files; do not write elsewhere outside the project. Deliver final changes in your assigned project files.",
            serde_json::to_string(dir).unwrap_or_default()
        ),
        None => String::new(),
    };
    format!(
        "{prompt}\n\n<monocode_assignment>\nYou are a worker managed by a MonoCode lead. Work only in the checkout selected for this run. The workspace, scope and Git rules in this assignment envelope override any contradictory wording in the task text above. Your assigned write scope is: {}.{scratch} Read other files as needed, but do not edit outside your scope. If another file or shared operation is needed, report the blocker and stop so the lead can expand or create a new assignment. Do not spawn agents, create worktrees, switch branches, stage, commit, push, install dependencies or run broad formatters/generators. A task owning '.' may run explicitly requested project-wide validation or generation, but Git finalization remains the lead's responsibility after integration. Other workers may be working concurrently in separate checkouts; do not rely on their work until the lead has accepted it. Report focused checks, changed files, remaining issues and a concise final result.\n</monocode_assignment>",
        files.join(", ")
    )
}

/// `visibleUserPrompt`: task text a person should see. The assignment
/// envelope stays in the send.
pub fn visible_user_prompt(text: &str) -> String {
    js::trim_end(&ASSIGNMENT_BLOCK.replace_all(text, "")).to_string()
}

/// `shellPath`: quote the executable for the shell the lead runs in, and only
/// when needed. The path is absolute, so a leading slash means a POSIX shell,
/// where a backslash escapes rather than separates, and so is never safe bare.
pub fn shell_path(path: &str) -> String {
    if path.starts_with('/') {
        return if path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._/:-".contains(c))
        {
            path.to_string()
        } else {
            format!("'{}'", path.replace('\'', r"'\''"))
        };
    }
    if path.chars().any(|c| js::is_space(c) || c == '"') {
        format!("\"{}\"", path.replace('"', ""))
    } else {
        path.to_string()
    }
}

/// `messageOf` for the error strings the engine passes around.
pub fn message_of(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// `text(value, label, max)`: a required, trimmed string.
pub fn text(value: Option<&Value>, label: &str, max: usize) -> Result<String, String> {
    let Some(value) = value
        .and_then(Value::as_str)
        .filter(|v| !js::trim(v).is_empty())
    else {
        return Err(format!("Invalid {label}: provide a non-empty string"));
    };
    if js::len(value) > max {
        return Err(format!("Invalid {label}: keep it under {max} characters"));
    }
    Ok(js::trim(value).to_string())
}

/// `strings(value, label, max)`: a list of required strings, without repeats.
pub fn strings(value: Option<&Value>, label: &str, max: usize) -> Result<Vec<String>, String> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Err(format!("Invalid {label}: provide an array of strings"));
    };
    if items.len() > max {
        return Err(format!("Invalid {label}: at most {max} entries"));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let item = text(Some(item), label, 512)?;
        if seen.insert(item.clone()) {
            out.push(item);
        }
    }
    Ok(out)
}

/// `listed`: a comma list that names at most `max` values.
pub fn listed(values: &[String], max: usize) -> String {
    if values.len() > max {
        format!(
            "{} (+{} more)",
            values[..max].join(", "),
            values.len() - max
        )
    } else {
        values.join(", ")
    }
}

/// `listed(values)` with the default limit of 12.
pub fn listed12(values: &[String]) -> String {
    listed(values, 12)
}

/// `FIELDS`: the input fields each control action accepts. A mistyped field
/// must fail loudly rather than silently change the task.
pub const FIELDS: [(&str, &[&str]); 12] = [
    ("list", &[]),
    (
        "delegate",
        &["title", "harness", "model", "prompt", "files", "dependsOn"],
    ),
    ("get", &["taskId"]),
    ("message", &["taskId", "text"]),
    ("retry", &["taskId", "text", "files"]),
    ("cancel", &["taskId"]),
    ("wait", &["timeoutSeconds"]),
    ("review", &["taskId"]),
    ("finish", &[]),
    ("steer", &["taskId", "text"]),
    ("respond", &["taskId", "requestId", "decision"]),
    ("answer", &["taskId", "requestId", "answers", "skip"]),
];

/// `checkFields`.
pub fn check_fields(action: &str, input: &Map<String, Value>) -> Result<(), String> {
    let Some((_, allowed)) = FIELDS.iter().find(|(name, _)| *name == action) else {
        let names: Vec<&str> = FIELDS.iter().map(|(name, _)| *name).collect();
        return Err(format!(
            "Unknown action \"{action}\". Use one of: {}. Run the control CLI with --help.",
            names.join(", ")
        ));
    };
    let unknown: Vec<&str> = input
        .keys()
        .map(String::as_str)
        .filter(|key| !allowed.contains(key))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    let accepts = if allowed.is_empty() {
        format!("{action} takes no input.")
    } else {
        format!("{action} accepts: {}.", allowed.join(", "))
    };
    Err(format!(
        "Unknown {action} field{}: {}. {accepts}",
        if unknown.len() > 1 { "s" } else { "" },
        unknown.join(", ")
    ))
}

/// `questionAnswers`: map the lead's chosen option ids onto the worker's own
/// question shape.
pub fn question_answers(
    value: Option<&Value>,
    questions: &[UserQuestion],
) -> Result<QuestionAnswers, String> {
    let Some(entries) = value.and_then(Value::as_object) else {
        return Err(
            "answers must be an object of questionId -> [optionId], or pass {\"skip\":true}".into(),
        );
    };
    let mut answers = QuestionAnswers::new();
    for (id, chosen) in entries {
        let Some(question) = questions.iter().find(|entry| &entry.id == id) else {
            let ids: Vec<String> = questions.iter().map(|entry| entry.id.clone()).collect();
            let known = listed12(&ids);
            return Err(format!(
                "Unknown question \"{id}\". Ask for: {}.",
                if known.is_empty() { "none" } else { &known }
            ));
        };
        let ids = strings(Some(chosen), &format!("answers.{id}"), 16)?;
        let unknown: Vec<String> = ids
            .iter()
            .filter(|option| !question.options.iter().any(|entry| &entry.id == *option))
            .cloned()
            .collect();
        if !unknown.is_empty() {
            let options: Vec<String> = question
                .options
                .iter()
                .map(|entry| entry.id.clone())
                .collect();
            return Err(format!(
                "Unknown option{} for \"{id}\": {}. Choose from: {}.",
                if unknown.len() > 1 { "s" } else { "" },
                listed12(&unknown),
                listed12(&options)
            ));
        }
        answers.insert(id.clone(), ids);
    }
    if answers.is_empty() {
        return Err("Answer at least one question, or pass {\"skip\":true}".into());
    }
    Ok(answers)
}

/// `value.slice(-units)` measured in UTF-16 code units.
pub fn slice_tail(value: &str, units: usize) -> &str {
    let total = js::len(value);
    if total <= units {
        return value;
    }
    let mut skip = total - units;
    for (index, c) in value.char_indices() {
        if skip == 0 {
            return &value[index..];
        }
        skip = skip.saturating_sub(c.len_utf16());
    }
    ""
}

/// `Orchestrator.prompt`'s envelope: how the lead drives its run.
pub fn lead_prompt(prompt: &str, cli_path: &str) -> String {
    let cli = format!("{} control", shell_path(cli_path));
    format!(
        "{prompt}\n\n<monocode_orchestration>\nYou are the lead of a local MonoCode run. Coordinate the user's task using {cli}. Run `{cli} --help` before your first command; it documents every action, its exact JSON fields and the retry rule. Credentials are already in your environment; never print them.\nEach call prints one JSON line and exits non-zero unless \"ok\" is true; read the \"error\" text, it says what to do next. Unknown JSON fields are rejected rather than ignored, so fix the field name instead of guessing. If a call fails before reaching MonoCode, retry it with the \"requestId\" from that response so the work is never queued twice.\nUse list to discover allowed harness/model IDs. Delegate bounded tasks with project-relative files (directories reserve their descendants), self-contained prompts and dependsOn task IDs. Use the checkout selected for this run. You may read and plan; leave project file edits to workers. Never start workers outside this CLI. Workers with overlapping files are queued. For project-wide validation, generators or broad formatting, assign a separate task with files [\".\"] and wait for other workers to finish. Workers must never commit, push, switch branches or write outside the selected checkout. If the user requested those final operations, review and integrate every worker, call finish, then perform the explicitly authorized finalization yourself from the lead checkout.\nAgents never prompt the user. When one needs an approval or answers a question, list, get and wait report it as needsInput on that task, and you decide with respond or answer; it stays stopped until you do. Judge the request against the task you assigned, and put it to the user in this conversation only when the call is genuinely theirs.\nSteer a running agent with steer to correct its course without losing its work; use message only once it has stopped. Read results with get or wait; completed means a turn finished, not that the work passed review. Review the actual changes, message a worker for fixes, and use review to accept each completed task. A scope-blocked worker is isolated to that task: use message if it should stay within its existing scope, retry with corrected project-relative files if the assignment was too narrow, or cancel it if no longer needed. Never expand scope merely to excuse an unexpected write. Call finish only when required work and combined validation are complete. You receive worker results automatically when idle; use bounded wait calls while supervising. If the run is paused, list/get/wait remain readable and explain the reason. Stop polling, report that reason, and ask the user to click Resume; Resume automatically continues interrupted workers from their retained checkouts. Do not expose credentials, create worktrees, switch branches or silently escalate worker permissions.\n</monocode_orchestration>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // orchestration.test.ts: "worker assignment prompts"
    #[test]
    fn keeps_the_task_text_and_wraps_it_in_the_assignment_envelope() {
        let sent = worker_turn_prompt("Review the branch.", &["src/App.tsx".into()], None);
        assert!(sent.starts_with("Review the branch."));
        assert!(sent.contains("<monocode_assignment>"));
        assert!(sent.contains("src/App.tsx"));
        assert!(sent.contains("override any contradictory wording"));
        assert!(sent.contains("stage, commit, push"));
        assert!(sent.contains("Git finalization remains the lead's responsibility"));
        assert_eq!(visible_user_prompt(&sent), "Review the branch.");
    }

    #[test]
    fn names_the_scratch_directory_as_a_json_string() {
        let sent = worker_turn_prompt("Do it", &["a".into()], Some("/tmp/x y"));
        assert!(sent.contains("scratch directory: \"/tmp/x y\"."));
    }

    #[test]
    fn treats_directory_scopes_as_overlapping_only_at_path_boundaries() {
        let s = |v: &str| vec![v.to_string()];
        assert!(scopes_overlap(&s("/repo/src"), &s("/repo/src/file.ts")));
        assert!(!scopes_overlap(&s("/repo/src"), &s("/repo/src2/file.ts")));
        assert!(scopes_overlap(&s("/repo"), &s("/repo/anything")));
        assert!(scopes_overlap(&s("/"), &s("/repo/anything")));
    }

    #[test]
    fn compares_windows_canonical_and_provider_paths_as_the_same_scope() {
        let s = |v: &str| vec![v.to_string()];
        assert_eq!(
            orchestration_path_key("\\\\?\\D:\\Projects\\Repo\\src"),
            "d:/projects/repo/src"
        );
        assert_eq!(
            orchestration_path_key("\\\\?\\UNC\\Server\\Share\\Repo\\src"),
            "//server/share/repo/src"
        );
        assert!(scopes_overlap(
            &s("//?/d:/projects/repo/src"),
            &s("D:/Projects/Repo/src/file.ts")
        ));
        assert!(scopes_overlap(&s("//?/d:/"), &s("D:/Projects/Repo")));
    }

    #[test]
    fn preserves_case_for_posix_checkout_and_scope_identities() {
        assert_eq!(orchestration_path_key("/repo/Foo"), "/repo/Foo");
        assert!(!scopes_overlap(
            &["/repo/Foo".to_string()],
            &["/repo/foo/file.ts".to_string()]
        ));
        assert_eq!(
            orchestration_path_key("src/a/../b/file.ts"),
            "src/b/file.ts"
        );
        assert_eq!(orchestration_path_key(""), ".");
    }

    #[test]
    fn quotes_the_control_path_only_when_the_shell_needs_it() {
        assert_eq!(
            shell_path("/Applications/MonoCode.app/Contents/MacOS/monocode"),
            "/Applications/MonoCode.app/Contents/MacOS/monocode"
        );
        assert_eq!(shell_path("/Users/a b/MonoCode"), "'/Users/a b/MonoCode'");
        assert_eq!(
            shell_path("C:/Program Files/MonoCode/monocode.exe"),
            "\"C:/Program Files/MonoCode/monocode.exe\""
        );
        assert_eq!(
            shell_path("C:\\Tools\\monocode.exe"),
            "C:\\Tools\\monocode.exe"
        );
        // A backslash escapes in a POSIX shell, so bare would rewrite the path.
        assert_eq!(shell_path("/Users/a\\b/MonoCode"), "'/Users/a\\b/MonoCode'");
        assert_eq!(
            shell_path("/Users/it's/MonoCode"),
            "'/Users/it'\\''s/MonoCode'"
        );
    }

    #[test]
    fn rejects_unknown_actions_and_fields_by_name() {
        let input = |v: Value| v.as_object().unwrap().clone();
        assert_eq!(
            check_fields(
                "delegate",
                &input(json!({"title": "t", "depends_on": [], "modelId": "x"}))
            )
            .unwrap_err(),
            "Unknown delegate fields: depends_on, modelId. delegate accepts: title, harness, model, prompt, files, dependsOn."
        );
        assert!(
            check_fields("finish", &input(json!({"taskId": "x"})))
                .unwrap_err()
                .contains("finish takes no input")
        );
        for action in ["constructor", "toString", "__proto__"] {
            assert!(
                check_fields(action, &Map::new())
                    .unwrap_err()
                    .starts_with(&format!("Unknown action \"{action}\""))
            );
        }
    }

    #[test]
    fn validates_text_and_string_lists() {
        assert_eq!(text(Some(&json!("  hi ")), "title", 10).unwrap(), "hi");
        assert_eq!(
            text(Some(&json!(" ")), "title", 10).unwrap_err(),
            "Invalid title: provide a non-empty string"
        );
        assert_eq!(
            text(Some(&json!("abcdef")), "title", 3).unwrap_err(),
            "Invalid title: keep it under 3 characters"
        );
        assert_eq!(
            strings(Some(&json!(["a", " a", "b"])), "files", 64).unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );
        assert!(strings(Some(&json!("a")), "files", 64).is_err());
        assert_eq!(
            listed(&(0..14).map(|i| i.to_string()).collect::<Vec<_>>(), 12),
            "0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11 (+2 more)"
        );
    }

    #[test]
    fn slices_the_tail_in_utf16_units() {
        assert_eq!(slice_tail("abcdef", 3), "def");
        assert_eq!(slice_tail("ab", 3), "ab");
        assert_eq!(slice_tail("a😀b", 3), "😀b");
    }
}
