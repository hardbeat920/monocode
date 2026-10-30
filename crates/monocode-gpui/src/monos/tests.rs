use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_core::store::Store;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, mpsc};
struct Setup {
    window: WindowHandle<Shell>,
    manager: Entity<Monos>,
    id: String,
    project: String,
    session: i64,
    root: PathBuf,
    store: Database,
}

#[gpui::test]
fn missing_document_uses_error_cards_without_moving_content(cx: &mut TestAppContext) {
    let s = setup(cx);
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    visual.refresh().unwrap();
    let body = visual.debug_bounds("main-pane-frame").unwrap();
    s.manager.update(visual, |m, cx| {
        m.failed_reports.insert("failed-report".into());
        m.open_document("deleted-document", cx);
    });
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert_eq!(
        s.manager
            .read_with(visual, |m, _| m.error.clone())
            .as_deref(),
        Some("Document unavailable. It may have been deleted.")
    );
    let error = visual.debug_bounds("mono-error").unwrap();
    let viewport = s
        .window
        .update(visual, |_, w, _| w.viewport_size())
        .unwrap();
    assert_eq!(error.top(), px(12.));
    assert_eq!(error.right(), viewport.width - px(12.));
    assert_eq!(error.size.width, px(360.));
    assert_eq!(visual.debug_bounds("main-pane-frame").unwrap(), body);

    let close = visual.debug_bounds("mono-error-close").unwrap();
    visual.simulate_click(close.center(), Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("mono-error").is_none());
    s.manager.read_with(visual, |m, _| {
        assert!(m.error.is_none());
        assert!(m.failed_reports.is_empty());
        assert!(
            m.settings.is_some(),
            "dismissing an error keeps settings open"
        );
        assert_eq!(m.active.as_deref(), Some(s.id.as_str()));
    });
}

#[gpui::test]
fn saved_desktop_mono_opens_in_actual_manager_without_replacing_its_conversation(
    cx: &mut TestAppContext,
) {
    let db = Store::open_monocode(std::path::Path::new(":memory:")).unwrap();
    let session = db.create("codex", "/saved-project").unwrap();
    let original = db.persisted_session_id(session.id).unwrap();
    let settings = Settings {
        model: Some("gpt-6.1-sol".into()),
        access: AccessMode::Auto,
        ..Default::default()
    };
    db.configure_agent(session.id, "codex", &settings).unwrap();
    let mut roster = Roster::default();
    roster.monos.push(Mono {
        id: "desktop-original-mono".into(),
        session_id: Some(session.id),
        name: "Big Boss".into(),
        mascot: "ghost".into(),
        color: 0xe25a9e,
        projects: vec!["/saved-project".into()],
        ..Default::default()
    });
    roster.save(&db).unwrap();
    let summaries = db.sessions().unwrap();
    let store = Arc::new(Mutex::new(db));
    let database = store.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Shell::new(database, summaries, "/saved-project".into(), None, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    let manager = window
        .update(cx, |shell, _, _| shell.mono_manager())
        .unwrap();
    window
        .update(cx, |_, window, cx| {
            manager.update(cx, |m, cx| m.open("desktop-original-mono", window, cx));
        })
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        manager.update(cx, |m, cx| {
            m.ensure_chat("desktop-original-mono", cx);
            let mono = m.current().unwrap();
            assert_eq!(mono.id, "desktop-original-mono");
            assert_eq!(mono.session_id, Some(session.id));
            assert_eq!(mono.name, "Big Boss");
            assert_eq!(mono.mascot, "ghost");
            let chat = m.chats[&mono.id].read(cx);
            assert_eq!(chat.summary.id, session.id);
            assert_eq!(chat.summary.provider, "codex");
            assert_eq!(chat.settings.model.as_deref(), Some("gpt-6.1-sol"));
            assert_eq!(m.chats.len(), 1);
        });
    });
    let db = locked(&store).unwrap();
    assert_eq!(db.sessions().unwrap().len(), 1);
    assert_eq!(db.persisted_session_id(session.id).unwrap(), original);
}

#[gpui::test]
fn settings_reset_restores_only_mono_soul_and_name(cx: &mut TestAppContext) {
    let s = setup(cx);
    mono::files::save(&s.root, &s.id, None, "SOUL.md", "# Custom soul", None).unwrap();
    mono::files::save(&s.root, &s.id, None, "MEMORY.md", "Keep this memory", None).unwrap();
    let before = cx.update(|cx| {
        s.manager.update(cx, |m, cx| {
            m.roster.get_mut(&s.id).unwrap().name = "Custom name".into();
            m.save(cx);
            m.roster.get(&s.id).unwrap().clone()
        })
    });
    cx.run_until_parked();
    let _reset = cx.update(|cx| s.manager.update(cx, |m, cx| m.reset_defaults(&s.id, cx)));
    cx.run_until_parked();
    let roster = Roster::load(&locked(&s.store).unwrap()).unwrap();
    let after = roster.get(&s.id).unwrap();
    assert!(after.name.is_empty());
    assert_eq!(after.session_id, before.session_id);
    assert_eq!(after.projects, before.projects);
    assert_eq!(after.habit_sessions, before.habit_sessions);
    let files = mono::files::load(&s.root, &s.id, None).unwrap();
    assert_eq!(files.soul.as_deref(), Some(mono::DEFAULT_SOUL));
    assert_eq!(files.memory, "Keep this memory");
}

