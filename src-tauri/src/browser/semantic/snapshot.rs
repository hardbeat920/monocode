//! Snapshot (spec 8): a compact text tree of the accessibility (AX) tree of
//! the main frame and its iframes, with a ref on every node the agent can
//! act on.
//!
//! Source: `Accessibility.getFullAXTree` in the session of each frame (no
//! `Accessibility.enable`). Same-process iframes are fetched with their
//! `frameId` in the parent's session, out-of-process iframes (OOPIFs) in
//! their own session; both are grafted under their `<iframe>` node and
//! marked `[frame]`. The AX tree sees closed shadow roots, which page
//! scripts cannot, so there is no DOM walk in JS. Only `backendDOMNodeId`
//! is kept per ref; boxes are looked up at action time.
//!
//! Format (close to the ARIA snapshot models already read well):
//!
//! ```text
//! - document "Login" [ref=e1]
//!   - heading "Sign in" [level=1] [ref=e2]
//!   - textbox "Email" [required] [ref=e3]: ana@example.com
//!   - button "Continue" [ref=e4]
//! ```

use std::collections::HashMap;

use serde_json::{json, Value};

use super::super::link::{CdpError, Method};
use super::super::protocol::{strip_marker, ActionResult, Reason, SnapshotMode, SnapshotParams};
use super::element;
use super::refs::{self, FrameCtx};
use super::{Fail, Run};

/// Node budget (spec 8 "Kompaktovanje i limiti").
pub const MAX_NODES: u32 = 2000;
/// Output limit in bytes.
pub const MAX_BYTES: usize = 100 * 1024;
const MAX_NAME: usize = 120;
const MAX_TEXT: usize = 200;
const MAX_VALUE: usize = 80;
const MAX_HREF: usize = 200;
/// Iframes fetched per snapshot, and how deep frames may nest.
const MAX_FRAMES: usize = 16;
const MAX_FRAME_DEPTH: usize = 4;

/// Roles that get a ref (spec 8 "Format").
const REF_ROLES: [&str; 20] = [
    "link",
    "button",
    "textbox",
    "checkbox",
    "radio",
    "combobox",
    "listbox",
    "option",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "tab",
    "slider",
    "switch",
    "searchbox",
    "spinbutton",
    "heading",
    "RootWebArea",
    "Iframe",
    "treeitem",
];

/// Roles dropped when they carry no name (their children move up).
const DROP_ROLES: [&str; 6] = [
    "generic",
    "none",
    "presentation",
    "InlineTextBox",
    "LineBreak",
    "ListMarker",
];

#[derive(Clone, Debug, Default)]
pub struct AxNode {
    pub role: String,
    pub name: String,
    pub value: Option<String>,
    pub flags: Vec<String>,
    pub backend: Option<i64>,
    pub ignored: bool,
    pub children: Vec<usize>,
    /// Index into the frame list.
    pub frame: usize,
    /// Root of a grafted iframe document.
    pub frame_root: bool,
}

fn ax_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn flags(node: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(properties) = node["properties"].as_array() else {
        return out;
    };
    let mut href = None;
    for property in properties {
        let name = property["name"].as_str().unwrap_or("");
        let value = &property["value"]["value"];
        let truthy = matches!(value, Value::Bool(true)) || value.as_str() == Some("true");
        match name {
            "checked" | "pressed" => {
                if truthy {
                    out.push(name.to_string());
                } else if value.as_str() == Some("mixed") {
                    out.push(format!("{name}=mixed"));
                }
            }
            "expanded" => out.push(if truthy {
                "expanded".into()
            } else {
                "expanded=false".into()
            }),
            "disabled" | "focused" | "required" | "selected" | "readonly" | "modal" => {
                if truthy {
                    out.push(name.to_string());
                }
            }
            "invalid" => {
                if value.as_str().is_some_and(|v| v != "false") || truthy {
                    out.push("invalid".into());
                }
            }
            "level" => {
                if let Some(level) = ax_string(value) {
                    out.push(format!("level={level}"));
                }
            }
            "url" => href = ax_string(value),
            _ => {}
        }
    }
    if let Some(href) = href.filter(|h| !h.is_empty()) {
        out.push(format!("href={}", clip(&href, MAX_HREF)));
    }
    out
}

