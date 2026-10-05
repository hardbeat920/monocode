//! Port of src/features/sessions/model/archiveShortcut.ts: the archive key
//! acts only when the focused conversation owns it.
//!
//! The TypeScript read the DOM: the event target's ancestors and every open
//! overlay. A GPUI view knows those from its own focus handles, so it passes
//! them in as `ArchiveKeyEvent`, and the function returns the session to
//! archive instead of calling back. When it returns `Some`, the caller stops
//! the key from propagating before archiving, as `preventDefault` and
//! `stopPropagation` did.

/// One tab, as the archive shortcut reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchiveTab {
    pub id: String,
    pub focused_id: String,
    pub diff_focused: bool,
}

/// `ArchiveContext`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchiveContext {
    pub active_tab_id: String,
    pub tabs: Vec<ArchiveTab>,
    /// Ids of the open sessions.
    pub session_ids: Vec<String>,
    pub project_terminal_focused: bool,
    /// A full page (search, inbox, notes, automations, settings, the file
    /// picker, or What's new) covers the workspace.
    pub surface_open: bool,
}

/// Where keyboard focus sits when the key arrives: the parts of the event
/// target the TypeScript checked with `closest`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArchiveTarget {
    /// Inside a code editor (`.cm-editor`).
    pub in_code_editor: bool,
    /// Inside a terminal (`.monocode-terminal`).
    pub in_terminal: bool,
    /// Inside a text input, text area, select, or other editable field.
    pub in_text_field: bool,
    /// Inside the composer (`[data-composer]`).
    pub in_composer: bool,
}

/// The key event, as the archive shortcut reads it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArchiveKeyEvent {
    /// Another handler already consumed the key (`defaultPrevented`).
    pub default_prevented: bool,
    pub target: ArchiveTarget,
    /// A visible popover, dialog, alert, menu, skill picker, or mention picker
    /// is open anywhere in the window. Overlays in hidden or inactive
    /// surfaces do not count.
    pub overlay_open: bool,
}

/// `archiveFocusedSession`: the session the key archives, or `None` when
/// something else owns the key.
pub fn archive_focused_session(
    event: &ArchiveKeyEvent,
    context: &ArchiveContext,
) -> Option<String> {
    if event.default_prevented || context.project_terminal_focused || context.surface_open {
        return None;
    }
    let tab = context
        .tabs
        .iter()
        .find(|entry| entry.id == context.active_tab_id)?;
    if tab.diff_focused {
        return None;
    }
    let session = context
        .session_ids
        .iter()
        .find(|id| **id == tab.focused_id)?;
    let target = event.target;
    if target.in_code_editor || target.in_terminal {
        return None;
    }
    if target.in_text_field && !target.in_composer {
        return None;
    }
    // Popovers can leave focus in the composer, so any open overlay blocks.
    if event.overlay_open {
        return None;
    }
    Some(session.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (ArchiveKeyEvent, ArchiveContext) {
        let context = ArchiveContext {
            active_tab_id: "tab".into(),
            tabs: vec![ArchiveTab {
                id: "tab".into(),
                focused_id: "session".into(),
                diff_focused: false,
            }],
            session_ids: vec!["session".into(), "other".into()],
            project_terminal_focused: false,
            surface_open: false,
        };
        let event = ArchiveKeyEvent {
            default_prevented: false,
            target: ArchiveTarget {
                in_text_field: true,
                in_composer: true,
                ..Default::default()
            },
            overlay_open: false,
        };
        (event, context)
    }

    #[test]
    fn archives_exactly_the_focused_session() {
        let (event, mut context) = fixture();
        context.tabs[0].focused_id = "other".into();
        assert_eq!(
            archive_focused_session(&event, &context).as_deref(),
            Some("other")
        );
    }

    #[test]
    fn preserves_the_key_in_a_focused_editor_or_terminal_pane() {
        for pane in ["editor", "terminal"] {
            let (event, mut context) = fixture();
            context.tabs[0].focused_id = pane.into();
            assert_eq!(archive_focused_session(&event, &context), None, "{pane}");
        }
    }

    #[test]
    fn preserves_the_key_when_blocked() {
        for reason in ["diff", "dock", "surface", "missing tab", "already handled"] {
            let (mut event, mut context) = fixture();
            match reason {
                "diff" => context.tabs[0].diff_focused = true,
                "dock" => context.project_terminal_focused = true,
                "surface" => context.surface_open = true,
                "missing tab" => context.active_tab_id = "missing".into(),
                _ => event.default_prevented = true,
            }
            assert_eq!(archive_focused_session(&event, &context), None, "{reason}");
        }
    }

    #[test]
    fn respects_editor_terminal_and_input_focus_before_workspace_focus_updates() {
        let targets = [
            ArchiveTarget {
                in_code_editor: true,
                ..Default::default()
            },
            ArchiveTarget {
                in_terminal: true,
                ..Default::default()
            },
            ArchiveTarget {
                in_text_field: true,
                ..Default::default()
            },
        ];
        for target in targets {
            let (mut event, context) = fixture();
            event.target = target;
            assert_eq!(
                archive_focused_session(&event, &context),
                None,
                "{target:?}"
            );
        }
    }

    #[test]
    fn blocks_an_open_overlay_even_when_focus_remains_in_the_composer() {
        let (mut event, context) = fixture();
        event.overlay_open = true;
        assert_eq!(archive_focused_session(&event, &context), None);
        event.overlay_open = false;
        assert_eq!(
            archive_focused_session(&event, &context).as_deref(),
            Some("session")
        );
    }

    #[test]
    fn leaves_a_menu_rename_input_event_untouched() {
        let (mut event, context) = fixture();
        event.target = ArchiveTarget {
            in_text_field: true,
            ..Default::default()
        };
        event.overlay_open = true;
        assert_eq!(archive_focused_session(&event, &context), None);
    }
}
