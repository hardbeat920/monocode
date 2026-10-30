use super::*;
impl Shell {
    pub(super) fn configure_quick_composer(&mut self, cx: &mut Context<Self>) {
        if let Some(receiver) = self.quick.configure(&self.workspace.preferences) {
            self.quick.task = Some(cx.spawn(async move |this, cx| {
                while receiver.recv().await.is_ok() {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.toggle_quick_composer(window, cx)
                    });
                }
            }));
        }
    }
    fn toggle_quick_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.quick.window
            && handle
                .update(cx, |view, window, cx| {
                    let shown = view.shown();
                    if shown {
                        view.dismiss(window, cx);
                    }
                    shown
                })
                .unwrap_or(false)
        {
            return;
        }
        self.show_quick_composer(window, cx);
    }
    /// The project and model a fresh show starts from: the last launch while
    /// its project is still open, else the current project's defaults.
    fn quick_choice(&self, cx: &App) -> crate::quick_composer::Launch {
        let cwd = self.workspace.project.clone();
        let kind = self.default_kind();
        let p = &self.workspace.preferences;
        let initial = crate::quick_composer::Launch::choice(
            cwd.clone(),
            kind,
            p.model_for_new_chat(kind.id(), &cwd),
            crate::conversation::default_access(cx),
        );
        let mut choice = self
            .quick
            .last
            .clone()
            .filter(|v| self.workspace.projects.contains(&v.cwd))
            .unwrap_or(initial);
        choice.access = crate::conversation::default_access(cx);
        choice
    }
    /// The menu bar's Quick Composer: brings the composer forward rather
    /// than dismissing it, and never raises the workspace window.
    pub(super) fn show_quick_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let choice = self.quick_choice(cx);
        let projects = self.workspace.projects.clone();
        let appearance = self.workspace.project_appearance.clone();
        if let Some(handle) = self.quick.window
            && handle
                .update(cx, |view, window, cx| {
                    view.reopen(projects.clone(), appearance.clone(), choice.clone(), cx);
                    view.focus(window, cx);
                })
                .is_ok()
        {
            return;
        }
        let display = cx.primary_display();
        let size = gpui::size(
            px(crate::quick_composer::WIDTH),
            px(crate::quick_composer::INITIAL_HEIGHT),
        );
        // Centered, a little down from the top of the work area, like Spotlight.
        let bounds = match &display {
            Some(display) => {
                let area = display.visible_bounds();
                gpui::Bounds {
                    origin: gpui::point(
                        area.origin.x + (area.size.width - size.width) / 2.,
                        area.origin.y + area.size.height * crate::quick_composer::TOP_FRACTION,
                    ),
                    size,
                }
            }
            None => gpui::Bounds::centered(None, size, cx),
        };
        let handle = cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                display_id: display.map(|d| d.id()),
                kind: gpui::WindowKind::PopUp,
                titlebar: None,
                is_resizable: false,
                is_minimizable: false,
                window_background: crate::theme::window_backdrop(true),
                app_id: Some("dev.usemono.quick-composer".into()),
                ..Default::default()
            },
            move |window, cx| {
                #[cfg(all(target_os = "macos", not(test)))]
                crate::traffic_lights::floating_panel(window, false);
                let view = cx.new(|cx| {
                    crate::quick_composer::QuickComposer::new(projects, appearance, choice, cx)
                });
                view.update(cx, |v, cx| v.focus(window, cx));
                view
            },
        );
        match handle {
            Ok(handle) => {
                let view = handle.update(cx, |_, _, cx| cx.entity()).unwrap();
                self.quick.events = Some(cx.subscribe_in(
                    &view,
                    window,
                    move |this, _, launch: &crate::quick_composer::Launch, window, cx| {
                        this.quick.last = Some(crate::quick_composer::Launch {
                            prompt: String::new(),
                            attachments: Vec::new(),
                            staged_attachments: Vec::new(),
                            ..launch.clone()
                        });
                        let _ = handle.update(cx, |view, window, cx| view.dismiss(window, cx));
                        this.launch_quick_composer(launch.clone(), window, cx);
                    },
                ));
                self.quick.window = Some(handle);
            }
            Err(e) => self.quick.error = Some(e.to_string()),
        }
    }
    fn launch_quick_composer(
        &mut self,
        launch: crate::quick_composer::Launch,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = self.store.clone();
        let mut choice = launch.clone();
        let attachments = std::mem::take(&mut choice.attachments);
        crate::conversation::remember_access(choice.access, cx);
        crate::pickers::remember_model(choice.kind, choice.model.clone(), cx);
        let account = crate::accounts::get(cx)
            .map(|a| a.read(cx).selected(launch.kind, &launch.cwd))
            .unwrap_or_default();
        let work = cx.background_executor().spawn(async move {
            let store = locked(&store)?;
            let summary = store.create(choice.kind.id(), &choice.cwd)?;
            let settings = monocode_core::agent::Settings {
                account,
                model: choice.model,
                access: choice.access,
                effort: choice.effort,
                fast: choice.fast,
                new_worktree: choice.new_worktree,
                worktree_base: choice.worktree_base,
                worktree_cwd: choice.worktree_cwd,
                ..Default::default()
            };
            store.save_agent_settings(summary.id, &settings)?;
            let attachments = store.adopt_attachments(
                summary.id,
                &crate::quick_composer::paste_staging_dir(),
                attachments,
                &choice.staged_attachments,
            )?;
            Ok::<_, String>((summary, settings, attachments))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok((summary, settings, attachments)) => {
                    let id = summary.id;
                    if !this.workspace.projects.contains(&launch.cwd) {
                        this.workspace.projects.push(launch.cwd.clone());
                    }
                    this.add(summary, cx);
                    this.sessions[&id].update(cx, |v, cx| {
                        v.quick_attach(attachments, cx);
                        v.quick_submit(launch.prompt, settings, cx)
                    });
                    this.order.retain(|i| *i != id);
                    this.order.insert(0, id);
                    if launch.foreground {
                        this.workspace.project = launch.cwd;
                        this.close_settings(window, cx);
                        this.activate_session(id, window, cx);
                        window.activate_window();
                        cx.activate(true);
                    }
                    this.invalidate_chrome(cx);
                    this.persist(cx);
                    cx.notify();
                }
                Err(e) => {
                    this.error = Some(e);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
