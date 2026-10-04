//! Focus checks for workspace shortcuts and unhandled Escape.

use gpui::{App, Context, KeyDownEvent, Window};
use monocode_core::Session;
use monocode_engine::{remote::RemoteGlobal, runtime::Engine, submit::Submit};
use monocode_layout::paths::is_remote_project_path;

use super::{Shell, SidebarTab};

pub(super) fn in_context(window: &Window, name: &str) -> bool {
    window
        .context_stack()
        .iter()
        .any(|context| context.contains(name))
}

impl Shell {
    pub(super) fn shortcut_overlay_open(&self, window: &mut Window, cx: &mut App) -> bool {
        self.file_picker.is_some()
            || self.project_menu.is_some()
            || self.project_picker.is_some()
            || self.project_dialog.is_some()
            || self.title_bar.read(cx).has_open_menu()
            || (self.layout.session_sidebar_open
                && self.layout.sidebar_tab == SidebarTab::Sessions
                && self.sidebar.read(cx).has_open_overlay(cx))
            || crate::panes::workspace_area(window, cx)
                .read(cx)
                .session_shortcuts_blocked(cx)
            || [
                "Dialog",
                "Popover",
                "ModelPicker",
                "FilePicker",
                "AppSearch",
            ]
            .iter()
            .any(|name| in_context(window, name))
    }

    pub(super) fn list_navigation_allowed(&self, window: &mut Window, cx: &mut App) -> bool {
        if self.layout.page.is_some() || self.shortcut_overlay_open(window, cx) {
            return false;
        }
        if in_context(window, "PromptInput") {
            return crate::panes::workspace_area(window, cx)
                .read(cx)
                .list_navigation_allowed(cx);
        }
        !["Input", "CodeEditor", "Terminal"]
            .iter()
            .any(|name| in_context(window, name))
    }

    pub(super) fn focused_session(&self, cx: &App) -> Option<(String, Session)> {
        let workspace = self.workspace.as_ref()?.read(cx);
        if workspace.full_page_open() || workspace.terminals().read(cx).is_focused() {
            return None;
        }
        let tab = workspace.active_tab()?;
        if tab.diff_focused == Some(true) {
            return None;
        }
        let session = Engine::sessions(cx).read(cx).get(&tab.focused_id)?.clone();
        Some((tab.id.clone(), session))
    }

    pub(super) fn on_unhandled_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key != "escape"
            || event.is_held
            || event.prefer_character_input
            || event.keystroke.modifiers.modified()
            || self.layout.page.is_some()
            || cx.has_active_drag()
            || in_context(window, "Terminal")
            || self.shortcut_overlay_open(window, cx)
        {
            return;
        }
        let Some((tab_id, session)) = self.focused_session(cx) else {
            return;
        };
        if !session.is_busy() {
            return;
        }
        let shell = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            let _ = shell.update(cx, |shell, cx| {
                if shell.layout.page.is_some()
                    || cx.has_active_drag()
                    || in_context(window, "Terminal")
                    || shell.shortcut_overlay_open(window, cx)
                {
                    return;
                }
                let Some((current_tab, current)) = shell.focused_session(cx) else {
                    return;
                };
                if current_tab != tab_id || current.id != session.id || !current.is_busy() {
                    return;
                }
                if is_remote_project_path(&current.cwd) {
                    RemoteGlobal::stop(&current.id, cx);
                } else if let Some(submit) = Submit::try_global(cx) {
                    submit.update(cx, |submit, cx| submit.stop(&current.id, false, cx));
                }
            });
        });
        cx.stop_propagation();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::{
        AppContext as _, Entity, FocusHandle, InteractiveElement as _, IntoElement,
        ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, div,
    };
    use monocode_engine::{
        history::History, runtime::testing::init_test_engine, submit::SubmitConfig,
        workspace::WorkspaceConfig,
    };
    use monocode_harness::core::{
        catalog::SharedCatalog,
        registry::{HarnessRegistry, RegistryOptions},
        task::SharedSpawner,
    };
    use monocode_settings::Kv;

    use super::*;
    use crate::shell::ShellOptions;

    struct KeyRoot {
        shell: Entity<Shell>,
        focus: FocusHandle,
        claim_escape: bool,
        terminal: bool,
    }

    impl Render for KeyRoot {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let root = div().id("escape-test").key_context("Shell").size_full();
            let root = self.shell.update(cx, |shell, cx| {
                shell
                    .register_actions(root, cx)
                    .on_key_down(cx.listener(Shell::on_unhandled_key))
            });
            root.child(
                div()
                    .id("escape-child")
                    .key_context(if self.terminal { "Terminal" } else { "Session" })
                    .track_focus(&self.focus)
                    .size_full()
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape" && this.claim_escape {
                            cx.stop_propagation();
                        }
                    })),
            )
        }
    }

    fn mount(cx: &mut TestAppContext) -> (Entity<KeyRoot>, &mut VisualTestContext, String) {
        init_test_engine(cx);
        let executor = cx.executor();
        let spawner: SharedSpawner =
            Arc::new(move |future: futures::future::BoxFuture<'static, ()>| {
                executor.spawn(future).detach()
            });
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
            Submit::init(
                SubmitConfig::new(
                    HarnessRegistry::new(spawner.clone(), RegistryOptions::default()),
                    SharedCatalog::new(),
                    Kv::in_memory(),
                    spawner,
                ),
                cx,
            );
        });
        let (root, cx) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| {
                let mut shell = Shell::new(ShellOptions::full(), window, cx);
                let history = cx.new(|cx| History::new(Kv::in_memory(), cx));
                shell.attach(WorkspaceConfig::fresh(Some("/repo")), history, window, cx);
                shell
            });
            let focus = cx.focus_handle();
            focus.focus(window, cx);
            KeyRoot {
                shell,
                focus,
                claim_escape: false,
                terminal: false,
            }
        });
        let id = cx.update(|window, cx| {
            window.activate_window();
            root.read(cx)
                .shell
                .read(cx)
                .focused_session(cx)
                .unwrap()
                .1
                .id
        });
        cx.run_until_parked();
        (root, cx, id)
    }

    fn busy(id: &str, value: bool, cx: &mut VisualTestContext) {
        cx.update(|_, cx| {
            Engine::sessions(cx).update(cx, |sessions, cx| {
                sessions.update(id, cx, |session| session.busy = Some(value));
            });
        });
        cx.run_until_parked();
    }

    fn is_busy(id: &str, cx: &mut VisualTestContext) -> bool {
        cx.update(|_, cx| Engine::sessions(cx).read(cx).get(id).unwrap().is_busy())
    }

    #[gpui::test]
    fn only_unclaimed_escape_stops_the_focused_turn(cx: &mut TestAppContext) {
        let (root, cx, id) = mount(cx);
        busy(&id, true, cx);
        root.update(cx, |root, cx| {
            root.claim_escape = true;
            cx.notify();
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(is_busy(&id, cx));
        root.update(cx, |root, cx| {
            root.claim_escape = false;
            cx.notify();
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(!is_busy(&id, cx));
    }

    #[gpui::test]
    fn escape_preserves_terminal_and_hidden_workspace_turns(cx: &mut TestAppContext) {
        let (root, cx, id) = mount(cx);
        busy(&id, true, cx);
        root.update(cx, |root, cx| {
            root.terminal = true;
            cx.notify();
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(is_busy(&id, cx));
        root.update(cx, |root, cx| {
            root.terminal = false;
            root.shell.update(cx, |shell, cx| {
                shell.open_page(crate::slots::Page::Settings, cx)
            });
            cx.notify();
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(is_busy(&id, cx));
    }
}
