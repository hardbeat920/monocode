use gpui::{AnyElement, Context, IntoElement as _};
use monocode_engine::history::session_removal::SessionRemovalMode;
use monocode_engine::runtime::Engine;
use monocode_ui::widgets::{MenuEntry, MenuItem, context_menu, menu};

use super::model::{CloseSide, context_close_ids, tab_closable};
use super::{TitleBar, TitleTabView};

impl TitleBar {
    pub(super) fn render_tab_menu(
        &self,
        tabs: &[TitleTabView],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (id, position) = self.menu.as_ref()?;
        let tab = tabs.iter().find(|tab| &tab.id == id)?;
        let mut entries = vec![
            MenuItem::new("close", "Close Tab")
                .shortcut(if cfg!(target_os = "macos") {
                    "⌘W"
                } else {
                    "Ctrl+W"
                })
                .disabled(!tab_closable(tab, tabs.len()))
                .into(),
            MenuEntry::Separator,
        ];
        for (action, label, side) in [
            ("others", "Close Other Tabs", CloseSide::Others),
            ("right", "Close Tabs to the Right", CloseSide::Right),
            ("left", "Close Tabs to the Left", CloseSide::Left),
        ] {
            entries.push(
                MenuItem::new(action, label)
                    .disabled(context_close_ids(tabs, id, side).is_empty())
                    .into(),
            );
        }
        if tab.session_count > 0 {
            let mut archive = MenuItem::new("archive", "Archive");
            let mut delete = MenuItem::new("delete", "Delete").danger();
            if tab.session_count > 1 {
                archive = archive.description(format!(
                    "All {} conversations in this tab",
                    tab.session_count
                ));
                delete = delete.description(format!(
                    "Permanently delete all {} conversations in this tab",
                    tab.session_count
                ));
            }
            entries.extend([MenuEntry::Separator, archive.into(), delete.into()]);
        }
        let target = id.clone();
        let pick = cx.weak_entity();
        let dismiss = pick.clone();
        Some(
            context_menu(
                *position,
                menu("title-tab-menu", entries).on_pick(move |action, _, cx| {
                    pick.update(cx, |this, cx| {
                        this.menu = None;
                        match action.as_ref() {
                            "close" => {
                                this.with_shell(cx, |shell, cx| shell.close_tab(&target, cx))
                            }
                            "others" => this.close_tab_group(&target, CloseSide::Others, cx),
                            "right" => this.close_tab_group(&target, CloseSide::Right, cx),
                            "left" => this.close_tab_group(&target, CloseSide::Left, cx),
                            "archive" => this.remove_tab_conversations(
                                &target,
                                SessionRemovalMode::Archive,
                                cx,
                            ),
                            "delete" => this.remove_tab_conversations(
                                &target,
                                SessionRemovalMode::Delete,
                                cx,
                            ),
                            _ => {}
                        }
                        cx.notify();
                    })
                    .ok();
                }),
                move |_, cx| {
                    dismiss
                        .update(cx, |this, cx| {
                            this.menu = None;
                            cx.notify();
                        })
                        .ok();
                },
                cx,
            )
            .into_any_element(),
        )
    }

    fn close_tab_group(&self, target: &str, side: CloseSide, cx: &mut Context<Self>) {
        let (tabs, _) = self.tabs(cx);
        let ids = context_close_ids(&tabs, target, side);
        self.with_shell(cx, |shell, cx| {
            if let Some(workspace) = &shell.workspace {
                workspace
                    .update(cx, |workspace, cx| {
                        workspace.close_tabs(&ids, target, false, cx)
                    })
                    .detach();
            }
        });
    }

    fn remove_tab_conversations(
        &self,
        target: &str,
        mode: SessionRemovalMode,
        cx: &mut Context<Self>,
    ) {
        let Some(shell) = self.shell.upgrade() else {
            return;
        };
        let shell = shell.read(cx);
        let Some(workspace) = &shell.workspace else {
            return;
        };
        let Some(tab) = workspace
            .read(cx)
            .tabs()
            .iter()
            .find(|tab| tab.id == target)
        else {
            return;
        };
        let sessions = Engine::sessions(cx).read(cx);
        let sessions = monocode_layout::leaf_ids(&tab.layout)
            .iter()
            .filter_map(|id| sessions.get(id).cloned())
            .collect::<Vec<_>>();
        let history = shell.history.clone();
        super::super::session_actions::remove_sessions(sessions, history, mode, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::{Shell, ShellOptions};
    use gpui::{AppContext as _, TestAppContext};
    use monocode_core::{HarnessId, Session};
    use monocode_engine::history::History;
    use monocode_engine::runtime::testing::init_test_engine;
    use monocode_engine::workspace::WorkspaceConfig;
    use monocode_settings::Kv;

    #[gpui::test]
    fn closing_context_siblings_keeps_the_clicked_tab_and_selects_its_fallback(
        cx: &mut TestAppContext,
    ) {
        cx.skip_drawing();
        init_test_engine(cx);
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
            // Tab selection needs a workspace slot, without mounting session services.
            cx.set_global(crate::slots::AppSlots {
                workspace: Some(std::rc::Rc::new(|_, cx| cx.new(|_| gpui::EmptyView).into())),
                ..Default::default()
            });
        });
        let window = cx.add_window(|window, cx| {
            let mut shell = Shell::new(ShellOptions::full(), window, cx);
            let history = cx.new(|cx| History::new(Kv::in_memory(), cx));
            shell.attach(WorkspaceConfig::fresh(Some("/repo")), history, window, cx);
            shell
        });
        let (title_bar, workspace) = window
            .update(cx, |shell, _, _| {
                (shell.title_bar.clone(), shell.workspace.clone().unwrap())
            })
            .unwrap();
        cx.update(|cx| {
            let tabs = ["a", "b", "c", "d"].map(|id| {
                Engine::sessions(cx).update(cx, |sessions, cx| {
                    sessions.insert(Session::blank(id, HarnessId::Codex, "model", "/repo"), cx);
                });
                let mut tab = monocode_layout::new_tab(id);
                tab.id = id.into();
                tab
            });
            workspace.update(cx, |workspace, cx| {
                workspace.replace_project_tabs(tabs.into(), "d", cx)
            });
            title_bar.update(cx, |title_bar, cx| {
                title_bar.close_tab_group("b", CloseSide::Right, cx)
            });
        });
        cx.run_until_parked();
        workspace.read_with(cx, |workspace, _| {
            assert_eq!(
                workspace
                    .tabs()
                    .iter()
                    .map(|tab| tab.id.as_str())
                    .collect::<Vec<_>>(),
                ["a", "b"]
            );
            assert_eq!(workspace.active_tab_id(), "b");
        });
        cx.update(|cx| {
            title_bar.update(cx, |title_bar, cx| {
                title_bar.close_tab_group("b", CloseSide::Others, cx)
            })
        });
        cx.run_until_parked();
        workspace.read_with(cx, |workspace, _| {
            assert_eq!(
                workspace
                    .tabs()
                    .iter()
                    .map(|tab| tab.id.as_str())
                    .collect::<Vec<_>>(),
                ["b"]
            );
            assert_eq!(workspace.active_tab_id(), "b");
        });
    }
}
