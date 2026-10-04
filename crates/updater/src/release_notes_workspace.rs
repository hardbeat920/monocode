//! Port of src/app/model/releaseNotesWorkspace.ts: open the release notes
//! tab, or focus the one that is already open for that version.

use monocode_layout::{WorkspaceTab, is_release_notes_tab};

/// `ReleaseNotesOpenPlan`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseNotesOpenPlan {
    /// No tab shows this release yet.
    Open,
    /// A tab already shows it.
    Focus(ReleaseNotesFocusTarget),
}

/// The `kind: "focus"` plan's fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseNotesFocusTarget {
    pub tab_id: String,
    pub pane_id: String,
    pub file_id: String,
}

/// `planReleaseNotesOpen`.
pub fn plan_release_notes_open(tabs: &[WorkspaceTab], version: &str) -> ReleaseNotesOpenPlan {
    for tab in tabs {
        for pane in &tab.editor_panes {
            let file = pane.files.iter().find(|entry| {
                is_release_notes_tab(entry)
                    && entry
                        .release_notes
                        .as_ref()
                        .is_some_and(|notes| notes.version == version)
            });
            if let Some(file) = file {
                return ReleaseNotesOpenPlan::Focus(ReleaseNotesFocusTarget {
                    tab_id: tab.id.clone(),
                    pane_id: pane.id.clone(),
                    file_id: file.id.clone(),
                });
            }
        }
    }
    ReleaseNotesOpenPlan::Open
}

/// `focusReleaseNotesTarget`: focus the pane and make the file its active
/// tab, without adding another copy.
pub fn focus_release_notes_target(
    tabs: &[WorkspaceTab],
    target: &ReleaseNotesFocusTarget,
) -> Vec<WorkspaceTab> {
    tabs.iter()
        .map(|tab| {
            if tab.id != target.tab_id {
                return tab.clone();
            }
            let mut tab = tab.clone();
            tab.focused_id = target.pane_id.clone();
            tab.diff_focused = Some(false);
            for pane in &mut tab.editor_panes {
                if pane.id == target.pane_id {
                    pane.active_file_id = target.file_id.clone();
                }
            }
            tab
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Port of src/app/model/releaseNotesWorkspace.test.ts.

    use monocode_layout::{
        ReleaseNotesTabSource, SplitDir, leaf, new_editor_pane, new_file_tab,
        new_release_notes_workspace_tab, new_tab, split_pane,
    };

    use super::*;

    fn tab_with_hidden_release() -> WorkspaceTab {
        let tab = new_release_notes_workspace_tab(ReleaseNotesTabSource::new("0.1.23"));
        let release_pane = tab.editor_panes[0].clone();
        let active_file = new_file_tab("/repo/App.tsx", "/repo", false, None, None);
        let other_pane =
            new_editor_pane(new_file_tab("/repo/Other.tsx", "/repo", false, None, None));
        let mut release_pane_with_file = release_pane.clone();
        release_pane_with_file.files.push(active_file.clone());
        release_pane_with_file.active_file_id = active_file.id.clone();
        WorkspaceTab {
            layout: split_pane(
                &leaf(release_pane.id.clone()),
                &release_pane.id,
                SplitDir::Right,
                &other_pane.id,
            ),
            focused_id: other_pane.id.clone(),
            editor_panes: vec![release_pane_with_file, other_pane],
            ..tab
        }
    }

    #[test]
    fn opens_when_the_release_is_missing() {
        assert_eq!(
            plan_release_notes_open(&[new_tab("session-a")], "0.1.23"),
            ReleaseNotesOpenPlan::Open
        );
    }

    #[test]
    fn identifies_the_exact_release_tab_pane_and_file() {
        let tab = tab_with_hidden_release();
        let release_pane = &tab.editor_panes[0];
        let release_file = &release_pane.files[0];

        assert_eq!(
            plan_release_notes_open(std::slice::from_ref(&tab), "0.1.23"),
            ReleaseNotesOpenPlan::Focus(ReleaseNotesFocusTarget {
                tab_id: tab.id.clone(),
                pane_id: release_pane.id.clone(),
                file_id: release_file.id.clone(),
            })
        );
    }

    #[test]
    fn reveals_an_existing_release_without_adding_a_duplicate() {
        let tab = tab_with_hidden_release();
        let ReleaseNotesOpenPlan::Focus(target) =
            plan_release_notes_open(std::slice::from_ref(&tab), "0.1.23")
        else {
            panic!("expected focus plan");
        };

        let result = focus_release_notes_target(&[tab], &target);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].focused_id, target.pane_id);
        assert_eq!(result[0].diff_focused, Some(false));
        assert_eq!(result[0].editor_panes[0].active_file_id, target.file_id);
        let copies = result[0]
            .editor_panes
            .iter()
            .flat_map(|pane| &pane.files)
            .filter(|file| {
                file.release_notes
                    .as_ref()
                    .is_some_and(|notes| notes.version == "0.1.23")
            })
            .count();
        assert_eq!(copies, 1);
    }
}
