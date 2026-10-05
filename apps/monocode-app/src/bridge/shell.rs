//! Requests from engine hooks to the window shell: page and sidebar state
//! that lived in App.tsx's React state (`setSearchViewOpen`,
//! `setInboxViewOpen`, `setSidebarTab`, and so on). The shell subscribes to
//! [`ShellRequests::entity`] and applies them.

use gpui::{App, AppContext as _, Entity, EventEmitter, Global};

/// A full page, as the shell names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShellPage {
    Search,
    Inbox,
    Notes,
    Automations,
    Settings,
}

/// What an engine flow asks the shell to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellRequest {
    /// Close the Search, Inbox, Notes, and Automations pages.
    ClosePages,
    /// Close one page (for example `setInboxViewOpen(false)`).
    ClosePage(ShellPage),
    /// Open a page.
    OpenPage(ShellPage),
    /// Show the sessions sidebar tab, for `cwd` when given.
    ShowSessions {
        cwd: Option<String>,
    },
    /// Unminimize, show, and focus the window.
    BringForward,
    /// Hide the active workspace while its agent processes keep running.
    HideWindow,
    /// Apply the saved rail mode to every open window.
    SetCompactRail(bool),
    /// The remembered sidebar selection follows project removal or rename.
    ProjectSidebarRemoved(String),
    ProjectSidebarMoved {
        from: String,
        to: String,
    },
}

/// The emitter the shell subscribes to.
pub struct ShellRequests;

impl EventEmitter<ShellRequest> for ShellRequests {}

struct ShellRequestsGlobal(Entity<ShellRequests>);

impl Global for ShellRequestsGlobal {}

impl ShellRequests {
    /// The app's emitter, created on first use.
    pub fn entity(cx: &mut App) -> Entity<ShellRequests> {
        if let Some(global) = cx.try_global::<ShellRequestsGlobal>() {
            return global.0.clone();
        }
        let entity = cx.new(|_| ShellRequests);
        cx.set_global(ShellRequestsGlobal(entity.clone()));
        entity
    }

    /// Send a request to the shell.
    pub fn send(request: ShellRequest, cx: &mut App) {
        let entity = Self::entity(cx);
        entity.update(cx, |_, cx| cx.emit(request));
    }
}
