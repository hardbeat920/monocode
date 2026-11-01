//! Native transcript projection. Neither watch frames nor parsed text cross JS.
//! Row IDs survive appends, so the platform only measures changed blocks.
use crate::LinkResult;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Default, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    text: String,
    bold: bool,
    italic: bool,
    code: bool,
    strike: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    link: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    id: String,
    kind: String,
    runs: Vec<Run>,
    level: usize,
    indent: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cells: Vec<Vec<Run>>,
    presentation: Value,
    /// Native action: toggle, error, copy/metrics, or timestamp.
    action: Value,
}

impl Row {
    fn text(id: String, kind: &str, text: &str) -> Self {
        Self {
            id,
            kind: kind.into(),
            runs: vec![Run {
                text: text.into(),
                ..Run::default()
            }],
            level: 0,
            indent: 0,
            cells: Vec::new(),
            presentation: json!({}),
            action: Value::Null,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Block {
    start: usize,
    end: usize,
    rows: Vec<Arc<Row>>,
}

#[derive(Default)]
struct Markdown {
    source: String,
    blocks: Vec<Block>,
}

impl Markdown {
    fn update(&mut self, id: &str, text: &str) {
        if self.source == text {
            return;
        }
        // Two top-level blocks can merge (setext headings, adjacent lists).
        // Reference definitions can resolve links anywhere in the document.
        // Use a complete parse for those and for replacements/truncations.
        let keep = if text.starts_with(&self.source) && !text.contains("]:") {
            self.blocks.len().saturating_sub(2)
        } else {
            0
        };
        let start = self.blocks.get(keep).map(|b| b.start).unwrap_or(0);
        self.blocks.truncate(keep);
        self.blocks.extend(parse_blocks(id, &text[start..], start));
        self.source.clear();
        self.source.push_str(text);
    }
}

fn parse_blocks(id: &str, text: &str, offset: usize) -> Vec<Block> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut blocks = Vec::new();
    let mut block: Option<Block> = None;
    let mut depth = 0usize;
    let mut style = Run::default();
    let mut styles = Vec::new();
    let mut list: Vec<Option<u64>> = Vec::new();
    let mut quote = 0usize;
    let mut current: Option<Row> = None;
    let mut code = false;
    let mut code_group = String::new();
    let mut code_line = 0usize;
    let mut table_group = String::new();
    let mut table_columns = 0usize;
    let flush = |current: &mut Option<Row>, block: &mut Option<Block>| {
        if let (Some(row), Some(block)) = (current.take(), block.as_mut()) {
            block.rows.push(Arc::new(row));
        }
    };
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0 {
                    block = Some(Block {
                        start: offset + range.start,
                        end: offset + range.end,
                        rows: Vec::new(),
                    });
                }
                depth += 1;
                match tag {
                    Tag::List(start) => list.push(start),
                    Tag::Table(columns) => {
                        table_group = format!("{id}:{}:table", offset + range.start);
                        table_columns = columns.len();
                    }
                    Tag::BlockQuote(_) => quote += 1,
                    Tag::Item => {
                        flush(&mut current, &mut block);
                        let prefix = match list.last_mut() {
                            Some(Some(n)) => {
                                let p = format!("{}. ", n);
                                *n += 1;
                                p
                            }
                            _ => "• ".into(),
                        };
                        let mut row =
                            Row::text(format!("{id}:{}", offset + range.start), "list", "");
                        row.presentation = json!({"marker":prefix.trim(),"listDepth":list.len(),"quoteDepth":quote});
                        row.indent = list.len().saturating_sub(1);
                        current = Some(row);
                    }
                    Tag::Paragraph
                    | Tag::Heading { .. }
                    | Tag::CodeBlock(_)
                    | Tag::TableRow
                    | Tag::TableHead => {
                        if !matches!(tag, Tag::Paragraph) || current.is_none() {
                            flush(&mut current, &mut block);
                        }
                        let kind = match tag {
                            Tag::Heading { .. } => "heading",
                            Tag::CodeBlock(_) => "code",
                            Tag::TableRow | Tag::TableHead => "table",
                            _ if quote > 0 => "quote",
                            _ => "text",
                        };
                        if current.is_none() {
                            let mut row =
                                Row::text(format!("{id}:{}", offset + range.start), kind, "");
                            if let Tag::Heading { level, .. } = tag {
                                row.level = level as usize;
                            }
                            row.indent = quote + list.len().saturating_sub(1);
                            row.presentation = json!({"listDepth":list.len(),"quoteDepth":quote});
                            current = Some(row);
                        }
                        code = kind == "code";
                        if code {
                            code_group = format!("{id}:{}:code", offset + range.start);
                            code_line = 0;
                            current.as_mut().unwrap().presentation["group"] = json!(code_group);
                        }
                        if kind == "table" {
                            let row = current.as_mut().unwrap();
                            row.presentation["group"] = json!(table_group);
                            row.presentation["columns"] = json!(table_columns);
                            row.presentation["header"] = json!(matches!(tag, Tag::TableHead));
                        }
                    }
                    Tag::Strong
                    | Tag::Emphasis
                    | Tag::Strikethrough
                    | Tag::Link { .. }
                    | Tag::Image { .. } => {
                        styles.push(style.clone());
                        match tag {
                            Tag::Strong => style.bold = true,
                            Tag::Emphasis => style.italic = true,
                            Tag::Strikethrough => style.strike = true,
                            Tag::Link { dest_url, .. } => style.link = Some(dest_url.to_string()),
                            Tag::Image { .. } => append(&mut current, &style, "[Image] "),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Event::End(tag) => {
                match tag {
                    TagEnd::TableCell => {
                        if let Some(row) = current.as_mut() {
                            row.cells.push(std::mem::take(&mut row.runs));
                        }
                    }
                    TagEnd::Paragraph
                    | TagEnd::Heading(_)
                    | TagEnd::CodeBlock
                    | TagEnd::Item
                    | TagEnd::TableRow
                    | TagEnd::TableHead => {
                        if tag == TagEnd::CodeBlock
                            && code_line > 0
                            && current
                                .as_ref()
                                .is_some_and(|r| r.runs.iter().all(|run| run.text.is_empty()))
                        {
                            current = None;
                        }
                        flush(&mut current, &mut block);
                        code = false;
                    }
                    TagEnd::List(_) => {
                        list.pop();
                    }
                    TagEnd::BlockQuote(_) => quote = quote.saturating_sub(1),
                    TagEnd::Strong
                    | TagEnd::Emphasis
                    | TagEnd::Strikethrough
                    | TagEnd::Link
                    | TagEnd::Image => {
                        style = styles.pop().unwrap_or_default();
                    }
                    _ => {}
                }
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    flush(&mut current, &mut block);
                    if let Some(mut b) = block.take() {
                        b.end = offset + range.end;
                        blocks.push(b);
                    }
                }
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if code {
                    // Bound the size of a drawable row even for a huge code fence.
                    // Line IDs remain stable as the fence streams.
                    for part in text.split_inclusive('\n') {
                        append(&mut current, &style, part.trim_end_matches('\n'));
                        if part.ends_with('\n') {
                            flush(&mut current, &mut block);
                            code_line += 1;
                            let mut row =
                                Row::text(format!("{code_group}:line{code_line}"), "code", "");
                            row.presentation = json!({"group":code_group,"listDepth":list.len(),"quoteDepth":quote});
                            current = Some(row);
                        }
                    }
                } else {
                    append(&mut current, &style, &text);
                }
            }
            Event::Code(text) => {
                let mut inline = style.clone();
                inline.code = true;
                append(&mut current, &inline, &text);
            }
            Event::SoftBreak | Event::HardBreak => append(&mut current, &style, "\n"),
            Event::TaskListMarker(checked) => {
                append(&mut current, &style, if checked { "☑ " } else { "☐ " })
            }
            Event::Rule => {
                let row = Arc::new(Row::text(
                    format!("{id}:{}", offset + range.start),
                    "rule",
                    "",
                ));
                if let Some(block) = block.as_mut() {
                    block.rows.push(row);
                } else {
                    blocks.push(Block {
                        start: offset + range.start,
                        end: offset + range.end,
                        rows: vec![row],
                    });
                }
            }
            _ => {}
        }
    }
    // Native rows retain block boundaries without turning the whole block into
    // one giant view. This keeps long code/table blocks virtualized vertically.
    for (block_index, block) in blocks.iter_mut().enumerate() {
        let count = block.rows.len();
        for index in 0..count {
            let group = string(&block.rows[index].presentation, "group").to_owned();
            let first = group.is_empty()
                || index == 0
                || string(&block.rows[index - 1].presentation, "group") != group;
            let last = group.is_empty()
                || index + 1 == count
                || string(&block.rows[index + 1].presentation, "group") != group;
            let nested_list = index > 0
                && block.rows[index].presentation["listDepth"]
                    .as_u64()
                    .unwrap_or(0)
                    > block.rows[index - 1].presentation["listDepth"]
                        .as_u64()
                        .unwrap_or(0);
            let row = Arc::make_mut(&mut block.rows[index]);
            row.presentation["first"] = json!(first);
            row.presentation["last"] = json!(last);
            row.presentation["gap"] = json!(if index == 0 {
                if block_index == 0 && offset == 0 {
                    0
                } else {
                    12
                }
            } else if !first {
                0
            } else if row.kind == "list" && !nested_list {
                6
            } else {
                12
            });
            if row.presentation["quoteDepth"].as_u64().unwrap_or(0) > 0 {
                row.presentation["quoteFirst"] = json!(index == 0);
            }
        }
    }
    blocks
}

fn append(row: &mut Option<Row>, style: &Run, text: &str) {
    if let Some(row) = row.as_mut() {
        let mut run = style.clone();
        run.text = text.into();
        if let Some(last) = row.runs.last_mut() {
            let mut previous_style = last.clone();
            previous_style.text.clear();
            let mut next_style = run.clone();
            next_style.text.clear();
            if previous_style == next_style {
                last.text.push_str(text);
                return;
            }
        }
        row.runs.push(run);
    }
}

#[derive(Default)]
struct TurnCache {
    source: Value,
    rows: Vec<Arc<Row>>,
    markdown: HashMap<String, Markdown>,
}

pub struct Transcript {
    turns: Vec<Value>,
    history: Vec<Value>,
    history_older: Option<bool>,
    pending: Value,
    busy: bool,
    /// What the agent waits on the user to answer, as the desktop sent it.
    question: Value,
    question_generation: u64,
    older: bool,
    error: Option<String>,
    ready: bool,
    collapsed: bool,
    expanded: HashMap<String, bool>,
    cache: HashMap<String, TurnCache>,
    sent: HashMap<String, Arc<Row>>,
    order: Vec<String>,
}

impl Transcript {
    pub fn new(turns: Vec<Value>, collapsed: bool) -> Self {
        Self {
            turns,
            history: Vec::new(),
            history_older: None,
            pending: Value::Null,
            busy: false,
            question: Value::Null,
            question_generation: 0,
            older: false,
            error: None,
            ready: false,
            collapsed,
            expanded: HashMap::new(),
            cache: HashMap::new(),
            sent: HashMap::new(),
            order: Vec::new(),
        }
    }

    pub fn update(&mut self, command: Value) -> LinkResult<Value> {
        match command["type"].as_str().unwrap_or("") {
            "frames" => self.frames(
                command["frames"]
                    .as_array()
                    .ok_or("Invalid transcript frames")?,
            )?,
            "replace" => {
                self.history.clear();
                self.history_older = None;
                for cache in self.cache.values_mut() {
                    cache.source = Value::Null;
                }
                self.turns = command["turns"]
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| from_messages(&command["messages"]));
                self.busy = command["busy"].as_bool().unwrap_or(false);
                self.set_question(&command["question"]);
                // A read says whether turns precede its window, as a watch does.
                self.older = command["older"].as_bool().unwrap_or(false);
                self.error = None;
                self.ready = true;
                self.confirm_pending();
            }
            "prepend" => {
                let mut earlier = command["turns"].as_array().cloned().unwrap_or_default();
                for turn in &earlier {
                    if let Some(cache) = self.cache.get_mut(string(turn, "id")) {
                        cache.source = Value::Null;
                    }
                }
                earlier.append(&mut self.history);
                let mut seen = HashSet::new();
                earlier.retain(|t| seen.insert(string(t, "id").to_owned()));
                self.history = earlier;
                self.history_older = Some(command["older"].as_bool().unwrap_or(false));
            }
            "pending" => {
                self.pending = command["pending"].clone();
                if let Some(id) = self.pending["turn"]["id"].as_str() {
                    if let Some(cache) = self.cache.get_mut(id) {
                        cache.source = Value::Null;
                    }
                }
                self.confirm_pending();
            }
            "toggle" => {
                let id = string(&command, "id").to_owned();
                let open = !command["open"].as_bool().unwrap_or(false);
                for (turn_id, cache) in &mut self.cache {
                    if id.starts_with(&format!("{turn_id}/")) {
                        cache.source = Value::Null;
                    }
                }
                self.expanded.insert(id, open);
            }
            "collapsed" => {
                self.collapsed = command["value"].as_bool().unwrap_or(true);
                for cache in self.cache.values_mut() {
                    cache.source = Value::Null;
                }
            }
            "error" => self.error = command["error"].as_str().map(str::to_owned),
            "cursor" => {
                if let Some(from) = command["from"].as_str() {
                    if self.turns.first().is_some_and(|t| string(t, "id") != from) {
                        self.history.clear();
                        self.history_older = None;
                    }
                }
            }
            "snapshot" => {
                return Ok(
                    json!({"turns":self.turns.iter().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>()}),
                )
            }
            "selection" => {
                let id = string(&command, "id");
                for cache in self.cache.values() {
                    if cache.rows.iter().any(|r| r.id == id) {
                        for item in cache.source["items"].as_array().into_iter().flatten() {
                            let prefix =
                                format!("{}/{}", string(&cache.source, "id"), string(item, "id"));
                            if id == prefix || id.starts_with(&format!("{prefix}:")) {
                                return Ok(json!({"text":item["text"]}));
                            }
                            for entry in item["entries"].as_array().into_iter().flatten() {
                                let entry_prefix = format!("{prefix}/{}:", string(entry, "id"));
                                if id.starts_with(&entry_prefix) {
                                    return Ok(json!({"text":entry["text"]}));
                                }
                            }
                        }
                        return Ok(json!({"text":cache.source["copyText"]}));
                    }
                }
                return Ok(json!({"text":""}));
            }
            "render" => {}
            _ => return Err("Invalid transcript command".into()),
        }
        Ok(self.frame())
    }

    fn set_question(&mut self, question: &Value) {
        // Keep clears even when a burst or the UI relay publishes only its last snapshot.
        if question["requestId"] != self.question["requestId"] {
            self.question_generation += 1;
        }
        self.question = question.clone();
    }

    fn frames(&mut self, frames: &[Value]) -> LinkResult<()> {
        // Merge a whole transport burst before projecting/layout.
        let mut order: Vec<String> = self.turns.iter().map(|t| string(t, "id").into()).collect();
        let original_order = order.clone();
        let mut known: HashMap<String, Value> = std::mem::take(&mut self.turns)
            .into_iter()
            .map(|t| (string(&t, "id").into(), t))
            .collect();
        let applied = (|| -> LinkResult<()> {
            for frame in frames {
                if let Some(error) = frame["error"].as_str() {
                    self.error = Some(error.into());
                    continue;
                }
                self.error = None;
                self.ready = true;
                if let Some(busy) = frame["busy"].as_bool() {
                    self.busy = busy;
                }
                if let Some(older) = frame["older"].as_bool() {
                    self.older = older;
                }
                // Sent when it changes; null once answered.
                if let Some(question) = frame.get("question") {
                    self.set_question(question);
                }
                for turn in frame["turns"].as_array().into_iter().flatten() {
                    let id = string(turn, "id");
                    if known.get(id).is_none_or(|before| before != turn) {
                        known.insert(id.into(), turn.clone());
                        if let Some(cache) = self.cache.get_mut(id) {
                            cache.source = Value::Null;
                        }
                    }
                }
                for patch in frame["patches"].as_array().into_iter().flatten() {
                    let id = string(patch, "id");
                    let turn = known
                        .get_mut(id)
                        .ok_or("Transcript window needs to reopen")?;
                    if let Some(cache) = self.cache.get_mut(id) {
                        cache.source = Value::Null;
                    }
                    apply_edits(turn, patch)?;
                    if string(turn, "id") != id {
                        return Err("Transcript window needs to reopen".into());
                    }
                }
                if let Some(ids) = frame["order"].as_array() {
                    order = ids
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                }
            }
            if order.iter().any(|id| !known.contains_key(id)) {
                return Err("Transcript window needs to reopen".into());
            }
            Ok(())
        })();
        let retained = if applied.is_ok() {
            &order
        } else {
            &original_order
        };
        self.turns = retained.iter().filter_map(|id| known.remove(id)).collect();
        applied?;
        self.confirm_pending();
        Ok(())
    }

    fn user_count(&self) -> usize {
        self.turns
            .iter()
            .flat_map(|t| t["items"].as_array().into_iter().flatten())
            .filter(|i| i["type"] == "user")
            .count()
    }

    fn confirm_pending(&mut self) {
        let confirmed = if let Some(after) = self.pending["afterUserId"].as_str() {
            self.last_user_id().is_some_and(|id| id != after)
        } else {
            self.user_count() > self.pending["userCount"].as_u64().unwrap_or(0) as usize
        };
        if self.pending.is_object() && confirmed && self.ready {
            self.pending = Value::Null;
        }
    }

    fn last_user_id(&self) -> Option<String> {
        self.turns.iter().rev().find_map(|t| {
            t["items"].as_array().and_then(|items| {
                items
                    .iter()
                    .rev()
                    .find(|i| i["type"] == "user")
                    .map(|i| format!("{}/{}", string(t, "id"), string(i, "id")))
            })
        })
    }

    fn frame(&mut self) -> Value {
        let watched: HashSet<&str> = self.turns.iter().map(|t| string(t, "id")).collect();
        let mut turns: Vec<&Value> = self
            .history
            .iter()
            .filter(|t| !watched.contains(string(t, "id")))
            .chain(self.turns.iter())
            .collect();
        if self.pending["turn"].is_object() {
            turns.push(&self.pending["turn"]);
        }
        let mut rows: Vec<Arc<Row>> = Vec::new();
        let mut live_turns = HashSet::new();
        let pending_id = self.pending["turn"]["id"].as_str();
        let mut pending_from = None;
        for turn in &turns {
            let id = string(turn, "id");
            if Some(id) == pending_id {
                pending_from = Some(rows.len());
            }
            live_turns.insert(id.to_owned());
            let cache = self.cache.entry(id.into()).or_default();
            if cache.source.is_null() {
                let mut next = project(turn, &mut cache.markdown, &self.expanded, self.collapsed);
                let previous: HashMap<&str, &Arc<Row>> =
                    cache.rows.iter().map(|r| (r.id.as_str(), r)).collect();
                // Reuse identical blocks after parsing the mutable tail.
                cache.rows = next
                    .drain(..)
                    .map(|r| match previous.get(r.id.as_str()) {
                        Some(old) if Arc::ptr_eq(old, &r) || ***old == *r => (*old).clone(),
                        _ => r,
                    })
                    .collect();
                cache.source = (*turn).clone();
            }
            rows.extend(cache.rows.iter().cloned());
        }
        self.cache.retain(|id, _| live_turns.contains(id));
        // A message sent while a Mono works would show above its working
        // mascot until the desktop confirms it; carry the mascot under it at
        // once, where that confirmation puts it. Only a Mono's has nothing
        // folded under it to leave behind.
        let working = pending_from.and_then(|from| {
            rows[..from].iter().rposition(|r| {
                r.kind == "fold" && r.action["live"] == true && r.action["expandable"] == false
            })
        });
        if let Some(at) = working {
            let row = rows.remove(at);
            rows.push(row);
        }
        stack_user_rows(&mut rows);
        let order: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
        // Stacking copies rows each frame; an equal copy is nothing new.
        let changed: Vec<&Row> = rows
            .iter()
            .filter(|r| {
                self.sent
                    .get(&r.id)
                    .is_none_or(|old| !Arc::ptr_eq(old, r) && **old != ***r)
            })
            .map(|r| r.as_ref())
            .collect();
        let latest_error = self
            .turns
            .last()
            .and_then(|t| t["items"].as_array())
            .and_then(|items| {
                items
                    .iter()
                    .rev()
                    .find(|i| i["type"] == "notice" && i["error"] == true)
            });
        let last_user = rows.iter().rev().find(|r| r.kind == "user").map(|r| &r.id);
        let output = json!({
            "rows": changed,
            "order": if order == self.order { None } else { Some(&order) },
            "status": { "busy":self.busy, "error":self.error, "hasTurns":!turns.is_empty(), "ready":self.ready, "lastUserId":self.last_user_id(),
                "userCount":self.user_count(), "pendingId":self.pending["turn"]["id"], "notice":latest_error,
                "question":self.question, "questionGeneration":self.question_generation },
            "older": self.history_older.unwrap_or(self.older),
            "firstTurnId": self.history.first().or(self.turns.first()).map(|t| string(t,"id")),
            "watchFrom": self.turns.first().map(|t| string(t,"id")),
            "lastUserId":last_user,
        });
        self.sent = rows.into_iter().map(|r| (r.id.clone(), r)).collect();
        self.order = order;
        output
    }
}

/// The desktop opts capable phones into field edits, including UTF-16 text
/// append offsets (the same units as JavaScript's String.length).
fn apply_edits(turn: &mut Value, patch: &Value) -> LinkResult<()> {
    let invalid = || "Transcript window needs to reopen".to_owned();
    for edit in patch["edits"].as_array().ok_or_else(invalid)? {
        let path = edit["path"].as_array().ok_or_else(invalid)?;
        let (last, parents) = path.split_last().ok_or_else(invalid)?;
        let mut parent = &mut *turn;
        for key in parents {
            parent = match key {
                Value::String(key) => parent.get_mut(key),
                Value::Number(index) => index.as_u64().and_then(|i| parent.get_mut(i as usize)),
                _ => None,
            }
            .ok_or_else(invalid)?;
        }
        if edit["remove"] == true {
            match (parent, last) {
                (Value::Object(object), Value::String(key)) if object.contains_key(key) => {
                    object.remove(key);
                }
                (Value::Array(array), Value::Number(index)) => {
                    let index = index.as_u64().ok_or_else(invalid)? as usize;
                    if index >= array.len() {
                        return Err(invalid());
                    }
                    array.remove(index);
                }
                _ => return Err(invalid()),
            }
        } else if let Some(append) = edit["append"].as_str() {
            let target = match last {
                Value::String(key) => parent.get_mut(key),
                Value::Number(index) => index.as_u64().and_then(|i| parent.get_mut(i as usize)),
                _ => None,
            }
            .ok_or_else(invalid)?;
            let Value::String(text) = target else {
                return Err(invalid());
            };
            if edit["from"].as_u64() != Some(text.encode_utf16().count() as u64) {
                return Err(invalid());
            }
            text.push_str(append);
        } else {
            let value = edit.get("value").ok_or_else(invalid)?.clone();
            match (parent, last) {
                (Value::Object(object), Value::String(key)) => {
                    object.insert(key.clone(), value);
                }
                (Value::Array(array), Value::Number(index)) => {
                    let index = index.as_u64().ok_or_else(invalid)? as usize;
                    if index < array.len() {
                        array[index] = value;
                    } else if index == array.len() {
                        array.push(value);
                    } else {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            }
        }
    }
    Ok(())
}

/// The emoji of a reply that is only `<reaction>…</reaction>`, which a Mono
/// sends to acknowledge a message without words.
fn reaction(text: &str) -> Option<&str> {
    text.trim()
        .strip_prefix("<reaction>")
        .and_then(|s| s.strip_suffix("</reaction>"))
        .map(str::trim)
        .filter(|emoji| !emoji.is_empty() && emoji.chars().count() <= 16)
}

/// Pins each reaction reply onto the message before it, as the desktop shows
/// it, and drops the reply, even from a desktop that sends the raw tag. True
/// when the turn's replies were all reactions.
fn fold_reactions(items: &mut Vec<Value>) -> bool {
    let mut reacted = false;
    let mut replied = false;
    let mut message: Option<usize> = None;
    let mut kept = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        match string(&item, "type") {
            "user" => {
                message = Some(kept.len());
                kept.push(item);
            }
            "text" => match (reaction(string(&item, "text")), message) {
                (Some(emoji), Some(at)) => {
                    kept[at]["reaction"] = json!(emoji);
                    reacted = true;
                }
                _ => {
                    replied = true;
                    kept.push(item);
                }
            },
            _ => kept.push(item),
        }
    }
    *items = kept;
    reacted && !replied
}

/// Messages sent back to back are turns of their own; each still sits close
/// under the one before, as two in one turn do.
fn stack_user_rows(rows: &mut [Arc<Row>]) {
    for at in 1..rows.len() {
        if rows[at].kind != "user" || rows[at - 1].kind != "user" {
            continue;
        }
        if rows[at].presentation["afterUser"] != true {
            Arc::make_mut(&mut rows[at]).presentation["afterUser"] = json!(true);
        }
        if rows[at - 1].presentation["beforeUser"] != true {
            Arc::make_mut(&mut rows[at - 1]).presentation["beforeUser"] = json!(true);
        }
    }
}

fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

fn project(
    turn: &Value,
    markdown: &mut HashMap<String, Markdown>,
    expanded: &HashMap<String, bool>,
    collapsed: bool,
) -> Vec<Arc<Row>> {
    let turn_id = string(turn, "id");
    let mut rows = Vec::new();
    let mut used_markdown = HashSet::new();
    if turn["stampAt"].is_number() {
        let mut row = Row::text(format!("{turn_id}:stamp"), "stamp", "");
        row.action = turn["stampAt"].clone();
        rows.push(Arc::new(row));
    }
    let mut items = turn["items"].as_array().cloned().unwrap_or_default();
    let reacted_only = fold_reactions(&mut items);
    for (item_index, item) in items.iter().enumerate() {
        let id = format!("{turn_id}/{}", string(item, "id"));
        match string(item, "type") {
            "user" => {
                let mut row = Row::text(id.clone(), "user", string(item, "text"));
                row.action = item.clone();
                row.action["nativeExpanded"] = json!(expanded.get(&id).copied().unwrap_or(false));
                // Stacked under another of the user's messages, it sits close.
                row.presentation["afterUser"] =
                    json!(item_index > 0 && string(&items[item_index - 1], "type") == "user");
                rows.push(Arc::new(row));
            }
            "text" => {
                // Two replies in a row in one turn: the desktop's Mono chat
                // breaks them with its pixel divider (drawn on Mono screens only).
                if item_index > 0 && string(&items[item_index - 1], "type") == "text" {
                    rows.push(Arc::new(Row::text(format!("{id}:divider"), "divider", "")));
                }
                let from = rows.len();
                markdown_rows(
                    &id,
                    string(item, "text"),
                    &mut rows,
                    markdown,
                    &mut used_markdown,
                );
                if rows.len() > from {
                    let under_line = item_index > 0
                        && !matches!(string(&items[item_index - 1], "type"), "user" | "text");
                    Arc::make_mut(&mut rows[from]).presentation["top"] =
                        json!(if under_line { 4 } else { 12 });
                    Arc::make_mut(rows.last_mut().unwrap()).presentation["bottom"] = json!(4);
                    // A Mono's chat draws the reply as one bubble across its rows.
                    let last = rows.len() - 1;
                    for (index, row) in rows[from..].iter_mut().enumerate() {
                        let p = &mut Arc::make_mut(row).presentation;
                        p["reply"] = json!(true);
                        p["replyFirst"] = json!(index == 0);
                        p["replyLast"] = json!(from + index == last);
                    }
                }
            }
            "notice" => {
                let mut row = Row::text(
                    id,
                    if item["error"] == true {
                        "error"
                    } else {
                        "notice"
                    },
                    string(item, "text"),
                );
                row.action = item.clone();
                rows.push(Arc::new(row));
            }
            "header" => {
                let entries = item["entries"].as_array().cloned().unwrap_or_default();
                let open = expanded.get(&id).copied().unwrap_or(false);
                let mut row = Row::text(id.clone(), "fold", string(item, "label"));
                row.action = item.clone();
                row.action.as_object_mut().unwrap().remove("entries");
                row.action["type"] = json!("toggle");
                row.action["id"] = json!(id);
                row.action["open"] = json!(open && !entries.is_empty());
                row.action["expandable"] = json!(!entries.is_empty());
                rows.push(Arc::new(row));
                if open {
                    for (index, entry) in entries.iter().enumerate() {
                        let eid = format!("{id}/{}", string(entry, "id"));
                        let from = rows.len();
                        if entry["type"] == "prose" {
                            markdown_rows(
                                &eid,
                                string(entry, "text"),
                                &mut rows,
                                markdown,
                                &mut used_markdown,
                            );
                            if rows.len() > from {
                                Arc::make_mut(&mut rows[from]).presentation["top"] = json!(4);
                                Arc::make_mut(rows.last_mut().unwrap()).presentation["bottom"] =
                                    json!(4);
                                for r in &mut rows[from..] {
                                    Arc::make_mut(r).presentation["muted"] = json!(true);
                                }
                            }
                        } else {
                            phase(&eid, entry, false, collapsed, expanded, &mut rows);
                        }
                        let last = rows.len().saturating_sub(1);
                        for (offset, row) in rows[from..].iter_mut().enumerate() {
                            let p = &mut Arc::make_mut(row).presentation;
                            p["foldRail"] = json!(true);
                            p["foldBranch"] = json!(offset == 0);
                            p["foldTail"] = json!(index + 1 == entries.len());
                        }
                        if rows.len() > from {
                            let p = &mut Arc::make_mut(&mut rows[last]).presentation;
                            let bottom = p["bottom"].as_u64().unwrap_or(0);
                            p["bottom"] =
                                json!(bottom + 4 + if index + 1 == entries.len() { 4 } else { 0 });
                        }
                    }
                }
            }
            "work" => {
                let phases = item["phases"].as_array().cloned().unwrap_or_default();
                for (index, p) in phases.iter().enumerate() {
                    let from = rows.len();
                    phase(
                        &format!("{id}/{}", string(p, "id")),
                        p,
                        item["live"] == true && index + 1 == phases.len(),
                        collapsed,
                        expanded,
                        &mut rows,
                    );
                    Arc::make_mut(&mut rows[from]).presentation["top"] = json!(2);
                    if index + 1 == phases.len() {
                        let last = Arc::make_mut(rows.last_mut().unwrap());
                        let bottom = last.presentation["bottom"].as_u64().unwrap_or(0);
                        last.presentation["bottom"] = json!(bottom + 2);
                    }
                }
            }
            "choices" => {
                // Suggested replies, lettered in one card. A tap sends one;
                // once a reply follows, the one sent stays and the rest dim.
                let options: Vec<&str> = item["options"]
                    .as_array()
                    .map(|options| options.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                if !options.is_empty() {
                    let mut row = Row::text(id, "choices", &options.join("\n"));
                    row.action = json!({"options": options, "chosen": item["chosen"]});
                    row.presentation = json!({"top": 4, "bottom": 8});
                    rows.push(Arc::new(row));
                }
            }
            "visual" => {
                // A chart or dashboard the agent drew, laid out natively from
                // the tree the desktop rendered.
                let mut row = Row::text(id, "visual", string(item, "title"));
                row.action = item.clone();
                row.presentation = json!({"top": 8, "bottom": 8});
                rows.push(Arc::new(row));
            }
            "document" => {
                // A document the Mono wrote: a tap opens it in the reader.
                let mut row = Row::text(id, "document", string(item, "title"));
                row.action = item.clone();
                row.presentation = json!({"top": 4, "bottom": 8});
                rows.push(Arc::new(row));
            }
            "sessions" => {
                // Sessions a Mono started: a card each, under the reply after
                // the launch. A tap opens the session over the Mono's chat.
                let sessions = item["sessions"].as_array().cloned().unwrap_or_default();
                for (index, session) in sessions.iter().enumerate() {
                    let mut row = Row::text(
                        format!("{id}/{}", string(session, "id")),
                        "launch",
                        string(session, "title"),
                    );
                    row.action = session.clone();
                    row.action["sessionId"] = session["id"].clone();
                    row.action["type"] = json!("session");
                    row.presentation = json!({
                        "top": if index == 0 { 4 } else { 0 },
                        "bottom": if index + 1 == sessions.len() { 8 } else { 0 },
                    });
                    rows.push(Arc::new(row));
                }
            }
            _ => {}
        }
    }
    // When the reply landed, shown beside its last bubble on a slide aside.
    if turn["completedAt"].is_number() {
        if let Some(row) = rows
            .iter_mut()
            .rev()
            .find(|r| r.presentation["replyLast"] == true)
        {
            Arc::make_mut(row).presentation["at"] = turn["completedAt"].clone();
        }
    }
    // A reply that was only a reaction has nothing of its own to copy or time.
    if turn["footer"] == true && !reacted_only {
        let mut row = Row::text(format!("{turn_id}:footer"), "footer", "");
        row.action = json!({"type":"footer","copyText":turn["copyText"],"metrics":turn["metrics"],"durationMs":turn["durationMs"],"completedAt":turn["completedAt"]});
        rows.push(Arc::new(row));
    }
    markdown.retain(|id, _| used_markdown.contains(id));
    rows
}

fn markdown_rows(
    id: &str,
    text: &str,
    rows: &mut Vec<Arc<Row>>,
    cache: &mut HashMap<String, Markdown>,
    used: &mut HashSet<String>,
) {
    used.insert(id.to_owned());
    let parsed = cache.entry(id.into()).or_default();
    parsed.update(id, text);
    for block in &parsed.blocks {
        rows.extend(block.rows.iter().cloned());
    }
}

fn phase(
    id: &str,
    phase: &Value,
    active: bool,
    collapsed: bool,
    expanded: &HashMap<String, bool>,
    rows: &mut Vec<Arc<Row>>,
) {
    let steps = phase["steps"].as_array().cloned().unwrap_or_default();
    let override_open = expanded.get(id).copied();
    let open = override_open.unwrap_or(active && !collapsed);
    let mut row = Row::text(id.into(), "phase", string(phase, "title"));
    row.action = json!({"type":"toggle","id":id,"open":open,"expandable":!steps.is_empty(),"live":active,"kind":phase["kind"]});
    rows.push(Arc::new(row));
    if open {
        for (index, step) in steps
            .iter()
            .enumerate()
            .skip(if active && override_open.is_none() {
                steps.len().saturating_sub(4)
            } else {
                0
            })
        {
            let mut row = Row::text(
                format!("{id}/{}:{index}", string(step, "id")),
                if step["failed"] == true {
                    "errorStep"
                } else {
                    "step"
                },
                string(step, "label"),
            );
            row.indent = 1;
            row.presentation = json!({"stepLast":index + 1 == steps.len(),"bottom":if index+1 == steps.len() {4} else {0}});
            rows.push(Arc::new(row));
        }
    }
}

fn from_messages(messages: &Value) -> Vec<Value> {
    let mut turns: Vec<Value> = Vec::new();
    for message in messages.as_array().into_iter().flatten() {
        let user = message["role"] == "user";
        let item = json!({"type":if user {"user"} else {"text"},"id":message["id"],"text":message["text"]});
        if user || turns.is_empty() {
            turns.push(json!({"id":message["id"],"items":[item]}));
        } else {
            turns.last_mut().unwrap()["items"]
                .as_array_mut()
                .unwrap()
                .push(item);
        }
    }
    turns
}

#[cfg(test)]
mod reaction_tests {
    use super::*;

    #[test]
    fn a_reaction_reply_lands_on_the_message_before_it() {
        let mut items = vec![
            json!({"type":"user","id":"u","text":"cool"}),
            json!({"type":"text","id":"a","text":" <reaction>👍</reaction>\n"}),
        ];
        assert!(fold_reactions(&mut items));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["reaction"], "👍");
    }

    #[test]
    fn back_to_back_replies_get_a_divider_between_them() {
        let turn = json!({"id":"t","items":[
            {"type":"user","id":"u","text":"go"},
            {"type":"text","id":"a","text":"First."},
            {"type":"text","id":"b","text":"Second."},
        ]});
        let rows = project(&turn, &mut HashMap::new(), &HashMap::new(), false);
        let kinds: Vec<&str> = rows.iter().map(|row| row.kind.as_str()).collect();
        let divider = kinds.iter().position(|kind| *kind == "divider").unwrap();
        // After the first reply, before the second; none after the message.
        assert_eq!(kinds.iter().filter(|kind| **kind == "divider").count(), 1);
        assert!(divider > 1 && divider < kinds.len() - 1);
    }

    #[test]
    fn a_monos_launched_sessions_become_cards_that_open_them() {
        let turn = json!({"id":"t","items":[
            {"type":"user","id":"u","text":"go"},
            {"type":"text","id":"a","text":"On it."},
            {"type":"sessions","id":"a:launches","sessions":[
                {"id":"app-1","projectId":"p","project":"repo","title":"Fix","harness":"codex","state":"working"},
                {"id":"app-2","projectId":"p","project":"repo","title":"Test","harness":"claude","state":"done"},
            ]},
        ]});
        let rows = project(&turn, &mut HashMap::new(), &HashMap::new(), false);
        let cards: Vec<_> = rows.iter().filter(|row| row.kind == "launch").collect();
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].id, "t/a:launches/app-1");
        assert_eq!(cards[0].action["type"], "session");
        assert_eq!(cards[0].action["sessionId"], "app-1");
        assert_eq!(cards[0].action["projectId"], "p");
        assert_eq!(cards[1].runs[0].text, "Test");
    }

