//! The phone's transcript (mobileTranscript.ts): turns the desktop has
//! already grouped and folded as `TranscriptView` does, so the phone and the
//! desktop follow one set of rules. Pure: rows and turn provenance in, the
//! phone's `Turn` JSON (mobile/src/features/workspace/types.ts) out.
//!
//! Known gaps against the React port, because native rows do not carry them:
//! approvals are not persisted, so no "Waiting for approval on desktop" notice
//! (the live header still says `waiting`); there are no Mono habits, operator
//! prompts or background-task labels; and assignment wrappers stay in user
//! text, as the desktop shows them.
use crate::transcript::{self as desk, attachments, dates, phases};
use monocode_core::{
    agent::TurnInfo,
    tool::{
        ToolCall, ToolState,
        tasks::{Panel, TaskStatus},
    },
    transcript::{Interjection, Row, attachments::ROLE as ATTACHMENTS},
};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

const MAX_STEPS: usize = 40;
const MAX_MESSAGES: usize = 200;

pub(crate) struct Source<'a> {
    pub rows: &'a [Row],
    /// Provenance by turn: the first row after the turn's prompt block.
    pub turns: &'a HashMap<usize, TurnInfo>,
    pub cwd: &'a str,
    /// Provider id, e.g. "claude".
    pub harness: &'a str,
    /// The current model's display name.
    pub model_name: Option<String>,
    pub busy: bool,
    /// "Waiting for approval" or "Waiting for answers", for the live header.
    pub waiting: Option<&'a str>,
    /// The Mono's display name, for a Mono conversation (inline work).
    pub mono: Option<&'a str>,
    /// Approvals and questions the live chat waits on. Rows never record
    /// them, so they come from the conversation and end the latest turn.
    pub pending: &'a [Pending],
}

/// An approval or question waiting on the desktop.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pending {
    pub id: String,
    pub question: bool,
    pub label: String,
}

/// One prompt block and the response under it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Run {
    /// The prompt block's first row.
    prompt: usize,
    /// The first row after the prompt block: the `turns` key.
    key: usize,
    end: usize,
}

/// A turn as the phone shows it: one run, or a Mono's run and the completion
/// runs that continue it (groupMonoChatTurns).
struct Turn {
    runs: Vec<Run>,
}
impl Turn {
    fn id(&self) -> String {
        format!("t{}", self.runs[0].prompt)
    }
}

fn message_end(rows: &[Row], at: usize) -> usize {
    (at + 1..rows.len())
        .find(|&i| rows[i].first)
        .unwrap_or(rows.len())
}
fn joined(rows: &[Row]) -> String {
    rows.iter().map(|r| r.text.as_str()).collect()
}
fn readable(rows: &[Row]) -> bool {
    rows.iter().any(|r| !r.text.trim().is_empty())
}
/// Each message's first row and end, in order.
fn messages_in(rows: &[Row], from: usize, to: usize) -> impl Iterator<Item = (usize, usize)> {
    let mut at = from;
    std::iter::from_fn(move || {
        (at < to).then(|| {
            let end = message_end(rows, at).min(to);
            let message = (at, end);
            at = end;
            message
        })
    })
}

fn runs(rows: &[Row]) -> Vec<Run> {
    let next_prompt = |from: usize| {
        (from..rows.len())
            .find(|&i| rows[i].is_prompt())
            .unwrap_or(rows.len())
    };
    let mut out = Vec::new();
    let mut at = 0;
    if rows.first().is_some_and(|r| !r.is_prompt()) {
        at = next_prompt(0);
        out.push(Run {
            prompt: 0,
            key: 0,
            end: at,
        });
    }
    while at < rows.len() {
        let key = (at..rows.len())
            .find(|&i| !rows[i].is_prompt())
            .unwrap_or(rows.len());
        let end = next_prompt(key);
        out.push(Run {
            prompt: at,
            key,
            end,
        });
        at = end;
    }
    out
}

fn turns(source: &Source) -> Vec<Turn> {
    let mut out: Vec<Turn> = Vec::new();
    for run in runs(source.rows) {
        let continues = source.mono.is_some()
            && !out.is_empty()
            && desk::mono_continues(source.rows, run.key).is_some();
        match out.last_mut() {
            Some(turn) if continues => turn.runs.push(run),
            _ => out.push(Turn { runs: vec![run] }),
        }
    }
    out
}

/// Every turn's stable ID in order. An ID is its first row's index, which
/// appends never move.
pub(crate) fn turn_ids(source: &Source) -> Vec<String> {
    turns(source).iter().map(Turn::id).collect()
}

