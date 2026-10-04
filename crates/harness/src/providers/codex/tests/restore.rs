//! Port of the "Codex Shell row recovery" cases in
//! src/features/sessions/data/sessionStore.test.ts.

use serde_json::{Value, json};

use monocode_core::block::Block;
use monocode_core::harness::HarnessId;
use monocode_core::reducer::apply_harness_events;
use monocode_core::session::Session;
use monocode_core::transcript::activity::tool_call_label;

use super::super::protocol::{backfill_codex_shell_commands, map_codex_notification};

fn blocks(value: Value) -> Vec<Block> {
    serde_json::from_value(value).unwrap()
}

fn shell_row(call_id: &str, saved_title: &str) -> Value {
    json!({
        "id": "shell",
        "role": "tool",
        "text": "Shell",
        "tool": {
            "callId": call_id,
            "title": "Shell",
            "kind": "execute",
            "preview": { "kind": "shell", "title": saved_title },
        },
    })
}

#[test]
fn relabels_from_the_command_saved_on_the_row_keeping_redactions() {
    // The command Codex sent with the item is already on the row as its
    // preview title. Reading it back keeps whatever Codex redacted redacted.
    let redacted = "/usr/bin/zsh -lc 'curl -H \"token=[redacted]\" example'";
    let repaired =
        backfill_codex_shell_commands(&blocks(json!([shell_row("exec-1", redacted)]))).unwrap();
    assert_ne!(repaired[0].text, "Shell");
    let preview = repaired[0].tool.as_ref().unwrap().preview.as_ref().unwrap();
    assert!(preview.title.as_deref().unwrap().contains("[redacted]"));
}

#[test]
fn leaves_a_row_with_no_usable_saved_command_as_it_is() {
    // A weak preview title names no command, so the row keeps its placeholder.
    assert_eq!(
        backfill_codex_shell_commands(&blocks(json!([shell_row("exec-1", "Shell")]))),
        None
    );
}

#[test]
fn labels_placeholder_rows_with_the_saved_command_and_rebuilds_the_preview() {
    let saved = blocks(json!([
        {
            "id": "shell",
            "role": "tool",
            "text": "Shell",
            "tool": {
                "callId": "exec-1",
                "title": "Shell",
                "kind": "execute",
                "status": "failed",
                "detail": "exit 1",
                "preview": {
                    "kind": "shell",
                    "title": "rg --files -g AGENTS.md -g '!node_modules'",
                },
            },
        },
        {
            "id": "read",
            "role": "tool",
            "text": "Read file.ts",
            "tool": { "callId": "exec-2", "kind": "read" },
        },
    ]));
    let repaired = backfill_codex_shell_commands(&saved).unwrap();
    assert_eq!(repaired[0].text, "Find files");
    let tool = repaired[0].tool.as_ref().unwrap();
    assert_eq!(tool.title.as_deref(), Some("Find files"));
    assert_eq!(tool.status.as_deref(), Some("failed"));
    assert_eq!(tool.detail.as_deref(), Some("exit 1"));
    let preview = serde_json::to_value(tool.preview.as_ref().unwrap()).unwrap();
    assert_eq!(preview["kind"], "shell");
    assert_eq!(
        preview["title"],
        "rg --files -g AGENTS.md -g '!node_modules'"
    );
    assert_eq!(repaired[1], saved[1]);
    assert_eq!(backfill_codex_shell_commands(&repaired), None);
}

#[test]
fn keeps_the_raw_command_when_no_readable_intent_is_inferred() {
    let repaired = backfill_codex_shell_commands(&blocks(json!([shell_row(
        "exec-3",
        "git commit -m 'Fix shell labels'"
    )])))
    .unwrap();
    assert_eq!(repaired[0].text, "git commit -m 'Fix shell labels'");
}

/// A row repaired from the saved preview has to read the same as one
/// rendered live, or reopening a session would relabel work the user already
/// saw.
#[test]
fn labels_a_recovered_row_exactly_as_the_live_item_does() {
    // Captured from `codex app-server`. The reported session's middle row was
    // `rg --files -g AGENTS.md`, which Codex labels a path-less `listFiles`.
    let item = json!({
        "type": "commandExecution",
        "id": "exec-88885872",
        "status": "inProgress",
        "command": "/usr/bin/zsh -lc \"rg --files -g AGENTS.md -g '\"'\"'!node_modules'\"'\"'\"",
        "commandActions": [
            { "type": "listFiles", "command": "rg --files -g AGENTS.md -g '!node_modules'", "path": null },
        ],
    });
    let cwd = "/home/me/proj";
    let live = Session::blank("s", HarnessId::Codex, "codex:gpt-5", cwd);
    let events = map_codex_notification("item/started", &json!({ "item": item })).events;
    let live = apply_harness_events(&live, &events);
    let live_row = &live.blocks[0];

    // The same row as the buggy build saved it. The command is already on the
    // row, which is how it reads in production.
    let mut saved = live_row.clone();
    saved.id = "e84ab067".into();
    saved.text = "Shell".into();
    saved.tool.as_mut().unwrap().title = Some("Shell".into());
    let recovered = backfill_codex_shell_commands(&[saved]).unwrap().remove(0);

    assert_ne!(recovered.text, "Shell");
    assert_eq!(recovered.text, live_row.text);
    assert_eq!(
        recovered.tool.as_ref().unwrap().title,
        live_row.tool.as_ref().unwrap().title
    );
    assert_eq!(
        tool_call_label(&recovered, Some(cwd)),
        tool_call_label(live_row, Some(cwd))
    );
}
