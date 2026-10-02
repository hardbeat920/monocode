//! Port of src/features/sessions/model/taskList.ts.

use serde_json::Value;

use crate::block::{TaskListItem, TaskListItemStatus};
use crate::js;

fn squash_separators(value: &str) -> String {
    value
        .chars()
        .filter(|c| !(js::is_space(*c) || *c == '_' || *c == '-'))
        .collect()
}

/// `isTaskListToolName`: TodoWrite, write_todos, update-todos, and names
/// ending in one of those.
pub fn is_task_list_tool_name(value: &str) -> bool {
    let name = squash_separators(&js::trim(value).to_lowercase());
    ["todowrite", "writetodos", "updatetodos"]
        .iter()
        .any(|candidate| name == *candidate || name.ends_with(candidate))
}

/// `taskListFromToolInput`: the common todo-write payload used by Claude,
/// OpenCode, and Pi extensions.
pub fn task_list_from_tool_input(tool_name: &str, input: &Value) -> Option<Vec<TaskListItem>> {
    if !is_task_list_tool_name(tool_name) {
        return None;
    }
    let todos = input.as_object()?.get("todos")?.as_array()?;
    Some(
        todos
            .iter()
            .filter_map(|todo| {
                let row = todo.as_object();
                let text = ["content", "activeForm", "text"]
                    .into_iter()
                    .find_map(|key| {
                        let text = row?.get(key)?.as_str()?;
                        let trimmed = js::trim(text);
                        (!trimmed.is_empty()).then(|| trimmed.to_string())
                    })?;
                Some(TaskListItem {
                    id: row.and_then(|row| task_list_item_id(row.get("id"))),
                    text,
                    status: normalize_task_list_status(row.and_then(|row| row.get("status"))),
                    extra: Default::default(),
                })
            })
            .collect(),
    )
}

/// `normalizeTaskListStatus`: provider status spellings to one of four.
pub fn normalize_task_list_status(value: Option<&Value>) -> TaskListItemStatus {
    let raw = match value {
        None | Some(Value::Null) => "pending".to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(n)) => js::number_to_string(n.as_f64().unwrap_or(0.0)),
        Some(_) => String::new(),
    };
    normalize_task_list_status_str(&raw)
}

/// `normalizeTaskListStatus` for a status that is already a string.
pub fn normalize_task_list_status_str(value: &str) -> TaskListItemStatus {
    let status = squash_separators(&js::trim(value).to_lowercase());
    match status.as_str() {
        "completed" | "complete" | "done" => TaskListItemStatus::Completed,
        "inprogress" | "active" | "running" => TaskListItemStatus::InProgress,
        "cancelled" | "canceled" | "skipped" => TaskListItemStatus::Cancelled,
        _ => TaskListItemStatus::Pending,
    }
}

