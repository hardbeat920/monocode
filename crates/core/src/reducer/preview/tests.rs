//! Port of src/integrations/harness/core/preview.test.ts.

use serde_json::{Value, json};

use super::*;

fn rec(value: Value) -> Record {
    match value {
        Value::Object(rec) => rec,
        other => panic!("not an object: {other}"),
    }
}

/// `toolKindFromName` from the Claude adapter, for the tool names these tests use.
fn tool_kind_from_name(name: &str) -> String {
    let normalized = name.to_lowercase();
    let has = |needle: &str| normalized.contains(needle);
    if has("bash") || has("command") || has("shell") || has("terminal") {
        return "execute".into();
    }
    if has("edit") || has("write") || has("patch") || has("replace") {
        return "edit".into();
    }
    if has("read") {
        return "read".into();
    }
    if has("grep") || has("glob") || has("search") {
        return "search".into();
    }
    if normalized == "skill" || normalized == "skills" {
        return "skill".into();
    }
    if is_agent_tool_name(name) {
        return "agent".into();
    }
    name.into()
}

/// `previewFromTool` from the Claude adapter.
pub(crate) fn preview_from_tool(
    name: &str,
    input: Value,
    output: Option<&str>,
) -> Option<ToolPreview> {
    let kind = tool_kind_from_name(name);
    let mut update = json!({
        "title": name, "name": name, "kind": kind, "input": input, "rawInput": input,
    });
    if let Some(output) = output {
        update["content"] = json!(output);
    }
    let tool = json!({ "title": name, "name": name, "kind": kind, "rawInput": input });
    extract_tool_preview(&rec(update), &rec(tool))
}

fn texts(preview: &ToolPreview) -> Vec<(ToolPreviewLineKind, &str)> {
    preview
        .lines
        .as_ref()
        .unwrap()
        .iter()
        .map(|line| (line.kind, line.text.as_str()))
        .collect()
}

fn title(opts: ToolTitleInput<'_>) -> String {
    compose_tool_title(&opts)
}

// tool input change previews
#[test]
fn compares_claude_edit_excerpts_outside_the_workspace_without_inventing_file_line_numbers() {
    let preview = preview_from_tool(
        "Edit",
        json!({
            "file_path": "/Users/me/Documents/notes.md",
            "old_string": "  before\nkeep\n",
            "new_string": "  after\nkeep\n",
        }),
        None,
    )
    .unwrap();
    assert_eq!(preview.kind, ToolPreviewKind::Write);
    assert_eq!(
        preview.path.as_deref(),
        Some("/Users/me/Documents/notes.md")
    );
    assert_eq!((preview.additions, preview.deletions), (Some(1), Some(1)));
    use ToolPreviewLineKind::*;
    assert_eq!(
        texts(&preview),
        [(Del, "  before"), (Add, "  after"), (Context, "keep")]
    );
    assert!(
        preview
            .lines
            .unwrap()
            .iter()
            .all(|line| line.number.is_none())
    );
}

#[test]
fn preserves_whitespace_only_replacements_insertions_and_deletions() {
    for (before, after, additions, deletions) in [
        ("  ", "    ", 1, 1),
        ("", "new\n", 1, 0),
        ("old\n", "", 0, 1),
        ("same\n", "same\n", 0, 0),
    ] {
        let preview = preview_from_tool(
            "Edit",
            json!({ "file_path": "notes.md", "old_string": before, "new_string": after }),
            None,
        )
        .unwrap();
        assert_eq!(
            (preview.additions, preview.deletions),
            (Some(additions), Some(deletions)),
            "{before:?} -> {after:?}"
        );
    }
}

#[test]
fn reads_complete_nested_replacement_inputs_but_waits_for_both_strings() {
    let preview = extract_tool_preview(
        &rec(json!({
            "kind": "edit",
            "args": { "input": { "path": "a.ts", "oldText": "old", "newText": "new" } },
        })),
        &Record::new(),
    )
    .unwrap();
    assert_eq!((preview.additions, preview.deletions), (Some(1), Some(1)));
    let partial = preview_from_tool(
        "Edit",
        json!({ "file_path": "a.ts", "old_string": "old" }),
        None,
    );
    assert!(partial.unwrap().lines.is_none());
}

