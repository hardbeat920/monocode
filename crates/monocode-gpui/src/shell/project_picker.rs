use super::*;
use crate::project_picker::{Labels, Picked, Project, ProjectPicker};
pub(super) struct Open {
    picker: Entity<ProjectPicker>,
    restore: Option<FocusHandle>,
    _events: Subscription,
}
impl Shell {
    pub(super) fn close_project_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.project_picker.take() {
            if let Some(restore) = open.restore {
                window.focus(&restore, cx);
            } else {
                self.focus_active(window, cx);
            }
        }
        cx.notify();
    }
    /// Chooses the project for a copy of Mono `id`. Its own projects are
    /// listed but cannot be picked.
    pub(super) fn open_mono_duplicate(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mono) = self
            .monos
            .as_ref()
            .and_then(|m| m.read(cx).roster.get(&id).cloned())
        else {
            return;
        };
        let restore = self
            .project_picker
            .as_ref()
            .and_then(|o| o.restore.clone())
            .or_else(|| window.focused(cx));
        self.close_menu(cx);
        self.close_preview(cx);
        let projects = self
            .workspace
            .projects
            .iter()
            .map(|path| Project {
                path: path.clone(),
                label: self.project_label(path),
                appearance: self.appearance(path),
                current: *path == self.workspace.project,
                unavailable: mono.works_on(path).then_some("Already assigned"),
            })
            .collect();
        let labels = Labels {
            title: format!("Duplicate {} to project", mono.label()).into(),
            action: "duplicate",
            empty: "No projects yet. Open one with ⌘O.",
        };
        let picker = cx.new(|cx| ProjectPicker::new(projects, labels, cx));
        let events = cx.subscribe_in(
            &picker,
            window,
            move |this, _, event: &Picked, window, cx| {
                this.close_project_picker(window, cx);
                if let Picked::Project(path) = event {
                    this.duplicate_mono_to(&id, path.clone(), window, cx);
                }
            },
        );
        window.focus(&picker.focus_handle(cx), cx);
        self.project_picker = Some(Open {
            picker,
            restore,
            _events: events,
        });
        cx.notify();
    }
    #[cfg(test)]
    pub(crate) fn project_picker_open(&self) -> bool {
        self.project_picker.is_some()
    }
    /// The copy opens in its project, so go there first: Monos close when
    /// the project changes.
    fn duplicate_mono_to(
        &mut self,
        id: &str,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if path != self.workspace.project {
            self.select_project(path.clone(), window, cx);
        }
        if let Some(monos) = self.monos.clone() {
            monos.update(cx, |m, cx| m.duplicate(id, path, window, cx));
        }
    }
    pub(super) fn project_picker_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.project_picker.as_ref()?;
        let viewport = window.viewport_size();
        let width = 520f32.min(viewport.width.as_f32() - 32.);
        let anchor = Anchor::Point(gpui::point(
            px((viewport.width.as_f32() - width) / 2.),
            px((viewport.height.as_f32() * 0.16).max(32.)),
        ));
        let weak = cx.weak_entity();
        Some(
            div()
                .absolute()
                .size_full()
                .child(crate::modal::scrim("project-picker-scrim"))
                .child(
                    popover("project-picker-modal", anchor)
                        .modal_surface(self.workspace.boxy_layout())
                        .width(width)
                        .gap(0.)
                        .priority(80)
                        .on_dismiss(move |window, cx| {
                            let _ =
                                weak.update(cx, |this, cx| this.close_project_picker(window, cx));
                        })
                        .child(open.picker.clone()),
                )
                .into_any_element(),
        )
    }
}