/// turns[range] as the phone's `Turn` objects.
pub(crate) fn build(source: &Source, range: std::ops::Range<usize>) -> Vec<Value> {
    let turns = turns(source);
    let end = range.end.min(turns.len());
    let start = range.start.min(end);
    (start..end)
        .map(|index| {
            let live = source.busy && index + 1 == turns.len();
            let mut turn = if let Some(name) = source.mono {
                mono_turn(source, &turns[index], live, name)
            } else {
                agent_turn(source, &turns[index], live)
            };
            if index + 1 == turns.len()
                && let Some(items) = turn["items"].as_array_mut()
            {
                items.extend(source.pending.iter().map(|pending| {
                    let what = if pending.question {
                        "Question waiting on desktop"
                    } else {
                        "Waiting for approval on desktop"
                    };
                    json!({
                        "type": "notice",
                        "id": format!("input:{}", pending.id),
                        "text": format!("{what}: {}", pending.label),
                    })
                }));
            }
            turn
        })
        .collect()
}

/// mobileMessages: the last 200 user and assistant messages.
pub(crate) fn messages(rows: &[Row]) -> Vec<Value> {
    let mut out: Vec<Value> = messages_in(rows, 0, rows.len())
        .filter(|&(at, _)| matches!(rows[at].role.as_str(), "user" | "assistant"))
        .map(|(at, end)| json!({"id": format!("r{at}"), "role": rows[at].role, "text": joined(&rows[at..end])}))
        .collect();
    out.drain(..out.len().saturating_sub(MAX_MESSAGES));
    out
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
fn put(map: &mut Map<String, Value>, key: &str, value: Option<impl Into<Value>>) {
    if let Some(value) = value {
        map.insert(key.into(), value.into());
    }
}

/// The user bubbles of a prompt block; `mono-system` prompts are the app's.
fn user_items(rows: &[Row], run: Run, out: &mut Vec<Value>, by_row: &mut HashMap<usize, usize>) {
    for (at, end) in messages_in(rows, run.prompt, run.key) {
        if rows[at].role == monocode_core::shell_run::ROLE {
            if let Some(shell) = monocode_core::shell_run::ShellRun::parse(&rows[at].text) {
                by_row.insert(at, out.len());
                let mut item = object(
                    json!({"type": "user", "id": format!("r{at}"), "text": format!("$ {}", shell.command), "shell": true}),
                );
                put(&mut item, "at", rows[at].sent_at);
                out.push(Value::Object(item));
            }
            continue;
        }
        if rows[at].role != "user" {
            continue;
        }
        let raw = joined(&rows[at..end]);
        let (shown, files) = attachments::message(rows, end, &raw);
        let text = shown.unwrap_or(raw).trim_end().to_owned();
        if text.trim().is_empty() && files.is_empty() {
            continue;
        }
        let mut item = object(json!({"type": "user", "id": format!("r{at}"), "text": text}));
        put(&mut item, "at", rows[at].sent_at);
        if !text.trim().is_empty() && desk::is_emoji_only(&text) {
            item.insert("emojiOnly".into(), true.into());
        }
        if !files.is_empty() {
            item.insert("attachments".into(), files.len().into());
        }
        by_row.insert(at, out.len());
        out.push(Value::Object(item));
    }
}

/// Messages stacked one after another share the last one's time.
fn compact(items: &mut [Value]) {
    for at in 1..items.len() {
        if items[at]["type"] == "user" && items[at - 1]["type"] == "user" {
            items[at - 1]["compact"] = true.into();
        }
    }
}

fn notice(rows: &[Row], at: usize, end: usize) -> Option<Value> {
    let text = joined(&rows[at..end]);
    let id = format!("r{at}");
    match rows[at].role.as_str() {
        // Older native versions saved a user's stop as an error; the desktop hides it.
        "error" if text.trim() == "Stopped" => None,
        "error" if !text.trim().is_empty() => {
            Some(json!({"type": "notice", "id": id, "text": text, "error": true}))
        }
        "interjection" => serde_json::from_str::<Interjection>(&text)
            .ok()
            .filter(|note| !note.text.trim().is_empty())
            .map(|note| json!({"type": "notice", "id": id, "text": note.text})),
        "visual" => {
            let title = serde_json::from_str::<monocode_core::visual::VisualDocument>(&text)
                .ok()
                .filter(|document| document.validate().is_ok())
                .map(|document| document.title)
                .unwrap_or_else(|| "Interactive visual".into());
            Some(
                json!({"type": "notice", "id": id, "text": format!("{title} · Open on desktop to interact")}),
            )
        }
        "system" if !text.trim().is_empty() => {
            Some(json!({"type": "notice", "id": id, "text": text}))
        }
        _ => None,
    }
}

fn tool_label(call: &ToolCall, cwd: &str) -> String {
    let shown = call.presentation_in(Some(cwd));
    let label = [shown.action, shown.target]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if label.is_empty() {
        "Working".into()
    } else {
        label
    }
}

/// toolCategory, by the action `activity::summary` sorts calls by.
fn work_kind(call: &ToolCall) -> &'static str {
    match call.summary().0.as_str() {
        "Read" | "List" | "Search" | "Find" => "research",
        "Edit" | "Write" => "edit",
        "Run" => "run",
        "Agent" => "agent",
        _ => "other",
    }
}