/// Add one `getFullAXTree` answer to the arena; returns the root's index.
pub fn add_tree(arena: &mut Vec<AxNode>, nodes: &[Value], frame: usize) -> Option<usize> {
    let base = arena.len();
    let mut index: HashMap<&str, usize> = HashMap::new();
    for (offset, node) in nodes.iter().enumerate() {
        if let Some(id) = node["nodeId"].as_str() {
            index.insert(id, base + offset);
        }
    }
    let mut has_parent = vec![false; nodes.len()];
    for node in nodes {
        let role = node["role"]["value"].as_str().unwrap_or("").to_string();
        let mut name = node["name"]["value"].as_str().unwrap_or("").to_string();
        if role == "RootWebArea" {
            name = strip_marker(&name).to_string();
        }
        let children: Vec<usize> = node["childIds"]
            .as_array()
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| id.as_str().and_then(|id| index.get(id).copied()))
                    .collect()
            })
            .unwrap_or_default();
        for child in &children {
            has_parent[child - base] = true;
        }
        arena.push(AxNode {
            role,
            name,
            value: ax_string(&node["value"]["value"]).filter(|v| !v.is_empty()),
            flags: flags(node),
            backend: node["backendDOMNodeId"].as_i64(),
            ignored: node["ignored"].as_bool().unwrap_or(false),
            children,
            frame,
            frame_root: false,
        });
    }
    (0..nodes.len()).find(|i| !has_parent[*i]).map(|i| base + i)
}

/// Collapse whitespace and cut to `max` characters with an ellipsis.
pub fn clip(text: &str, max: usize) -> String {
    let squashed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if squashed.chars().count() <= max {
        return squashed;
    }
    let mut out: String = squashed.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

pub struct RenderOpts {
    pub mode: SnapshotMode,
    pub max_nodes: u32,
}

pub struct Rendered {
    pub text: String,
    pub refs: usize,
    pub truncated: bool,
}

fn ref_worthy(node: &AxNode, mode: SnapshotMode) -> bool {
    if node.ignored || node.backend.is_none() {
        return false;
    }
    if REF_ROLES.contains(&node.role.as_str()) {
        return true;
    }
    mode == SnapshotMode::Full && node.role == "StaticText" && node.name.trim().chars().count() > 2
}

struct Renderer<'a> {
    arena: &'a [AxNode],
    opts: &'a RenderOpts,
    keep: Vec<Option<bool>>,
    lines: Vec<String>,
    refs: usize,
    budget_hit: bool,
    skipped: usize,
}

impl Renderer<'_> {
    fn keep(&mut self, i: usize) -> bool {
        if let Some(known) = self.keep[i] {
            return known;
        }
        // Mark before recursing so a malformed (cyclic) tree terminates.
        self.keep[i] = Some(false);
        let node = &self.arena[i];
        let mut keep = !node.ignored && ref_worthy(node, self.opts.mode);
        let children = node.children.clone();
        for child in children {
            if self.keep(child) {
                keep = true;
            }
        }
        self.keep[i] = Some(keep);
        keep
    }

    fn kept_children(&mut self, i: usize) -> Vec<usize> {
        let children = self.arena[i].children.clone();
        children.into_iter().filter(|c| self.keep(*c)).collect()
    }

    fn count_kept(&mut self, i: usize) -> usize {
        let children = self.kept_children(i);
        1 + children
            .into_iter()
            .map(|c| self.count_kept(c))
            .sum::<usize>()
    }

    fn emit(
        &mut self,
        i: usize,
        depth: usize,
        parent_name: Option<&str>,
        assign: &mut dyn FnMut(usize, i64, &str, &str) -> u32,
    ) {
        if !self.keep(i) {
            return;
        }
        if self.budget_hit {
            self.skipped += self.count_kept(i);
            return;
        }
        let node = self.arena[i].clone();
        let children = self.kept_children(i);
        let worthy = ref_worthy(&node, self.opts.mode);
        let unnamed = node.name.trim().is_empty();
        let duplicate_text =
            node.role == "StaticText" && parent_name.is_some_and(|p| p.contains(node.name.trim()));
        let skip_line = node.ignored
            || node.role == "InlineTextBox"
            || node.role == "ListMarker"
            || (DROP_ROLES.contains(&node.role.as_str()) && unnamed)
            || (!worthy && unnamed && children.len() <= 1)
            || duplicate_text
            || (node.role == "StaticText" && !worthy);
        if skip_line {
            for child in children {
                self.emit(child, depth, parent_name, assign);
            }
            return;
        }
        if self.lines.len() as u32 >= self.opts.max_nodes {
            self.budget_hit = true;
            self.skipped += self.count_kept(i);
            return;
        }
        let role = match node.role.as_str() {
            "RootWebArea" => "document",
            "Iframe" => "iframe",
            "StaticText" => "text",
            other => other,
        };
        let mut line = format!("{}- {role}", "  ".repeat(depth));
        if !unnamed {
            let max = if node.role == "StaticText" {
                MAX_TEXT
            } else {
                MAX_NAME
            };
            line.push(' ');
            line.push_str(&quoted(&clip(&node.name, max)));
        }
        for flag in &node.flags {
            line.push_str(&format!(" [{flag}]"));
        }
        if node.role == "Iframe" && children.iter().any(|c| self.arena[*c].frame_root) {
            line.push_str(" [frame]");
        }
        if worthy {
            if let Some(backend) = node.backend {
                let number = assign(node.frame, backend, &node.role, &node.name);
                line.push_str(&format!(" [ref=e{number}]"));
                self.refs += 1;
            }
        }
        if let Some(value) = node.value.as_deref() {
            if value.trim() != node.name.trim() {
                line.push_str(": ");
                line.push_str(&clip(value, MAX_VALUE));
            }
        }
        self.lines.push(line);
        let name_for_children = if worthy && !unnamed && node.role != "RootWebArea" {
            Some(node.name.clone())
        } else {
            None
        };
        for child in children {
            self.emit(child, depth + 1, name_for_children.as_deref(), assign);
        }
    }
}

