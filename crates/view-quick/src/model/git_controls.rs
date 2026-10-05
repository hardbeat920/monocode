//! The request bookkeeping of
//! src/features/quick-composer/ui/QuickWorkspaceControls.tsx: which git
//! popup request is live, and whether a press on a trigger opens a popup or
//! closes the one it opened.
//!
//! The popup is a separate panel, so its blur and the trigger's mouse down
//! arrive in either order. A press on the open trigger records a toggle
//! intent; a blur caused by that press reports the trigger kind. Either one
//! turns the click that follows into "close".

use super::launch::{QuickGitKind, QuickGitResult, QuickWorkspace};

/// What a click on a trigger does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShowAction {
    /// Close. Complete `id` (if a request was live) without restoring focus.
    Close { complete: Option<String> },
    /// Open the popup for this request id.
    Open { id: String },
}

/// What a popup result does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultEffect {
    /// The new working copy, when it is for the current project.
    pub choice: Option<QuickWorkspace>,
    /// Put focus back in the prompt.
    pub restore_focus: bool,
}

/// The live request and the toggle bookkeeping.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitControls {
    open: Option<QuickGitKind>,
    active: Option<String>,
    active_kind: Option<QuickGitKind>,
    toggle_intent: Option<QuickGitKind>,
    closed_by_trigger: Option<QuickGitKind>,
}

impl GitControls {
    /// The popup the controls show as open (`aria-expanded`).
    pub fn open(&self) -> Option<QuickGitKind> {
        self.open
    }

    /// The live request id.
    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// `beginClick`: the mouse went down on a trigger.
    pub fn begin_click(&mut self, kind: QuickGitKind) {
        self.toggle_intent = (self.active_kind == Some(kind)).then_some(kind);
    }

    /// `show`: the trigger was clicked. `new_id` names the request when
    /// one opens.
    pub fn show(&mut self, kind: QuickGitKind, new_id: String) -> ShowAction {
        let closing = self.active_kind == Some(kind)
            || self.toggle_intent == Some(kind)
            || self.closed_by_trigger == Some(kind);
        self.toggle_intent = None;
        self.closed_by_trigger = None;
        if closing {
            return ShowAction::Close {
                complete: self.cancel(),
            };
        }
        self.active = Some(new_id.clone());
        self.active_kind = Some(kind);
        self.open = Some(kind);
        ShowAction::Open { id: new_id }
    }

    /// `cancel`: forget the live request. Returns its id, which the caller
    /// completes without restoring focus.
    pub fn cancel(&mut self) -> Option<String> {
        self.active_kind = None;
        self.toggle_intent = None;
        self.closed_by_trigger = None;
        self.open = None;
        self.active.take()
    }

    /// Opening `id` failed. Returns true when it was still the live request
    /// (so the caller reports the error).
    pub fn open_failed(&mut self, id: &str) -> bool {
        if self.active.as_deref() != Some(id) {
            return false;
        }
        self.active = None;
        self.active_kind = None;
        self.open = None;
        true
    }

    /// A popup finished. Results for a replaced request are ignored.
    /// `project` is the composer's current project.
    pub fn result(
        &mut self,
        result: &QuickGitResult,
        project: Option<&str>,
    ) -> Option<ResultEffect> {
        if self.active.as_deref() != Some(result.id.as_str()) {
            return None;
        }
        self.closed_by_trigger = result.trigger_kind;
        self.active = None;
        self.active_kind = None;
        self.open = None;
        let choice = result
            .choice
            .clone()
            .filter(|choice| choice.cwd.as_deref() == project);
        Some(ResultEffect {
            choice,
            restore_focus: result.restore_focus,
        })
    }
}

#[cfg(test)]
mod tests {
    use monocode_core::session::WorkspaceMode;

    use super::*;

    fn result(id: &str, restore_focus: bool, trigger: Option<QuickGitKind>) -> QuickGitResult {
        QuickGitResult {
            id: id.into(),
            choice: None,
            restore_focus,
            trigger_kind: trigger,
        }
    }

    fn open(controls: &mut GitControls, kind: QuickGitKind, id: &str) -> bool {
        matches!(controls.show(kind, id.into()), ShowAction::Open { .. })
    }

    #[test]
    fn applies_a_result_for_the_live_request() {
        let mut controls = GitControls::default();
        assert!(open(&mut controls, QuickGitKind::Branch, "a"));
        assert_eq!(controls.open(), Some(QuickGitKind::Branch));
        let mut done = result("a", true, None);
        done.choice = Some(QuickWorkspace {
            cwd: Some("/repo".into()),
            mode: WorkspaceMode::Worktree,
            base: Some("main".into()),
            tree: None,
        });
        let effect = controls.result(&done, Some("/repo")).unwrap();
        assert_eq!(effect.choice.unwrap().base.as_deref(), Some("main"));
        assert!(effect.restore_focus);
        assert_eq!(controls.open(), None);
    }

    #[test]
    fn ignores_results_from_a_picker_replaced_by_a_newer_request() {
        let mut controls = GitControls::default();
        open(&mut controls, QuickGitKind::Branch, "old");
        open(&mut controls, QuickGitKind::Workspace, "new");
        assert_eq!(
            controls.result(&result("old", true, None), Some("/repo")),
            None
        );
        assert_eq!(controls.open(), Some(QuickGitKind::Workspace));
    }

    #[test]
    fn drops_a_choice_for_another_project() {
        let mut controls = GitControls::default();
        open(&mut controls, QuickGitKind::Workspace, "a");
        let mut done = result("a", false, None);
        done.choice = Some(QuickWorkspace::current(Some("/other")));
        let effect = controls.result(&done, Some("/repo")).unwrap();
        assert_eq!(effect.choice, None);
        assert!(!effect.restore_focus);
    }

    #[test]
    fn keeps_a_trigger_toggle_closed_when_blur_arrives_between_mousedown_and_click() {
        let mut controls = GitControls::default();
        assert!(open(&mut controls, QuickGitKind::Branch, "a"));
        controls.begin_click(QuickGitKind::Branch);
        controls.result(&result("a", false, None), Some("/repo"));
        assert!(!open(&mut controls, QuickGitKind::Branch, "b"));
        assert!(open(&mut controls, QuickGitKind::Branch, "c"));
    }

    #[test]
    fn handles_native_blur_before_mousedown_and_still_allows_switching_pickers() {
        let mut controls = GitControls::default();
        assert!(open(&mut controls, QuickGitKind::Branch, "a"));
        controls.result(
            &result("a", false, Some(QuickGitKind::Branch)),
            Some("/repo"),
        );
        controls.begin_click(QuickGitKind::Branch);
        assert!(!open(&mut controls, QuickGitKind::Branch, "b"));
        assert!(open(&mut controls, QuickGitKind::Branch, "c"));
        controls.begin_click(QuickGitKind::Workspace);
        controls.result(&result("c", false, None), Some("/repo"));
        assert!(open(&mut controls, QuickGitKind::Workspace, "d"));
        assert_eq!(controls.active(), Some("d"));
    }

    #[test]
    fn a_failed_open_resets_only_the_live_request() {
        let mut controls = GitControls::default();
        open(&mut controls, QuickGitKind::Branch, "a");
        open(&mut controls, QuickGitKind::Workspace, "b");
        assert!(!controls.open_failed("a"));
        assert!(controls.open_failed("b"));
        assert_eq!(controls.open(), None);
        assert_eq!(controls.cancel(), None);
    }
}
