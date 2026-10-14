//! Element refs (spec 8 "Ref šema i verzionisanje").
//!
//! A snapshot gives every node the agent can act on a ref `e<N>`. The agent
//! only ever sees refs; the `backendNodeId`, the frame and the CDP session
//! that holds the node stay here.
//!
//! Refs are stable within one document: the same DOM node keeps its number
//! across snapshots, so a ref from the previous snapshot still names the same
//! element while the page has not navigated. A cross-document navigation of
//! the main frame (new `loaderId`) starts a new table, and every older ref is
//! `ref_stale` without a command being sent. Whether the node still exists is
//! decided at action time by `DOM.resolveNode`.
//!
//! A ref may carry the snapshot version (`e12@4`); only the last two
//! snapshot versions of the current document are accepted.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use super::super::link::CdpSessionId;
use super::super::policy::{Lease, PolicyState};
use super::super::protocol::{Reason, TabId};
use super::Fail;

/// Where a node lives: the CDP session that owns its DOM, the frame (for the
/// isolated world) and, for out-of-process iframes, the chain of `<iframe>`
/// owner elements from the main frame down, whose boxes give the offset of
/// the frame's viewport inside the tab's viewport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameCtx {
    pub session: CdpSessionId,
    /// `None` only when the frame id is unknown (the main frame before the
    /// first navigation event).
    pub frame_id: Option<String>,
    /// `(session holding the <iframe>, its backendNodeId)` per OOPIF level.
    pub owners: Vec<(CdpSessionId, i64)>,
}

impl FrameCtx {
    pub fn is_oopif(&self) -> bool {
        !self.owners.is_empty()
    }
}

/// One ref: the node behind `e<N>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeRef {
    pub number: u32,
    pub frame: FrameCtx,
    pub backend: i64,
    pub role: String,
    pub name: String,
}

#[derive(Debug, Default)]
pub struct RefTable {
    /// Main-frame loader the refs belong to.
    loader: Option<String>,
    /// Snapshot versions of this document the agent may cite (latest last).
    versions: Vec<u64>,
    next: u32,
    refs: HashMap<u32, NodeRef>,
    by_node: HashMap<(String, i64), u32>,
    /// Isolated-world execution contexts of same-process subframes (the
    /// main frame and OOPIF roots are cached on the lease).
    worlds: HashMap<(String, String), i64>,
    /// The element the user pointed at in this document, as
    /// `(CDP session, backendNodeId)`; snapshots mark it.
    picked: Option<(CdpSessionId, i64)>,
    /// The last whole-page snapshot of this document, so the snapshot an
    /// action returns can be sent as the changes since then.
    pub last: Option<LastSnapshot>,
}

/// The last whole-page snapshot (no scope, default size): the mode the agent
/// asked for, and the page rendered with its text (full mode), which the
/// changes after an action are computed on.
#[derive(Clone, Debug)]
pub struct LastSnapshot {
    pub version: u64,
    pub mode: super::super::protocol::SnapshotMode,
    pub body: String,
}

impl RefTable {
    fn node_key(frame: &FrameCtx, backend: i64) -> (String, i64) {
        (frame.session.0.clone(), backend)
    }

    /// The ref of a node, allocating the next number on first sight.
    pub fn assign(&mut self, frame: &FrameCtx, backend: i64, role: &str, name: &str) -> u32 {
        let key = Self::node_key(frame, backend);
        if let Some(number) = self.by_node.get(&key) {
            if let Some(node) = self.refs.get_mut(number) {
                node.role = role.to_string();
                node.name = name.to_string();
                node.frame = frame.clone();
            }
            return *number;
        }
        self.next += 1;
        let number = self.next;
        self.by_node.insert(key, number);
        self.refs.insert(
            number,
            NodeRef {
                number,
                frame: frame.clone(),
                backend,
                role: role.to_string(),
                name: name.to_string(),
            },
        );
        number
    }