    #[test]
    fn a_monos_choices_and_documents_become_cards() {
        let turn = json!({"id":"t","items":[
            {"type":"user","id":"u","text":"next?"},
            {"type":"choices","id":"c","options":["Review it","Wait"],"chosen":"Review it"},
            {"type":"document","id":"d","documentId":"doc","title":"Report","kind":"document"},
        ]});
        let rows = project(&turn, &mut HashMap::new(), &HashMap::new(), false);
        let choices: Vec<_> = rows.iter().filter(|row| row.kind == "choices").collect();
        assert_eq!(choices.len(), 1);
        assert_eq!(
            choices[0].action,
            json!({"options":["Review it","Wait"],"chosen":"Review it"})
        );
        let document = rows.iter().find(|row| row.kind == "document").unwrap();
        assert_eq!(document.action["documentId"], "doc");
        assert_eq!(document.runs[0].text, "Report");
    }

    #[test]
    fn written_replies_and_lone_tags_stay_as_text() {
        let mut items = vec![
            json!({"type":"user","id":"u","text":"hi"}),
            json!({"type":"text","id":"a","text":"<reaction>👍</reaction>"}),
            json!({"type":"text","id":"b","text":"Here is the answer."}),
        ];
        assert!(!fold_reactions(&mut items));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["reaction"], "👍");
        // With no message before it there is nothing to pin it to.
        let mut lone = vec![json!({"type":"text","id":"a","text":"<reaction>👍</reaction>"})];
        assert!(!fold_reactions(&mut lone));
        assert_eq!(lone.len(), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markdown_keeps_table_cells_and_horizontal_code_groups() {
        let mut md = Markdown::default();
        md.update("m", "Before\n\n```ts\nconst a = 1;\nconst b = 2;\n```\n\n| Name | Value |\n|---|---|\n| **bold** | `src/main.ts:4` |");
        let rows: Vec<_> = md.blocks.iter().flat_map(|b| &b.rows).collect();
        let code: Vec<_> = rows.iter().filter(|r| r.kind == "code").collect();
        assert_eq!(code.len(), 2); // No trailing blank line from the fence.
        assert_eq!(code[0].presentation["group"], code[1].presentation["group"]);
        assert_eq!(code[0].presentation["first"], true);
        assert_eq!(code[0].presentation["last"], false);
        assert_eq!(code[0].presentation["gap"], 12);
        assert_eq!(code[1].presentation["gap"], 0);
        assert_eq!(code[1].presentation["last"], true);
        let table: Vec<_> = rows.iter().filter(|r| r.kind == "table").collect();
        assert_eq!(table[0].presentation["header"], true);
        assert_eq!(table[0].presentation["columns"], 2);
        assert_eq!(table[1].cells.len(), 2);
        assert!(table[1].cells[0].iter().any(|r| r.bold && r.text == "bold"));
        assert!(table[1].cells[1]
            .iter()
            .any(|r| r.code && r.text == "src/main.ts:4"));
        assert!(table[1].runs.is_empty());
    }

    #[test]
    fn presentation_preserves_reply_insets_and_nested_work_rails() {
        let turn = json!({"id":"t","items":[
            {"id":"u","type":"user","text":"hello"},
            {"id":"h","type":"header","harness":"claude","model":"Claude Opus","durationMs":124000,"entries":[
                {"id":"p","type":"prose","text":"note\n\nmore"},
                {"id":"w","type":"phase","kind":"research","title":"Read files","steps":[{"id":"s","label":"file.ts"}]}
            ]},
            {"id":"a","type":"text","text":"# Answer\n\nBody"}
        ]});
        let mut expanded = HashMap::new();
        expanded.insert("t/h".into(), true);
        expanded.insert("t/h/w".into(), true);
        let rows = project(&turn, &mut HashMap::new(), &expanded, false);
        let header = rows.iter().find(|r| r.kind == "fold").unwrap();
        assert_eq!(header.action["model"], "Claude Opus");
        assert_eq!(header.action["durationMs"], 124000);
        assert!(header.action["entries"].is_null()); // No work content copied into the header.
        let note = rows.iter().find(|r| r.id == "t/h/p:0").unwrap();
        assert_eq!(note.presentation["muted"], true);
        assert_eq!(note.presentation["foldBranch"], true);
        assert_eq!(note.presentation["foldTail"], false);
        let phase = rows.iter().find(|r| r.kind == "phase").unwrap();
        assert_eq!(phase.action["kind"], "research");
        assert_eq!(phase.presentation["foldTail"], true);
        let step = rows.iter().find(|r| r.kind == "step").unwrap();
        assert_eq!(step.presentation["stepLast"], true);
        let reply: Vec<_> = rows.iter().filter(|r| r.id.starts_with("t/a:")).collect();
        assert_eq!(reply[0].presentation["top"], 4);
        assert_eq!(reply[1].presentation["gap"], 12);
        assert_eq!(reply[1].presentation["bottom"], 4);
    }

    #[test]
    fn appending_code_changes_only_the_previous_last_line_and_new_lines() {
        let turn = |text: &str| json!({"id":"t","items":[{"id":"a","type":"text","text":text}]});
        let mut t = Transcript::new(vec![turn("```ts\none\ntwo\nthree")], true);
        t.update(json!({"type":"render"})).unwrap();
        let delta = t.update(json!({"type":"frames","frames":[{"order":["t"],"turns":[turn("```ts\none\ntwo\nthree\nfour")]}]})).unwrap();
        let changed = delta["rows"].as_array().unwrap();
        assert_eq!(changed.len(), 2);
        assert!(changed.iter().all(|r| r["id"] != "t/a:0"));
    }
    #[test]
    fn incremental_markdown_matches_complete_parse_at_every_unicode_boundary() {
        for source in ["# Hello 👋\n\nFirst **bold** and [link](https://example.com).\n\n- one\n- two\n\n```rust\nlet x = 1;\n```\n\nlast", "One\n\nTwo\n---\n\n> quote\n> more\n\n| a | b |\n|---|---|\n| c | d |", "[hello][ref]\n\nother\n\n[ref]: https://example.com"] {
            let mut md = Markdown::default();
            for end in source.char_indices().map(|(i,_)|i).chain(std::iter::once(source.len())) {
                md.update("item",&source[..end]);
                assert_eq!(md.blocks,parse_blocks("item",&source[..end],0),"prefix {:?}",&source[..end]);
            }
        }
    }
    #[test]
    fn incremental_watch_edits_append_unicode_and_keep_completed_rows() {
        let stable = json!({"id":"a","items":[{"id":"reply","type":"text","text":"Done"}]});
        let live = json!({"id":"b","items":[{"id":"reply","type":"text","text":"Hello 👋","streaming":true}]});
        let mut t = Transcript::new(vec![stable.clone(), live], true);
        t.update(json!({"type":"render"})).unwrap();
        let delta = t
            .update(json!({"type":"frames","frames":[{
                "order":["a","b"],"busy":true,"patches":[{"id":"b","edits":[
                    {"path":["items",0,"text"],"from":8,"append":" world"}
                ]}]
            }]}))
            .unwrap();
        assert_eq!(t.turns[0], stable);
        assert_eq!(t.turns[1]["items"][0]["text"], "Hello 👋 world");
        let rows = delta["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0]["id"].as_str().unwrap().starts_with("b/"));
    }

    #[test]
    fn incremental_watch_edits_change_nested_state_and_recover_from_stale_appends() {
        let original = json!({"id":"a","durationMs":1,"items":[
            {"id":"reply","type":"text","text":"Hello"},
            {"id":"h","type":"header","live":true,"entries":[{"id":"p","type":"prose","text":"Read"}]}
        ]});
        let mut t = Transcript::new(vec![original.clone()], true);
        t.update(
            json!({"type":"frames","frames":[{"order":["a"],"patches":[{"id":"a","edits":[
                {"path":["items",1,"entries",0,"text"],"from":4,"append":" files"},
                {"path":["items",1,"entries",1],"value":{"id":"p2","type":"prose","text":"Next"}},
                {"path":["items",1,"live"],"value":false},
                {"path":["durationMs"],"remove":true}
            ]}]}]}),
        )
        .unwrap();
        assert_eq!(t.turns[0]["items"][1]["entries"][0]["text"], "Read files");
        assert_eq!(
            t.turns[0]["items"][1]["entries"].as_array().unwrap().len(),
            2
        );
        assert_eq!(t.turns[0]["items"][1]["live"], false);
        assert!(t.turns[0].get("durationMs").is_none());
        t.update(
            json!({"type":"frames","frames":[{"order":["a"],"patches":[{"id":"a","edits":[
                {"path":["items",1],"remove":true}
            ]}]}]}),
        )
        .unwrap();
        assert_eq!(t.turns[0]["items"].as_array().unwrap().len(), 1);
        assert!(t
            .update(
                json!({"type":"frames","frames":[{"order":["a"],"patches":[{"id":"a","edits":[
                    {"path":["items",0,"text"],"from":999,"append":" lost"}
                ]}]}]})
            )
            .is_err());
        t.update(json!({"type":"replace","turns":[original.clone()],"busy":false}))
            .unwrap();
        assert_eq!(t.turns, vec![original]);
    }
    #[test]
    fn changed_tail_sends_no_completed_rows_and_history_does_not_confirm_pending() {
        let turn = |id: &str, text: &str| json!({"id":id,"items":[{"id":"u","type":"user","text":"prompt"},{"id":"t","type":"text","text":text,"streaming":true}]});
        let mut t = Transcript::new(vec![turn("1", "one\n\ntwo\n\nthree\n\nfour")], true);
        let initial = t.update(json!({"type":"render"})).unwrap();
        assert!(initial["rows"].as_array().unwrap().len() > 3);
        let delta = t.update(json!({"type":"frames","frames":[{"order":["1"],"turns":[turn("1","one\n\ntwo\n\nthree\n\nfour more")]}]})).unwrap();
        assert!(delta["order"].is_null());
        assert_eq!(delta["rows"].as_array().unwrap().len(), 1);
        t.update(
            json!({"type":"pending","pending":{"userCount":1,"turn":turn("pending:1","sent")}}),
        )
        .unwrap();
        let older = t
            .update(json!({"type":"prepend","turns":[turn("0","old")],"older":false}))
            .unwrap();
        assert_eq!(older["status"]["pendingId"], "pending:1");
        let echoed = t
            .update(
                json!({"type":"frames","frames":[{"order":["1","2"],"older":true,"turns":[turn("2","echo")]}]}),
            )
            .unwrap();
        assert!(echoed["status"]["pendingId"].is_null());
        assert_eq!(echoed["older"], false);
    }
    #[test]
    fn replacements_prune_rows_and_unknown_cursors_fail_without_losing_turns() {
        let mut t = Transcript::new(
            vec![json!({"id":"1","items":[{"type":"text","id":"t","text":"old"}]})],
            true,
        );
        t.update(json!({"type":"render"})).unwrap();
        assert!(t
            .update(json!({"type":"frames","frames":[{"order":["missing"]}]}))
            .is_err());
        let frame = t.update(json!({"type":"replace","turns":[]})).unwrap();
        assert_eq!(frame["order"], json!([]));
        assert!(t.cache.is_empty());
    }
    #[test]
    fn a_message_sent_while_a_mono_works_takes_its_mascot_and_stacks_under_the_last() {
        let working = json!({"id":"1","items":[
            {"id":"u","type":"user","text":"Hey"},
            {"id":"h","type":"header","live":true,"label":"Thinking…","entries":[]},
        ]});
        let mut t = Transcript::new(vec![working], true);
        let frame = t
            .update(
                json!({"type":"pending","pending":{"afterUserId":"1/u","userCount":1,
                "turn":{"id":"pending:1","items":[{"id":"u","type":"user","text":"You there"}]}}}),
            )
            .unwrap();
        assert_eq!(frame["order"], json!(["1/u", "pending:1/u", "1/h"]));
        let rows = frame["rows"].as_array().unwrap();
        let row = |id: &str| rows.iter().find(|r| r["id"] == id).unwrap();
        assert_eq!(row("1/u")["presentation"]["beforeUser"], true);
        assert_eq!(row("pending:1/u")["presentation"]["afterUser"], true);
        // The next frame resends none of the stacked copies.
        let again = t
            .update(json!({"type":"frames","frames":[{"order":["1"],"turns":[]}]}))
            .unwrap();
        assert_eq!(again["rows"], json!([]));
    }
    #[test]
    fn a_reopened_window_confirms_by_last_user_id_even_when_its_count_shrinks() {
        let turn =
            |id: &str| json!({"id":id,"items":[{"id":"u","type":"user","text":"same prompt"}]});
        let mut t = Transcript::new(vec![turn("1"), turn("2"), turn("3")], true);
        t.update(json!({"type":"pending","pending":{"afterUserId":"3/u","userCount":20,"turn":turn("pending:1")}})).unwrap();
        let before = t
            .update(json!({"type":"frames","frames":[{"order":["1","2","3"],"turns":[]}]}))
            .unwrap();
        assert_eq!(before["status"]["pendingId"], "pending:1");
        let after = t
            .update(json!({"type":"frames","frames":[{"order":["2","3","4"],"turns":[turn("4")]}]}))
            .unwrap();
        assert!(after["status"]["pendingId"].is_null());
        assert_eq!(after["status"]["lastUserId"], "4/u");
    }
    #[test]
    fn question_generation_survives_coalesced_clears_and_snapshot_refreshes() {
        let mut transcript = Transcript::new(vec![], false);
        for command in [
            json!({"type":"frames","frames":[{"question":null},{"question":null}]}),
            json!({"type":"replace","turns":[]}),
        ] {
            let snapshot = transcript.update(command).unwrap();
            assert_eq!(snapshot["status"]["questionGeneration"], 0);
        }
        let question = json!({"requestId":7,"questions":[]});
        let asked = transcript
            .update(json!({"type":"frames","frames":[{"question":question}]}))
            .unwrap();
        let first = asked["status"]["questionGeneration"].as_u64().unwrap();
        let replacement = transcript
            .update(json!({"type":"frames","frames":[{"question":null},{"question":question}]}))
            .unwrap();
        let generation = replacement["status"]["questionGeneration"]
            .as_u64()
            .unwrap();
        assert!(generation > first);
        assert_eq!(replacement["status"]["question"], question);
        let refreshed = json!({"requestId":7,"questions":[],"autoResolveAt":1234});
        for command in [
            json!({"type":"frames","frames":[{"question":refreshed}]}),
            json!({"type":"replace","turns":[],"question":refreshed}),
            json!({"type":"frames","frames":[{"busy":true}]}),
        ] {
            let snapshot = transcript.update(command).unwrap();
            assert_eq!(snapshot["status"]["questionGeneration"], generation);
        }
        let different = transcript
            .update(json!({"type":"frames","frames":[{"question":{"requestId":8,"questions":[]}}]}))
            .unwrap();
        assert!(different["status"]["questionGeneration"].as_u64().unwrap() > generation);
        let cleared = transcript
            .update(json!({"type":"frames","frames":[{"question":null}]}))
            .unwrap();
        let idle = transcript
            .update(json!({"type":"frames","frames":[{"question":null},{"question":null}]}))
            .unwrap();
        assert_eq!(
            idle["status"]["questionGeneration"],
            cleared["status"]["questionGeneration"]
        );
    }

    #[test]
    fn pending_question_holds_until_the_desktop_clears_it() {
        let mut t = Transcript::new(Vec::new(), true);
        let question = json!({"requestId":7,"questions":[{"id":"q","prompt":"Which?","multiSelect":false,"allowCustom":true,"options":[]}]});
        let asked = t
            .update(json!({"type":"frames","frames":[{"busy":true,"order":[],"turns":[],"question":question.clone()}]}))
            .unwrap();
        assert_eq!(asked["status"]["question"], question);
        let streaming = t
            .update(json!({"type":"frames","frames":[{"busy":true,"order":[],"turns":[]}]}))
            .unwrap();
        assert_eq!(streaming["status"]["question"], question);
        let answered = t
            .update(json!({"type":"frames","frames":[{"order":[],"turns":[],"question":null}]}))
            .unwrap();
        assert!(answered["status"]["question"].is_null());
        let read = t
            .update(json!({"type":"replace","turns":[],"busy":true,"question":question.clone()}))
            .unwrap();
        assert_eq!(read["status"]["question"], question);
    }
    #[test]
    fn completed_blocks_share_allocations_and_stream_completion_has_no_text_delta() {
        let mut md = Markdown::default();
        md.update("m", "one\n\ntwo\n\nthree\n\nfour");
        let first = md.blocks[0].rows[0].clone();
        md.update("m", "one\n\ntwo\n\nthree\n\nfour more");
        assert!(Arc::ptr_eq(&first, &md.blocks[0].rows[0]));
        let mut t = Transcript::new(Vec::new(), true);
        let mut turn = json!({"id":"1","items":[{"id":"t","type":"text","text":"**Answer**","streaming":true}]});
        t.update(json!({"type":"frames","frames":[{"order":["1"],"turns":[turn.clone()]}]}))
            .unwrap();
        turn["items"][0]["streaming"] = json!(false);
        let end = t
            .update(json!({"type":"frames","frames":[{"busy":false,"order":["1"],"turns":[turn]}]}))
            .unwrap();
        assert!(end["rows"].as_array().unwrap().is_empty());
        assert!(end["order"].is_null());
    }
    #[test]
    fn fold_actions_and_selection_keep_the_source_text() {
        let mut t = Transcript::new(
            vec![
                json!({"id":"1","items":[{"id":"h","type":"header","entries":[{"type":"prose","id":"p","text":"hidden **detail**"}]},{"id":"r","type":"text","text":"# Reply\n\nwhole reply"}]}),
            ],
            true,
        );
        let initial = t.update(json!({"type":"render"})).unwrap();
        assert_eq!(initial["rows"].as_array().unwrap().len(), 3);
        let open = t
            .update(json!({"type":"toggle","id":"1/h","open":false}))
            .unwrap();
        assert!(open["order"].as_array().unwrap().len() > 3);
        let selected = t.update(json!({"type":"selection","id":"1/r:0"})).unwrap();
        assert_eq!(selected["text"], "# Reply\n\nwhole reply");
        let detail = t
            .update(json!({"type":"selection","id":"1/h/p:0"}))
            .unwrap();
        assert_eq!(detail["text"], "hidden **detail**");
    }
}