#[test]
fn finds_edits_after_long_unchanged_prefixes_and_counts_beyond_the_preview() {
    let prefix = (0..900)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let added = vec!["added"; 20].join("\n");
    let preview = preview_from_tool(
        "Edit",
        json!({
            "file_path": "long.md",
            "old_string": prefix,
            "new_string": format!("{prefix}\n{added}"),
        }),
        None,
    )
    .unwrap();
    assert_eq!((preview.additions, preview.deletions), (Some(20), Some(0)));
    let lines = preview.lines.unwrap();
    assert_eq!(lines.len(), MAX_PREVIEW_LINES);
    assert!(lines.iter().any(|line| line.text == "added"));
}

#[test]
fn shows_write_contents_without_claiming_to_know_the_previous_file() {
    let preview = preview_from_tool(
        "Write",
        json!({ "file_path": "notes.md", "content": "  heading\n\nbody\n" }),
        Some("File successfully written"),
    )
    .unwrap();
    assert_eq!(preview.content_only, Some(true));
    let lines: Vec<_> = preview
        .lines
        .as_ref()
        .unwrap()
        .iter()
        .map(|line| (line.number, line.kind, line.text.as_str()))
        .collect();
    use ToolPreviewLineKind::Context;
    assert_eq!(
        lines,
        [
            (Some(1), Context, "  heading"),
            (Some(2), Context, ""),
            (Some(3), Context, "body")
        ]
    );
    assert_eq!(preview.additions, None);
    assert_eq!(preview.deletions, None);
    let empty = preview_from_tool(
        "Write",
        json!({ "file_path": "notes.md", "content": "" }),
        None,
    );
    assert_eq!(
        merge_tool_preview(empty.as_ref(), Some(&preview))
            .unwrap()
            .lines,
        Some(vec![])
    );
    let read = preview_from_tool("Read", json!({ "file_path": "notes.md" }), Some("contents"));
    assert!(read.unwrap().lines.is_none());
}

#[test]
fn prefers_provider_diffs_and_preserves_indentation_in_them() {
    let preview = extract_tool_preview(
        &rec(json!({
            "kind": "edit",
            "input": { "old_string": "wrong", "new_string": "fallback" },
            "content": [{ "type": "diff", "path": "a.ts", "oldText": "  old\n", "newText": "  new\n" }],
        })),
        &Record::new(),
    )
    .unwrap();
    let lines: Vec<_> = texts(&preview).into_iter().map(|(_, text)| text).collect();
    assert_eq!(lines, ["  old", "  new"]);
}

#[test]
fn parses_git_patches_from_provider_diffs() {
    let preview = extract_tool_preview(
        &rec(json!({
            "kind": "edit",
            "content": [{
                "type": "diff",
                "patch": "diff --git a/src/a.ts b/src/a.ts\r\n--- a/src/a.ts\r\n+++ b/src/a.ts\r\n@@ -1,2 +1,2 @@\r\n keep\r\n-old\r\n+new\r\n",
            }],
        })),
        &Record::new(),
    )
    .unwrap();
    assert_eq!(preview.path.as_deref(), Some("src/a.ts"));
    assert_eq!(preview.file_name.as_deref(), Some("a.ts"));
    assert_eq!((preview.additions, preview.deletions), (Some(1), Some(1)));
    // coerceString trims the patch, so the final newline adds no context line.
    let lines: Vec<_> = preview
        .lines
        .unwrap()
        .into_iter()
        .map(|line| (line.number, line.kind, line.text))
        .collect();
    use ToolPreviewLineKind::*;
    assert_eq!(
        lines,
        [
            (Some(1), Context, "keep".to_string()),
            (Some(2), Del, "old".to_string()),
            (Some(2), Add, "new".to_string()),
        ]
    );
}

// extractToolPreview
#[test]
fn reads_nested_args_bags_from_acp_tool_calls() {
    let preview = extract_tool_preview(
        &rec(json!({
            "kind": "read", "title": "Read",
            "rawInput": { "args": { "path": "src/chrome/TitleBar.tsx" } },
        })),
        &Record::new(),
    )
    .unwrap();
    assert_eq!(preview.kind, ToolPreviewKind::Read);
    assert_eq!(preview.path.as_deref(), Some("src/chrome/TitleBar.tsx"));
    assert_eq!(preview.file_name.as_deref(), Some("TitleBar.tsx"));
    assert_eq!(
        title(ToolTitleInput {
            kind: Some("read"),
            title: Some("Read"),
            path: preview.path.as_deref(),
            preview_kind: Some(preview.kind),
            ..Default::default()
        }),
        "Read src/chrome/TitleBar.tsx"
    );
}

#[test]
fn accepts_relative_single_segment_paths() {
    let preview = extract_tool_preview(
        &rec(json!({ "kind": "read", "title": "Read", "rawInput": { "path": "README.md" } })),
        &Record::new(),
    );
    assert_eq!(preview.unwrap().path.as_deref(), Some("README.md"));
}

