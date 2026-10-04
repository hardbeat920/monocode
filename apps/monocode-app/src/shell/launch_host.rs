//! Quick launches and reminders target the workspace that owns this window.
use super::{Shell, SidebarTab};
use gpui::{AnyWindowHandle, App, Task, WeakEntity, Window};
use monocode_core::Session;
use monocode_engine::automations::{
    AutomationsPackage,
    host::{LaunchHost, ReminderHost, SessionPlacement},
};
use monocode_layout::{PaneEdge, SplitDir, WorkspaceTab, leaf_ids};
use std::rc::Rc;

pub struct WindowLaunchHost {
    pub shell: WeakEntity<Shell>,
    pub window: AnyWindowHandle,
    visibility: Option<Rc<dyn Fn() -> bool>>,
    focus: Option<Rc<dyn Fn() -> bool>>,
}
impl WindowLaunchHost {
    pub fn new(shell: WeakEntity<Shell>, window: &Window) -> Self {
        #[cfg(target_os = "macos")]
        let visibility = monocode_platform::macos_panel::visibility_reader(window).ok();
        #[cfg(all(windows, not(test)))]
        let visibility = monocode_platform::windows::visibility_reader(window).ok();
        #[cfg(any(not(any(target_os = "macos", windows)), all(windows, test)))]
        let visibility = None;
        #[cfg(target_os = "macos")]
        let focus = monocode_platform::macos_panel::focus_reader(window).ok();
        #[cfg(all(windows, not(test)))]
        let focus = monocode_platform::windows::focus_reader(window).ok();
        #[cfg(any(not(any(target_os = "macos", windows)), all(windows, test)))]
        let focus = None;
        Self {
            shell,
            window: window.window_handle(),
            visibility,
            focus,
        }
    }

    fn workspace(&self, cx: &App) -> Option<gpui::Entity<monocode_engine::workspace::Workspace>> {
        self.shell.upgrade()?.read(cx).workspace.clone()
    }
    fn show(&self, cwd: &str, cx: &mut App) {
        if let Some(shell) = self.shell.upgrade() {
            shell.update(cx, |shell, cx| {
                shell.close_page(cx);
                shell.set_sidebar_tab(SidebarTab::Sessions, cx);
                if let Some(workspace) = &shell.workspace {
                    workspace.update(cx, |workspace, cx| workspace.set_project_cwd(cwd, cx));
                }
                shell.set_session_sidebar_open(true, cx);
            });
        }
    }
    fn focused(&self, cx: &App) -> bool {
        self.shell.upgrade().is_some()
            && self.focus.as_ref().map_or_else(
                || {
                    cx.active_window()
                        .is_some_and(|window| window.window_id() == self.window.window_id())
                },
                |focused| focused(),
            )
    }
    fn bring(&self, cx: &mut App) {
        let _ = self.window.update(cx, |_, window, cx| {
            super::windows::bring_forward(window, cx);
        });
    }
}
impl LaunchHost for WindowLaunchHost {
    fn append_tab(&self, tab: WorkspaceTab, cwd: &str, cx: &mut App) {
        if let Some(workspace) = self.workspace(cx) {
            workspace.update(cx, |workspace, cx| {
                workspace.append_project_tab(tab, Some(cwd), cx)
            });
        }
    }
    fn activate_tab(&self, tab_id: &str, cx: &mut App) {
        if let Some(workspace) = self.workspace(cx) {
            workspace.update(cx, |workspace, cx| {
                workspace.activate_tab(tab_id, None, cx);
                workspace.set_composer_focused(false, cx);
            });
        }
    }
    fn focus_open_session(&self, session_id: &str, cx: &mut App) {
        if let Some(workspace) = self.workspace(cx) {
            workspace.update(cx, |workspace, cx| {
                workspace.focus_open_session(session_id, cx);
            });
        }
    }
    fn show_sessions(&self, cwd: &str, cx: &mut App) {
        self.show(cwd, cx);
    }
    fn set_project_cwd(&self, cwd: &str, cx: &mut App) {
        if let Some(workspace) = self.workspace(cx) {
            workspace.update(cx, |workspace, cx| workspace.set_project_cwd(cwd, cx));
        }
    }
    fn place_session(
        &self,
        session_id: &str,
        placement: &SessionPlacement,
        _: &str,
        reveal: bool,
        cx: &mut App,
    ) -> Result<String, String> {
        let workspace = self.workspace(cx).ok_or("The workspace window closed.")?;
        let tab_id = workspace
            .read(cx)
            .tabs()
            .iter()
            .find(|tab| leaf_ids(&tab.layout).contains(&placement.beside_session_id))
            .map(|tab| tab.id.clone())
            .ok_or("The target session must be open in this project")?;
        let edge = if placement.direction == SplitDir::Right {
            PaneEdge::Right
        } else {
            PaneEdge::Bottom
        };
        workspace.update(cx, |workspace, cx| {
            workspace
                .place_session_on_pane(session_id, &placement.beside_session_id, edge, cx)
                .detach();
            if reveal {
                workspace.activate_tab(&tab_id, None, cx);
            }
        });
        Ok(tab_id)
    }
    fn is_focused(&self, cx: &App) -> bool {
        self.focused(cx)
    }
    fn is_visible(&self, cx: &App) -> bool {
        self.shell.upgrade().is_some()
            && cx
                .windows()
                .iter()
                .any(|window| window.window_id() == self.window.window_id())
            && self.visibility.as_ref().is_none_or(|visible| visible())
    }
    fn bring_forward(&self, cx: &mut App) {
        self.bring(cx);
    }
}
impl ReminderHost for WindowLaunchHost {
    fn show_session(&self, session: &Session, cx: &mut App) -> Task<Result<(), String>> {
        self.show(&session.cwd, cx);
        let Some(workspace) = self.workspace(cx) else {
            return Task::ready(Err("The workspace window closed.".into()));
        };
        let opening = workspace.update(cx, |workspace, cx| workspace.open_session(&session.id, cx));
        cx.spawn(async move |_| {
            opening.await;
            Ok(())
        })
    }
    fn is_focused(&self, cx: &App) -> bool {
        self.focused(cx)
    }
    fn bring_forward(&self, cx: &mut App) {
        self.bring(cx);
    }
}