/// Render the subtree at `root` (spec 8 "Format", "Kompaktovanje i
/// limiti"). `assign` gives the ref number of `(frame, backend)`.
pub fn render(
    arena: &[AxNode],
    root: usize,
    opts: &RenderOpts,
    assign: &mut dyn FnMut(usize, i64, &str, &str) -> u32,
) -> Rendered {
    let mut renderer = Renderer {
        arena,
        opts,
        keep: vec![None; arena.len()],
        lines: Vec::new(),
        refs: 0,
        budget_hit: false,
        skipped: 0,
    };
    renderer.emit(root, 0, None, assign);
    let mut truncated = renderer.budget_hit;
    let mut text = String::new();
    let mut written = 0usize;
    for line in &renderer.lines {
        if text.len() + line.len() + 1 > MAX_BYTES {
            truncated = true;
            break;
        }
        text.push_str(line);
        text.push('\n');
        written += 1;
    }
    let remaining = renderer.skipped + (renderer.lines.len() - written);
    if truncated {
        text.push_str(&format!(
            "[... {remaining} more nodes; use scope with a ref to see a part of the page]\n"
        ));
    }
    Rendered {
        text,
        refs: renderer.refs,
        truncated,
    }
}

fn timeout_as_snapshot(fail: Fail) -> Fail {
    match fail {
        Fail::Cdp(CdpError::Timeout, _) | Fail::Reason(Reason::Timeout, _) => {
            Fail::reason(Reason::SnapshotTimeout)
        }
        other => other,
    }
}

fn fetch(run: &Run<'_>, frame: &FrameCtx, frame_id: Option<&str>) -> Result<Vec<Value>, Fail> {
    let params = match frame_id {
        Some(id) => json!({ "frameId": id }),
        None => json!({}),
    };
    let tree = run.send(&frame.session, Method::AccessibilityGetFullAXTree, params)?;
    Ok(tree["nodes"].as_array().cloned().unwrap_or_default())
}

/// Main frame plus iframes, as one arena.
fn gather(run: &Run<'_>) -> Result<(Vec<AxNode>, Vec<FrameCtx>, usize), Fail> {
    let main = element::main_frame(run)?;
    let mut arena = Vec::new();
    let nodes = fetch(run, &main, None)?;
    let root = add_tree(&mut arena, &nodes, 0).ok_or_else(|| {
        Fail::detail(
            Reason::SnapshotTimeout,
            "The page has no accessibility tree yet; wait for it to load and retry.",
        )
    })?;
    let mut frames = vec![main];
    let mut scanned = 0;
    let mut fetched = 0;
    while scanned < arena.len() && fetched < MAX_FRAMES {
        let i = scanned;
        scanned += 1;
        let node = &arena[i];
        if node.role != "Iframe" || node.ignored {
            continue;
        }
        let Some(backend) = node.backend else {
            continue;
        };
        if !node.children.is_empty() {
            continue;
        }
        let parent = frames[node.frame].clone();
        if parent.owners.len() >= MAX_FRAME_DEPTH {
            continue;
        }
        let described = match run.send(
            &parent.session,
            Method::DomDescribeNode,
            json!({ "backendNodeId": backend }),
        ) {
            Ok(described) => described,
            Err(fail) if fail.protocol_message().is_some() => continue,
            Err(fail) => return Err(fail),
        };
        let Some(frame_id) = described["node"]["frameId"].as_str().map(str::to_string) else {
            continue;
        };
        let child_session =
            run.lease(|l| l.frames.get(&frame_id).map(|f| f.cdp_session.clone()))?;
        let (frame, request_id) = match child_session {
            Some(session) => {
                let mut owners = parent.owners.clone();
                owners.push((parent.session.clone(), backend));
                (
                    FrameCtx {
                        session,
                        frame_id: Some(frame_id.clone()),
                        owners,
                    },
                    None,
                )
            }
            None => (
                FrameCtx {
                    session: parent.session.clone(),
                    frame_id: Some(frame_id.clone()),
                    owners: parent.owners.clone(),
                },
                Some(frame_id.clone()),
            ),
        };
        fetched += 1;
        let nodes = match fetch(run, &frame, request_id.as_deref()) {
            Ok(nodes) => nodes,
            Err(fail) if fail.protocol_message().is_some() => continue,
            Err(fail) => return Err(fail),
        };
        let index = frames.len();
        frames.push(frame);
        if let Some(child_root) = add_tree(&mut arena, &nodes, index) {
            arena[child_root].frame_root = true;
            arena[i].children.push(child_root);
        }
    }
    Ok((arena, frames, root))
}

