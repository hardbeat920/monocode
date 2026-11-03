//! SessionFolderPicker.tsx and the composer's local /add-to-folder command.
use super::*;
use monocode_core::session_groups::Folder;
actions!(session_folders, [Dismiss]);
pub(super) fn bind(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        Dismiss,
        Some("SessionFolderPicker"),
    )]);
}

#[derive(Clone)]
enum Target {
    Existing(String),
    New(String),
    Dismiss,
}
struct Picker {
    input: Entity<input::Input>,
    folders: Vec<Folder>,
    active: usize,
    scroll: ScrollHandle,
    _changed: Subscription,
}
impl gpui::EventEmitter<Target> for Picker {}
impl Picker {
    fn new(folders: Vec<Folder>, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            input::Input::field(
                "Session folder",
                "Choose or name a session folder…",
                content(0.35),
                cx,
            )
        });
        let changed = cx.subscribe(&input, |this, _, _: &input::Changed, cx| {
            this.active = 0;
            cx.notify();
        });
        Self {
            input,
            folders,
            active: 0,
            scroll: ScrollHandle::new(),
            _changed: changed,
        }
    }
    fn rows(&self, cx: &App) -> Vec<(Target, String, Option<usize>)> {
        let name = self.input.read(cx).content.trim();
        let query = name.to_lowercase();
        let mut rows: Vec<_> = self
            .folders
            .iter()
            .filter(|f| f.name.to_lowercase().contains(&query))
            .map(|f| {
                (
                    Target::Existing(f.id.clone()),
                    f.name.clone(),
                    Some(f.session_ids.len()),
                )
            })
            .collect();
        if !name.is_empty() && !self.folders.iter().any(|f| f.name.to_lowercase() == query) {
            rows.push((Target::New(name.into()), format!("Create “{name}”"), None));
        }
        rows
    }
    fn step(&mut self, step: isize, cx: &mut Context<Self>) {
        let len = self.rows(cx).len();
        if len > 0 {
            self.active = (self.active as isize + step).rem_euclid(len as isize) as usize;
        }
        self.scroll.scroll_to_item(self.active);
        cx.notify();
    }
    fn pick(&self, cx: &mut Context<Self>) {
        if let Some((target, _, _)) = self.rows(cx).get(self.active) {
            cx.emit(target.clone());
        }
    }
}
impl Render for Picker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows(cx);
        div()
            .id("session-folder-picker")
            .debug_selector(|| "session-folder-picker".into())
            .role(Role::Dialog)
            .aria_label("Choose a session folder")
            .key_context("SessionFolderPicker")
            .capture_action(cx.listener(|_, _: &Dismiss, _, cx| {
                cx.emit(Target::Dismiss);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                if !this.input.read(cx).is_composing() {
                    this.step(-1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                if !this.input.read(cx).is_composing() {
                    this.step(1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &input::Submit, _, cx| {
                if !this.input.read(cx).is_composing() {
                    this.pick(cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .px_2p5()
                    .py_2()
                    .border_b_1()
                    .border_color(content(0.1))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon("folder", 14.).text_color(content(0.5)))
                    .child(div().min_w_0().flex_1().child(self.input.clone()))
                    .child(
                        icon_button("cancel-session-folder", "Cancel", "x")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Target::Dismiss))),
                    ),
            )
            .child(
                div()
                    .id("session-folders")
                    .role(Role::ListBox)
                    .aria_label("Session folders")
                    .max_h(px(240.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p_1()
                    .when(rows.is_empty(), |d| {
                        d.child(
                            div()
                                .px_2()
                                .py_2()
                                .text_size(px(12.))
                                .text_color(content(0.5))
                                .child("Type a name to create the first folder"),
                        )
                    })
                    .children(rows.into_iter().enumerate().map(|(ix, (_, label, count))| {
                        div()
                            .id(("session-folder-option", ix))
                            .debug_selector(move || format!("session-folder-option-{ix}"))
                            .role(Role::ListBoxOption)
                            .aria_label(label.clone())
                            .aria_selected(ix == self.active)
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .py_2()
                            .rounded_md()
                            .text_size(px(13.))
                            .when(ix == self.active, |d| d.bg(content(0.1)))
                            .child(
                                icon(if count.is_some() { "folder" } else { "plus" }, 14.)
                                    .text_color(content(0.5)),
                            )
                            .child(div().min_w_0().flex_1().truncate().child(label))
                            .when_some(count, |d, n| {
                                d.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(content(0.4))
                                        .child(n.to_string()),
                                )
                            })
                            .on_hover(cx.listener(move |this, hovered, _, cx| {
                                if *hovered {
                                    this.active = ix;
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.active = ix;
                                this.pick(cx);
                            }))
                    })),
            )
    }
}

pub(super) struct Open {
    session: i64,
    picker: Entity<Picker>,
    anchor: AnchorSlot,
    _events: Subscription,
}
impl Shell {
    pub(super) fn observe_session_folders(
        &mut self,
        view: &Entity<Conversation>,
        id: i64,
        cx: &mut Context<Self>,
    ) {
        self.observations.push(cx.subscribe(
            view,
            move |this, _, event: &crate::conversation::ChooseFolder, cx| {
                this.pending_folder = Some((id, event.0.clone()));
                cx.notify();
            },
        ));
    }
    fn open_folder_picker(
        &mut self,
        id: i64,
        anchor: AnchorSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project = self.sessions[&id].read(cx).summary.cwd.clone();
        let folders = self
            .groups
            .get(&project)
            .cloned()
            .unwrap_or_default()
            .folders;
        let picker = cx.new(|cx| Picker::new(folders, cx));
        let events = cx.subscribe_in(
            &picker,
            window,
            move |this, _, target: &Target, window, cx| {
                if !matches!(target, Target::Dismiss) && this.sessions.contains_key(&id) {
                    let groups = this.groups.entry(project.clone()).or_default();
                    match target {
                        Target::Existing(folder) => groups.add_to_folder(folder, id),
                        Target::New(name) => {
                            let folder = monocode_core::mono::new_id();
                            groups.create_folder(&[id], folder.clone());
                            groups.rename_folder(&folder, name);
                        }
                        Target::Dismiss => {}
                    }
                    let groups = groups.clone();
                    let project = project.clone();
                    this.store_write("save the session folder", cx, move |store| {
                        store.save_groups(&project, &groups)
                    });
                    this.sessions[&id].update(cx, |session, cx| session.folder_chosen(cx));
                    this.invalidate_chrome(cx);
                }
                this.close_folder_picker(window, cx);
            },
        );
        window.focus(&picker.read(cx).input.focus_handle(cx), cx);
        self.folder_picker = Some(Open {
            session: id,
            picker,
            anchor,
            _events: events,
        });
        cx.notify();
    }
    pub(super) fn close_folder_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.folder_picker.take() {
            if self.workspace.active == Some(open.session)
                && let Some(session) = self.sessions.get(&open.session)
            {
                window.focus(&session.read(cx).input.focus_handle(cx), cx);
            }
            cx.notify();
        }
    }
    pub(super) fn folder_picker_popover(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some((id, anchor)) = self.pending_folder.take()
            && self.workspace.active == Some(id)
            && self.sessions.contains_key(&id)
        {
            self.open_folder_picker(id, anchor, window, cx);
        }
        let open = self.folder_picker.as_ref()?;
        if self.workspace.active != Some(open.session)
            || self.settings.is_some()
            || self.app_view_open()
        {
            self.folder_picker = None;
            return None;
        }
        let width = open.anchor.get().map_or(400., |b| b.size.width.as_f32());
        let weak = cx.weak_entity();
        Some(
            popover("session-folder-popover", Anchor::Slot(open.anchor.clone()))
                .side(Side::Top)
                .gap(4.)
                .width(width)
                .priority(50)
                .on_dismiss(move |window, cx| {
                    let _ = weak.update(cx, |this, cx| this.close_folder_picker(window, cx));
                })
                .child(open.picker.clone())
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn folder_command_searches_creates_and_moves_without_sending_or_losing_a_draft(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            super::super::bind_keys(cx);
            crate::input::bind(cx);
        });
        let (window, _, id) = super::super::tests::window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&id].update(cx, |view, cx| {
                    view.input
                        .update(cx, |i, cx| i.set_text("/add-to-folder ".into(), cx));
                });
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(visual.debug_bounds("session-folder-picker").is_some());
        visual.simulate_input("Migration");
        visual.run_until_parked();
        visual.simulate_keystrokes("enter");
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert!(shell.folder_picker.is_none());
                let project = shell.workspace.project.clone();
                let groups = &shell.groups[&project];
                assert_eq!(groups.folder_of(id).unwrap().name, "Migration");
                assert_eq!(
                    locked(&shell.store)
                        .unwrap()
                        .groups(&project)
                        .unwrap()
                        .folder_of(id)
                        .unwrap()
                        .name,
                    "Migration"
                );
                let session = shell.sessions[&id].read(cx);
                assert!(!session.busy);
                assert_eq!(session.input.read(cx).content.as_ref(), "/add-to-folder ");
                assert!(session.input.focus_handle(cx).is_focused(w));
                assert!(
                    locked(&shell.store)
                        .unwrap()
                        .load(id)
                        .unwrap()
                        .rows
                        .is_empty()
                );
                shell.sessions[&id].update(cx, |view, cx| {
                    view.input.update(cx, |i, cx| {
                        i.set_text("/add-to-folder Preserve this prompt".into(), cx)
                    });
                    view.request_folder(cx);
                });
            })
            .unwrap();
        visual.run_until_parked();
        visual.simulate_input("migration");
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                let picker = shell.folder_picker.as_ref().unwrap().picker.read(cx);
                assert_eq!(
                    picker.rows(cx).len(),
                    1,
                    "an exact name cannot create a duplicate folder"
                );
            })
            .unwrap();
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert!(shell.folder_picker.is_none());
                let session = shell.sessions[&id].read(cx);
                assert_eq!(
                    session.input.read(cx).content.as_ref(),
                    "/add-to-folder Preserve this prompt"
                );
                assert!(!session.busy);
                assert!(session.input.focus_handle(cx).is_focused(w));
            })
            .unwrap();
    }
}
