//! Project workspace selection, independent of the project's identity. Existing
//! conversations keep their execution directory and running provider process.
use super::*;
use crate::worktree_switcher::{Event, TabSession, WorktreeFocus, WorktreeSwitcher};
use monocode_core::files::path_key;

impl Shell {
    pub(super) fn working_copy(&self) -> &str {
        self.worktree_focus
            .get(&self.workspace.project)
            .map_or(&self.workspace.project, |focus| &focus.path)
    }
    pub(super) fn session_working_copy<'a>(&'a self, id: i64, cx: &'a App) -> &'a str {
        let id = self.workspace.session_for_pane(id);
        let session = self.session_metadata(id, cx);
        self.session_worktrees
            .get(&id)
            .and_then(|p| p.as_deref())
            .or_else(|| {
                self.sessions
                    .get(&id)
                    .and_then(|v| v.read(cx).settings.worktree_cwd.as_deref())
            })
            .unwrap_or(&session.summary.cwd)
    }
    pub(super) fn in_worktree_scope(&self, id: i64, cx: &App) -> bool {
        let scope = self
            .workspace
            .tab_of(id)
            .and_then(|tab| self.workspace.tab_workspaces.get(&tab))
            .map_or_else(|| self.session_working_copy(id, cx), String::as_str);
        path_key(scope) == path_key(self.working_copy())
    }
    pub(super) fn in_sidebar_worktree_scope(&self, id: i64, cx: &App) -> bool {
        self.worktree_focus
            .get(&self.workspace.project)
            .is_none_or(|focus| {
                path_key(self.session_working_copy(id, cx)) == path_key(&focus.path)
            })
    }
    pub(super) fn record_worktree_session(&mut self, id: i64, _cx: &App) {
        let project = self.workspace.project.clone();
        let path = self.working_copy().to_owned();
        // Opening a saved chat must not silently narrow the sidebar. Only an
        // explicit workspace selection changes that project-wide filter.
        if let Some(tab) = self.workspace.tab_of(id) {
            self.workspace.tab_workspaces.insert(tab, path.clone());
        }
        self.worktree_returns.insert((project, path), id);
    }
    pub(super) fn worktree_flyout(
        &self,
        events: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Flyout {
        let project = self.workspace.project.clone();
        let ids = self
            .order
            .iter()
            .copied()
            .filter(|id| self.session_metadata(*id, cx).summary.cwd == project)
            .collect();
        let sessions = self
            .workspace
            .tabs
            .iter()
            .flat_map(|tab| {
                self.workspace
                    .layout(*tab)
                    .leaf_ids()
                    .into_iter()
                    .filter_map(|id| {
                        if !self.session_summaries.contains_key(&id) {
                            return None;
                        }
                        let session = self.session_metadata(id, cx);
                        (session.summary.cwd == project).then(|| TabSession {
                            id,
                            tab: *tab,
                            cwd: self.session_working_copy(id, cx).to_owned(),
                            workspace: self.workspace.tab_workspaces.get(tab).cloned(),
                            busy: session.busy,
                        })
                    })
            })
            .collect();
        let focus = self.worktree_focus.get(&project).cloned();
        let view = cx.new(|cx| {
            WorktreeSwitcher::new(
                project.clone(),
                focus,
                sessions,
                ids,
                self.store.clone(),
                cx,
            )
        });
        events.push(
            cx.subscribe_in(&view, window, move |this, _, event: &Event, window, cx| {
                match event {
                    Event::Metadata(paths) => {
                        for (id, path) in paths {
                            this.session_worktrees.insert(*id, path.clone());
                            if let Some(session) = this.sessions.get(id) {
                                let path = session.update(cx, |session, cx| {
                                    session.restore_worktree(path.clone(), None, cx)
                                });
                                this.session_worktrees.insert(*id, path);
                            }
                        }
                        this.sync_git(cx);
                        this.invalidate_chrome(cx);
                    }
                    Event::CopySelected(copy) => this.select_copy(copy.clone(), cx),
                    Event::Selected(focus) if this.workspace.project == project => {
                        this.close_menu(cx);
                        this.select_worktree(focus.clone(), window, cx);
                    }
                    Event::Selected(_) | Event::Dismissed => {
                        this.close_menu(cx);
                        this.focus_workspace(window, cx);
                    }
                }
                cx.notify();
            }),
        );
        Flyout {
            focus: Some(view.read(cx).focus(cx)),
            view: view.into(),
            width: 280.,
            padding: 0.,
            height: None,
            max_height: Some(360.),
            surface: None,
        }
    }
    pub(super) fn select_copy(
        &mut self,
        copy: monocode_core::isolation::Workspace,
        cx: &mut Context<Self>,
    ) {
        let project = self.workspace.project.clone();
        let revision = self.navigation_revision;
        let store = self.store.clone();
        let owner = copy.session_id.clone();
        let expected_copy = copy.clone();
        let work = cx.background_executor().spawn(async move {
            let (id, isolation, mut settings, cwd) = {
                let store = locked(&store)?;
                let id = store
                    .resolve_existing_session_id(&owner)
                    .ok_or("The copy's owning session is unavailable")?;
                let summary = store
                    .sessions()?
                    .into_iter()
                    .find(|summary| summary.id == id)
                    .ok_or("Session unavailable")?;
                (
                    id,
                    monocode_core::isolation::Isolation::for_store(&store),
                    store.agent_settings(id)?.unwrap_or_default(),
                    summary.cwd,
                )
            };
            isolation.authorize(std::path::Path::new(&cwd), &owner, &mut settings)?;
            if settings.cow_id.as_ref() != Some(&expected_copy.id)
                || !settings.worktree_cwd.as_deref().is_some_and(|path| {
                    monocode_core::files::is_equal_or_inside(path, &expected_copy.path)
                })
            {
                return Err("The session no longer owns this copy".into());
            }
            let reattached = settings.worktree_removed;
            if reattached {
                settings.worktree_removed = false;
                locked(&store)?.save_agent_settings(id, &settings)?;
            }
            let execution = settings.worktree_cwd.clone().ok_or("Copy unavailable")?;
            Ok::<_, String>((id, execution, reattached.then_some(settings)))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.workspace.project != project || this.navigation_revision != revision {
                    return;
                }
                this.close_menu(cx);
                this.classic_worktree_picker = None;
                match result {
                    Ok((id, execution, settings))
                        if this
                            .session_summaries
                            .get(&id)
                            .is_some_and(|summary| summary.cwd == project) =>
                    {
                        this.worktree_focus.insert(
                            project,
                            WorktreeFocus {
                                path: execution,
                                branch: copy.branch,
                            },
                        );
                        if let Some(settings) = settings
                            && let Some(view) = this.sessions.get(&id)
                        {
                            view.update(cx, |view, cx| view.install_transfer(settings, None, cx));
                        }
                        this.open(id, window, cx);
                    }
                    Ok(_) => {
                        this.error =
                            Some("The copy's owning session is unavailable in this project".into())
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn select_worktree(
        &mut self,
        focus: Option<WorktreeFocus>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project = self.workspace.project.clone();
        if let Some(id) = self.workspace.active {
            self.worktree_returns
                .insert((project.clone(), self.working_copy().into()), id);
        }
        let target = focus.as_ref().map_or(&project, |f| &f.path).clone();
        match focus {
            Some(focus) => {
                self.worktree_focus.insert(project.clone(), focus);
            }
            None => {
                self.worktree_focus.remove(&project);
            }
        }
        self.navigation_revision += 1;
        self.leave_app_views(cx);
        self.close_preview(cx);
        if let Some(monos) = self.monos.clone() {
            monos.update(cx, |m, cx| m.close(cx));
        }
        if self.restore_scope_tab(window, cx) {
            return;
        }
        let matches = |id: &i64| {
            self.session_summaries
                .contains_key(&self.workspace.session_for_pane(*id))
                && {
                    let s = self.session_metadata(self.workspace.session_for_pane(*id), cx);
                    s.summary.cwd == project && !s.summary.archived
                }
                && self.in_worktree_scope(*id, cx)
                && self.workspace.tab_of(*id).is_some()
        };
        let active = self
            .worktree_returns
            .get(&(project.clone(), target.clone()))
            .copied()
            .filter(matches)
            .or_else(|| self.workspace.active.filter(matches))
            .or_else(|| {
                self.workspace
                    .tabs
                    .iter()
                    .rev()
                    .flat_map(|tab| self.workspace.layout(*tab).leaf_ids())
                    .find(matches)
            });
        self.workspace.active = active;
        if let Some(id) = active {
            self.activate_session(id, window, cx);
        } else {
            self.ensure_project_session(window, cx);
        }
        self.focus_active(window, cx);
        self.persist(cx);
        cx.notify();
    }
    pub(super) fn seed_worktree(
        &mut self,
        id: i64,
        focus: Option<&WorktreeFocus>,
        cx: &mut Context<Self>,
    ) {
        let path = focus.map(|f| f.path.clone());
        self.session_worktrees.insert(id, path.clone());
        if let Some(session) = self.sessions.get(&id) {
            session.update(cx, |session, cx| {
                if !session.settings.new_cow {
                    session.restore_worktree(path, focus.and_then(|f| f.branch.clone()), cx);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::tests::window;
    use gpui::TestAppContext;
    use monocode_core::store::Store;

    #[gpui::test]
    fn header_settings_and_worktree_actions_open_focused_native_surfaces(cx: &mut TestAppContext) {
        cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            crate::shell::bind_keys(cx);
        });
        let (window, _, active) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.open_menu(
                    MenuTarget::Header,
                    Anchor::Slot(shell.project_anchor.clone()),
                    w,
                    cx,
                );
            })
            .unwrap();
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let settings = visual.debug_bounds("menu-action-settings").unwrap();
        visual.simulate_click(settings.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                let settings = shell.settings.as_ref().expect("Settings is enabled");
                assert!(settings.focus_handle(cx).contains_focused(w, cx));
                assert!(shell.menu.is_none());
                assert_eq!(shell.workspace.active, Some(active));
            })
            .unwrap();
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert!(shell.settings.is_none());
                shell.open_menu(
                    MenuTarget::Header,
                    Anchor::Slot(shell.project_anchor.clone()),
                    w,
                    cx,
                );
            })
            .unwrap();
        visual.run_until_parked();
        let row = visual.debug_bounds("menu-action-switch-worktree").unwrap();
        visual.simulate_click(row.center(), Default::default());
        visual.run_until_parked();
        let panel = visual
            .debug_bounds("worktree-switcher")
            .expect("worktree flyout opens");
        assert!((panel.size.width - px(278.)).abs() < px(1.));
        assert_eq!(
            visual
                .debug_bounds("worktree-switcher-search")
                .unwrap()
                .size
                .height,
            px(44.)
        );
        visual.simulate_input("a-copy");
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert!(shell.menu.is_some());
                assert!(
                    shell.sessions[&active]
                        .read(cx)
                        .input
                        .read(cx)
                        .content
                        .is_empty(),
                    "typing belongs to the flyout"
                );
            })
            .unwrap();
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        window
            .update(visual, |shell, _, _| assert!(shell.menu.is_none()))
            .unwrap();
    }

    #[gpui::test]
    fn project_folder_selection_keeps_subfolder_sessions_and_new_session_cwd(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            crate::shell::bind_keys(cx);
        });
        let project = "/work/monocode";
        crate::git_cache::remember(
            project,
            &monocode_core::git::Snapshot {
                worktrees: vec![monocode_core::git::Worktree {
                    path: "/work".into(),
                    branch: Some("main".into()),
                    main: true,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let (window, _, existing) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.workspace.preferences.default_isolation = "cow".into();
                shell.sessions[&existing]
                    .update(cx, |session, _| session.summary.has_messages = true);
                shell.open_menu(
                    MenuTarget::Header,
                    Anchor::Slot(shell.project_anchor.clone()),
                    w,
                    cx,
                );
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let row = visual.debug_bounds("menu-action-switch-worktree").unwrap();
        visual.simulate_click(row.center(), Default::default());
        visual.run_until_parked();
        let row = visual.debug_bounds("working-copy-1").unwrap();
        visual.simulate_click(row.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert!(shell.sidebar_session_ids(cx).contains(&existing));
                shell.create(Kind::Claude, cx);
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                let id = shell.workspace.active.unwrap();
                assert_ne!(id, existing);
                let session = shell.sessions[&id].read(cx);
                assert_eq!(session.summary.cwd, project);
                assert!(!session.settings.pending_isolation());
                assert_eq!(
                    session
                        .settings
                        .worktree_cwd
                        .as_deref()
                        .unwrap_or(&session.summary.cwd),
                    project
                );
            })
            .unwrap();
    }

    #[cfg(target_os = "macos")]
    #[gpui::test]
    fn new_tabs_fork_the_active_copy_under_all_checkouts(cx: &mut TestAppContext) {
        let root = std::env::temp_dir().join(format!(
            "monocode-cow-tabs-{}",
            monocode_core::mono::new_id()
        ));
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-m",
                "Base",
            ],
        ] {
            let result = std::process::Command::new("git")
                .current_dir(&project)
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        let store = Store::open_monocode(&root.join("app/monocode.db")).unwrap();
        let summary = store.create("codex", project.to_str().unwrap()).unwrap();
        let isolation = monocode_core::isolation::Isolation::for_store(&store);
        if !isolation.supported(&project) {
            assert!(
                std::env::var_os("MONOCODE_REQUIRE_COW").is_none(),
                "APFS required"
            );
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
        let mut settings = monocode_core::agent::Settings::default();
        settings.isolation_default("cow");
        let copy = isolation
            .create(
                &project,
                &store.persisted_session_id(summary.id).unwrap(),
                &mut settings,
            )
            .unwrap();
        store.save_agent_settings(summary.id, &settings).unwrap();
        let original = summary.id;
        let store = Arc::new(Mutex::new(store));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    Shell::new(
                        store,
                        vec![summary],
                        project.to_string_lossy().into_owned(),
                        None,
                        cx,
                    )
                })
            })
            .unwrap()
        });
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                shell.open(original, w, cx);
                assert!(shell.worktree_focus.is_empty());
                shell
                    .session_worktrees
                    .insert(original, Some(copy.to_string_lossy().into_owned()));
                shell.sessions[&original].update(cx, |session, _| session.settings.cow_id = None);
                shell.workspace.preferences.default_isolation = "local".into();
                shell.create(Kind::Codex, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                let active = shell.workspace.active.unwrap();
                assert_ne!(active, original);
                let child = shell.sessions[&active].read(cx);
                assert!(child.settings.new_cow);
                assert!(child.settings.cow_id.is_none() && child.settings.worktree_cwd.is_none());
                assert_eq!(child.settings.cow_source_cwd.as_deref(), copy.to_str());
            })
            .unwrap();
        let metadata = isolation.owner(&project, &copy).unwrap().unwrap();
        window
            .update(cx, |shell, w, cx| {
                let (_, detached) = locked(&shell.store)
                    .unwrap()
                    .detach_worktree(copy.to_str().unwrap(), project.to_str().unwrap())
                    .unwrap()
                    .remove(0);
                shell.sessions[&original].update(cx, |session, cx| {
                    session.install_transfer(detached, None, cx)
                });
                shell.open(original, w, cx);
                shell.select_copy(metadata, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                let session = shell.sessions[&original].read(cx);
                assert!(!session.settings.worktree_removed);
                assert!(session.settings.cow_id.is_some());
                assert_eq!(shell.working_copy(), copy.to_str().unwrap());
            })
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn copies_keep_running_sessions_and_route_new_tabs_and_splits(cx: &mut TestAppContext) {
        let (window, feature_session, main_session) = window(cx);
        window
            .update(cx, |shell, w, cx| shell.open(feature_session, w, cx))
            .unwrap();
        cx.run_until_parked();
        let project = "/work/monocode";
        let feature = WorktreeFocus {
            path: "/work/monocode-worktrees/feature".into(),
            branch: Some("feature".into()),
        };
        let other = WorktreeFocus {
            path: "/work/monocode-worktrees/other".into(),
            branch: Some("other".into()),
        };
        window
            .update(cx, |shell, w, cx| {
                shell.sessions[&feature_session].update(cx, |session, cx| {
                    session.summary.cwd = project.into();
                    session.settings.worktree_cwd = Some(feature.path.clone());
                    session.busy = true;
                    session
                        .input
                        .update(cx, |input, cx| input.set_text("keep this draft".into(), cx));
                });
                shell
                    .session_worktrees
                    .insert(feature_session, Some(feature.path.clone()));
                shell
                    .workspace
                    .tab_workspaces
                    .insert(feature_session, feature.path.clone());
                shell.sessions[&main_session]
                    .update(cx, |session, _| session.summary.has_messages = true);
                shell.open(main_session, w, cx);
                shell
                    .groups
                    .entry(project.into())
                    .or_default()
                    .create_folder(&[main_session], "saved-folder".into());
                shell.select_worktree(Some(feature.clone()), w, cx);
                assert_eq!(shell.workspace.project, project);
                assert_eq!(shell.workspace.active, Some(feature_session));
                assert_eq!(shell.sidebar_session_ids(cx), [feature_session]);
                assert_eq!(shell.terminal_cwd(cx), feature.path);
                shell.select_worktree(None, w, cx);
                assert_eq!(shell.workspace.active, Some(main_session));
                assert_eq!(
                    shell
                        .sidebar_session_ids(cx)
                        .into_iter()
                        .collect::<HashSet<_>>(),
                    HashSet::from([main_session, feature_session]),
                    "the default workspace lists the whole project"
                );
                assert_eq!(shell.terminal_cwd(cx), project);
                shell.open(feature_session, w, cx);
                assert_eq!(shell.working_copy(), project);
                assert!(shell.worktree_focus.is_empty());
                assert!(
                    shell
                        .visible_tabs(cx)
                        .contains(&TitleTab::Session(feature_session)),
                    "opening a worktree chat keeps its tab in the current workspace"
                );
                assert!(shell
                    .session_entries(&shell.sidebar_session_ids(cx), cx)
                    .iter()
                    .any(|entry| matches!(entry, monocode_core::session_groups::Entry::Folder { id, .. } if id == "saved-folder")),
                    "opening a worktree chat must not hide the project's other folders");
                shell.open(main_session, w, cx);
                // A late blank from the other copy cannot take over after returning.
                shell.select_worktree(Some(other.clone()), w, cx);
                shell.select_worktree(None, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(shell.workspace.active, Some(main_session));
                assert_eq!(shell.sessions.len(), 3);
                shell.select_worktree(Some(other.clone()), w, cx);
                let id = shell
                    .workspace
                    .active
                    .expect("reuse the completed blank for this copy");
                assert_eq!(shell.sessions.len(), 3);
                assert_eq!(shell.sessions[&id].read(cx).summary.cwd, project);
                assert_eq!(
                    shell.sessions[&id]
                        .read(cx)
                        .settings
                        .worktree_cwd
                        .as_deref(),
                    Some(other.path.as_str())
                );
                assert_eq!(
                    locked(&shell.store)
                        .unwrap()
                        .agent_settings(id)
                        .unwrap()
                        .unwrap()
                        .worktree_cwd
                        .as_deref(),
                    Some(other.path.as_str())
                );
                shell.create(Kind::Claude, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(shell.sessions.len(), 4);
                assert_eq!(shell.terminal_cwd(cx), other.path);
                shell.split_session(SplitDir::Right, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(shell.sessions.len(), 5);
                assert_eq!(shell.workspace.project, project);
                assert_eq!(shell.terminal_cwd(cx), other.path);
                let running = shell.sessions[&feature_session].read(cx);
                assert!(running.busy);
                assert_eq!(running.input.read(cx).content.as_ref(), "keep this draft");
                assert!(shell.worktree_focus.contains_key(project));
                // A directly opened chat joins the selected workspace without
                // changing either its checkout or the explicit sidebar filter.
                shell.open(main_session, w, cx);
                assert_eq!(shell.worktree_focus[project].path, other.path);
                assert!(
                    shell
                        .visible_tabs(cx)
                        .contains(&TitleTab::Session(main_session))
                );
                assert_eq!(shell.terminal_cwd(cx), project);
                assert_eq!(
                    shell.worktree_returns[&(project.to_owned(), other.path.clone())],
                    main_session
                );
                shell.select_project("/work/another-project".into(), w, cx);
                shell.select_project(project.into(), w, cx);
                assert_eq!(shell.worktree_focus[project].path, other.path);
                assert_eq!(shell.workspace.active, Some(main_session));
            })
            .unwrap();
    }
}