#[gpui::test]
fn detail_pages_slide_over_the_overview_and_back_without_recreating_content(
    cx: &mut TestAppContext,
) {
    for kind in ["markdown", "html", "session"] {
        cx.update(|cx| cx.set_reduce_motion(true));
        let s = setup(cx);
        let session = s
            .window
            .update(cx, |shell, _, _| shell.workspace.active.unwrap())
            .unwrap();
        s.window
            .update(cx, |_, _, cx| {
                s.manager.update(cx, |m, cx| {
                    m.settings = None;
                    m.overview_tab = OverviewTab::Documents;
                    m.roster
                        .get_mut(&s.id)
                        .unwrap()
                        .documents
                        .push(mono::Document {
                            id: "slide-document".into(),
                            kind: kind.into(),
                            title: "Slide document".into(),
                            summary: String::new(),
                            body: "# Document\n\nKeep this reader alive.".into(),
                            created_at: 1,
                            updated_at: 1,
                        });
                    cx.notify();
                });
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
        visual.run_until_parked();
        visual.update(|_, cx| cx.set_reduce_motion(false));
        let action = if kind == "session" {
            json!({"type":"session", "sessionId":session})
        } else {
            json!({"type":"document", "documentId":"slide-document"})
        };
        s.window
            .update(visual, |_, _, cx| {
                s.manager
                    .update(cx, |m, cx| m.card_action(&s.id, &action, cx))
            })
            .unwrap();
        visual.run_until_parked();
        let page: &'static str = Box::leak(
            format!(
                "mono-detail-page-{}",
                if kind == "session" {
                    format!("session:{session}")
                } else {
                    "document:slide-document".into()
                }
            )
            .into_boxed_str(),
        );
        let advance = |visual: &mut gpui::VisualTestContext, ms| {
            let details = s.manager.read_with(visual, |m, _| m.details.clone());
            details.update(visual, |d, cx| d.elapsed(Duration::from_millis(ms), cx));
            visual
                .update_window(s.window.into(), |_, w, cx| {
                    w.simulate_next_frame(cx);
                    w.draw(cx).clear(cx);
                })
                .unwrap();
        };
        advance(visual, 70);
        let stack = visual.debug_bounds("mono-detail-stack").unwrap();
        let incoming = visual.debug_bounds(page).unwrap();
        assert!(incoming.left() > stack.left() && incoming.left() < stack.right());
        assert_eq!(
            incoming.size, stack.size,
            "sliding pages keep their full layout size"
        );
        assert!(visual.debug_bounds("mono-detail-base").unwrap().left() < stack.left());
        advance(visual, 1000);
        assert_eq!(visual.debug_bounds(page).unwrap().left(), stack.left());
        let reader = s.manager.read_with(visual, |m, cx| match kind {
            "html" => m.html_documents["slide-document"].entity_id(),
            "session" => m
                .owner
                .upgrade()
                .unwrap()
                .read(cx)
                .mono_session_entity(session)
                .unwrap()
                .entity_id(),
            _ => m.documents["slide-document"].entity_id(),
        });
        assert_eq!(
            s.manager
                .read_with(visual, |m, cx| m.native_detail_content(cx)),
            kind == "html"
        );
        let back = visual.debug_bounds("mono-page-back").unwrap().center();
        visual.simulate_click(back, Default::default());
        visual.run_until_parked();
        advance(visual, 70);
        let outgoing = visual
            .debug_bounds(page)
            .expect("Back keeps the outgoing page mounted");
        assert!(outgoing.left() > stack.left() && outgoing.left() < stack.right());
        advance(visual, 1000);
        assert!(visual.debug_bounds(page).is_none());
        assert_eq!(
            visual.debug_bounds("mono-detail-base").unwrap().left(),
            stack.left()
        );
        assert!(
            !s.manager
                .read_with(visual, |m, cx| m.native_detail_content(cx))
        );

        visual.update(|_, cx| cx.set_reduce_motion(true));
        s.window
            .update(visual, |_, _, cx| {
                s.manager
                    .update(cx, |m, cx| m.card_action(&s.id, &action, cx))
            })
            .unwrap();
        visual.run_until_parked();
        assert_eq!(visual.debug_bounds(page).unwrap().left(), stack.left());
        s.manager.read_with(visual, |m, cx| {
            let reopened = match kind {
                "html" => m.html_documents["slide-document"].entity_id(),
                "session" => m
                    .owner
                    .upgrade()
                    .unwrap()
                    .read(cx)
                    .mono_session_entity(session)
                    .unwrap()
                    .entity_id(),
                _ => m.documents["slide-document"].entity_id(),
            };
            assert_eq!(reader, reopened);
        });
        visual.update(|_, cx| cx.set_reduce_motion(false));
        s.window
            .update(visual, |_, w, cx| {
                s.manager.update(cx, |m, cx| {
                    m.close(cx);
                    m.open(&s.id, w, cx);
                });
            })
            .unwrap();
        visual.run_until_parked();
        assert!(
            visual.debug_bounds(page).is_none(),
            "returning to a Mono must not replay an old page's exit"
        );
        assert_eq!(
            visual.debug_bounds("mono-detail-base").unwrap().left(),
            stack.left()
        );
    }
}
fn setup(cx: &mut TestAppContext) -> Setup {
    let dir = std::env::temp_dir().join(format!("native-monos-{}", mono::new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let project = dir.to_string_lossy().to_string();
    let store = Arc::new(Mutex::new(Store::open(&dir.join("native.sqlite")).unwrap()));
    let db = store.clone();
    let cwd = project.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Shell::new(db, Vec::new(), cwd, None, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    let manager = window
        .update(cx, |shell, _, cx| {
            shell.workspace.right_panel_open = true;
            cx.notify();
            shell.mono_manager()
        })
        .unwrap();
    let handle: gpui::AnyWindowHandle = window.into();
    handle
        .update(cx, |_, w, cx| {
            manager.update(cx, |m, cx| m.create(vec![project.clone()], w, cx))
        })
        .unwrap();
    cx.run_until_parked();
    let (id, session, root) = cx.update(|cx| {
        let m = manager.read(cx);
        let mono = m.current().unwrap();
        (mono.id.clone(), mono.session_id.unwrap(), m.root.clone())
    });
    Setup {
        window,
        manager,
        id,
        project,
        session,
        root,
        store,
    }
}

#[gpui::test]
fn heartbeat_runs_keep_the_monos_selected_permissions(cx: &mut TestAppContext) {
    for mode in [
        AccessMode::ReadOnly,
        AccessMode::Supervised,
        AccessMode::AutoAcceptEdits,
        AccessMode::Auto,
        AccessMode::FullAccess,
    ] {
        let s = setup(cx);
        s.manager.update(cx, |m, cx| {
            m.chats[&s.id].update(cx, |chat, _| {
                // Avoid catalog probes or provider requests: exercise the actual
                // background runner, stopping before it dispatches a prompt.
                chat.settings.access = mode;
                chat.settings.usage_limit = Some(Default::default());
            });
            m.start_heartbeat(&s.id, "Check for follow-ups".into(), cx);
        });
        cx.run_until_parked();
        s.manager.read_with(cx, |m, cx| {
            assert!(m.error.is_none(), "heartbeat failed: {:?}", m.error);
            let run = &m.running[&m.active_run.expect("heartbeat session was created")];
            assert_eq!(run.view.read(cx).settings.access, mode);
            assert!(
                !run.view.read(cx).busy,
                "the fixture must not start a provider"
            );
        });
    }
}

fn call(
    s: &Setup,
    request: &str,
    action: &str,
    input: Value,
    cx: &mut TestAppContext,
) -> mpsc::Receiver<Result<Value, String>> {
    let (reply, rx) = mpsc::channel();
    let call = mono::bridge::Call {
        session: s.session,
        request_id: request.into(),
        action: action.into(),
        input,
        reply,
    };
    let handle: gpui::AnyWindowHandle = s.window.into();
    handle
        .update(cx, |_, w, cx| {
            s.manager.update(cx, |m, cx| m.app_call(call, w, cx))
        })
        .unwrap();
    cx.run_until_parked();
    rx
}
#[gpui::test]
fn settings_match_react_dialog_and_left_navigation_geometry(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let dialog = visual.debug_bounds("mono-settings-dialog").unwrap();
    let nav = visual.debug_bounds("mono-settings-navigation").unwrap();
    let viewport = s
        .window
        .update(visual, |_, w, _| w.viewport_size())
        .unwrap();
    assert_eq!(
        dialog.size.width + px(2.),
        px(880.).min(viewport.width - px(24.))
    );
    assert!((dialog.center().x - viewport.width / 2.).abs() <= px(1.));
    assert_eq!(nav.size.width, px(192.));
    for section in ["General", "Soul", "Memory"] {
        let row = visual
            .debug_bounds(Box::leak(
                format!("mono-settings-{section}").into_boxed_str(),
            ))
            .unwrap();
        let icon = visual
            .debug_bounds(Box::leak(
                format!("mono-settings-icon-{section}").into_boxed_str(),
            ))
            .unwrap();
        let label = visual
            .debug_bounds(Box::leak(
                format!("mono-settings-label-{section}").into_boxed_str(),
            ))
            .unwrap();
        assert_eq!(row.size.height, px(32.));
        assert_eq!(icon.left() - row.left(), px(10.));
        assert_eq!(label.left() - icon.right(), px(10.));
    }
    assert_eq!(
        visual.debug_bounds("mono-look").unwrap().size,
        gpui::size(px(80.), px(80.))
    );
    assert_eq!(
        visual.debug_bounds("mono-name-field").unwrap().size,
        gpui::size(px(256.), px(32.))
    );
}

#[gpui::test]
fn settings_color_popover_follows_its_mascot_when_the_window_resizes(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| {
        crate::shell::bind_keys(cx);
        crate::popover::bind(cx);
    });
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let face = visual.debug_bounds("mono-look").unwrap();
    visual.simulate_click(face.center(), Default::default());
    visual.run_until_parked();
    for size in [
        gpui::size(px(1300.), px(900.)),
        gpui::size(px(1050.), px(760.)),
    ] {
        visual.simulate_resize(size);
        visual.run_until_parked();
        let face = visual.debug_bounds("mono-look").unwrap();
        let picker = visual.debug_bounds("mono-settings-look-picker").unwrap();
        assert!((picker.center().x - face.center().x).abs() <= px(1.));
        assert!(picker.top() >= face.bottom());
    }
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    visual.update(|_, cx| {
        let m = s.manager.read(cx);
        assert!(m.picker.is_none());
        assert!(m.settings.is_some());
    });
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    visual.update(|_, cx| assert!(s.manager.read(cx).settings.is_none()));
}

#[gpui::test]
fn memory_editor_saves_with_enter_and_stays_open_for_the_next_fact(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(input::bind);
    s.window
        .update(cx, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = Some(SettingsSection::Memory);
                m.memory_adding = true;
                cx.notify();
            })
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    s.window
        .update(visual, |_, w, cx| {
            let field = s.manager.read(cx).fields["memory-fact"].clone();
            w.focus(&field.focus_handle(cx), cx);
        })
        .unwrap();
    visual.simulate_input("Keep UI aligned with React.");
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    assert!(
        mono::files::read(&s.root, &s.id, None, "MEMORY.md")
            .unwrap()
            .text
            .unwrap()
            .contains("Keep UI aligned with React.")
    );
    visual.update(|_, cx| {
        let m = s.manager.read(cx);
        assert!(m.memory_adding);
        assert!(m.fields["memory-fact"].read(cx).content.is_empty());
    });
}

#[gpui::test]
fn overview_activity_returns_to_its_previous_tab_and_uses_the_shared_reveal(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    s.window
        .update(cx, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.select_overview(OverviewTab::Documents, cx);
                assert!(m.overview_reveal.is_some());
                m.select_overview(OverviewTab::Activity, cx);
                assert!(m.overview_previous == OverviewTab::Documents);
                m.select_overview(m.overview_previous, cx);
                assert!(m.overview_tab == OverviewTab::Documents);
                cx.set_reduce_motion(true);
                assert!(
                    m.overview_reveal
                        .as_ref()
                        .unwrap()
                        .progress(OverviewTab::Documents, window, cx)
                        .is_none()
                );
            });
        })
        .unwrap();
}