#[test]
fn does_not_treat_file_contents_as_a_path() {
    let preview = extract_tool_preview(
        &rec(
            json!({ "kind": "read", "title": "Read", "rawInput": { "path": "/** Structured language…" } }),
        ),
        &Record::new(),
    );
    assert_eq!(preview.unwrap().path, None);
}

#[test]
fn finds_search_queries_in_nested_input() {
    assert_eq!(
        extract_search_query(&json!([{ "arguments": { "pattern": "busyHarness|busy.*tab" } }]))
            .as_deref(),
        Some("busyHarness|busy.*tab")
    );
}

#[test]
fn uses_locations_when_raw_input_is_empty() {
    let preview = extract_tool_preview(
        &rec(json!({ "kind": "read", "title": "Read", "locations": [{ "path": "src/App.tsx" }] })),
        &Record::new(),
    );
    assert_eq!(preview.unwrap().path.as_deref(), Some("src/App.tsx"));
}

#[test]
fn treats_glob_pattern_as_a_find_query() {
    let preview = extract_tool_preview(
        &rec(json!({
            "kind": "search", "title": "Find", "name": "Glob",
            "rawInput": { "glob_pattern": "**/*.{json,md}" },
        })),
        &Record::new(),
    )
    .unwrap();
    assert_eq!(preview.kind, ToolPreviewKind::Search);
    assert_eq!(preview.query.as_deref(), Some("**/*.{json,md}"));
    assert_eq!(
        title(ToolTitleInput {
            kind: Some("search"),
            title: Some("Find"),
            query: preview.query.as_deref(),
            preview_kind: Some(preview.kind),
            ..Default::default()
        }),
        "Find **/*.{json,md}"
    );
}

#[test]
fn decodes_file_uris_and_reads_json_string_inputs() {
    let preview = extract_tool_preview(
        &rec(json!({
            "kind": "read", "title": "Read",
            "locations": [{ "uri": "file:///Users/me/My%20Notes/a.md", "line": 4 }],
        })),
        &Record::new(),
    )
    .unwrap();
    assert_eq!(preview.path.as_deref(), Some("/Users/me/My Notes/a.md"));
    assert_eq!(preview.start_line, Some(4));
    let parsed = extract_tool_preview(
        &rec(json!({ "kind": "read", "title": "Read", "arguments": "{\"path\":\"src/b.rs\",\"offset\":\"12\"}" })),
        &Record::new(),
    )
    .unwrap();
    assert_eq!(parsed.path.as_deref(), Some("src/b.rs"));
    assert_eq!(parsed.start_line, Some(12));
}

// shell command titles
#[test]
fn pulls_the_command_out_of_nested_input_bags() {
    assert_eq!(
        extract_shell_command(&[&json!({ "args": { "command": "git status -s" } })]).as_deref(),
        Some("git status -s")
    );
    assert_eq!(
        extract_shell_command(&[&json!({ "cmd": "pwd" })]).as_deref(),
        Some("pwd")
    );
    assert_eq!(
        extract_shell_command(&[&json!({ "command": ["npm", "test"] })]).as_deref(),
        Some("npm test")
    );
    assert_eq!(
        extract_shell_command(&[
            &json!({}),
            &json!({ "rawInput": {} }),
            &json!({ "input": { "command": "git status" } }),
        ])
        .as_deref(),
        Some("git status")
    );
}

#[test]
fn labels_execute_tools_with_the_command_not_the_tool_name() {
    assert_eq!(
        title(ToolTitleInput {
            kind: Some("execute"),
            title: Some("Bash"),
            command: Some("rm -rf /tmp/build"),
            ..Default::default()
        }),
        "rm -rf /tmp/build"
    );
    let from = |name: &str, input: Value| title_from_tool_input(name, "execute", &rec(input));
    assert_eq!(from("bash", json!({ "command": "ls -la" })), "ls -la");
    assert_eq!(
        from(
            "Bash",
            json!({ "description": "List files", "command": "ls -la" })
        ),
        "ls -la"
    );
    assert_eq!(
        from(
            "bash",
            json!({ "command": "cat src/lib/harness/preview.ts" })
        ),
        "Read src/lib/harness/preview.ts"
    );
    assert_eq!(
        from(
            "Bash",
            json!({ "command": "sed -n '713,1200p' src/surfaces/AgentTranscript.tsx" })
        ),
        "Read src/surfaces/AgentTranscript.tsx"
    );
    assert_eq!(
        from(
            "bash",
            json!({ "command": "grep -n \"isReadTool\" src/lib/harness/preview.ts" })
        ),
        "Find isReadTool"
    );
    assert_eq!(
        from(
            "bash",
            json!({ "command": "git diff src/surfaces/AgentTranscript.tsx" })
        ),
        "git diff src/surfaces/AgentTranscript.tsx"
    );
    assert_eq!(
        from("bash", json!({ "command": "sed -i 's/a/b/' src/app.ts" })),
        "Edit src/app.ts"
    );
    assert_eq!(
        from("bash", json!({ "command": "cat package.json > out.json" })),
        "Write out.json"
    );
}