/// dominantCountedWorkKind: ties go to the kind that changed the most.
fn dominant_kind(calls: &[ToolCall]) -> &'static str {
    let mut best = ("think", 0);
    for kind in ["edit", "run", "agent", "research", "other"] {
        let count = calls.iter().filter(|call| work_kind(call) == kind).count();
        if count > best.1 {
            best = (kind, count);
        }
    }
    best.0
}

/// One phase as the desktop groups it (`phases::collect`), titled as its
/// header is (`activity::summary`). A lone step is its own row, not a group.
fn phase(
    source: &Source,
    activity: &phases::Phases,
    group: &phases::Group,
    active: bool,
    budget: &mut usize,
) -> Value {
    let rows = source.rows;
    let mut steps = Vec::new();
    for (at, end) in messages_in(rows, group.start, group.last + 1) {
        if activity.at(at).is_none_or(|g| g.start != group.start) {
            continue;
        }
        let text = joined(&rows[at..end]);
        let step = if rows[at].role == "tool" {
            let Ok(call) = serde_json::from_str::<ToolCall>(&text) else {
                continue;
            };
            let mut step =
                object(json!({"id": format!("r{at}"), "label": tool_label(&call, source.cwd)}));
            if matches!(call.state, ToolState::Failed | ToolState::Cancelled) {
                step.insert("failed".into(), true.into());
            }
            step
        } else {
            let summary = desk::tasks::prose_summary(&text);
            object(json!({
                "id": format!("r{at}"),
                "label": if summary.is_empty() { "Thinking".into() } else { summary },
            }))
        };
        steps.push(Value::Object(step));
    }
    let id = format!("p{}", group.start);
    let kind = dominant_kind(&group.calls);
    if steps.len() == 1 {
        let title = steps[0]["label"].clone();
        *budget = budget.saturating_sub(1);
        return json!({"id": id, "kind": kind, "title": title, "steps": []});
    }
    let title = if group.calls.is_empty() {
        if active { "Thinking…" } else { "Thought" }.to_owned()
    } else {
        crate::activity::summary(&group.calls, active).0
    };
    let keep = steps.len().min(*budget);
    *budget -= keep;
    steps.drain(..steps.len() - keep);
    json!({"id": id, "kind": kind, "title": title, "steps": steps})
}

/// A standalone call: a delegated run pinned outside the work.
fn lone_phase(source: &Source, at: usize, call: &ToolCall) -> Value {
    json!({
        "id": format!("p{at}"),
        "kind": work_kind(call),
        "title": tool_label(call, source.cwd),
        "steps": [],
    })
}

fn task_list(at: usize, call: &ToolCall) -> Option<Value> {
    let Some(Panel::Tasks(list)) = call.panel() else {
        return None;
    };
    (!list.items.is_empty()).then(|| {
        let text = list
            .items
            .iter()
            .map(|task| match task.status {
                TaskStatus::Completed => format!("- ~~{}~~", task.text),
                _ => format!("- {}", task.text),
            })
            .collect::<Vec<_>>()
            .join("\n");
        json!({"type": "text", "id": format!("r{at}"), "text": text})
    })
}

fn info<'a>(source: &'a Source, run: Run) -> Option<&'a TurnInfo> {
    source.turns.get(&run.key)
}
fn duration(source: &Source, run: Run) -> Option<u64> {
    info(source, run)
        .filter(|info| info.has_duration())
        .map(|info| info.duration_ms)
}
/// When the run started: its provenance, else the latest prompt it was sent with.
fn started_at(source: &Source, run: Run) -> Option<u64> {
    info(source, run)
        .and_then(|info| info.started_at)
        .or_else(|| {
            source.rows[run.prompt..run.key]
                .iter()
                .rev()
                .find_map(|row| row.sent_at.filter(|_| row.first))
        })
}
fn harness(source: &Source, run: Run) -> String {
    info(source, run)
        .map(|info| info.provider.trim())
        .filter(|provider| !provider.is_empty())
        .unwrap_or(source.harness)
        .to_owned()
}

