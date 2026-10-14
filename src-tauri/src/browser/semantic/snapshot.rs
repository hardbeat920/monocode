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

use serde::Deserialize;
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

fn flags(properties: &[AxProperty]) -> Vec<String> {
    let mut out = Vec::new();
    let mut href = None;
    for property in properties {
        let name = property.name.as_str();
        let value = &property.value.value;
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
        out.push(format!("href={href}"));
    }
    out
}

/// One node of a `getFullAXTree` answer, with only the fields the snapshot
/// reads; the rest (name sources, ignored reasons, ...) is skipped while
/// parsing, which is most of the answer's size.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawAxNode {
    #[serde(default)]
    node_id: String,
    #[serde(default)]
    ignored: bool,
    role: Option<AxValue>,
    name: Option<AxValue>,
    value: Option<AxValue>,
    #[serde(default)]
    properties: Vec<AxProperty>,
    #[serde(default)]
    child_ids: Vec<String>,
    #[serde(rename = "backendDOMNodeId")]
    backend_dom_node_id: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct AxValue {
    #[serde(default)]
    value: Value,
}

#[derive(Debug, Deserialize)]
struct AxProperty {
    #[serde(default)]
    name: String,
    #[serde(default)]
    value: AxValue,
}

#[derive(Default, Deserialize)]
struct AxTree {
    #[serde(default)]
    nodes: Vec<RawAxNode>,
}

impl AxValue {
    fn into_string(self) -> String {
        match self.value {
            Value::String(s) => s,
            _ => String::new(),
        }
    }
}

/// Add one `getFullAXTree` answer to the arena; returns the root's index.
pub fn add_tree(arena: &mut Vec<AxNode>, nodes: Vec<RawAxNode>, frame: usize) -> Option<usize> {
    let base = arena.len();
    let children: Vec<Vec<usize>> = {
        let index: HashMap<&str, usize> = nodes
            .iter()
            .enumerate()
            .map(|(offset, node)| (node.node_id.as_str(), base + offset))
            .collect();
        nodes
            .iter()
            .map(|node| {
                node.child_ids
                    .iter()
                    .filter_map(|id| index.get(id.as_str()).copied())
                    .collect()
            })
            .collect()
    };
    let mut has_parent = vec![false; nodes.len()];
    for child in children.iter().flatten() {
        has_parent[child - base] = true;
    }
    arena.reserve(nodes.len());
    for (node, children) in nodes.into_iter().zip(children) {
        let role = node.role.map(AxValue::into_string).unwrap_or_default();
        let mut name = node.name.map(AxValue::into_string).unwrap_or_default();
        if role == "RootWebArea" {
            name = strip_marker(&name).to_string();
        }
        arena.push(AxNode {
            role,
            name,
            value: node
                .value
                .and_then(|v| ax_string(&v.value))
                .filter(|v| !v.is_empty()),
            flags: flags(&node.properties),
            backend: node.backend_dom_node_id,
            ignored: node.ignored,
            children,
            frame,
            frame_root: false,
        });
    }
    (0..has_parent.len())
        .find(|i| !has_parent[*i])
        .map(|i| base + i)
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
    /// `(frame, backendNodeId)` of the element the user pointed at: always
    /// shown, with a ref, and marked.
    pub picked: Option<(usize, i64)>,
    /// Origin of the page (`https://host:port`): links to it are shown as
    /// their path, which is most of a link's line on a typical page.
    pub origin: Option<String>,
}

/// `href` as shown: the path when it is on the page's own origin, and cut
/// to a length a model reads at a glance.
fn shown_href(href: &str, origin: Option<&str>) -> String {
    let local = origin
        .and_then(|origin| href.strip_prefix(origin))
        .filter(|rest| rest.starts_with('/'));
    clip(local.unwrap_or(href), MAX_HREF)
}