#[test]
fn does_not_substitute_claudes_description_for_the_command() {
    assert_eq!(
        title_from_tool_input(
            "Bash",
            "execute",
            &rec(json!({ "description": "List files" }))
        ),
        "Shell"
    );
    assert_eq!(
        title_from_tool_input("bash", "execute", &Record::new()),
        "Shell"
    );
}

#[test]
fn treats_bash_as_a_weak_placeholder_so_a_later_command_can_replace_it() {
    assert!(is_weak_tool_title("Bash"));
    assert!(is_weak_tool_title("bash"));
    assert!(!is_weak_tool_title("ls"));
    assert!(is_weak_tool_title("MCP:  tool"));
}

// skill titles
#[test]
fn labels_a_skill_tool_with_slash_name() {
    assert_eq!(
        extract_skill_name(&[&json!({ "skill": "code-review" })]).as_deref(),
        Some("code-review")
    );
    assert_eq!(
        extract_skill_name(&[&json!({ "args": { "skill_name": "commit" } })]).as_deref(),
        Some("commit")
    );
    assert_eq!(
        extract_skill_name(&[
            &json!({}),
            &json!({ "rawInput": {} }),
            &json!({ "skill": "code-review" })
        ])
        .as_deref(),
        Some("code-review")
    );
    assert_eq!(
        title_from_tool_input("Skill", "skill", &rec(json!({ "skill": "code-review" }))),
        "Skill /code-review"
    );
    assert_eq!(
        title(ToolTitleInput {
            kind: Some("skill"),
            title: Some("Skill"),
            skill: Some("code-review"),
            ..Default::default()
        }),
        "Skill /code-review"
    );
}

#[test]
fn does_not_treat_a_bare_skill_placeholder_as_the_name() {
    assert_eq!(
        title_from_tool_input("Skill", "skill", &Record::new()),
        "Skill"
    );
    assert!(is_weak_tool_title("Skill"));
    assert!(!is_weak_tool_title("Skill /code-review"));
}

// agent titles
#[test]
fn prefers_the_description_then_the_subagent_type() {
    assert_eq!(
        title_from_tool_input(
            "Agent",
            "agent",
            &rec(json!({ "description": "Explore the auth module", "subagent_type": "explore" }))
        ),
        "Explore the auth module"
    );
    assert_eq!(
        title_from_tool_input("Task", "agent", &rec(json!({ "subagent_type": "explore" }))),
        "Explore subagent"
    );
    assert_eq!(
        title_from_tool_input("Agent", "agent", &Record::new()),
        "Subagent"
    );
    assert_eq!(format_agent_type("code_reviewer-2x"), "Code Reviewer 2x");
}

#[test]
fn labels_a_composed_agent_kind_as_subagent_when_the_title_is_a_placeholder() {
    assert_eq!(
        title(ToolTitleInput {
            kind: Some("agent"),
            title: Some("Task"),
            ..Default::default()
        }),
        "Subagent"
    );
    assert_eq!(
        title(ToolTitleInput {
            kind: Some("agent"),
            title: Some("Explore the auth module"),
            ..Default::default()
        }),
        "Explore the auth module"
    );
}

#[test]
fn composes_read_and_find_titles_from_bare_labels() {
    let read = |text| {
        title(ToolTitleInput {
            kind: Some("read"),
            title: Some(text),
            ..Default::default()
        })
    };
    assert_eq!(read("Reading file src/a.ts"), "Read src/a.ts");
    assert_eq!(read("Read files"), "Read files");
    assert_eq!(read("Reading"), "Read");
    assert_eq!(read("List ."), "List .");
    assert_eq!(
        title(ToolTitleInput {
            kind: Some("search"),
            title: Some("Searching TODO"),
            ..Default::default()
        }),
        "Find TODO"
    );
    assert_eq!(context_lines("a\r\nb\nc\n\n", Some(2)).len(), 2);
    assert_eq!(context_lines("", None)[0].number, Some(1));
}