/// A turn as `TranscriptView::rebuild` lays it out for an agent session: the
/// prompt, the work line, then the response with its work folded behind the
/// line once an answer follows it.
fn agent_turn(source: &Source, turn: &Turn, live: bool) -> Value {
    let rows = source.rows;
    let run = turn.runs[0];
    let mut items = Vec::new();
    user_items(rows, run, &mut items, &mut HashMap::new());
    let header_at = items.len();

    let tail = run.end == rows.len();
    let activity = phases::collect(&rows[..run.end], run.key, live && tail);
    let answer = (run.key..run.end)
        .rev()
        .find(|&i| rows[i].role == "assistant" && rows[i].first);
    let ready =
        answer.is_some_and(|answer| !live || readable(&rows[answer..message_end(rows, answer)]));
    let fold =
        answer.filter(|&answer| ready && rows[run.key..answer].iter().any(|r| r.role == "tool"));
    let active = |group: &phases::Group| {
        live && (group.end == rows.len() || (answer == Some(group.end) && !ready))
    };

    let mut entries = Vec::new();
    let mut fold_budget = MAX_STEPS;
    let mut work: Option<(usize, Vec<Value>, bool, usize)> = None;
    let mut responded = false;
    let close = |work: &mut Option<(usize, Vec<Value>, bool, usize)>, items: &mut Vec<Value>| {
        if let Some((at, phases, live, _)) = work.take() {
            items.push(
                json!({"type": "work", "id": format!("w{at}"), "live": live, "phases": phases}),
            );
        }
    };
    for (at, end) in messages_in(rows, run.key, run.end) {
        let role = rows[at].role.as_str();
        if matches!(
            role,
            "mono-card" | "mono-document" | "mono-note" | "mono-launch"
        ) || role == ATTACHMENTS
            || activity.hidden.contains(&at)
        {
            continue;
        }
        responded = true;
        if role == "visual" {
            close(&mut work, &mut items);
            if let Some(notice) = notice(rows, at, end) {
                items.push(notice);
            }
            continue;
        }
        let group = activity.at(at);
        if fold.is_some_and(|answer| at < answer) && !activity.pinned.contains(&at) {
            if let Some(group) = group.filter(|g| g.start == at) {
                let mut entry = object(phase(source, &activity, group, false, &mut fold_budget));
                entry.insert("type".into(), "phase".into());
                entries.push(Value::Object(entry));
            } else if role == "assistant" && readable(&rows[at..end]) {
                entries.push(json!({"type": "prose", "id": format!("r{at}"), "text": joined(&rows[at..end])}));
            }
            continue;
        }
        if let Some(group) = group {
            if group.start == at {
                let active = active(group);
                let (_, phases, live, budget) =
                    work.get_or_insert_with(|| (at, Vec::new(), false, MAX_STEPS));
                phases.push(phase(source, &activity, group, active, budget));
                *live |= active;
            }
            continue;
        }
        if role == "tool" {
            let Ok(call) = serde_json::from_str::<ToolCall>(&joined(&rows[at..end])) else {
                continue;
            };
            if let Some(list) = task_list(at, &call) {
                close(&mut work, &mut items);
                items.push(list);
            } else if !matches!(call.panel(), Some(Panel::Tasks(_))) {
                let (_, phases, _, _) =
                    work.get_or_insert_with(|| (at, Vec::new(), false, MAX_STEPS));
                phases.push(lone_phase(source, at, &call));
            }
            continue;
        }
        close(&mut work, &mut items);
        if role == "assistant" {
            if readable(&rows[at..end]) {
                let mut item = object(
                    json!({"type": "text", "id": format!("r{at}"), "text": joined(&rows[at..end])}),
                );
                if live && end == rows.len() {
                    item.insert("streaming".into(), true.into());
                }
                items.push(Value::Object(item));
            }
        } else if let Some(notice) = notice(rows, at, end) {
            items.push(notice);
        }
    }
    close(&mut work, &mut items);

    let info = info(source, run);
    let started = started_at(source, run);
    let duration = duration(source, run).filter(|_| !live);
    if live || responded {
        let model = info
            .map(|info| info.model.trim().to_owned())
            .filter(|model| !model.is_empty())
            .or_else(|| source.model_name.clone().filter(|_| live));
        let mut header = object(json!({
            "type": "header",
            "id": format!("{}:header", turn.id()),
            "live": live,
            "harness": harness(source, run),
            "entries": entries,
        }));
        put(&mut header, "startedAt", started);
        put(&mut header, "durationMs", duration);
        put(&mut header, "model", model);
        if live {
            put(&mut header, "waiting", source.waiting);
        }
        items.insert(header_at, Value::Object(header));
    }
    compact(&mut items);

    let mut out = object(json!({"id": turn.id(), "items": items}));
    if !live && responded {
        out.insert("footer".into(), true.into());
        put(
            &mut out,
            "completedAt",
            info.and_then(|info| info.completed_at)
                .or_else(|| started.map(|at| at + duration.unwrap_or(0))),
        );
        out.insert(
            "copyText".into(),
            monocode_core::response::prose(&rows[run.prompt..run.end]).into(),
        );
        put(&mut out, "durationMs", duration);
        if let Some(metrics) = info.and_then(|info| info.metrics).filter(|m| m.reported()) {
            let mut value = object(json!({
                "inputTokens": metrics.input_tokens,
                "outputTokens": metrics.output_tokens,
                "cacheReadTokens": metrics.cache_read_tokens,
                "cacheWriteTokens": metrics.cache_write_tokens,
            }));
            put(&mut value, "cacheHitPercent", metrics.cache_hit_percent);
            out.insert("metrics".into(), Value::Object(value));
        }
    }
    Value::Object(out)
}