pub struct Rendered {
    pub text: String,
    pub refs: usize,
    pub truncated: bool,
    /// The ref of the picked element, when it was rendered.
    pub picked: Option<u32>,
}

/// Flag on the line of the element the user pointed at.
pub const PICKED_FLAG: &str = "[picked by the user]";

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
    picked: Option<u32>,
}

impl Renderer<'_> {
    fn is_picked(&self, node: &AxNode) -> bool {
        !node.ignored
            && node
                .backend
                .is_some_and(|backend| self.opts.picked == Some((node.frame, backend)))
    }

    fn keep(&mut self, i: usize) -> bool {
        if let Some(known) = self.keep[i] {
            return known;
        }
        // Mark before recursing so a malformed (cyclic) tree terminates.
        self.keep[i] = Some(false);
        let arena = self.arena;
        let node = &arena[i];
        let mut keep = !node.ignored && ref_worthy(node, self.opts.mode) || self.is_picked(node);
        for &child in &node.children {
            if self.keep(child) {
                keep = true;
            }
        }
        self.keep[i] = Some(keep);
        keep
    }

    fn kept_children(&mut self, i: usize) -> Vec<usize> {
        let arena = self.arena;
        arena[i]
            .children
            .iter()
            .copied()
            .filter(|c| self.keep(*c))
            .collect()
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
        let arena = self.arena;
        let node = &arena[i];
        let children = self.kept_children(i);
        let picked = self.is_picked(node);
        let worthy = ref_worthy(node, self.opts.mode) || picked;
        let unnamed = node.name.trim().is_empty();
        let duplicate_text =
            node.role == "StaticText" && parent_name.is_some_and(|p| p.contains(node.name.trim()));
        let skip_line = !picked
            && (node.ignored
                || node.role == "InlineTextBox"
                || node.role == "ListMarker"
                || (DROP_ROLES.contains(&node.role.as_str()) && unnamed)
                || (!worthy && unnamed && children.len() <= 1)
                || duplicate_text
                || (node.role == "StaticText" && !worthy));
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
            match flag.strip_prefix("href=") {
                // The document's own address is in the header.
                Some(_) if node.role == "RootWebArea" => {}
                // A list item's level is its indentation.
                None if node.role == "listitem" && flag.starts_with("level=") => {}
                Some(href) => {
                    let href = shown_href(href, self.opts.origin.as_deref());
                    line.push_str(&format!(" [href={href}]"));
                }
                None => line.push_str(&format!(" [{flag}]")),
            }
        }
        if node.role == "Iframe" && children.iter().any(|c| self.arena[*c].frame_root) {
            line.push_str(" [frame]");
        }
        if worthy {
            if let Some(backend) = node.backend {
                let number = assign(node.frame, backend, &node.role, &node.name);
                line.push_str(&format!(" [ref=e{number}]"));
                self.refs += 1;
                if picked {
                    line.push(' ');
                    line.push_str(PICKED_FLAG);
                    self.picked = Some(number);
                }
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
        picked: None,
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
    // A picked line cut off by the size limit is not in the text.
    let picked = renderer
        .picked
        .filter(|number| text.contains(&format!("[ref=e{number}] {PICKED_FLAG}")));
    Rendered {
        text,
        refs: renderer.refs,
        truncated,
        picked,
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

fn fetch(run: &Run<'_>, frame: &FrameCtx, frame_id: Option<&str>) -> Result<Vec<RawAxNode>, Fail> {
    let params = match frame_id {
        Some(id) => json!({ "frameId": id }),
        None => json!({}),
    };
    let tree: AxTree = run.send_as(&frame.session, Method::AccessibilityGetFullAXTree, params)?;
    Ok(tree.nodes)
}

/// Main frame plus iframes, as one arena.
fn gather(run: &Run<'_>) -> Result<(Vec<AxNode>, Vec<FrameCtx>, usize), Fail> {
    let main = element::main_frame(run)?;
    let mut arena = Vec::new();
    let nodes = fetch(run, &main, None)?;
    let root = add_tree(&mut arena, nodes, 0).ok_or_else(|| {
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
        if let Some(child_root) = add_tree(&mut arena, nodes, index) {
            arena[child_root].frame_root = true;
            arena[i].children.push(child_root);
        }
    }
    Ok((arena, frames, root))
}

/// `snapshot` (spec 8).
pub fn snapshot(run: &Run<'_>, p: SnapshotParams) -> Result<ActionResult, Fail> {
    let taken = take(run, &p, false).map_err(timeout_as_snapshot)?;
    let Taken {
        header,
        body,
        version,
        url,
        title,
        refs,
        truncated,
        picked,
        ..
    } = taken;
    let mut message = format!(
        "Snapshot v{version} of {url}: {refs} refs{}. Act with refs like `e3`; take a new snapshot after the page changes.",
        if truncated {
            " (truncated; use scope with a ref for the rest)"
        } else {
            ""
        }
    );
    if let Some(number) = picked {
        message.push_str(&format!(" The user pointed at e{number} in the browser."));
    }
    Ok(ActionResult::done(run.id(), message)
        .with_data(json!({
            "snapshot": format!("{header}{body}"),
            "version": version,
            "url": url,
            "title": title,
            "refs": refs,
            "truncated": truncated,
            "picked": picked.map(|number| format!("e{number}")),
        }))
        .with_observed(run.observed_now()))
}

/// One rendered snapshot.
struct Taken {
    header: String,
    body: String,
    version: u64,
    url: String,
    title: String,
    refs: usize,
    truncated: bool,
    picked: Option<u32>,
    /// The page with its text (full mode), for whole-page snapshots only.
    text_body: String,
    /// The previous whole-page snapshot of this document (its full-mode body),
    /// when this one is a whole-page snapshot too.
    previous: Option<refs::LastSnapshot>,
}

/// Gather and render a snapshot. Without a `mode` in `p` it is the mode of
/// the document's last whole-page snapshot when `inherit` (an action's
/// snapshot looks like the one the agent asked for), else the default.
fn take(run: &Run<'_>, p: &SnapshotParams, inherit: bool) -> Result<Taken, Fail> {
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
    let whole_page = scope.is_none() && max_nodes == MAX_NODES;
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
    let (rendered, previous, text_body) =
        refs::with_snapshot_table(&run.key(), loader.as_deref(), version, |table| {
            let mode = match (p.mode, inherit, &table.last) {
                (Some(mode), _, _) => mode,
                (None, true, Some(last)) => last.mode,
                _ => SnapshotMode::default(),
            };
            // Backend ids are unique within a session; same-process frames share
            // their parent's session, so the node's own tree names its frame.
            let picked = refs::picked(table).and_then(|(session, backend)| {
                let node = arena
                    .iter()
                    .find(|n| n.backend == Some(*backend) && frames[n.frame].session == *session)?;
                Some((node.frame, *backend))
            });
            let opts = RenderOpts {
                mode,
                max_nodes,
                picked,
                origin: url::Url::parse(&url)
                    .ok()
                    .map(|u| u.origin())
                    .filter(url::Origin::is_tuple)
                    .map(|o| o.ascii_serialization()),
            };
            let rendered = render(&arena, start, &opts, &mut |frame, backend, role, name| {
                table.assign(&frames[frame], backend, role, name)
            });
            // Changes after an action are computed on the page with its text
            // (full mode), so a message such as "Saved" shows up even when the
            // agent reads interactive snapshots. Text nodes get refs too.
            let (previous, text_body) = if whole_page {
                let text_body = if mode == SnapshotMode::Full {
                    rendered.text.clone()
                } else {
                    let opts = RenderOpts {
                        mode: SnapshotMode::Full,
                        ..opts
                    };
                    render(&arena, start, &opts, &mut |frame, backend, role, name| {
                        table.assign(&frames[frame], backend, role, name)
                    })
                    .text
                };
                let previous = table.last.take();
                table.last = Some(refs::LastSnapshot {
                    version,
                    mode,
                    body: text_body.clone(),
                });
                (previous, text_body)
            } else {
                (None, String::new())
            };
            (rendered, previous, text_body)
        });
    let mut header = format!(
        "url: {url}\ntitle: {}\nsnapshot: v{version}, {} refs{}\n",
        quoted(&clip(&title, MAX_NAME)),
        rendered.refs,
        if rendered.truncated {
            ", truncated"
        } else {
            ""
        }
    );
    if let Some(number) = rendered.picked {
        header.push_str(&format!(
            "picked: e{number} is the element the user pointed at in the browser\n"
        ));
    }
    Ok(Taken {
        header,
        body: rendered.text,
        version,
        url,
        title,
        refs: rendered.refs,
        truncated: rendered.truncated,
        picked: rendered.picked,
        previous,
        text_body,
    })
}

/// The page snapshot an action returns (see [`after_action`]).
pub struct Inline {
    pub text: String,
    pub version: u64,
    /// `full`, `changes` or `unchanged`.
    pub kind: &'static str,
}

/// Snapshot of the page after an action: the whole page when there is no
/// earlier snapshot of this document (a navigation starts a new one), else
/// only what changed since the last one, when that is clearly shorter.
pub fn after_action(run: &Run<'_>) -> Result<Inline, Fail> {
    let params = SnapshotParams {
        tab: run.tab().clone(),
        scope: None,
        mode: None,
        max_nodes: None,
        timeout_ms: None,
    };
    let taken = take(run, &params, true).map_err(timeout_as_snapshot)?;
    let version = taken.version;
    let full = || Inline {
        text: format!("{}{}", taken.header, taken.body),
        version,
        kind: "full",
    };
    let Some(previous) = &taken.previous else {
        return Ok(full());
    };
    let head = format!(
        "url: {}\ntitle: {}\n",
        taken.url,
        quoted(&clip(&taken.title, MAX_NAME))
    );
    match changes(&previous.body, &taken.text_body) {
        Some(Changes::None) => Ok(Inline {
            text: format!(
                "{head}snapshot: v{version}, no change since v{} ({} refs)\n",
                previous.version, taken.refs
            ),
            version,
            kind: "unchanged",
        }),
        Some(Changes::Some {
            text,
            added,
            removed,
        }) if text.len() * 10 < (taken.header.len() + taken.body.len()) * 6 => {
            let mut summary = format!(
                "{head}snapshot: v{version}, changes since v{}: {added} line(s) added, {removed} removed; lines not shown are as before and their refs still work\n",
                previous.version
            );
            if let Some(number) = taken.picked {
                summary.push_str(&format!(
                    "picked: e{number} is the element the user pointed at in the browser\n"
                ));
            }
            Ok(Inline {
                text: format!("{summary}{text}"),
                version,
                kind: "changes",
            })
        }
        _ => Ok(full()),
    }
}

/// What [`changes`] found between two snapshot bodies.
#[derive(Debug, PartialEq)]
pub enum Changes {
    None,
    Some {
        text: String,
        added: usize,
        removed: usize,
    },
}

/// Lines of change context shown around each change.
const CONTEXT: usize = 1;
/// Largest middle part (old lines x new lines) compared line by line.
const MAX_DIFF_CELLS: usize = 1_000_000;

/// A line diff of two snapshot bodies in unified style: `-` for a removed
/// line, `+` for an added one, a space for context, and an `@@` line naming
/// where in the page each group of changes is. `None` when the bodies are
/// too different to compare cheaply.
pub fn changes(old: &str, new: &str) -> Option<Changes> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    if am.is_empty() && bm.is_empty() {
        return Some(Changes::None);
    }
    if am.len().saturating_mul(bm.len()) > MAX_DIFF_CELLS {
        return None;
    }
    // Longest common subsequence of the middle parts.
    let width = bm.len() + 1;
    let mut lcs = vec![0u32; (am.len() + 1) * width];
    for i in (0..am.len()).rev() {
        for j in (0..bm.len()).rev() {
            lcs[i * width + j] = if am[i] == bm[j] {
                lcs[(i + 1) * width + j + 1] + 1
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }
    #[derive(Clone, Copy, PartialEq)]
    enum Op {
        Same(usize),
        Removed(usize),
        Added(usize),
    }
    let mut ops: Vec<Op> = (0..prefix).map(Op::Same).collect();
    let (mut i, mut j) = (0, 0);
    while i < am.len() || j < bm.len() {
        if i < am.len() && j < bm.len() && am[i] == bm[j] {
            ops.push(Op::Same(prefix + j));
            i += 1;
            j += 1;
        } else if i < am.len()
            && (j == bm.len() || lcs[(i + 1) * width + j] >= lcs[i * width + j + 1])
        {
            // Removals before additions, as in a unified diff.
            ops.push(Op::Removed(prefix + i));
            i += 1;
        } else {
            ops.push(Op::Added(prefix + j));
            j += 1;
        }
    }
    for k in 0..suffix {
        ops.push(Op::Same(b.len() - suffix + k));
    }
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| !matches!(op, Op::Same(..)))
        .map(|(k, _)| k)
        .collect();
    let mut text = String::new();
    let (mut added, mut removed) = (0, 0);
    let mut k = 0;
    while k < changed.len() {
        // One group: changes whose context windows touch.
        let first = changed[k];
        let mut last = first;
        while k + 1 < changed.len() && changed[k + 1] <= last + 2 * CONTEXT + 1 {
            k += 1;
            last = changed[k];
        }
        k += 1;
        let from = first.saturating_sub(CONTEXT);
        let to = (last + CONTEXT).min(ops.len() - 1);
        let anchor = match ops[from] {
            Op::Same(j) | Op::Added(j) => where_in(&b, j),
            Op::Removed(i) => where_in(&a, i),
        };
        text.push_str(&format!("@@ {anchor}\n"));
        for op in &ops[from..=to] {
            match *op {
                Op::Same(j) => text.push_str(&format!(" {}\n", b[j])),
                Op::Removed(i) => {
                    removed += 1;
                    text.push_str(&format!("-{}\n", a[i]));
                }
                Op::Added(j) => {
                    added += 1;
                    text.push_str(&format!("+{}\n", b[j]));
                }
            }
        }
    }
    Some(Changes::Some {
        text,
        added,
        removed,
    })
}

/// The ancestors of line `at` (by indentation), outermost first, as
/// `role "name"` without attributes: `main > list > listitem`.
fn where_in(lines: &[&str], at: usize) -> String {
    let indent = |line: &str| line.len() - line.trim_start().len();
    let label = |line: &str| {
        let line = line.trim_start().trim_start_matches("- ");
        let end = line.find(" [").unwrap_or(line.len());
        let end = line[..end].find(": ").unwrap_or(end);
        clip(&line[..end], 60)
    };
    let Some(line) = lines.get(at) else {
        return "end of page".into();
    };
    let mut depth = indent(line);
    let mut path = Vec::new();
    for line in lines[..at].iter().rev() {
        if depth == 0 || path.len() == 3 {
            break;
        }
        if indent(line) < depth {
            depth = indent(line);
            path.push(label(line));
        }
    }
    if path.is_empty() {
        return "top of page".into();
    }
    path.reverse();
    path.join(" > ")
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

    fn raw(nodes: Vec<Value>) -> Vec<RawAxNode> {
        serde_json::from_value(Value::Array(nodes)).unwrap()
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
        let root = add_tree(&mut arena, raw(sample()), 0).unwrap();
        let opts = RenderOpts {
            mode: SnapshotMode::Interactive,
            max_nodes: MAX_NODES,
            picked: None,
            origin: None,
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
        // Links on the page's own origin show their path.
        for (origin, shown) in [("https://x", "/reset"), ("https://y", "https://x/reset")] {
            let opts = RenderOpts {
                mode: SnapshotMode::Interactive,
                max_nodes: MAX_NODES,
                picked: None,
                origin: Some(origin.into()),
            };
            let out = render(&arena, root, &opts, &mut numbering());
            assert!(
                out.text.contains(&format!("[href={shown}] [ref=e4]")),
                "{}",
                out.text
            );
        }
    }

    #[test]
    fn the_picked_element_is_shown_with_a_ref_and_marked() {
        let mut arena = Vec::new();
        let root = add_tree(&mut arena, raw(sample()), 0).unwrap();
        let opts = RenderOpts {
            mode: SnapshotMode::Interactive,
            max_nodes: MAX_NODES,
            picked: Some((0, 106)),
            origin: None,
        };
        let out = render(&arena, root, &opts, &mut numbering());
        assert!(out
            .text
            .contains("\n  - paragraph [ref=e4] [picked by the user]\n"));
        assert_eq!(out.picked, Some(4));
        // Another frame's node with the same backend id is not the pick.
        let opts = RenderOpts {
            picked: Some((1, 106)),
            ..opts
        };
        let out = render(&arena, root, &opts, &mut numbering());
        assert!(!out.text.contains(PICKED_FLAG));
        assert_eq!(out.picked, None);
    }

    #[test]
    fn full_snapshot_adds_text_without_repeating_names() {
        let mut arena = Vec::new();
        let root = add_tree(&mut arena, raw(sample()), 0).unwrap();
        let opts = RenderOpts {
            mode: SnapshotMode::Full,
            max_nodes: MAX_NODES,
            picked: None,
            origin: None,
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
        let root = add_tree(&mut arena, raw(nodes), 0).unwrap();
        let opts = RenderOpts {
            mode: SnapshotMode::Interactive,
            max_nodes: 10,
            picked: None,
            origin: None,
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

    #[test]
    fn changes_show_only_what_differs_with_where_it_is() {
        let old = "- document \"App\" [ref=e1]\n  - main\n    - list\n      - listitem\n        - button \"Mark done\" [ref=e5]\n      - listitem\n        - button \"Mark done\" [ref=e6]\n  - contentinfo\n    - link \"Help\" [ref=e9]\n";
        assert_eq!(changes(old, old), Some(Changes::None));
        let new = old.replace(
            "        - button \"Mark done\" [ref=e5]\n",
            "        - button \"Done\" [ref=e5]\n        - status \"Saved\"\n",
        );
        let Some(Changes::Some {
            text,
            added,
            removed,
        }) = changes(old, &new)
        else {
            panic!("expected changes");
        };
        assert_eq!((added, removed), (2, 1));
        assert_eq!(
            text,
            "@@ document \"App\" > main > list\n       - listitem\n-        - button \"Mark done\" [ref=e5]\n+        - button \"Done\" [ref=e5]\n+        - status \"Saved\"\n       - listitem\n"
        );
        // Two changes far apart are two groups.
        let far = new.replace(
            "link \"Help\" [ref=e9]",
            "link \"Help\" [disabled] [ref=e9]",
        );
        let Some(Changes::Some { text, .. }) = changes(old, &far) else {
            panic!("expected changes");
        };
        assert_eq!(text.matches("@@ ").count(), 2, "{text}");
        assert!(
            text.contains("@@ document \"App\"\n   - contentinfo\n-    - link"),
            "{text}"
        );
        // A line added at the very top.
        let top = format!("- banner [ref=e20]\n{old}");
        let Some(Changes::Some { text, .. }) = changes(old, &top) else {
            panic!("expected changes");
        };
        assert!(
            text.starts_with("@@ top of page\n+- banner [ref=e20]\n"),
            "{text}"
        );
    }
}