    pub fn ref_for(&self, frame: &FrameCtx, backend: i64) -> Option<u32> {
        self.by_node.get(&Self::node_key(frame, backend)).copied()
    }
}

/// A tab of one browser host. The host part (address of its policy) keeps
/// hosts apart; the app has one, tests run many side by side.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RefKey {
    pub host: usize,
    pub tab: TabId,
}

static STORE: LazyLock<Mutex<HashMap<RefKey, RefTable>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn store() -> std::sync::MutexGuard<'static, HashMap<RefKey, RefTable>> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Drop this host's tables of tabs that are gone (closed, crashed, revoked,
/// forgotten).
pub fn prune(host: usize, state: &PolicyState) {
    store().retain(|key, _| key.host != host || state.lease(&key.tab).is_some_and(Lease::is_live));
}

/// Forget every ref of a tab (the action layer saw a cross-document
/// navigation of the main frame).
pub fn clear(tab: &RefKey) {
    store().remove(tab);
}

/// Run `f` on the tab's table for a new snapshot of version `version`. A
/// table of another document is replaced first.
pub fn with_snapshot_table<R>(
    tab: &RefKey,
    loader: Option<&str>,
    version: u64,
    f: impl FnOnce(&mut RefTable) -> R,
) -> R {
    let mut store = store();
    let table = store.entry(tab.clone()).or_default();
    if table.loader.as_deref() != loader {
        *table = RefTable::default();
    }
    table.loader = loader.map(str::to_string);
    table.versions.push(version);
    if table.versions.len() > 2 {
        table.versions.remove(0);
    }
    f(table)
}

/// The user pointed at a node of the tab's current document (`loader`). A
/// table of another document is replaced; returns the node's ref when a
/// snapshot already gave it one.
pub fn set_picked(
    tab: &RefKey,
    loader: Option<&str>,
    session: &CdpSessionId,
    backend: i64,
) -> Option<u32> {
    let mut store = store();
    let table = store.entry(tab.clone()).or_default();
    if table.loader.as_deref() != loader {
        *table = RefTable::default();
        table.loader = loader.map(str::to_string);
    }
    table.picked = Some((session.clone(), backend));
    table.by_node.get(&(session.0.clone(), backend)).copied()
}

/// The node the user pointed at in the table's document, if any.
pub fn picked(table: &RefTable) -> Option<&(CdpSessionId, i64)> {
    table.picked.as_ref()
}

/// Cached isolated world of a same-process subframe.
pub fn world(tab: &RefKey, frame: &FrameCtx) -> Option<i64> {
    let frame_id = frame.frame_id.clone()?;
    store()
        .get(tab)
        .and_then(|t| t.worlds.get(&(frame.session.0.clone(), frame_id)).copied())
}

pub fn set_world(tab: &RefKey, frame: &FrameCtx, context: Option<i64>) {
    let Some(frame_id) = frame.frame_id.clone() else {
        return;
    };
    let mut store = store();
    let Some(table) = store.get_mut(tab) else {
        return;
    };
    let key = (frame.session.0.clone(), frame_id);
    match context {
        Some(context) => {
            table.worlds.insert(key, context);
        }
        None => {
            table.worlds.remove(&key);
        }
    }
}

/// Parse `e12` or `e12@4`.
pub fn parse(raw: &str) -> Result<(u32, Option<u64>), Fail> {
    let invalid = || {
        Fail::detail(
            Reason::InvalidRequest,
            format!("`{raw}` is not a ref; use a ref like `e12` from browser_snapshot."),
        )
    };
    let trimmed = raw.trim();
    let (number, version) = match trimmed.split_once('@') {
        Some((number, version)) => (number, Some(version)),
        None => (trimmed, None),
    };
    let number = number
        .strip_prefix('e')
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .ok_or_else(invalid)?;
    let version = match version {
        Some(v) => Some(v.parse::<u64>().map_err(|_| invalid())?),
        None => None,
    };
    Ok((number, version))
}

/// Resolve a ref the agent sent against the tab's current table (spec 9.2
/// step 2). Never guesses: anything not provably the same document and node
/// is `ref_stale`, before a single command is sent.
pub fn lookup(tab: &RefKey, lease: &Lease, raw: &str) -> Result<NodeRef, Fail> {
    let (number, version) = parse(raw)?;
    let stale = |why: &str| {
        Fail::detail(
            Reason::RefStale,
            format!("Ref {raw} is stale ({why}); call browser_snapshot and use fresh refs."),
        )
    };
    let store = store();
    let Some(table) = store.get(tab).filter(|t| !t.versions.is_empty()) else {
        return Err(stale("no snapshot of this page yet"));
    };
    if table.loader != lease.main_loader_id {
        return Err(stale("the page navigated since the snapshot"));
    }
    if let Some(version) = version {
        if !table.versions.contains(&version) {
            return Err(stale("it comes from an older snapshot"));
        }
    }
    let node = table
        .refs
        .get(&number)
        .cloned()
        .ok_or_else(|| stale("this page has no such ref"))?;
    if node.frame.is_oopif() && !lease.holds_session(&node.frame.session) {
        return Err(stale("its frame is gone"));
    }
    Ok(node)
}

/// Latest snapshot version of the tab's current table, if any.
pub fn latest_version(tab: &RefKey) -> Option<u64> {
    store().get(tab).and_then(|t| t.versions.last().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(session: &str) -> FrameCtx {
        FrameCtx {
            session: CdpSessionId(session.into()),
            frame_id: Some("F".into()),
            owners: Vec::new(),
        }
    }

    #[test]
    fn a_pick_lives_as_long_as_its_document() {
        let key = RefKey {
            host: 0x9177,
            tab: TabId("t_pick".into()),
        };
        let main = frame("S1");
        let number = with_snapshot_table(&key, Some("L1"), 1, |table| {
            table.assign(&main, 42, "button", "Save")
        });
        assert_eq!(
            set_picked(&key, Some("L1"), &main.session, 42),
            Some(number)
        );
        with_snapshot_table(&key, Some("L1"), 2, |table| {
            assert_eq!(picked(table), Some(&(main.session.clone(), 42)));
        });
        // Another document: the pick and the refs are gone.
        with_snapshot_table(&key, Some("L2"), 3, |table| {
            assert!(picked(table).is_none());
        });
        assert_eq!(set_picked(&key, Some("L3"), &main.session, 42), None);
        clear(&key);
    }

    #[test]
    fn parses_refs_with_and_without_versions() {
        assert_eq!(parse("e12").unwrap(), (12, None));
        assert_eq!(parse(" e3@7 ").unwrap(), (3, Some(7)));
        for bad in [
            "", "e", "12", "e0", "e-1", "ex", "e1@", "e1@x", "#id", "e1 2",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn numbers_are_stable_per_node_and_reset_per_document() {
        let tab = RefKey {
            host: 0,
            tab: TabId("t_0000aaaa".into()),
        };
        let main = frame("S1");
        let (a, b) = with_snapshot_table(&tab, Some("L1"), 1, |t| {
            (
                t.assign(&main, 10, "button", "OK"),
                t.assign(&main, 11, "link", "x"),
            )
        });
        assert_eq!((a, b), (1, 2));
        let again = with_snapshot_table(&tab, Some("L1"), 2, |t| {
            (
                t.assign(&main, 11, "link", "x"),
                t.assign(&main, 12, "textbox", ""),
            )
        });
        assert_eq!(again, (2, 3));
        // Other session, same backend id: another node.
        let other = with_snapshot_table(&tab, Some("L1"), 3, |t| {
            t.assign(&frame("F9"), 10, "button", "")
        });
        assert_eq!(other, 4);
        assert_eq!(latest_version(&tab), Some(3));
        // New document: numbering restarts.
        let fresh = with_snapshot_table(&tab, Some("L2"), 4, |t| t.assign(&main, 11, "link", ""));
        assert_eq!(fresh, 1);
        clear(&tab);
        assert_eq!(latest_version(&tab), None);
    }
}