#[gpui::test]
fn floating_chat_opens_after_releasing_the_manager_and_shares_the_runtime(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| {
        s.manager
            .update(cx, |m, cx| m.open_floating(s.id.clone(), cx))
    });
    cx.run_until_parked();
    cx.update(|cx| {
        s.manager
            .update(cx, |m, cx| m.open_floating(s.id.clone(), cx))
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let manager = s.manager.read(cx);
        assert_eq!(manager.floating.len(), 1);
        assert_eq!(manager.chats.len(), 1);
        assert!(manager.chats.contains_key(&s.id));
        // The panel owns the chat; the workspace draws a placeholder rather
        // than the same transcript in a second window.
        assert!(manager.floating_shows(&s.id, cx));
    });
    cx.update(|cx| {
        s.manager
            .update(cx, |m, cx| m.hide_floating(&s.id.clone(), cx))
    });
    cx.run_until_parked();
    cx.update(|cx| assert!(!s.manager.read(cx).floating_shows(&s.id, cx)));
}

#[gpui::test]
fn floating_chat_leaves_the_workspace_selection_alone(cx: &mut TestAppContext) {
    let s = setup(cx);
    let other = cx.update(|cx| {
        s.manager.update(cx, |m, cx| {
            let before = m.active.clone();
            let id = m.create_floating(Vec::new(), cx);
            assert_eq!(
                m.active, before,
                "a panel's new Mono stays out of the workspace"
            );
            m.open_floating(id.clone(), cx);
            assert_eq!(m.active, before);
            id
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let manager = s.manager.read(cx);
        assert!(manager.floating_shows(&other, cx));
        assert!(!manager.floating_shows(&s.id, cx));
    });
}

#[gpui::test]
fn note_actions_preserve_tags_when_updating_the_body(cx: &mut TestAppContext) {
    let s = setup(cx);
    let note = call(
        &s,
        "note-create",
        "notes.write",
        json!({"title":"QA note","body":"First","tags":["work","mono"]}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    let updated = call(
        &s,
        "note-update",
        "notes.write",
        json!({"id":note["id"],"title":"QA note","body":"Second"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    assert_eq!(updated["tags"], json!(["work", "mono"]));
    assert_eq!(updated["body"], "Second");
}
#[gpui::test]
fn identity_and_context_editors_survive_switches_and_save_to_their_own_files(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    cx.update(|cx| {
        s.manager.update(cx, |m, cx| {
            m.settings = Some(SettingsSection::Soul);
            let mono = m.current().unwrap().clone();
            let _ = m.soul_page(&mono, cx);
            let key = format!("file:{}:SOUL.md", s.id);
            m.fields[&key].update(cx, |f, cx| {
                f.set_text("# Soul\nKeep replies concise.\n".into(), cx)
            });
            m.save_open_file(cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(
        mono::files::read(&s.root, &s.id, None, "SOUL.md")
            .unwrap()
            .text
            .as_deref(),
        Some("# Soul\nKeep replies concise.\n")
    );
    let saved = Roster::load(&locked(&s.store).unwrap()).unwrap();
    assert_eq!(saved.monos.len(), 1);
    assert_eq!(saved.monos[0].session_id, Some(s.session));
    assert!(saved.hidden_sessions.contains(&s.session));
    let handle: gpui::AnyWindowHandle = s.window.into();
    handle
        .update(cx, |_, w, cx| {
            s.manager
                .update(cx, |m, cx| m.create(vec![s.project.clone()], w, cx))
        })
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        s.manager.update(cx, |m, cx| {
            let mono = m.current().unwrap().clone();
            assert_ne!(mono.id, s.id);
            assert_ne!(mono.mascot, m.roster.get(&s.id).unwrap().mascot);
            m.settings = Some(SettingsSection::Soul);
            let _ = m.soul_page(&mono, cx);
            let key = format!("file:{}:SOUL.md", mono.id);
            assert!(
                !m.fields[&key]
                    .read(cx)
                    .content
                    .contains("Keep replies concise")
            );
        })
    });
}
#[gpui::test]
fn app_receipt_lookup_does_not_wait_for_storage_on_the_ui_thread(cx: &mut TestAppContext) {
    let s = setup(cx);
    let store = s.store.clone();
    let (held, acquired) = mpsc::sync_channel(0);
    let (release, released) = mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _guard = store.lock().unwrap();
        held.send(()).unwrap();
        // A regression must fail rather than hang the whole test suite.
        released.recv_timeout(Duration::from_secs(2)).is_ok()
    });
    acquired.recv().unwrap();
    let (reply, response) = mpsc::channel();
    let call = mono::bridge::Call {
        session: s.session,
        request_id: "contended-lookup".into(),
        action: "sessions.list".into(),
        input: json!({}),
        reply,
    };
    s.window
        .update(cx, |_, window, cx| {
            s.manager
                .update(cx, |manager, cx| manager.app_call(call, window, cx));
        })
        .unwrap();
    assert!(response.try_recv().is_err());
    let _ = release.send(());
    assert!(
        holder.join().unwrap(),
        "app request blocked the UI waiting for the store"
    );
    cx.run_until_parked();
    assert!(response.try_recv().unwrap().is_ok());
}

#[gpui::test]
fn durable_app_receipts_survive_cache_eviction_and_invalid_receipts_do_not_repeat_mutations(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    let input = json!({"name":"Digest","instructions":"Summarize changes","schedule":{"kind":"daily","time":"09:00"}});
    let first = call(&s, "durable-habit", "habits.add", input.clone(), cx)
        .try_recv()
        .unwrap()
        .unwrap();
    s.manager.update(cx, |m, _| m.app_results.clear());
    let retry = call(&s, "durable-habit", "habits.add", input, cx)
        .try_recv()
        .unwrap()
        .unwrap();
    assert_eq!(first, retry);
    assert_eq!(habits::load(&s.root, &s.id).unwrap().len(), 1);
    assert!(
        call(
            &s,
            "durable-habit",
            "habits.add",
            json!({"name":"different"}),
            cx
        )
        .try_recv()
        .unwrap()
        .is_err()
    );
    let key = format!(
        "mono-app-receipt:{}:{}",
        s.session,
        mono::files::content_hash("corrupt-habit")
    );
    locked(&s.store)
        .unwrap()
        .save_setting(&key, "invalid receipt")
        .unwrap();
    let error = call(&s, "corrupt-habit", "habits.add", json!({"name":"Another","instructions":"Summarize changes","schedule":{"kind":"daily","time":"09:00"}}), cx).try_recv().unwrap().unwrap_err();
    assert!(error.contains("Could not read app result"));
    assert_eq!(habits::load(&s.root, &s.id).unwrap().len(), 1);
}

#[gpui::test]
fn app_actions_are_project_scoped_and_mutation_retries_are_idempotent(cx: &mut TestAppContext) {
    let s = setup(cx);
    assert!(
        call(
            &s,
            "outside",
            "sessions.list",
            json!({"project":"/not-assigned"}),
            cx
        )
        .try_recv()
        .unwrap()
        .is_err()
    );
    let input = json!({"name":"Morning digest","instructions":"Summarize changes without editing","schedule":{"kind":"daily","time":"09:00"}});
    let first = call(&s, "habit", "habits.add", input.clone(), cx)
        .try_recv()
        .unwrap()
        .unwrap();
    let retry = call(&s, "habit", "habits.add", input, cx)
        .try_recv()
        .unwrap()
        .unwrap();
    assert_eq!(first, retry);
    assert_eq!(habits::load(&s.root, &s.id).unwrap().len(), 1);
    assert!(
        call(&s, "habit", "habits.add", json!({"name":"different"}), cx)
            .try_recv()
            .unwrap()
            .is_err()
    );
    let start=call(&s,"draft","sessions.start",json!({"project":s.project,"prompt":"Review the project","title":"Review draft","draft":true}),cx).try_recv().unwrap().unwrap();
    let child = start["sessionId"].as_i64().unwrap();
    cx.update(|cx| {
        let m = s.manager.read(cx);
        assert_eq!(m.roster.get(&s.id).unwrap().launches.len(), 1);
        let owner = m.owner.upgrade().unwrap();
        let view = owner.read(cx).mono_session_entity(child).unwrap();
        assert!(view.read(cx).has_saved_draft());
        assert_eq!(
            locked(&s.store)
                .unwrap()
                .follow_ups(child)
                .unwrap()
                .draft
                .unwrap()
                .text,
            "Review the project"
        );
        assert!(!view.read(cx).busy);
        assert!(m.hidden_session(child));
    });
}
#[gpui::test]
fn mono_launches_ignore_tab_reveal_and_keep_the_current_chat(cx: &mut TestAppContext) {
    let s = setup(cx);
    let before = s
        .window
        .update(cx, |shell, _, _| shell.workspace.open_sessions())
        .unwrap();
    for show in [false, true] {
        s.manager
            .update(cx, |m, cx| m.sidebar_preferences(&s.id, show, false, cx));
        let started = call(
            &s,
            if show {
                "listed-reveal"
            } else {
                "hidden-reveal"
            },
            "sessions.start",
            json!({"project":s.project,"prompt":"Test quietly","draft":true,"reveal":true}),
            cx,
        )
        .try_recv()
        .unwrap()
        .unwrap();
        let child = started["sessionId"].as_i64().unwrap();
        s.window
            .update(cx, |shell, _, cx| {
                assert_eq!(
                    shell.workspace.open_sessions(),
                    before,
                    "a Mono launch must not create or select a tab"
                );
                assert!(
                    shell.mono_session_entity(child).is_some(),
                    "the background session still exists"
                );
                let m = s.manager.read(cx);
                assert_eq!(m.active.as_deref(), Some(s.id.as_str()));
                assert_eq!(m.hidden_session(child), !show);
            })
            .unwrap();
    }
}

#[gpui::test]
fn internal_completion_queue_stays_hidden_until_delivery_fails(cx: &mut TestAppContext) {
    let s = setup(cx);
    let handle: gpui::AnyWindowHandle = s.window.into();
    handle
        .update(cx, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.dismiss_settings(window, cx);
                m.roster.get_mut(&s.id).unwrap().outbox.push(Message {
                    id: "completion".into(),
                    text: "[Delegated work completed]".into(),
                    internal: true,
                    sent_at: None,
                    error: None,
                    attachments: Vec::new(),
                });
                // Keep the notification queued without starting a provider.
                m.configure_chat(&s.id, cx);
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds("mono-internal-queue").is_none(),
        "a routine completion must not flash a Pending card in the composer"
    );
    s.manager.update(visual, |m, cx| {
        m.roster.get_mut(&s.id).unwrap().outbox[0].error = Some("Delivery failed".into());
        m.configure_chat(&s.id, cx);
    });
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds("mono-internal-queue").is_some(),
        "a failed notification keeps its retry and discard controls"
    );
}

#[gpui::test]
fn sidebar_hidden_launches_still_list_in_the_mono_panel_after_a_restart(cx: &mut TestAppContext) {
    let s = setup(cx);
    let started = call(
        &s,
        "hidden-launch",
        "sessions.start",
        json!({"project":s.project,"prompt":"Review quietly","title":"Quiet review","draft":true}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    let child = started["sessionId"].as_i64().unwrap();
    let owner = cx.update(|cx| s.manager.read(cx).owner.upgrade().unwrap());
    cx.update(|cx| {
        assert!(s.manager.read(cx).hidden_session(child));
        owner.update(cx, |shell, _| shell.mono_forget_session(child));
        assert!(
            !owner
                .read(cx)
                .mono_session_views(cx)
                .iter()
                .any(|v| v.id == child)
        );
    });
    s.manager.update(cx, |m, cx| m.configure_chat(&s.id, cx));
    cx.run_until_parked();
    cx.update(|cx| {
        let views = owner.read(cx).mono_session_views(cx);
        let view = views
            .iter()
            .find(|v| v.id == child)
            .expect("the launch lists again");
        assert!(view.title.contains("Quiet review"), "{}", view.title);
        assert!(
            owner
                .update(cx, |shell, cx| shell.mono_ensure_session(child, cx))
                .is_some()
        );
        assert!(s.manager.read(cx).hidden_session(child));
    });
}
#[gpui::test]
fn mono_app_launch_visibility_can_be_toggled_and_survives_roster_reload(cx: &mut TestAppContext) {
    let s = setup(cx);
    let started = call(
        &s,
        "visibility-draft",
        "sessions.start",
        json!({"project":s.project,"prompt":"Review quietly","draft":true}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    let child = started["sessionId"].as_i64().unwrap();
    cx.update(|cx| assert!(s.manager.read(cx).hidden_session(child)));
    s.manager
        .update(cx, |m, cx| m.sidebar_preferences(&s.id, true, true, cx));
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(!s.manager.read(cx).hidden_session(child));
        assert!(s.manager.read(cx).is_mono_session(s.session));
    });
    assert!(
        mono::Roster::load(&locked(&s.store).unwrap())
            .unwrap()
            .get(&s.id)
            .unwrap()
            .show_started_sessions_in_sidebar
    );
    s.manager
        .update(cx, |m, cx| m.sidebar_preferences(&s.id, false, false, cx));
    cx.run_until_parked();
    cx.update(|cx| assert!(s.manager.read(cx).hidden_session(child)));
    assert!(
        !mono::Roster::load(&locked(&s.store).unwrap())
            .unwrap()
            .get(&s.id)
            .unwrap()
            .show_started_sessions_in_sidebar
    );
}
#[gpui::test]
fn queued_messages_are_saved_before_dispatch_and_do_not_run_while_blocked(cx: &mut TestAppContext) {
    let s = setup(cx);
    let message = mono::new_id();
    cx.update(|cx| {
        s.manager.update(cx, |m, cx| {
            m.roster.get_mut(&s.id).unwrap().outbox.push(Message {
                sent_at: None,
                id: message.clone(),
                text: "Keep this queued".into(),
                internal: false,
                error: Some("Retry required".into()),
                attachments: Vec::new(),
            });
            m.save(cx);
            m.configure_chat(&s.id, cx);
        })
    });
    cx.run_until_parked();
    let saved = Roster::load(&locked(&s.store).unwrap()).unwrap();
    assert_eq!(saved.monos[0].outbox[0].id, message);
    assert_eq!(
        locked(&s.store)
            .unwrap()
            .load(s.session)
            .unwrap()
            .rows
            .len(),
        0
    );
}

#[gpui::test]
fn chat_composer_keeps_attach_under_the_box_and_opens_the_model_list(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let handle: gpui::AnyWindowHandle = s.window.into();
    handle
        .update(cx, |_, w, cx| {
            s.manager.update(cx, |m, cx| m.dismiss_settings(w, cx));
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let field = visual.debug_bounds("mono-composer").unwrap();
    let send = visual.debug_bounds("mono-send").unwrap();
    let attach = visual.debug_bounds("mono-attach").unwrap();
    let model = visual.debug_bounds("mono-model").unwrap();
    // MonoComposer with controls: send inside the box, attach and the model
    // on the row beneath it, the model at its right end.
    assert!(field.contains(&send.center()));
    assert!(attach.top() >= field.bottom());
    assert!(model.top() >= field.bottom());
    assert!(model.left() > attach.right());
    assert_eq!(send.size, gpui::size(px(26.), px(26.)));
    assert_eq!(attach.size, gpui::size(px(26.), px(26.)));
    let chat = visual.update(|_, cx| s.manager.read(cx).chat().unwrap());
    for (draft, single_line) in [
        ("".to_owned(), true),
        ("A short message".to_owned(), true),
        ("First line\nSecond line".to_owned(), false),
        ("A message that wraps automatically. ".repeat(40), false),
        ("Short again".to_owned(), true),
        ("".to_owned(), true),
    ] {
        chat.update(visual, |chat, cx| {
            chat.input.update(cx, |input, cx| input.set_text(draft, cx));
        });
        visual.run_until_parked();
        visual.refresh().unwrap();
        let field = visual.debug_bounds("mono-composer").unwrap();
        let send = visual.debug_bounds("mono-send").unwrap();
        s.window
            .update(visual, |_, window, _| {
                let scale = window.scale_factor();
                let quads = window.painted_quads();
                for (bounds, radius) in [
                    (
                        field,
                        if single_line {
                            f32::from(field.size.height) / 2.
                        } else {
                            12.
                        },
                    ),
                    (send, 13.),
                ] {
                    let quad = quads
                        .iter()
                        .find(|quad| {
                            (quad.bounds.left().0 / scale - f32::from(bounds.left())).abs() < 0.1
                                && (quad.bounds.top().0 / scale - f32::from(bounds.top())).abs()
                                    < 0.1
                                && (quad.bounds.size.width.0 / scale - f32::from(bounds.size.width))
                                    .abs()
                                    < 0.1
                                && (quad.bounds.size.height.0 / scale
                                    - f32::from(bounds.size.height))
                                .abs()
                                    < 0.1
                        })
                        .expect("the composer and send button paint their surfaces");
                    assert!(
                        (quad.corner_radii.top_left.0 / scale - radius).abs() < 0.1,
                        "expected a {radius}px corner radius, got {:?}",
                        quad.corner_radii
                    );
                }
                assert_eq!(field.size.height <= px(38.), single_line);
            })
            .unwrap();
    }
    handle
        .update(visual, |_, w, cx| {
            chat.update(cx, |c, cx| c.toggle_model_menu(w, cx))
        })
        .unwrap();
    visual.run_until_parked();
    visual.update(|_, cx| {
        let (models, menu) = chat.read(cx).mono_menus_open();
        assert!(models, "the trigger opens the model list itself");
        assert!(!menu, "not the session composer's settings menu");
    });
}

#[gpui::test]
fn details_tabs_share_the_session_titlebar_in_both_layouts(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        crate::shell::bind_keys(cx);
    });
    let session = s
        .window
        .update(cx, |shell, w, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.focus_active(w, cx);
                cx.notify();
            });
            shell.workspace.active.unwrap()
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    for classic in [false, true] {
        s.window
            .update(visual, |shell, _, cx| {
                shell.workspace.new_layout_boxy = classic;
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        let panel = visual.debug_bounds("right-panel-frame").unwrap();
        let tabs = visual.debug_bounds("mono-overview-tabs").unwrap();
        let toggle = visual.debug_bounds("toggle-right-panel").unwrap();
        let title_tabs = visual.debug_bounds("title-tab-strip").unwrap();
        assert!((tabs.top() - title_tabs.top()).abs() <= px(0.5));
        assert_eq!(tabs.size.height, px(TITLE_BAR_HEIGHT));
        assert_eq!(
            tabs.left(),
            panel.left() + px(if classic { 6. } else { 7. })
        );
        let activity = visual.debug_bounds("mono-activity").unwrap();
        let search = visual.debug_bounds("titlebar-file-search").unwrap();
        assert_eq!(search.left() - activity.right(), px(6.));
        assert_eq!(toggle.left() - search.right(), px(4.));
        assert!(tabs.bottom() <= panel.top() + px(if classic { TITLE_BAR_HEIGHT } else { 0. }));
        assert_eq!(
            visual.debug_bounds("mono-dashboard").unwrap().top(),
            panel.top() + px(if classic { TITLE_BAR_HEIGHT } else { 0. })
        );
        assert!(
            visual.debug_bounds("mono-overview-toggle").is_none(),
            "only the shell owns the panel toggle"
        );
        assert!(title_tabs.right() <= tabs.left());
        s.window
            .update(visual, |shell, w, cx| shell.open(session, w, cx))
            .unwrap();
        visual.run_until_parked();
        if !s
            .window
            .update(visual, |shell, _, _| shell.workspace.right_panel_open)
            .unwrap()
        {
            let button = visual.debug_bounds("toggle-right-panel").unwrap();
            visual.simulate_click(button.center(), Default::default());
            visual.run_until_parked();
        }
        assert_eq!(
            visual.debug_bounds("toggle-right-panel").unwrap(),
            toggle,
            "the toggle stays put when changing tab types"
        );
        let selector = if classic {
            "right-panel-explorer"
        } else {
            "project-tabs-separator"
        };
        assert_eq!(
            visual.debug_bounds(selector).unwrap().center().y,
            tabs.center().y
        );
        s.window
            .update(visual, |_, w, cx| {
                s.manager.update(cx, |m, cx| m.open(&s.id, w, cx))
            })
            .unwrap();
        visual.run_until_parked();
    }
}

#[gpui::test]
fn titlebar_details_tabs_switch_pages_and_keep_activity_and_habit_actions(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    s.window
        .update(cx, |shell, _, cx| {
            shell.workspace.right_panel_width = 400.;
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                cx.notify();
            })
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    for (selector, tab) in [
        ("mono-overview-Documents", OverviewTab::Documents),
        ("mono-overview-Habits", OverviewTab::Habits),
    ] {
        let button = visual.debug_bounds(selector).unwrap();
        visual.simulate_click(button.center(), Default::default());
        visual.run_until_parked();
        assert!(s.manager.read_with(visual, |m, _| m.overview_tab == tab));
        assert!(visual.debug_bounds(selector).unwrap().size.width > px(26.));
    }
    let add = visual.debug_bounds("mono-new-habit").unwrap();
    visual.simulate_click(add.center(), Default::default());
    visual.run_until_parked();
    assert!(
        s.manager
            .read_with(visual, |m, _| matches!(m.route, Some(Route::NewHabit)))
    );
    assert!(visual.debug_bounds("mono-overview-tabs").is_some());
    let sessions = visual.debug_bounds("mono-overview-Sessions").unwrap();
    visual.simulate_click(sessions.center(), Default::default());
    visual.run_until_parked();
    assert!(s.manager.read_with(visual, |m, _| m.route.is_none()
        && m.overview_tab == OverviewTab::Sessions));
    for expected in [OverviewTab::Activity, OverviewTab::Sessions] {
        let activity = visual.debug_bounds("mono-activity").unwrap();
        visual.simulate_click(activity.center(), Default::default());
        visual.run_until_parked();
        assert!(
            s.manager
                .read_with(visual, |m, _| m.overview_tab == expected)
        );
    }
}

#[gpui::test]
fn titlebar_divider_resizes_mono_details_and_closed_panels_keep_the_corner(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    s.window
        .update(cx, |shell, _, cx| {
            // The titlebar divider belongs to the rounded frame.
            shell.workspace.new_layout_boxy = false;
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                cx.notify();
            })
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    for width in [220., 280., 400., 500.] {
        s.window
            .update(visual, |shell, _, cx| {
                shell.workspace.right_panel_width = width;
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        assert_eq!(
            visual
                .debug_bounds("project-tabs-separator")
                .unwrap()
                .left(),
            visual.debug_bounds("right-panel-frame").unwrap().left()
        );
        let activity = visual.debug_bounds("mono-activity").unwrap();
        let toggle = visual.debug_bounds("toggle-right-panel").unwrap();
        assert!(
            activity.right() < toggle.left(),
            "Mono controls fit the session sidebar's minimum width"
        );
    }
    let start = visual
        .debug_bounds("titlebar-right-panel-resize")
        .unwrap()
        .center();
    let end = start - gpui::point(px(40.), px(0.));
    visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
    visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
    visual.run_until_parked();
    s.window
        .update(visual, |shell, _, _| {
            assert_eq!(shell.workspace.right_panel_width, 540.);
        })
        .unwrap();
    assert_eq!(
        visual
            .debug_bounds("project-tabs-separator")
            .unwrap()
            .left(),
        visual.debug_bounds("right-panel-frame").unwrap().left()
    );
    for classic in [false, true] {
        s.window
            .update(visual, |shell, _, cx| {
                shell.workspace.new_layout_boxy = classic;
                shell.git.update(cx, |g, cx| g.set_project("~".into(), cx));
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        let open = visual.debug_bounds("toggle-right-panel").unwrap();
        visual.simulate_click(open.center(), Default::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("mono-overview-tabs").is_none());
        assert!(visual.debug_bounds("mono-overview-panel").is_none());
        assert_eq!(visual.debug_bounds("toggle-right-panel").unwrap(), open);
        visual.simulate_click(open.center(), Default::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("mono-overview-tabs").is_some());
    }
}

#[gpui::test]
fn opening_a_session_card_uses_the_shared_sidebar_state(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let session = s
        .window
        .update(cx, |shell, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                cx.notify();
            });
            shell.workspace.active.unwrap()
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let toggle = visual.debug_bounds("toggle-right-panel").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("right-panel-frame").is_none());
    s.window
        .update(visual, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.card_action(&s.id, &json!({"type":"session", "sessionId":session}), cx);
            })
        })
        .unwrap();
    visual.run_until_parked();
    let details = visual.debug_bounds("right-panel-frame").unwrap();
    assert!(s.manager.read_with(
        visual,
        |m, _| matches!(m.route, Some(Route::Session(id)) if id == session)
    ));
    s.window
        .update(visual, |shell, w, cx| {
            assert!(shell.workspace.right_panel_open);
            shell.open(session, w, cx);
        })
        .unwrap();
    visual.run_until_parked();
    assert_eq!(visual.debug_bounds("right-panel-frame").unwrap(), details);
}

#[gpui::test]
fn long_habit_draft_keeps_fields_and_schedule_inside_the_scrolling_page(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        input::bind(cx);
        crate::popover::bind(cx);
    });
    s.window
        .update(cx, |shell, _, cx| {
            shell.workspace.right_panel_width = 220.;
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.overview_tab = OverviewTab::Habits;
                m.route = Some(Route::NewHabit);
                cx.notify();
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    s.manager.update(visual, |m, cx| {
        m.fields["habit-name"].update(cx, |f, cx| {
            f.set_text(
                "A long but valid habit name that fills the narrow panel".into(),
                cx,
            )
        });
        m.fields["habit-instructions"].update(cx, |f, cx| {
            f.set_text(
                "Report useful updates on the current project.\n".repeat(65),
                cx,
            )
        });
        m.fields["habit-kind"].update(cx, |f, cx| f.set_text("weekly".into(), cx));
        cx.notify();
    });
    visual.run_until_parked();
    let body = visual.debug_bounds("mono-habit-editor").unwrap();
    let name = visual.debug_bounds("mono-habit-name-field").unwrap();
    let schedule = visual.debug_bounds("mono-habit-schedule").unwrap();
    let prompt = visual.debug_bounds("mono-habit-prompt-field").unwrap();
    let create = visual.debug_bounds("mono-habit-save").unwrap();
    assert!(name.size.height >= px(36.));
    assert!(schedule.top() >= name.bottom());
    assert!(prompt.top() > schedule.bottom());
    assert!(prompt.size.height >= px(300.));
    assert!(prompt.size.height <= px(328.));
    for bounds in [name, schedule, prompt] {
        assert!(bounds.left() >= body.left());
        assert!(bounds.right() <= body.right() + px(1.));
    }
    assert!(create.bottom() <= body.top());
    let day = visual.debug_bounds("mono-habit-weekday-picker").unwrap();
    assert!(day.left() >= schedule.left());
    assert!(day.right() <= schedule.right() + px(1.));
    visual.simulate_click(day.center(), Default::default());
    visual.run_until_parked();
    // Escape returns focus to the trigger; arrow keys reopen the list with
    // the current weekday highlighted, and Enter retains that selection.
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    assert!(s.manager.read_with(visual, |m, _| m.picker.is_none()));
    visual.simulate_keystrokes("down");
    visual.run_until_parked();
    assert!(s.manager.read_with(visual, |m, _| m.picker.is_some()));
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    assert_eq!(
        s.manager
            .read_with(visual, |m, cx| m.value("habit-weekday", cx)),
        "1"
    );
    assert!(s.manager.read_with(visual, |m, _| m.picker.is_none()));
    visual.simulate_click(day.center(), Default::default());
    visual.run_until_parked();
    let saturday = visual.debug_bounds("menu-action-6").unwrap();
    visual.simulate_click(saturday.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(
        s.manager
            .read_with(visual, |m, cx| m.value("habit-weekday", cx)),
        "6"
    );
    // Invalid schedules keep the full draft in place instead of navigating away.
    s.manager.update(visual, |m, cx| {
        m.fields["habit-time"].update(cx, |f, cx| f.set_text("25:00".into(), cx));
    });
    visual.run_until_parked();
    visual.simulate_click(create.center(), Default::default());
    visual.run_until_parked();
    assert!(
        s.manager
            .read_with(visual, |m, _| m.route == Some(Route::NewHabit))
    );
    assert!(s.manager.read_with(visual, |m, cx| {
        m.fields["habit-instructions"].read(cx).content.len() > 2000
    }));
    assert!(habits::load(&s.root, &s.id).unwrap().is_empty());
    s.manager.update(visual, |m, cx| {
        m.fields["habit-time"].update(cx, |f, cx| f.set_text("09:00".into(), cx));
    });
    visual.run_until_parked();
    visual.simulate_click(create.center(), Default::default());
    visual.run_until_parked();
    let saved = habits::load(&s.root, &s.id).unwrap();
    assert_eq!(saved.len(), 1);
    assert!(saved[0].instructions.len() > 2000);
    assert_eq!(saved[0].schedule.schedule_kind, "weekly");
    assert_eq!(saved[0].schedule.day_of_week, 6);
    assert!(s.manager.read_with(visual, |m, _| m.route.is_none()));
}

#[gpui::test]
fn habit_detail_preserves_long_content_history_and_actions_without_edit_fields(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let mut h = Habit::new(
        "A long habit title that should truncate before the action buttons".into(),
        "Review changes and report anything that needs attention.".into(),
        habits::Schedule::default(),
        monocode_core::time::now_ms(),
    )
    .unwrap();
    h.id = "layout-history".into();
    h.runs.push(habits::Run {
        at: monocode_core::time::now_ms() - 60_000,
        duration_ms: 72_000,
        outcome: "posted".into(),
        report: Some("**Result**\n\nThe project is ready to review.".into()),
        error: None,
    });
    habits::update(&s.root, &s.id, |all| {
        all.push(h.clone());
        Ok(())
    })
    .unwrap();
    s.window
        .update(cx, |shell, _, cx| {
            shell.workspace.right_panel_width = 280.;
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.overview_tab = OverviewTab::Habits;
                m.habits = vec![h.clone()];
                m.route = Some(Route::Habit(h.id.clone()));
                cx.notify();
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    assert!(
        s.manager
            .read_with(visual, |m, _| !m.fields.contains_key("habit-name"))
    );
    let body = visual.debug_bounds("mono-habit-editor").unwrap();
    let description = visual.debug_bounds("mono-habit-instructions").unwrap();
    let row_id = format!("mono-habit-history-{}-{}", h.id, h.runs[0].at);
    let row = visual
        .debug_bounds(Box::leak(row_id.into_boxed_str()))
        .unwrap();
    assert!(row.top() > description.bottom());
    assert!(description.right() <= body.right());
    for selector in ["mono-habit-run", "mono-habit-toggle", "mono-habit-delete"] {
        let action = visual.debug_bounds(selector).unwrap();
        assert!(action.bottom() <= body.top());
        assert!(action.right() <= body.right());
    }
    visual.simulate_click(row.center(), Default::default());
    visual.run_until_parked();
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("mono-habit-report-{}-{}", h.id, h.runs[0].at).into_boxed_str()
            ))
            .is_some()
    );
    let toggle = visual.debug_bounds("mono-habit-toggle").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    visual.run_until_parked();
    assert!(!habits::load(&s.root, &s.id).unwrap()[0].enabled);
    assert!(
        s.manager
            .read_with(visual, |m, _| m.route == Some(Route::Habit(h.id.clone())))
    );
    let remove = visual.debug_bounds("mono-habit-delete").unwrap();
    visual.simulate_click(remove.center(), Default::default());
    visual.run_until_parked();
    assert!(habits::load(&s.root, &s.id).unwrap().is_empty());
    assert!(s.manager.read_with(visual, |m, _| m.route.is_none()));
}

#[gpui::test]
fn new_habit_click_transfers_composer_focus_after_the_animated_page_mounts(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(false));
    s.window
        .update(cx, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.overview_tab = OverviewTab::Habits;
                m.route = None;
                m.focus_active(window, cx);
                cx.notify();
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    s.window
        .update(visual, |_, window, cx| {
            s.manager.read_with(cx, |m, cx| {
                assert!(
                    m.chat()
                        .unwrap()
                        .read(cx)
                        .input
                        .focus_handle(cx)
                        .is_focused(window)
                );
            });
        })
        .unwrap();
    let advance_focus_frame = |visual: &mut gpui::VisualTestContext| {
        visual
            .update_window(s.window.into(), |_, window, cx| {
                // Use the untyped window context: callbacks and drawing may
                // update the root Shell, so it must not already be borrowed.
                // Parking executor tasks does not deliver platform frame callbacks.
                // Flush the requested next frame before drawing its focused field.
                assert!(window.simulate_next_frame(cx) > 0);
                window.draw(cx).clear(cx);
            })
            .unwrap();
        visual.run_until_parked();
    };
    // Follow the actual header action, with the old composer still focused.
    let add = visual.debug_bounds("mono-new-habit").unwrap();
    visual.simulate_click(add.center(), Default::default());
    visual.run_until_parked();
    advance_focus_frame(visual);
    s.window
        .update(visual, |_, window, cx| {
            s.manager.read_with(cx, |m, cx| {
                assert_eq!(m.route, Some(Route::NewHabit));
                assert!(m.fields["habit-name"].focus_handle(cx).is_focused(window));
                assert!(
                    !m.chat()
                        .unwrap()
                        .read(cx)
                        .input
                        .focus_handle(cx)
                        .is_focused(window)
                );
            });
        })
        .unwrap();
    let page = "mono-detail-page-new-habit";
    assert!(visual.debug_bounds(page).is_some());
    // Complete the real page transition, then leave/reopen through its controls.
    for reopen in [false, true] {
        visual
            .update_window(s.window.into(), |_, window, cx| {
                let details = s.manager.read(cx).details.clone();
                details.update(cx, |v, cx| v.elapsed(std::time::Duration::from_secs(1), cx));
                window.draw(cx).clear(cx);
            })
            .unwrap();
        let back = visual.debug_bounds("mono-page-back").unwrap();
        visual.simulate_click(back.center(), Default::default());
        visual.run_until_parked();
        assert!(s.manager.read_with(visual, |m, _| m.route.is_none()));
        if !reopen {
            let add = visual.debug_bounds("mono-new-habit").unwrap();
            visual.simulate_click(add.center(), Default::default());
            visual.run_until_parked();
            advance_focus_frame(visual);
            s.window
                .update(visual, |_, window, cx| {
                    s.manager.read_with(cx, |m, cx| {
                        assert!(m.fields["habit-name"].focus_handle(cx).is_focused(window))
                    });
                })
                .unwrap();
        }
    }
}

#[gpui::test]
fn sidebar_mono_only_marks_unread_replies_and_swaps_the_mark_for_options_on_hover(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    s.window
        .update(cx, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.close(cx);
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    let unread_selector: &'static str =
        Box::leak(format!("mono-rail-unread-{}", s.id).into_boxed_str());
    let options_selector: &'static str =
        Box::leak(format!("mono-rail-options-{}", s.id).into_boxed_str());
    let row_selector: &'static str = Box::leak(format!("mono-rail-{}", s.id).into_boxed_str());
    let away = gpui::point(px(5.), px(5.));
    visual.simulate_mouse_move(away, None, Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    let row = visual.debug_bounds(row_selector).unwrap();
    assert!(visual.debug_bounds(unread_selector).is_none());
    assert!(visual.debug_bounds(options_selector).is_none());
    // Check the painted output too: an idle Mono used to paint a gray
    // status dot here, even with no unread reply and no pointer over it.
    s.window
        .update(visual, |_, window, _| {
            let scale = window.scale_factor();
            assert!(!window.painted_quads().iter().any(|quad| {
                let bounds = quad.bounds;
                bounds.left().0 >= f32::from(row.right() - px(28.)) * scale
                    && bounds.right().0 <= f32::from(row.right() - px(4.)) * scale
                    && bounds.top().0 >= f32::from(row.top()) * scale
                    && bounds.bottom().0 <= f32::from(row.bottom()) * scale
                    && (3. * scale..=8. * scale).contains(&bounds.size.width.0)
                    && (3. * scale..=8. * scale).contains(&bounds.size.height.0)
            }));
        })
        .unwrap();

    let post = |role, text: &str, visual: &mut gpui::VisualTestContext| {
        s.window
            .update(visual, |_, _, cx| {
                let chat = s.manager.read(cx).chats[&s.id].clone();
                chat.update(cx, |v, cx| v.mono_post(role, text.into(), cx))
                    .unwrap();
            })
            .unwrap();
        visual.run_until_parked();
        visual.refresh().unwrap();
    };
    post("user", "My question", visual);
    assert!(visual.debug_bounds(unread_selector).is_none());
    post("assistant", "A new reply", visual);
    let unread = visual
        .debug_bounds(unread_selector)
        .expect("an unseen assistant reply has an accent circle");
    assert_eq!(unread.size, gpui::size(px(6.), px(6.)));
    assert!(visual.debug_bounds(options_selector).is_none());
    let before_hover = visual.debug_bounds(row_selector).unwrap();
    visual.simulate_mouse_move(before_hover.center(), None, Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds(unread_selector).is_none(),
        "options replace the unread circle on hover"
    );
    let options = visual.debug_bounds(options_selector).unwrap();
    assert_eq!(options.size, gpui::size(px(24.), px(24.)));
    assert_eq!(options.center(), unread.center());
    assert_eq!(visual.debug_bounds(row_selector).unwrap(), before_hover);
    visual.simulate_click(options.center(), Default::default());
    visual.run_until_parked();
    s.manager.read_with(visual, |m, _| {
        assert!(m.menu.is_some(), "the hover action opens Mono options");
        assert!(m.active.is_none(), "options must not open the Mono");
    });

    s.window
        .update(visual, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.menu = None;
                m.open(&s.id, window, cx);
            });
        })
        .unwrap();
    visual.simulate_mouse_move(away, None, Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds(unread_selector).is_none());
    s.window
        .update(visual, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                assert_eq!(
                    m.roster.get(&s.id).unwrap().seen_rows,
                    m.chats[&s.id].read(cx).mono_rows(cx).len()
                );
                m.close(cx);
            });
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds(unread_selector).is_none(),
        "a reply stays read when the Mono closes"
    );
}

#[gpui::test]
fn sidebar_right_click_opens_mono_menu_outside_the_mono_and_edits_its_look(
    cx: &mut TestAppContext,
) {
    cx.update(input::bind);
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    // Leave the Mono, as when a session is open: only the rail remains.
    s.window
        .update(cx, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.active = None;
                cx.notify();
            })
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let row = s
        .manager
        .read_with(visual, |m, _| m.rail_bounds[&s.id].get())
        .expect("the sidebar lists the Mono");
    let at = row.center();
    visual.simulate_event(gpui::MouseDownEvent {
        position: at,
        button: MouseButton::Right,
        modifiers: Default::default(),
        click_count: 1,
        first_mouse: false,
    });
    visual.simulate_event(gpui::MouseUpEvent {
        position: at,
        button: MouseButton::Right,
        modifiers: Default::default(),
        click_count: 1,
    });
    visual.run_until_parked();
    // MonoMenu opens beside the pointer with ten mascots and nine presets
    // plus the custom swatch, all in single rows.
    let first = visual
        .debug_bounds("mono-mascot-0")
        .expect("right-click opens the Mono's menu");
    let last = visual.debug_bounds("mono-mascot-9").unwrap();
    assert_eq!(first.top(), last.top());
    assert_eq!(first.size.height, px(32.));
    assert!(first.left() >= at.x);
    let custom = visual.debug_bounds("mono-custom-color").unwrap();
    assert_eq!(
        visual.debug_bounds("mono-color-0").unwrap().top(),
        custom.top()
    );
    let pink = visual.debug_bounds("mono-color-4").unwrap().center();
    visual.simulate_click(pink, Default::default());
    visual.run_until_parked();
    let color = s
        .manager
        .read_with(visual, |m, _| m.roster.get(&s.id).unwrap().color);
    assert_eq!(color, mono::COLORS_MONO[4]);
    // Enter in the name field commits and closes the menu.
    let name = visual.debug_bounds("mono-options-name").unwrap();
    visual.simulate_click(name.center(), Default::default());
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    s.manager
        .read_with(visual, |m, _| assert!(m.menu.is_none()));
    visual.refresh().unwrap();
    visual
        .update_window(s.window.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        })
        .unwrap();
    assert!(visual.debug_bounds("mono-mascot-0").is_none());
}

#[gpui::test]
fn habits_and_documents_search_like_sessions(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let habit = |id: &str, name: &str| {
        let mut h = Habit::new(
            name.into(),
            "Do the work.".into(),
            habits::Schedule::default(),
            monocode_core::time::now_ms(),
        )
        .unwrap();
        h.id = id.into();
        h
    };
    let document = |id: &str, title: &str| mono::Document {
        id: id.into(),
        kind: "markdown".into(),
        title: title.into(),
        summary: String::new(),
        body: String::new(),
        created_at: 1,
        updated_at: 1,
    };
    s.window
        .update(cx, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.overview_tab = OverviewTab::Habits;
                m.habits = vec![
                    habit("digest", "Morning digest"),
                    habit("report", "Weekly report"),
                ];
                let documents = &mut m.roster.get_mut(&s.id).unwrap().documents;
                documents.push(document("plan", "Launch plan"));
                documents.push(document("summary", "Release summary"));
                cx.notify();
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let search = |visual: &mut gpui::VisualTestContext, key: &str, text: &str| {
        s.manager.update(visual, |m, cx| {
            m.fields[key].update(cx, |f, cx| f.set_text(text.into(), cx));
        });
        visual.run_until_parked();
    };
    assert!(visual.debug_bounds("mono-habit-report").is_some());
    search(visual, "mono-habit-search", "DIGEST");
    assert!(visual.debug_bounds("mono-habit-digest").is_some());
    assert!(visual.debug_bounds("mono-habit-report").is_none());

    s.manager.update(visual, |m, cx| {
        m.select_overview(OverviewTab::Documents, cx)
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("mono-document-plan").is_some());
    search(visual, "mono-document-search", "release");
    assert!(visual.debug_bounds("mono-document-plan").is_none());
    assert!(visual.debug_bounds("mono-document-summary").is_some());
}