pub fn attach(shell: WeakEntity<Shell>, window: &Window, label: &str, cx: &mut App) {
    let Some(package) = AutomationsPackage::try_global(cx) else {
        return;
    };
    let (quick, reminders) = (package.quick_launch.clone(), package.reminders.clone());
    let host = Rc::new(WindowLaunchHost::new(shell, window));
    quick.update(cx, |quick, cx| {
        quick.detach_window("main");
        quick.attach_window(label, host.clone(), cx);
    });
    reminders.update(cx, |reminders, cx| {
        reminders.detach_window("main");
        reminders.attach_window(label, host, cx);
    });
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use gpui::TestAppContext;

    use super::*;
    use crate::shell::ShellOptions;

    #[gpui::test]
    fn launch_visibility_follows_the_native_window_without_detaching_hidden_windows(
        cx: &mut TestAppContext,
    ) {
        cx.skip_drawing();
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
        });
        let window = cx.add_window(|window, cx| Shell::new(ShellOptions::full(), window, cx));
        let visible = Rc::new(Cell::new(true));
        let native_visibility = visible.clone();
        let focused = Rc::new(Cell::new(false));
        let native_focus = focused.clone();
        let host = window
            .update(cx, |_, window, cx| {
                let mut host = WindowLaunchHost::new(cx.entity().downgrade(), window);
                host.visibility = Some(Rc::new(move || native_visibility.get()));
                host.focus = Some(Rc::new(move || native_focus.get()));
                host
            })
            .unwrap();
        assert!(cx.update(|cx| LaunchHost::is_visible(&host, cx)));
        assert!(!cx.update(|cx| LaunchHost::is_focused(&host, cx)));
        focused.set(true);
        assert!(cx.update(|cx| LaunchHost::is_focused(&host, cx)));
        focused.set(false);
        assert!(!cx.update(|cx| LaunchHost::is_focused(&host, cx)));
        visible.set(false);
        assert!(!cx.update(|cx| LaunchHost::is_visible(&host, cx)));
        assert!(host.shell.upgrade().is_some());
        assert!(cx.update(|cx| {
            cx.windows()
                .iter()
                .any(|open| open.window_id() == host.window.window_id())
        }));
        visible.set(true);
        assert!(cx.update(|cx| LaunchHost::is_visible(&host, cx)));
    }
}
