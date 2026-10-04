//! The live agent preview shared by the project rail and sidebar footer.

use super::Shell;
use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, WeakEntity,
    Window, div,
};
use monocode_engine::{attention::Attention, projects::ProjectsGlobal, runtime::Engine};
use monocode_layout::tab_groups::JsRecord;
use monocode_view_transcript::threads::{
    LiveAgent, LiveAgentsPreview, LiveAgentsPreviewEvent, ProjectAppearance,
};

#[derive(PartialEq)]
struct Snapshot {
    agents: Vec<LiveAgent>,
    active: Option<String>,
    bottom_spacing: bool,
    labels: JsRecord<String>,
    colors: JsRecord<usize>,
    custom_colors: JsRecord<String>,
    mascots: JsRecord<String>,
}

pub(super) struct LiveAgentsArea {
    shell: WeakEntity<Shell>,
    preview: Entity<LiveAgentsPreview>,
    snapshot: Option<Snapshot>,
    visible: bool,
}

impl LiveAgentsArea {
    pub(super) fn new(shell: WeakEntity<Shell>, cx: &mut Context<Self>) -> Self {
        let preview = cx.new(LiveAgentsPreview::new);
        cx.subscribe(&preview, |this, _, event: &LiveAgentsPreviewEvent, cx| {
            let LiveAgentsPreviewEvent::Select(id) = event;
            this.shell
                .update(cx, |shell, cx| shell.open_session(id, cx))
                .ok();
        })
        .detach();
        if let Some(sessions) = Engine::try_global(cx).map(|engine| engine.sessions.clone()) {
            cx.observe(&sessions, |_, _, cx| cx.notify()).detach();
        }
        if let Some(notifier) =
            Attention::try_global(cx).map(|attention| attention.notifier.clone())
        {
            cx.observe(&notifier, |_, _, cx| cx.notify()).detach();
        }
        if let Some(projects) = ProjectsGlobal::try_global(cx).map(|global| global.projects.clone())
        {
            cx.observe(&projects, |_, _, cx| cx.notify()).detach();
        }
        Self {
            shell,
            preview,
            snapshot: None,
            visible: true,
        }
    }

    pub(super) fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        self.snapshot = None;
        if !visible {
            self.preview
                .update(cx, |preview, cx| preview.set_agents(Vec::new(), cx));
        }
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut App) {
        let Some(shell) = self.shell.upgrade() else {
            return;
        };
        let (active, bottom_spacing) = {
            let shell = shell.read(cx);
            (
                shell
                    .workspace
                    .as_ref()
                    .and_then(|workspace| workspace.read(cx).active_session(cx))
                    .map(|session| session.id),
                shell.layout.compact_rail_visible(),
            )
        };
        let agents = monocode_engine::side_threads::live_agents(cx)
            .into_iter()
            .map(|agent| LiveAgent {
                id: agent.id,
                cwd: agent.cwd,
                title: agent.title,
                harness: agent.harness,
                activity: agent.activity,
                started_at: agent.started_at,
                duration_ms: agent.duration_ms,
                needs_approval: agent.needs_approval,
                done: agent.done,
            })
            .collect();
        let (labels, colors, custom_colors, mascots) = ProjectsGlobal::try_global(cx)
            .map(|global| global.projects.clone())
            .map(|projects| {
                projects.update(cx, |projects, _| {
                    (
                        projects.labels(),
                        projects.colors(),
                        projects.custom_colors(),
                        projects.mascots(),
                    )
                })
            })
            .unwrap_or_default();
        let snapshot = Snapshot {
            agents,
            active,
            bottom_spacing,
            labels,
            colors,
            custom_colors,
            mascots,
        };
        if self.snapshot.as_ref() == Some(&snapshot) {
            return;
        }
        self.preview.update(cx, |preview, cx| {
            preview.set_agents(snapshot.agents.clone(), cx);
            preview.set_active_session_id(snapshot.active.clone(), cx);
            preview.set_bottom_spacing(snapshot.bottom_spacing, cx);
            preview.set_appearance(
                ProjectAppearance {
                    labels: snapshot.labels.clone(),
                    colors: snapshot.colors.clone(),
                    custom_colors: snapshot.custom_colors.clone(),
                    mascots: snapshot.mascots.clone(),
                },
                cx,
            );
        });
        self.snapshot = Some(snapshot);
    }
}

impl Render for LiveAgentsArea {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.visible {
            self.refresh(cx);
        }
        div().child(self.preview.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::ShellOptions;
    use gpui::{Styled as _, TestAppContext};
    use monocode_core::{HarnessId, Session};
    use monocode_engine::{
        runtime::testing::init_test_engine,
        workspace::{Workspace, WorkspaceConfig},
    };

    struct PreviewRoot {
        shell: Entity<Shell>,
    }

    impl Render for PreviewRoot {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(self.shell.read(cx).live_agents.clone())
        }
    }

    #[gpui::test]
    fn live_preview_tracks_busy_sessions_routes_selection_and_stops_when_hidden(
        cx: &mut TestAppContext,
    ) {
        cx.skip_drawing();
        init_test_engine(cx);
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
            Engine::sessions(cx).update(cx, |sessions, cx| {
                for id in ["one", "two"] {
                    let mut session = Session::blank(id, HarnessId::Codex, "model", "/repo");
                    session.busy = Some(true);
                    sessions.insert(session, cx);
                }
            });
        });
        let (root, cx) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| {
                let mut shell = Shell::new(ShellOptions::full(), window, cx);
                // Preview selection uses the real workspace. The full pane lifecycle
                // requires the booted app packages and belongs to app integration tests.
                shell.workspace =
                    Some(cx.new(|cx| Workspace::new(WorkspaceConfig::fresh(Some("/repo")), cx)));
                shell
            });
            PreviewRoot { shell }
        });
        let area = cx.update(|_, cx| root.read(cx).shell.read(cx).live_agents.clone());
        area.update(cx, |area, cx| area.refresh(cx));
        let preview = area.read_with(cx, |area, _| area.preview.clone());
        preview.read_with(cx, |preview, _| {
            assert_eq!(preview.visible_cards().len(), 2)
        });
        preview.update(cx, |preview, cx| preview.select("two", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let shell = root.read(cx).shell.read(cx);
            assert_eq!(
                shell
                    .workspace
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .active_session(cx)
                    .unwrap()
                    .id,
                "two"
            );
        });
        area.update(cx, |area, cx| area.set_visible(false, cx));
        preview.read_with(cx, |preview, _| assert!(!preview.is_shown()));
        area.update(cx, |area, cx| {
            area.set_visible(true, cx);
            area.refresh(cx);
        });
        preview.read_with(cx, |preview, _| {
            assert_eq!(preview.visible_cards().len(), 2)
        });
    }
}
