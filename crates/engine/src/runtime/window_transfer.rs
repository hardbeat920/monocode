//! Port of src/app/model/windowTransfer.ts and windowTransferBootstrap.ts:
//! moving tabs and their sessions into a new window.
//!
//! The tab and terminal dock types belong to the workspace package, so the
//! payload is generic over them. `TransferTab` is what the collector needs
//! to know about a tab.

use std::collections::HashSet;

use monocode_core::Session;
use serde::{Deserialize, Serialize};

/// What `collect_window_transfer` reads from a workspace tab.
pub trait TransferTab: Clone {
    fn id(&self) -> &str;
    /// `leafIds(tab.layout)`: the session id of every pane.
    fn leaf_ids(&self) -> Vec<String>;
    /// Ids of every file in the tab's editor and terminal panes.
    fn file_ids(&self) -> Vec<String>;
}

/// `WindowTransferPayload`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowTransferPayload<Tab, Dock> {
    pub tabs: Vec<Tab>,
    pub sessions: Vec<Session>,
    pub active_tab_id: String,
    pub project_cwd: String,
    pub dirty_file_ids: Vec<String>,
    #[serde(default = "Option::default", skip_serializing_if = "Option::is_none")]
    pub project_terminals: Option<Vec<Dock>>,
}

/// `collectWindowTransfer`: the tabs in `tab_ids`, the sessions they show,
/// and their dirty files. `None` when no tab matches.
pub fn collect_window_transfer<Tab: TransferTab, Dock: Clone>(
    tabs: &[Tab],
    sessions: &[Session],
    tab_ids: &[String],
    active_tab_id: &str,
    dirty_files: &HashSet<String>,
    fallback_cwd: &str,
    project_terminals: &[Dock],
) -> Option<WindowTransferPayload<Tab, Dock>> {
    let id_set: HashSet<&str> = tab_ids.iter().map(String::as_str).collect();
    let moving_tabs: Vec<Tab> = tabs
        .iter()
        .filter(|tab| id_set.contains(tab.id()))
        .cloned()
        .collect();
    let first = moving_tabs.first()?;
    let session_ids: HashSet<String> = moving_tabs.iter().flat_map(TransferTab::leaf_ids).collect();
    let moving_sessions: Vec<Session> = sessions
        .iter()
        .filter(|session| session_ids.contains(&session.id))
        .cloned()
        .collect();
    let mut dirty_in_tabs = Vec::new();
    for tab in &moving_tabs {
        for id in tab.file_ids() {
            if dirty_files.contains(&id) && !dirty_in_tabs.contains(&id) {
                dirty_in_tabs.push(id);
            }
        }
    }
    let active_tab_id = if id_set.contains(active_tab_id) {
        active_tab_id.to_string()
    } else {
        first.id().to_string()
    };
    let project_cwd = moving_sessions
        .first()
        .map(|session| session.cwd.clone())
        .unwrap_or_else(|| fallback_cwd.to_string());
    Some(WindowTransferPayload {
        tabs: moving_tabs,
        sessions: moving_sessions,
        active_tab_id,
        project_cwd,
        dirty_file_ids: dirty_in_tabs,
        project_terminals: (!project_terminals.is_empty()).then(|| project_terminals.to_vec()),
    })
}

/// The window transfer waiting for the next window, as JSON.
///
/// `windowTransferBootstrap.ts` took it once per webview through
/// `take_window_transfer`. In one native process the opener puts it here
/// and the new window takes it.
#[derive(Debug, Default)]
pub struct PendingWindowTransfer {
    payload: Option<serde_json::Value>,
}

impl PendingWindowTransfer {
    pub fn put(&mut self, payload: serde_json::Value) {
        self.payload = Some(payload);
    }

    /// `loadWindowTransfer`: one take only.
    pub fn take(&mut self) -> Option<serde_json::Value> {
        self.payload.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::HarnessId;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Tab {
        id: String,
        leaves: Vec<String>,
        files: Vec<String>,
    }

    impl TransferTab for Tab {
        fn id(&self) -> &str {
            &self.id
        }
        fn leaf_ids(&self) -> Vec<String> {
            self.leaves.clone()
        }
        fn file_ids(&self) -> Vec<String> {
            self.files.clone()
        }
    }

    fn tab(id: &str, leaf: &str) -> Tab {
        Tab {
            id: id.into(),
            leaves: vec![leaf.into()],
            files: vec![format!("{id}-file")],
        }
    }

    fn session(id: &str) -> Session {
        Session::blank(id, HarnessId::Cursor, "", "/Users/me/agent-terminal")
    }

    #[test]
    fn collects_tabs_sessions_and_dirty_files_for_a_group() {
        let tabs = vec![tab("t1", "s1"), tab("t2", "s2"), tab("t3", "s9")];
        let dirty: HashSet<String> = ["t2-file".to_string(), "t3-file".to_string()].into();
        let payload = collect_window_transfer::<Tab, ()>(
            &tabs,
            &[session("s1"), session("s2")],
            &["t1".into(), "t2".into()],
            "t2",
            &dirty,
            "~",
            &[],
        )
        .unwrap();
        assert_eq!(payload.active_tab_id, "t2");
        assert_eq!(payload.project_cwd, "/Users/me/agent-terminal");
        let tab_ids: Vec<_> = payload.tabs.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(tab_ids, vec!["t1", "t2"]);
        let session_ids: Vec<_> = payload.sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(session_ids, vec!["s1", "s2"]);
        assert_eq!(payload.dirty_file_ids, vec!["t2-file"]);
        assert!(payload.project_terminals.is_none());
    }

    #[test]
    fn carries_a_project_terminal_dock_into_the_new_window() {
        let payload = collect_window_transfer(
            &[tab("t1", "s1")],
            &[session("s1")],
            &["t1".into()],
            "t1",
            &HashSet::new(),
            "~",
            &["dock".to_string()],
        )
        .unwrap();
        assert_eq!(payload.project_terminals, Some(vec!["dock".to_string()]));
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["projectTerminals"][0], "dock");
    }

    #[test]
    fn falls_back_when_nothing_moves() {
        assert!(
            collect_window_transfer::<Tab, ()>(
                &[],
                &[],
                &["x".into()],
                "x",
                &HashSet::new(),
                "~",
                &[]
            )
            .is_none()
        );
        let payload = collect_window_transfer::<Tab, ()>(
            &[tab("t1", "missing")],
            &[],
            &["t1".into()],
            "other",
            &HashSet::new(),
            "~",
            &[],
        )
        .unwrap();
        assert_eq!(
            (payload.active_tab_id.as_str(), payload.project_cwd.as_str()),
            ("t1", "~")
        );
        let mut pending = PendingWindowTransfer::default();
        pending.put(serde_json::json!({ "tabs": [] }));
        assert!(pending.take().is_some());
        assert!(pending.take().is_none());
    }
}
