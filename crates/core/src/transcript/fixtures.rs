//! Synthetic blocks for tests and the gallery. Shapes follow the builders in
//! src/features/sessions/model/transcriptActivity.test.ts.

use std::sync::Arc;

use crate::block::{
    AgentRunMeta, AgentStep, AgentStepKind, BlockApproval, BlockTool, HandoffMeta, HandoffStatus,
    InterjectionMeta, TaskListItem, TaskListItemStatus, TaskListMeta, ToolPreview, ToolPreviewKind,
    ToolPreviewLine, ToolPreviewLineKind,
};
use crate::{Block, BlockRole, HarnessId};

use super::activity::BlockRef;

/// Share each block, the way the view holds a session's blocks.
pub fn refs(blocks: Vec<Block>) -> Vec<BlockRef> {
    blocks.into_iter().map(Arc::new).collect()
}

pub fn user(id: &str, text: &str) -> Block {
    Block::new(id, BlockRole::User, text)
}

pub fn note(id: &str, text: &str) -> Block {
    Block::new(id, BlockRole::Assistant, text)
}

pub fn thought(id: &str, text: &str) -> Block {
    Block::new(id, BlockRole::Reasoning, text)
}

pub fn status(id: &str, text: &str) -> Block {
    Block::new(id, BlockRole::System, text)
}

pub fn irc(id: &str, text: &str) -> Block {
    let mut block = Block::new(id, BlockRole::System, text);
    block.interjection = Some(InterjectionMeta {
        custom_type: "irc:incoming".into(),
        severity: None,
        extra: Default::default(),
    });
    block
}

/// A completed `bash ls` shell call.
pub fn shell(id: &str) -> Block {
    shell_status(id, "completed")
}

pub fn shell_status(id: &str, status: &str) -> Block {
    let mut block = Block::new(id, BlockRole::Tool, "bash ls");
    block.tool = Some(BlockTool {
        kind: Some("shell".into()),
        title: Some("bash ls".into()),
        status: Some(status.into()),
        ..Default::default()
    });
    block
}

/// A shell call running `command`.
pub fn command(id: &str, command: &str, status: &str) -> Block {
    let mut block = Block::new(id, BlockRole::Tool, command);
    block.tool = Some(BlockTool {
        kind: Some("execute".into()),
        title: Some(command.into()),
        status: Some(status.into()),
        ..Default::default()
    });
    block
}

pub fn with_approval(mut block: Block, request_id: i64) -> Block {
    block.approval = Some(BlockApproval {
        request_id,
        decided: None,
        extra: Default::default(),
    });
    block
}

fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

pub fn edit(id: &str, path: &str) -> Block {
    let text = format!("Edited {path}");
    let mut preview = ToolPreview::new(ToolPreviewKind::Write);
    preview.path = Some(path.into());
    preview.file_name = Some(file_name(path));
    let mut block = Block::new(id, BlockRole::Tool, &text);
    block.tool = Some(BlockTool {
        kind: Some("edit".into()),
        title: Some(text),
        status: Some("completed".into()),
        preview: Some(preview),
        ..Default::default()
    });
    block
}

/// An edit with diff lines and counts, like a provider's write preview.
pub fn edit_with_diff(id: &str, path: &str, lines: &[(ToolPreviewLineKind, i64, &str)]) -> Block {
    let mut block = edit(id, path);
    let preview = block
        .tool
        .as_mut()
        .and_then(|tool| tool.preview.as_mut())
        .expect("edit preview");
    preview.additions = Some(
        lines
            .iter()
            .filter(|(kind, ..)| *kind == ToolPreviewLineKind::Add)
            .count() as i64,
    );
    preview.deletions = Some(
        lines
            .iter()
            .filter(|(kind, ..)| *kind == ToolPreviewLineKind::Del)
            .count() as i64,
    );
    preview.lines = Some(
        lines
            .iter()
            .map(|(kind, number, text)| ToolPreviewLine {
                number: Some(*number),
                kind: *kind,
                text: (*text).into(),
                extra: Default::default(),
            })
            .collect(),
    );
    block
}

pub fn read(id: &str, path: &str) -> Block {
    let text = format!("Read {path}");
    let mut preview = ToolPreview::new(ToolPreviewKind::Read);
    preview.path = Some(path.into());
    preview.file_name = Some(file_name(path));
    let mut block = Block::new(id, BlockRole::Tool, &text);
    block.tool = Some(BlockTool {
        kind: Some("read".into()),
        title: Some(text),
        status: Some("completed".into()),
        preview: Some(preview),
        ..Default::default()
    });
    block
}

pub fn search(id: &str, query: &str) -> Block {
    let text = format!("Find {query}");
    let mut preview = ToolPreview::new(ToolPreviewKind::Search);
    preview.query = Some(query.into());
    let mut block = Block::new(id, BlockRole::Tool, &text);
    block.tool = Some(BlockTool {
        kind: Some("search".into()),
        title: Some(text),
        status: Some("completed".into()),
        preview: Some(preview),
        ..Default::default()
    });
    block
}

/// A delegated run named `name`.
pub fn agent(id: &str, name: &str, status: &str) -> Block {
    let mut block = Block::new(id, BlockRole::Tool, name);
    block.tool = Some(BlockTool {
        kind: Some("agent".into()),
        title: Some(name.into()),
        status: Some(status.into()),
        ..Default::default()
    });
    block
}

/// A delegated run with a trail of steps.
pub fn agent_run(id: &str, name: &str, status: &str, steps: Vec<AgentStep>) -> Block {
    let mut block = agent(id, name, status);
    block.agent_run = Some(AgentRunMeta {
        name: name.into(),
        steps,
        ..Default::default()
    });
    block
}

pub fn step(id: &str, kind: AgentStepKind, text: &str, status: Option<&str>) -> AgentStep {
    AgentStep {
        id: id.into(),
        kind,
        text: text.into(),
        tool_kind: None,
        status: status.map(str::to_string),
        detail: None,
        preview: None,
        extra: Default::default(),
    }
}

pub fn handoff(id: &str) -> Block {
    let mut block = Block::new(id, BlockRole::Handoff, "Goal: go");
    block.handoff = Some(HandoffMeta {
        from: HarnessId::Cursor,
        to: HarnessId::Claude,
        status: HandoffStatus::Ready,
        pending: None,
        extra: Default::default(),
    });
    block
}

pub fn tasks_block(id: &str) -> Block {
    let mut block = Block::new(id, BlockRole::Tasks, "[~] Implement");
    block.task_list = Some(TaskListMeta {
        items: vec![
            TaskListItem {
                id: None,
                text: "inspect".into(),
                status: TaskListItemStatus::Completed,
                extra: Default::default(),
            },
            TaskListItem {
                id: None,
                text: "implement".into(),
                status: TaskListItemStatus::InProgress,
                extra: Default::default(),
            },
        ],
        ..Default::default()
    });
    block
}

/// A finished user turn that started at `started_at` and took `duration_ms`.
pub fn timed_user(id: &str, text: &str, started_at: i64, duration_ms: i64) -> Block {
    let mut block = user(id, text);
    block.started_at = Some(started_at);
    block.duration_ms = Some(duration_ms);
    block
}