fn reaction(text: &str) -> Option<&str> {
    text.trim()
        .strip_prefix("<reaction>")
        .and_then(|s| s.strip_suffix("</reaction>"))
        .filter(|emoji| emoji.chars().count() <= 16)
}

/// A Mono's turn, as its chat shows it: the user's messages, the Mono's
/// signature line, and the prose it settled on; its process stays in the
/// signature's live status.
fn mono_turn(source: &Source, turn: &Turn, live: bool, name: &str) -> Value {
    let rows = source.rows;
    let mut items = Vec::new();
    let mut by_row = HashMap::new();
    let mut reacted = false;
    let mut standalone = true;
    let mut copy = Vec::new();
    for &run in &turn.runs {
        let before = items.len();
        user_items(rows, run, &mut items, &mut by_row);
        standalone &= items.len() == before;
        let run_live = live && run.end == rows.len();
        let internal = run.key > run.prompt && rows[run.key - 1].role == "mono-system";
        for (at, end) in messages_in(rows, run.key, run.end) {
            let role = rows[at].role.as_str();
            let text = joined(&rows[at..end]);
            if role == "assistant" {
                if let Some(emoji) = reaction(&text) {
                    // MonoReactionBadge: the emoji sits on the message it answers.
                    let prompt = rows[..at]
                        .iter()
                        .rposition(Row::is_prompt)
                        .and_then(|p| rows[..=p].iter().rposition(|r| r.first));
                    if let Some(&item) = prompt.and_then(|p| by_row.get(&p)) {
                        items[item]["reaction"] = emoji.into();
                        reacted = true;
                    }
                    continue;
                }
                if !readable(&rows[at..end]) {
                    continue;
                }
                // groupMonoTurnItems: prose said while working stays in the
                // activity trail, as does all of it while the run is live.
                let hidden = run_live
                    || rows[at + 1..run.end].iter().any(|r| {
                        r.first
                            && (matches!(r.role.as_str(), "tool" | "reasoning")
                                || (internal && r.role == "assistant"))
                    });
                if hidden {
                    continue;
                }
                let mut item =
                    object(json!({"type": "text", "id": format!("r{at}"), "text": text}));
                if live && end == rows.len() {
                    item.insert("streaming".into(), true.into());
                }
                copy.push(text.replace("\r\n", "\n").trim().to_owned());
                items.push(Value::Object(item));
            } else if let Some(notice) = notice(rows, at, end) {
                items.push(notice);
            }
        }
    }
    let has_prose = turn.runs.iter().any(|run| {
        messages_in(rows, run.key, run.end).any(|(at, end)| {
            rows[at].role == "assistant"
                && readable(&rows[at..end])
                && reaction(&joined(&rows[at..end])).is_none()
        })
    });
    let standalone = standalone && has_prose;
    let first = turn.runs[0];
    let last = *turn.runs.last().unwrap();
    let durations: Vec<u64> = turn
        .runs
        .iter()
        .filter_map(|&run| duration(source, run))
        .collect();
    let duration = (!durations.is_empty()).then(|| durations.iter().sum::<u64>());
    let started = started_at(source, first);
    let first_work = items.iter().position(|item| item["type"] != "user");
    let reacted_only = !live && first_work.is_none() && reacted;
    if !reacted_only && (standalone || live || duration.is_some() || first_work.is_some()) {
        let label = if live && !standalone {
            source
                .waiting
                .map(|waiting| format!("{waiting}…"))
                .unwrap_or_else(|| desk::mono_work_label(rows, first.key))
        } else {
            name.to_owned()
        };
        let mut header = object(json!({
            "type": "header",
            "id": format!("{}:header", turn.id()),
            "live": live,
            "label": label,
            "model": name,
            "harness": harness(source, first),
            "entries": [],
        }));
        put(&mut header, "startedAt", started);
        put(&mut header, "durationMs", duration);
        items.insert(first_work.unwrap_or(items.len()), Value::Object(header));
    }
    compact(&mut items);

    // A user opens its stretch above its bubble; a run without one, above the
    // Mono's header.
    let stamp = (first.prompt..first.key)
        .find(|&i| rows[i].first && rows[i].role == "user")
        .unwrap_or(first.prompt);
    let stamp_at = rows
        .get(stamp)
        .filter(|_| dates::mono_opens_stretch(rows, stamp))
        .and_then(|row| row.sent_at);
    let mut out = object(json!({"id": turn.id(), "items": items}));
    put(&mut out, "stampAt", stamp_at);
    if !live && !reacted_only && (duration.is_some() || standalone || first_work.is_some()) {
        let completed = info(source, last)
            .and_then(|info| info.completed_at)
            .or_else(|| {
                if turn.runs.len() > 1 {
                    started_at(source, last).map(|at| at + duration_or_zero(source, last))
                } else {
                    started.map(|at| at + duration.unwrap_or(0))
                }
            });
        out.insert("footer".into(), true.into());
        put(&mut out, "completedAt", completed);
        out.insert(
            "copyText".into(),
            copy.into_iter()
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n")
                .into(),
        );
        put(&mut out, "durationMs", duration);
    }
    Value::Object(out)
}
fn duration_or_zero(source: &Source, run: Run) -> u64 {
    duration(source, run).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_approvals_and_questions_end_the_latest_turn() {
        let rows = [row("user", "fix it"), row("assistant", "on it")];
        let turns = HashMap::new();
        let pending = [
            Pending {
                id: "a1".into(),
                question: false,
                label: "Run npm test".into(),
            },
            Pending {
                id: "q1".into(),
                question: true,
                label: "Which branch?".into(),
            },
        ];
        let mut source = source(&rows, &turns, true);
        source.pending = &pending;
        let built = all(&source);
        let items = built.last().unwrap()["items"].as_array().unwrap();
        let notices: Vec<_> = items.iter().filter(|i| i["type"] == "notice").collect();
        assert_eq!(notices[0]["id"], "input:a1");
        assert_eq!(
            notices[0]["text"],
            "Waiting for approval on desktop: Run npm test"
        );
        assert_eq!(
            notices[1]["text"],
            "Question waiting on desktop: Which branch?"
        );
    }
    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }
    fn sent(role: &str, text: &str, at: u64) -> Row {
        Row {
            sent_at: Some(at),
            ..row(role, text)
        }
    }
    fn tool(name: &str, input: Value, state: ToolState) -> Row {
        let mut call = ToolCall::new("call", name);
        call.input = input.to_string();
        call.state = state;
        row("tool", &serde_json::to_string(&call).unwrap())
    }
    fn source<'a>(rows: &'a [Row], turns: &'a HashMap<usize, TurnInfo>, busy: bool) -> Source<'a> {
        Source {
            rows,
            turns,
            cwd: "/repo",
            harness: "claude",
            model_name: Some("Opus".into()),
            busy,
            waiting: None,
            mono: None,
            pending: &[],
        }
    }
    fn all(source: &Source) -> Vec<Value> {
        build(source, 0..usize::MAX)
    }
    #[test]
    fn native_visuals_keep_a_readable_phone_fallback_outside_folded_work() {
        let document = monocode_core::visual::VisualDocument {
            id: "phone-example".into(),
            title: "Capacity explorer".into(),
            source: "return () => visual.text('Example');".into(),
        };
        let rows = vec![
            row("user", "Show capacity"),
            tool("Read", json!({"path":"data.json"}), ToolState::Completed),
            row("visual", &serde_json::to_string(&document).unwrap()),
            row("assistant", "Ready"),
        ];
        let info = HashMap::new();
        let turns = all(&source(&rows, &info, false));
        let notice = turns[0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "notice")
            .unwrap();
        assert_eq!(
            notice["text"],
            "Capacity explorer · Open on desktop to interact"
        );
        assert!(!turns[0].to_string().contains("visual.text"));
    }
    fn types(turn: &Value) -> Vec<&str> {
        turn["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["type"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn prompt_and_answer_settle_with_a_footer() {
        let rows = [sent("user", "Hi 👋", 1_000), row("assistant", "Hello!")];
        let mut infos = HashMap::new();
        infos.insert(
            1,
            TurnInfo {
                provider: "codex".into(),
                model: "GPT".into(),
                duration_ms: 2_000,
                duration_known: true,
                started_at: Some(1_000),
                ..TurnInfo::default()
            },
        );
        let turns = all(&source(&rows, &infos, false));
        assert_eq!(turns.len(), 1);
        let turn = &turns[0];
        assert_eq!(turn["id"], "t0");
        assert_eq!(types(turn), ["user", "header", "text"]);
        let items = &turn["items"];
        assert_eq!(items[0]["text"], "Hi 👋");
        assert_eq!(items[0]["at"], 1_000);
        assert!(items[0].get("emojiOnly").is_none());
        assert_eq!(items[1]["harness"], "codex");
        assert_eq!(items[1]["model"], "GPT");
        assert_eq!(items[1]["durationMs"], 2_000);
        assert_eq!(items[1]["entries"], json!([]));
        assert!(items[1].get("label").is_none());
        assert_eq!(items[2]["text"], "Hello!");
        assert!(items[2].get("streaming").is_none());
        assert_eq!(turn["footer"], true);
        assert_eq!(turn["completedAt"], 3_000);
        assert_eq!(turn["copyText"], "Hello!");
        assert_eq!(turn["durationMs"], 2_000);
        assert!(turn.get("metrics").is_none());
        assert_eq!(
            messages(&rows),
            [
                json!({"id":"r0","role":"user","text":"Hi 👋"}),
                json!({"id":"r1","role":"assistant","text":"Hello!"})
            ]
        );
    }

    #[test]
    fn answered_work_folds_into_the_header() {
        let rows = [
            row("user", "Fix it"),
            row("assistant", "Let me look."),
            tool(
                "Read",
                json!({"file_path": "/repo/src/a.rs"}),
                ToolState::Completed,
            ),
            tool("Bash", json!({"command": "cargo test"}), ToolState::Failed),
            row("assistant", "Fixed."),
        ];
        let infos = HashMap::new();
        let turn = &all(&source(&rows, &infos, false))[0];
        assert_eq!(types(turn), ["user", "header", "text"]);
        let header = &turn["items"][1];
        let entries = header["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            json!({"type":"prose","id":"r1","text":"Let me look."})
        );
        assert_eq!(entries[1]["type"], "phase");
        assert_eq!(entries[1]["id"], "p2");
        // Ties go to the kind that changed the most: a run outranks a read.
        assert_eq!(entries[1]["kind"], "run");
        let steps = entries[1]["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 2);
        assert!(steps[0]["label"].as_str().unwrap().starts_with("Read"));
        assert_eq!(steps[1]["failed"], true);
        assert_eq!(turn["items"][2]["text"], "Fixed.");
        assert_eq!(turn["copyText"], "Let me look.\n\nFixed.");
        // Legacy turns without timing still settle, without an invented duration.
        assert!(header.get("durationMs").is_none());
        assert_eq!(turn["footer"], true);
    }

    #[test]
    fn live_turn_streams_its_work_and_answer() {
        let mut rows = vec![
            row("user", "Go"),
            tool(
                "Read",
                json!({"file_path": "/repo/a.rs"}),
                ToolState::Completed,
            ),
            tool(
                "Read",
                json!({"file_path": "/repo/b.rs"}),
                ToolState::Running,
            ),
        ];
        let infos = HashMap::new();
        let mut live = source(&rows, &infos, true);
        live.waiting = Some("Waiting for approval");
        let turn = &all(&live)[0];
        assert_eq!(types(turn), ["user", "header", "work"]);
        let header = &turn["items"][1];
        assert_eq!(header["live"], true);
        assert_eq!(header["model"], "Opus");
        assert_eq!(header["waiting"], "Waiting for approval");
        let work = &turn["items"][2];
        assert_eq!(work["live"], true);
        assert_eq!(work["phases"][0]["title"], "Reading 2 files");
        assert!(turn.get("footer").is_none());

        rows.push(row("assistant", "Both read"));
        let turn = &all(&source(&rows, &infos, true))[0];
        assert_eq!(types(turn), ["user", "header", "text"]);
        assert_eq!(turn["items"][2]["streaming"], true);
        assert_eq!(turn["items"][1]["entries"].as_array().unwrap().len(), 1);

        // A lone call with no group is its own row.
        let rows = [
            row("user", "Go"),
            tool("Bash", json!({"command": "ls"}), ToolState::Running),
        ];
        let turn = &all(&source(&rows, &infos, true))[0];
        let phase = &turn["items"][2]["phases"][0];
        assert_eq!(phase["steps"], json!([]));
        assert_eq!(phase["kind"], "run");
        assert!(phase["title"].as_str().unwrap().contains("ls"));
    }

    #[test]
    fn errors_and_interjections_are_notices() {
        let note = Interjection {
            text: "Heads up".into(),
            custom_type: "advisor".into(),
            severity: None,
        };
        let rows = [
            row("user", "Go"),
            row("interjection", &serde_json::to_string(&note).unwrap()),
            row("error", "Provider disconnected"),
            row("error", "Stopped"),
        ];
        let infos = HashMap::new();
        let turn = &all(&source(&rows, &infos, false))[0];
        assert_eq!(types(turn), ["user", "header", "notice", "notice"]);
        assert_eq!(
            turn["items"][2],
            json!({"type":"notice","id":"r1","text":"Heads up"})
        );
        assert_eq!(
            turn["items"][3],
            json!({"type":"notice","id":"r2","text":"Provider disconnected","error":true})
        );
    }

    #[test]
    fn turn_ids_survive_appends_and_ranges_slice() {
        let mut rows = vec![
            row("user", "One"),
            row("assistant", "A"),
            row("user", "Two"),
        ];
        let infos = HashMap::new();
        let before = turn_ids(&source(&rows, &infos, false));
        assert_eq!(before, ["t0", "t2"]);
        rows.push(row("assistant", "B"));
        rows.push(row("user", "Three"));
        let after = turn_ids(&source(&rows, &infos, false));
        assert_eq!(after[..2], before[..]);
        assert_eq!(after.len(), 3);
        let built = build(&source(&rows, &infos, false), 1..3);
        assert_eq!(built.len(), 2);
        assert_eq!(built[0]["id"], "t2");
    }

    #[test]
    fn stacked_prompts_compact_and_attachments_count() {
        let files = json!([{"name": "a.png", "kind": "image"}, {"name": "b.png", "kind": "image"}]);
        let rows = [
            row("user", "First"),
            row("user", "👍"),
            row(ATTACHMENTS, &files.to_string()),
            row("assistant", "Ok"),
        ];
        let infos = HashMap::new();
        let all_turns = all(&source(&rows, &infos, false));
        let items = &all_turns[0]["items"];
        assert_eq!(items[0]["compact"], true);
        assert_eq!(items[1]["emojiOnly"], true);
        assert_eq!(items[1]["attachments"], 2);
    }

    #[test]
    fn mono_notifications_keep_only_the_final_message() {
        let rows = [
            row("user", "Review the heartbeat"),
            row("assistant", "Waiting for the review."),
            row("mono-system", "The review session finished"),
            row("assistant", "I'll verify the findings."),
            row("assistant", "Verification is complete."),
            row("assistant", "The review found three bugs."),
            Row {
                first: false,
                ..row("assistant", " The report is in Documents.")
            },
        ];
        let infos = HashMap::new();
        let mut mono = source(&rows, &infos, true);
        mono.mono = Some("Ada");
        let text_items = |turn: &Value| {
            turn["items"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["type"] == "text")
                .map(|item| item["text"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(text_items(&all(&mono)[0]), ["Waiting for the review."]);
        mono.busy = false;
        let turns = all(&mono);
        assert_eq!(turns.len(), 1);
        assert_eq!(
            text_items(&turns[0]),
            [
                "Waiting for the review.",
                "The review found three bugs. The report is in Documents.",
            ]
        );
        assert_eq!(
            turns[0]["copyText"],
            "Waiting for the review.\n\nThe review found three bugs. The report is in Documents."
        );
    }

    #[test]
    fn mono_turns_sign_their_header_and_hide_working_prose() {
        let rows = [
            sent("user", "Plan my day", 1_000),
            row("assistant", "Checking your calendar"),
            tool(
                "Read",
                json!({"file_path": "/repo/cal"}),
                ToolState::Completed,
            ),
            row("assistant", "You are free after 3."),
            sent("user", "Thanks", 2_000),
            row("assistant", "<reaction>🙌</reaction>"),
        ];
        let infos = HashMap::new();
        let mut mono = source(&rows, &infos, false);
        mono.mono = Some("Ada");
        let turns = all(&mono);
        assert_eq!(turns.len(), 2);
        assert_eq!(types(&turns[0]), ["user", "header", "text"]);
        assert_eq!(turns[0]["items"][1]["label"], "Ada");
        assert_eq!(turns[0]["items"][2]["text"], "You are free after 3.");
        assert_eq!(turns[0]["stampAt"], 1_000);
        assert_eq!(turns[0]["copyText"], "You are free after 3.");
        // A reply that is only a reaction leaves the message on its own.
        assert_eq!(types(&turns[1]), ["user"]);
        assert_eq!(turns[1]["items"][0]["reaction"], "🙌");
        assert!(turns[1].get("footer").is_none());

        let mut live = source(&rows[..3], &infos, true);
        live.mono = Some("Ada");
        let turn = &all(&live)[0];
        assert_eq!(types(turn), ["user", "header"]);
        assert_eq!(turn["items"][1]["label"], "Reading file…");
    }
}