/// `taskListText`: a searchable text form, such as "[x] Inspect".
pub fn task_list_text(items: &[TaskListItem]) -> String {
    items
        .iter()
        .map(|item| format!("{} {}", task_list_mark(item.status), item.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `legacyTaskListFromText`: read task snapshots persisted before task lists
/// had their own block role.
pub fn legacy_task_list_from_text(text: &str) -> Option<Vec<TaskListItem>> {
    let lines: Vec<&str> = split_lines(text)
        .into_iter()
        .map(js::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    lines.into_iter().map(legacy_line).collect()
}

/// Match `/^\[([xX ~…-])\]\s+(.+)$/` against a trimmed line.
fn legacy_line(line: &str) -> Option<TaskListItem> {
    let rest = line.strip_prefix('[')?;
    let mut chars = rest.chars();
    let mark = chars.next()?;
    if !matches!(mark, 'x' | 'X' | ' ' | '~' | '…' | '-') {
        return None;
    }
    let rest = chars.as_str().strip_prefix(']')?;
    let after_space = rest.strip_prefix(|c: char| js::is_space(c))?;
    let body = after_space.trim_start_matches(js::is_space);
    if body.is_empty() || body.chars().any(js::is_line_terminator) {
        return None;
    }
    Some(TaskListItem {
        id: None,
        text: js::trim(body).to_string(),
        status: status_from_legacy_mark(mark),
        extra: Default::default(),
    })
}

/// `taskListProgressLabel`: "Complete" or "1 of 2", not counting cancelled tasks.
pub fn task_list_progress_label(items: &[TaskListItem]) -> String {
    let completed = items
        .iter()
        .filter(|item| item.status == TaskListItemStatus::Completed)
        .count();
    let actionable = items
        .iter()
        .filter(|item| item.status != TaskListItemStatus::Cancelled)
        .count();
    if actionable > 0 && completed == actionable {
        return "Complete".into();
    }
    let total = if actionable > 0 {
        actionable
    } else {
        items.len()
    };
    format!("{completed} of {total}")
}

fn task_list_mark(status: TaskListItemStatus) -> &'static str {
    match status {
        TaskListItemStatus::Completed => "[x]",
        TaskListItemStatus::InProgress => "[~]",
        TaskListItemStatus::Cancelled => "[-]",
        TaskListItemStatus::Pending => "[ ]",
    }
}

fn status_from_legacy_mark(mark: char) -> TaskListItemStatus {
    match mark {
        'x' | 'X' => TaskListItemStatus::Completed,
        '~' | '…' => TaskListItemStatus::InProgress,
        '-' => TaskListItemStatus::Cancelled,
        _ => TaskListItemStatus::Pending,
    }
}

fn task_list_item_id(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => {
            let trimmed = js::trim(text);
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Number(n) => n.as_f64().map(js::number_to_string),
        _ => None,
    }
}

/// `text.split(/\r?\n/)`.
pub(crate) fn split_lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(id: Option<&str>, text: &str, status: TaskListItemStatus) -> TaskListItem {
        TaskListItem {
            id: id.map(str::to_string),
            text: text.into(),
            status,
            extra: Default::default(),
        }
    }

    #[test]
    fn normalizes_provider_status_spellings() {
        use TaskListItemStatus::*;
        assert_eq!(normalize_task_list_status_str("inProgress"), InProgress);
        assert_eq!(normalize_task_list_status_str("in_progress"), InProgress);
        assert_eq!(normalize_task_list_status_str("done"), Completed);
        assert_eq!(normalize_task_list_status_str("skipped"), Cancelled);
        assert_eq!(normalize_task_list_status_str("unknown"), Pending);
        assert_eq!(normalize_task_list_status(None), Pending);
    }

    #[test]
    fn normalizes_todo_write_tools_from_different_harnesses() {
        use TaskListItemStatus::*;
        assert!(is_task_list_tool_name("TodoWrite"));
        assert!(is_task_list_tool_name("update_todos"));
        assert!(!is_task_list_tool_name("edit"));
        assert_eq!(
            task_list_from_tool_input(
                "todowrite",
                &json!({
                    "todos": [
                        { "id": "inspect", "content": "Inspect", "status": "completed" },
                        { "id": 2, "activeForm": "Implementing", "status": "inProgress" },
                        { "text": "Verify", "status": "pending" }
                    ]
                })
            ),
            Some(vec![
                item(Some("inspect"), "Inspect", Completed),
                item(Some("2"), "Implementing", InProgress),
                item(None, "Verify", Pending),
            ])
        );
        assert_eq!(
            task_list_from_tool_input("edit", &json!({ "todos": [] })),
            None
        );
    }

    #[test]
    fn keeps_a_searchable_text_representation_and_reads_legacy_snapshots() {
        use TaskListItemStatus::*;
        let items = vec![
            item(None, "Inspect", Completed),
            item(None, "Implement", InProgress),
            item(None, "Verify", Pending),
        ];
        let text = task_list_text(&items);
        assert_eq!(text, "[x] Inspect\n[~] Implement\n[ ] Verify");
        assert_eq!(legacy_task_list_from_text(&text), Some(items));
        assert_eq!(
            legacy_task_list_from_text("## Plan\n\n- Implement it"),
            None
        );
    }

    #[test]
    fn summarizes_progress_without_counting_cancelled_tasks() {
        use TaskListItemStatus::*;
        assert_eq!(
            task_list_progress_label(&[item(None, "One", Completed), item(None, "Two", Cancelled)]),
            "Complete"
        );
        assert_eq!(
            task_list_progress_label(&[item(None, "One", Completed), item(None, "Two", Pending)]),
            "1 of 2"
        );
    }
}