/// `snapshot` (spec 8).
pub fn snapshot(run: &Run<'_>, p: SnapshotParams) -> Result<ActionResult, Fail> {
    snapshot_inner(run, p).map_err(timeout_as_snapshot)
}

fn snapshot_inner(run: &Run<'_>, p: SnapshotParams) -> Result<ActionResult, Fail> {
    let mode = p.mode.unwrap_or_default();
    let max_nodes = p.max_nodes.unwrap_or(MAX_NODES).clamp(1, MAX_NODES);
    let scope = match p.scope.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => {
            let state = run.link.policy().read();
            let lease = state
                .lease(run.tab())
                .ok_or_else(|| Fail::reason(Reason::TargetClosed))?;
            Some((raw.to_string(), refs::lookup(&run.key(), lease, raw)?))
        }
        None => None,
    };
    let (arena, frames, root) = gather(run)?;
    let start = match &scope {
        Some((raw, node)) => arena
            .iter()
            .position(|n| {
                n.backend == Some(node.backend) && frames[n.frame].session == node.frame.session
            })
            .ok_or_else(|| {
                Fail::detail(
                    Reason::RefStale,
                    format!(
                        "Ref {raw} is not in the page any more; take a snapshot without scope."
                    ),
                )
            })?,
        None => root,
    };
    let (version, loader, url, title) = {
        let mut state = run.link.policy().write();
        let lease = state
            .lease_mut(run.tab())
            .ok_or_else(|| Fail::reason(Reason::TargetClosed))?;
        lease.snapshot_version += 1;
        (
            lease.snapshot_version,
            lease.main_loader_id.clone(),
            lease.url.clone(),
            strip_marker(&lease.title).to_string(),
        )
    };
    let opts = RenderOpts { mode, max_nodes };
    let rendered = refs::with_snapshot_table(&run.key(), loader.as_deref(), version, |table| {
        render(&arena, start, &opts, &mut |frame, backend, role, name| {
            table.assign(&frames[frame], backend, role, name)
        })
    });
    let header = format!(
        "url: {url}\ntitle: {}\nsnapshot: v{version}, {} refs{}\n",
        quoted(&clip(&title, MAX_NAME)),
        rendered.refs,
        if rendered.truncated {
            ", truncated"
        } else {
            ""
        }
    );
    let text = format!("{header}{}", rendered.text);
    let message = format!(
        "Snapshot v{version} of {url}: {} refs{}. Act with refs like `e3`; take a new snapshot after the page changes.",
        rendered.refs,
        if rendered.truncated {
            " (truncated; use scope with a ref for the rest)"
        } else {
            ""
        }
    );
    Ok(ActionResult::done(run.id(), message)
        .with_data(json!({
            "snapshot": text,
            "version": version,
            "url": url,
            "title": title,
            "refs": rendered.refs,
            "truncated": rendered.truncated,
        }))
        .with_observed(run.observed_now()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ax(id: &str, role: &str, name: &str, backend: i64, children: &[&str]) -> Value {
        json!({
            "nodeId": id,
            "ignored": false,
            "role": { "type": "role", "value": role },
            "name": { "type": "computedString", "value": name },
            "backendDOMNodeId": backend,
            "childIds": children,
        })
    }

    fn sample() -> Vec<Value> {
        let mut textbox = ax("5", "textbox", "Email", 105, &[]);
        textbox["value"] = json!({ "type": "string", "value": "ana@example.com" });
        textbox["properties"] = json!([
            { "name": "required", "value": { "type": "boolean", "value": true } },
            { "name": "focused", "value": { "type": "boolean", "value": true } },
        ]);
        let mut heading = ax("3", "heading", "Sign in", 103, &["4"]);
        heading["properties"] =
            json!([{ "name": "level", "value": { "type": "integer", "value": 1 } }]);
        let mut link = ax("8", "link", "Forgot \"password\"?", 108, &["9"]);
        link["properties"] =
            json!([{ "name": "url", "value": { "type": "string", "value": "https://x/reset" } }]);
        let mut ignored = ax("10", "generic", "", 110, &["11"]);
        ignored["ignored"] = json!(true);
        vec![
            ax(
                "1",
                "RootWebArea",
                "\u{25C6} MonoCode \u{B7} Login",
                101,
                &["2"],
            ),
            ax("2", "generic", "", 102, &["3", "5", "6", "8", "10"]),
            heading,
            ax("4", "StaticText", "Sign in", 104, &[]),
            textbox,
            ax("6", "paragraph", "", 106, &["7"]),
            ax(
                "7",
                "StaticText",
                "We never share your email address.",
                107,
                &[],
            ),
            link,
            ax("9", "StaticText", "Forgot \"password\"?", 109, &[]),
            ignored,
            ax("11", "button", "Continue", 111, &[]),
        ]
    }

    fn numbering() -> impl FnMut(usize, i64, &str, &str) -> u32 {
        let mut seen: HashMap<i64, u32> = HashMap::new();
        move |_, backend, _, _| {
            let next = seen.len() as u32 + 1;
            *seen.entry(backend).or_insert(next)
        }
    }

    #[test]
    fn interactive_snapshot_keeps_refs_and_structure_only() {
        let mut arena = Vec::new();
        let root = add_tree(&mut arena, &sample(), 0).unwrap();
        let opts = RenderOpts {
            mode: SnapshotMode::Interactive,
            max_nodes: MAX_NODES,
        };
        let out = render(&arena, root, &opts, &mut numbering());
        assert_eq!(
            out.text,
            "- document \"Login\" [ref=e1]\n\
             \x20 - heading \"Sign in\" [level=1] [ref=e2]\n\
             \x20 - textbox \"Email\" [required] [focused] [ref=e3]: ana@example.com\n\
             \x20 - link \"Forgot \\\"password\\\"?\" [href=https://x/reset] [ref=e4]\n\
             \x20 - button \"Continue\" [ref=e5]\n"
        );
        assert_eq!(out.refs, 5);
        assert!(!out.truncated);
    }

    #[test]
    fn full_snapshot_adds_text_without_repeating_names() {
        let mut arena = Vec::new();
        let root = add_tree(&mut arena, &sample(), 0).unwrap();
        let opts = RenderOpts {
            mode: SnapshotMode::Full,
            max_nodes: MAX_NODES,
        };
        let out = render(&arena, root, &opts, &mut numbering());
        assert!(out
            .text
            .contains("  - text \"We never share your email address.\" [ref="));
        // The heading's and the link's own text are not repeated.
        assert_eq!(out.text.matches("Sign in").count(), 1);
        assert_eq!(out.text.matches("Forgot").count(), 1);
    }

    #[test]
    fn budget_and_long_names_are_cut_with_a_hint() {
        let mut nodes = vec![ax("r", "RootWebArea", "Big", 1, &[])];
        let mut children = Vec::new();
        for n in 0..50 {
            let id = format!("b{n}");
            nodes.push(ax(&id, "button", &"x".repeat(500), 10 + n, &[]));
            children.push(id);
        }
        nodes[0]["childIds"] = json!(children);
        let mut arena = Vec::new();
        let root = add_tree(&mut arena, &nodes, 0).unwrap();
        let opts = RenderOpts {
            mode: SnapshotMode::Interactive,
            max_nodes: 10,
        };
        let out = render(&arena, root, &opts, &mut numbering());
        assert!(out.truncated);
        assert_eq!(out.text.lines().count(), 11);
        assert!(out
            .text
            .ends_with("[... 41 more nodes; use scope with a ref to see a part of the page]\n"));
        let first_button = out.text.lines().nth(1).unwrap();
        assert!(first_button.contains(&format!("{}…", "x".repeat(MAX_NAME - 1))));
        assert_eq!(out.refs, 10);
    }

    #[test]
    fn clip_squashes_whitespace() {
        assert_eq!(clip("  a \n\t b  ", 10), "a b");
        assert_eq!(clip("abcdef", 4), "abc…");
    }
}
