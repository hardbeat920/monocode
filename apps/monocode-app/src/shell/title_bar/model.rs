//! The title bar's tab model: `tabCopy`, `sessionMeta`, and the harness
//! marks of `TabHarnesses` in src/app/shell/TitleBar.tsx, over the
//! workspace's `TitleTab`.

use monocode_engine::workspace::title_tab::TitleTab;
use monocode_ui::ProviderLogo;

use crate::format;

/// A harness icon in a tab, with its turn state (`TabHarnesses`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarnessState {
    Idle,
    Busy,
    Done,
}

#[derive(Clone, Debug)]
pub enum TabLead {
    Harnesses(Vec<(ProviderLogo, HarnessState)>),
    File(String),
    Terminal,
}

/// One title bar tab, after `tabCopy`.
#[derive(Clone, Debug)]
pub struct TitleTabView {
    pub id: String,
    pub lead: TabLead,
    pub headline: String,
    pub meta: Option<String>,
    pub tooltip: String,
    pub dirty: bool,
    pub preview: bool,
    pub preview_file_id: Option<String>,
    pub blank: bool,
    pub session_count: usize,
}

#[derive(Clone, Copy)]
pub(super) enum CloseSide {
    Others,
    Right,
    Left,
}

pub(super) fn tab_closable(tab: &TitleTabView, count: usize) -> bool {
    count > 1 || !tab.blank
}

pub(super) fn context_close_ids(
    tabs: &[TitleTabView],
    target: &str,
    side: CloseSide,
) -> Vec<String> {
    let Some(target_index) = tabs.iter().position(|tab| tab.id == target) else {
        return Vec::new();
    };
    tabs.iter()
        .enumerate()
        .filter(|(index, _)| match side {
            CloseSide::Others => *index != target_index,
            CloseSide::Right => *index > target_index,
            CloseSide::Left => *index < target_index,
        })
        .map(|(_, tab)| tab.id.clone())
        .collect()
}

/// `sessionMeta` in TitleBar.tsx.
fn session_meta(tab: &TitleTab) -> String {
    if tab.more.len() == 1 {
        return tab.more[0].clone();
    }
    if tab.session_count > 1 {
        return format!("{} sessions", tab.session_count);
    }
    String::new()
}

/// `tabCopy` in TitleBar.tsx: the headline, meta line, and tooltip.
pub fn tab_copy(tab: &TitleTab) -> (String, String, String) {
    let project = match tab.project.trim() {
        "" => "~",
        project => project,
    };
    let conversation = tab.title.trim().to_string();
    let file = tab.files.first().cloned().unwrap_or_default();
    let sessions = session_meta(tab);
    let untitled = "New session".to_string();
    let mut meta: Vec<String> = Vec::new();
    let headline = if tab.multi_pane {
        if tab.file_focused && !file.is_empty() {
            if !conversation.is_empty() {
                meta.push(conversation.clone());
            } else if !sessions.is_empty() {
                meta.push(sessions.clone());
            }
            file.clone()
        } else if !conversation.is_empty() {
            if !file.is_empty() {
                meta.push(file.clone());
            } else if !sessions.is_empty() {
                meta.push(sessions.clone());
            }
            conversation.clone()
        } else if !file.is_empty() {
            if !sessions.is_empty() {
                meta.push(sessions.clone());
            }
            file.clone()
        } else {
            if !sessions.is_empty() {
                meta.push(sessions.clone());
            }
            untitled
        }
    } else {
        if !sessions.is_empty() {
            meta.push(sessions.clone());
        }
        [conversation.clone(), file.clone(), untitled]
            .into_iter()
            .find(|text| !text.is_empty())
            .unwrap_or_default()
    };
    let mut tooltip = vec![project.to_string()];
    if !conversation.is_empty() {
        tooltip.push(conversation);
    }
    tooltip.extend(tab.more.iter().cloned());
    if !tab.files.is_empty() {
        tooltip.push(tab.files.join(", "));
    }
    if tab.dirty {
        tooltip.push("Unsaved changes".into());
    }
    (headline, meta.join(" · "), tooltip.join(" · "))
}

pub fn title_tab_view(tab: &TitleTab) -> TitleTabView {
    let (headline, meta, tooltip) = tab_copy(tab);
    let lead = if !tab.harnesses.is_empty() {
        TabLead::Harnesses(
            tab.harnesses
                .iter()
                .take(3)
                .map(|harness| {
                    let state = if tab.busy_harnesses.contains(harness) {
                        HarnessState::Busy
                    } else if tab.done_harnesses.contains(harness) {
                        HarnessState::Done
                    } else {
                        HarnessState::Idle
                    };
                    (format::provider_logo(*harness), state)
                })
                .collect(),
        )
    } else if tab.terminal || tab.files.is_empty() {
        TabLead::Terminal
    } else {
        TabLead::File(tab.files[0].clone())
    };
    TitleTabView {
        id: tab.id.clone(),
        lead,
        headline,
        meta: (!meta.is_empty()).then_some(meta),
        tooltip,
        dirty: tab.dirty,
        preview: tab.preview_file_id.is_some(),
        preview_file_id: tab.preview_file_id.clone(),
        blank: tab.blank,
        session_count: tab.session_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(title: &str, files: &[&str], multi: bool, file_focused: bool) -> TitleTab {
        TitleTab {
            id: "t".into(),
            project: "repo".into(),
            title: title.into(),
            files: files.iter().map(|file| file.to_string()).collect(),
            multi_pane: multi,
            file_focused,
            session_count: 1,
            ..Default::default()
        }
    }

    #[test]
    fn tab_copy_follows_the_title_bar() {
        assert_eq!(tab_copy(&tab("", &[], false, false)).0, "New session");
        assert_eq!(tab_copy(&tab("Fix it", &[], false, false)).0, "Fix it");
        let split = tab_copy(&tab("Fix it", &["a.rs"], true, true));
        assert_eq!((split.0.as_str(), split.1.as_str()), ("a.rs", "Fix it"));
        let split = tab_copy(&tab("Fix it", &["a.rs"], true, false));
        assert_eq!((split.0.as_str(), split.1.as_str()), ("Fix it", "a.rs"));
        assert_eq!(
            tab_copy(&tab("Fix it", &[], false, false)).2,
            "repo · Fix it"
        );
    }

    #[test]
    fn the_last_nonblank_tab_can_close_but_the_last_blank_tab_stays() {
        let mut view = title_tab_view(&tab("", &[], false, false));
        view.blank = true;
        assert!(!tab_closable(&view, 1));
        assert!(tab_closable(&view, 2));
        view.blank = false;
        assert!(tab_closable(&view, 1));
    }

    #[test]
    fn context_close_ids_follow_the_clicked_tab_and_strip_order() {
        let tabs = ["a", "b", "c", "d"].map(|id| {
            let mut view = title_tab_view(&tab("", &[], false, false));
            view.id = id.into();
            view
        });
        assert_eq!(
            context_close_ids(&tabs, "b", CloseSide::Others),
            ["a", "c", "d"]
        );
        assert_eq!(context_close_ids(&tabs, "b", CloseSide::Right), ["c", "d"]);
        assert_eq!(context_close_ids(&tabs, "c", CloseSide::Left), ["a", "b"]);
        assert!(context_close_ids(&tabs, "a", CloseSide::Left).is_empty());
        assert!(context_close_ids(&tabs, "d", CloseSide::Right).is_empty());
        assert!(context_close_ids(&tabs, "missing", CloseSide::Others).is_empty());
    }
}
