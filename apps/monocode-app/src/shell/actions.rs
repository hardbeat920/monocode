//! The window's handlers for the keymap actions: what App.tsx's capture
//! key handler and its menu event listeners (lines 10259-10365) ran.

use gpui::{Context, Div, InteractiveElement as _, Stateful};

use super::Shell;
use super::keymap::*;
use crate::slots::Page;

impl Shell {
    /// Route the keymap's window-level actions to the shell.
    pub(super) fn register_actions(
        &mut self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        root.on_action(cx.listener(|this, _: &NewSession, _, cx| {
            this.new_session(cx);
        }))
        .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_project_rail(cx)))
        .on_action(
            cx.listener(|this, _: &ToggleSessionSidebar, _, cx| this.toggle_session_sidebar(cx)),
        )
        .on_action(
            cx.listener(|this, _: &OpenSettings, _, cx| this.toggle_page(Page::Settings, cx)),
        )
        .on_action(cx.listener(|this, _: &OpenSearch, _, cx| this.toggle_page(Page::Search, cx)))
        .on_action(cx.listener(|this, _: &FindInFiles, _, cx| this.open_page(Page::Search, cx)))
        .on_action(
            cx.listener(|this, _: &GoToFile, window, cx| this.open_file_picker(false, window, cx)),
        )
        .on_action(cx.listener(|this, _: &CommandPalette, window, cx| {
            this.open_file_picker(true, window, cx)
        }))
        .on_action(cx.listener(|this, _: &OpenInbox, _, cx| this.open_page(Page::Inbox, cx)))
        .on_action(cx.listener(|this, _: &OpenNotes, _, cx| this.open_page(Page::Notes, cx)))
        .on_action(cx.listener(|this, _: &SwitchModel, window, cx| {
            if this.layout.page.is_some()
                || this.file_picker.is_some()
                || this.project_menu.is_some()
                || super::shortcuts::in_context(window, "Terminal")
                || super::shortcuts::in_context(window, "FilePicker")
            {
                cx.propagate();
                return;
            }
            crate::panes::workspace_area(window, cx)
                .update(cx, |area, cx| area.switch_model(window, cx));
        }))
        .on_action(cx.listener(
            |this,
             _: &monocode_view_workbench::panes::workspace_picker::ToggleWorkspaceMode,
             window,
             cx| {
                if this.layout.page.is_some()
                    || this.shortcut_overlay_open(window, cx)
                    || super::shortcuts::in_context(window, "Terminal")
                {
                    cx.propagate();
                    return;
                }
                crate::panes::workspace_area(window, cx)
                    .update(cx, |area, cx| area.toggle_workspace_mode(window, cx));
            },
        ))
        .on_action(cx.listener(|this, _: &GoBack, _, cx| this.go_back(cx)))
        .on_action(cx.listener(|this, _: &GoForward, _, cx| this.go_forward(cx)))
        .on_action(cx.listener(|this, _: &NextTab, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.next_tab(cx));
            }
        }))
        .on_action(cx.listener(|this, _: &CycleNextTab, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.next_tab(cx));
            }
        }))
        .on_action(cx.listener(|this, _: &PrevTab, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.prev_tab(cx));
            }
        }))
        .on_action(cx.listener(|this, _: &CyclePrevTab, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.prev_tab(cx));
            }
        }))
        .on_action(cx.listener(|this, _: &CloseOtherTabs, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.close_other_tabs(cx).detach());
            }
        }))
        .on_action(cx.listener(|this, _: &CloseAllTabs, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.close_all_tabs(cx).detach());
            }
        }))
        .on_action(cx.listener(|this, _: &ClosePane, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.close_pane(None, cx).detach());
            }
        }))
        .on_action(cx.listener(|this, _: &SplitRight, window, cx| {
            if super::shortcuts::in_context(window, "CodeEditor") {
                cx.propagate();
                return;
            }
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| {
                    workspace.split(monocode_layout::SplitDir::Right, cx)
                });
            }
        }))
        .on_action(cx.listener(|this, _: &SplitDown, window, cx| {
            if super::shortcuts::in_context(window, "CodeEditor") {
                cx.propagate();
                return;
            }
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| {
                    workspace.split(monocode_layout::SplitDir::Down, cx)
                });
            }
        }))
        .on_action(cx.listener(|this, _: &FocusLeft, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| {
                    workspace.focus_dir(monocode_layout::FocusDir::Left, cx)
                });
            }
        }))
        .on_action(cx.listener(|this, _: &FocusRight, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| {
                    workspace.focus_dir(monocode_layout::FocusDir::Right, cx)
                });
            }
        }))
        .on_action(cx.listener(|this, _: &FocusUp, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| {
                    workspace.focus_dir(monocode_layout::FocusDir::Up, cx)
                });
            }
        }))
        .on_action(cx.listener(|this, _: &FocusDown, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| {
                    workspace.focus_dir(monocode_layout::FocusDir::Down, cx)
                });
            }
        }))
        .on_action(cx.listener(|this, _: &NewTerminal, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.new_terminal(cx));
            }
        }))
        .on_action(cx.listener(|this, _: &NewTerminalTab, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.new_terminal_tab(cx));
            }
        }))
        .on_action(cx.listener(|this, _: &ToggleTerminal, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.toggle_project_terminal(cx));
            }
        }))
        .on_action(cx.listener(|this, _: &ActivateTab1, window, cx| {
            if this.shortcut_overlay_open(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.activate_slot(1, cx));
            }
        }))
        .on_action(cx.listener(|this, _: &ActivateTab2, window, cx| {
            if this.shortcut_overlay_open(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.activate_slot(2, cx));
            }
        }))
        .on_action(cx.listener(|this, _: &ActivateTab3, window, cx| {
            if this.shortcut_overlay_open(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.activate_slot(3, cx));
            }
        }))
        .on_action(cx.listener(|this, _: &ActivateTab4, window, cx| {
            if this.shortcut_overlay_open(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.activate_slot(4, cx));
            }
        }))
        .on_action(cx.listener(|this, _: &ActivateTab5, window, cx| {
            if this.shortcut_overlay_open(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.activate_slot(5, cx));
            }
        }))
        .on_action(cx.listener(|this, _: &ActivateTab6, window, cx| {
            if this.shortcut_overlay_open(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.activate_slot(6, cx));
            }
        }))
        .on_action(cx.listener(|this, _: &ActivateTab7, window, cx| {
            if this.shortcut_overlay_open(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.activate_slot(7, cx));
            }
        }))
        .on_action(cx.listener(|this, _: &ActivateTab8, window, cx| {
            if this.shortcut_overlay_open(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.activate_slot(8, cx));
            }
        }))
        .on_action(cx.listener(|this, _: &ActivateLastTab, window, cx| {
            if this.shortcut_overlay_open(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.activate_slot(9, cx));
            }
        }))
        .on_action(
            cx.listener(|this, _: &OpenAutomations, _, cx| this.toggle_page(Page::Automations, cx)),
        )
        .on_action(cx.listener(|this, _: &PrevSession, window, cx| {
            if !this.list_navigation_allowed(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(history) = &this.history {
                history.update(cx, |history, cx| {
                    history.navigate_session_list(-1, false, cx)
                });
            }
        }))
        .on_action(cx.listener(|this, _: &NextSession, window, cx| {
            if !this.list_navigation_allowed(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(history) = &this.history {
                history.update(cx, |history, cx| {
                    history.navigate_session_list(1, false, cx)
                });
            }
        }))
        .on_action(cx.listener(|this, _: &PrevSessionInTab, window, cx| {
            if !this.list_navigation_allowed(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(history) = &this.history {
                history.update(cx, |history, cx| {
                    history.navigate_session_list(-1, true, cx)
                });
            }
        }))
        .on_action(cx.listener(|this, _: &NextSessionInTab, window, cx| {
            if !this.list_navigation_allowed(window, cx) {
                cx.propagate();
                return;
            }
            if let Some(history) = &this.history {
                history.update(cx, |history, cx| history.navigate_session_list(1, true, cx));
            }
        }))
        .on_action(cx.listener(|this, _: &PrevProject, window, cx| {
            if !this.list_navigation_allowed(window, cx) {
                cx.propagate();
                return;
            }
            this.navigate_project(-1, cx);
        }))
        .on_action(cx.listener(|this, _: &NextProject, window, cx| {
            if !this.list_navigation_allowed(window, cx) {
                cx.propagate();
                return;
            }
            this.navigate_project(1, cx);
        }))
        .on_action(cx.listener(|_, _: &MinimizeWindow, window, _| window.minimize_window()))
        .on_action(cx.listener(|_, _: &ZoomWindow, window, _| window.zoom_window()))
        .on_action(cx.listener(|this, _: &ToggleAutosave, _, cx| {
            if let Some(services) = monocode_app::boot::AppServices::try_global(cx) {
                let enabled = !monocode_settings::settings_store::load_autosave(&services.kv);
                monocode_settings::settings_store::save_autosave(&services.kv, enabled);
            }
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |_, cx| cx.notify());
            }
        }))
        .on_action(cx.listener(|this, _: &Reload, _, cx| {
            if let Some(workspace) = &this.workspace {
                workspace.update(cx, |workspace, cx| workspace.window_focused(cx));
            }
            if let Some(history) = &this.history {
                let cwd = this.sidebar_cwd(cx);
                history.update(cx, |history, cx| history.refresh(&cwd, cx));
            }
            if let Some(projects) = monocode_engine::projects::ProjectsGlobal::try_global(cx) {
                let git = projects.git.clone();
                git.update(cx, |git, cx| git.git_changed(cx));
            }
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &ArchiveSession, window, cx| {
            if this.layout.page.is_some()
                || this.shortcut_overlay_open(window, cx)
                || super::shortcuts::in_context(window, "CodeEditor")
                || super::shortcuts::in_context(window, "Terminal")
                || (super::shortcuts::in_context(window, "Input")
                    && !super::shortcuts::in_context(window, "PromptInput"))
            {
                cx.propagate();
                return;
            }
            if let Some((_, session)) = this.focused_session(cx) {
                super::session_actions::remove_sessions(
                    vec![session],
                    this.history.clone(),
                    monocode_engine::history::session_removal::SessionRemovalMode::Archive,
                    cx,
                );
            }
        }))
        .on_action(cx.listener(|_, _: &OpenWebsite, _, cx| cx.open_url("https://monocode.app")))
        .on_action(
            cx.listener(|_, _: &OpenGithub, _, cx| {
                cx.open_url("https://github.com/zaesho/monocode")
            }),
        )
        .on_action(cx.listener(|_, _: &ReportBug, _, cx| {
            cx.open_url("https://github.com/zaesho/monocode/issues/new")
        }))
        .on_action(cx.listener(|_, _: &OpenProject, _, cx| {
            let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
                files: false,
                directories: true,
                multiple: true,
                prompt: None,
            });
            cx.spawn(async move |_, cx| {
                if let Ok(Ok(Some(paths))) = picked.await {
                    let paths = paths
                        .iter()
                        .map(|path| path.to_string_lossy().to_string())
                        .collect::<Vec<_>>();
                    cx.update(|cx| monocode_engine::projects::actions::open_projects(&paths, cx));
                }
            })
            .detach();
        }))
    }
    fn navigate_project(&mut self, delta: i64, cx: &mut Context<Self>) {
        let Some(projects) = monocode_engine::projects::ProjectsGlobal::try_global(cx) else {
            return;
        };
        let projects = projects.projects.read(cx);
        let paths = projects
            .recents()
            .iter()
            .map(|project| project.path.clone())
            .collect::<Vec<_>>();
        let cwd = self.sidebar_cwd(cx);
        let Some(index) = paths
            .iter()
            .position(|path| monocode_layout::paths::same_project_path(path, &cwd))
        else {
            return;
        };
        if paths.len() < 2 {
            return;
        }
        let index = (index as i64 + delta).rem_euclid(paths.len() as i64) as usize;
        self.select_project(&paths[index], cx);
    }
}
