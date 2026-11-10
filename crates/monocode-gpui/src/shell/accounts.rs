use super::*;
use crate::accounts::AccountEvent;
impl Shell {
    pub(super) fn observe_accounts(&mut self, cx: &mut Context<Self>) {
        let Some(center) = crate::accounts::get(cx) else {
            return;
        };
        self.observations.push(cx.subscribe(
            &center,
            |this, _, event: &AccountEvent, cx| match event {
                AccountEvent::Select {
                    kind,
                    id,
                    project,
                    session,
                } => this.select_provider_account(*kind, id, project, *session, cx),
                AccountEvent::Removing(kind, id) => {
                    for view in this.sessions.values() {
                        if view.read(cx).summary.provider == kind.id()
                            && view.read(cx).settings.account.id == *id
                        {
                            view.update(cx, |v, cx| v.account_unavailable(cx));
                        }
                    }
                    if let Some(monos) = &this.monos {
                        monos.update(cx, |m, cx| m.account_changed(*kind, id, true, cx));
                    }
                }
                AccountEvent::Authenticated(kind, id) => {
                    for view in this.sessions.values() {
                        if view.read(cx).summary.provider == kind.id()
                            && view.read(cx).settings.account.id == *id
                        {
                            view.update(cx, |v, cx| v.account_authenticated(cx));
                        }
                    }
                    if let Some(monos) = &this.monos {
                        monos.update(cx, |m, cx| m.account_changed(*kind, id, false, cx));
                    }
                    this.update_usage_provider(cx);
                }
                AccountEvent::Updated => this.update_usage_provider(cx),
                AccountEvent::PickerDismiss | AccountEvent::AuthStarted => {}
                AccountEvent::Manage => {
                    cx.spawn(async move |this, cx| {
                        let _ = this.update_in(cx, |this, window, cx| {
                            this.open_settings(crate::settings::Section::Providers, window, cx)
                        });
                    })
                    .detach();
                }
            },
        ));
    }
    fn select_provider_account(
        &mut self,
        kind: Kind,
        id: &str,
        project: &str,
        session: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        let Some(center) = crate::accounts::get(cx) else {
            return;
        };
        if !center.read(cx).book.exists(kind, id) {
            return;
        }
        let profile = center.read(cx).profile(id);
        let view = session
            .and_then(|id| self.sessions.get(&id).cloned())
            .or_else(|| {
                self.monos
                    .as_ref()
                    .and_then(|m| m.read(cx).chat())
                    .filter(|v| Some(v.read(cx).summary.id) == session)
            });
        let Some(view) = view else {
            return;
        };
        let current = view.read(cx);
        if current.summary.provider != kind.id() || current.summary.cwd != project {
            return;
        }
        // The footer can be used while a restored/evicted conversation is
        // loading. Resolve its actual account and terminal error before
        // deciding whether the selection needs a new conversation.
        if current.needs_history_restore() {
            let owner = cx.weak_entity();
            let id = id.to_owned();
            let project = project.to_owned();
            let session = Some(current.summary.id);
            view.update(cx, |view, cx| {
                view.defer_history_action(
                    move |_, cx| {
                        cx.defer(move |cx| {
                            let _ = owner.update(cx, |shell, cx| {
                                shell.select_provider_account(kind, &id, &project, session, cx)
                            });
                        });
                    },
                    cx,
                );
            });
            return;
        }
        if current.settings.account.id == id {
            return;
        }
        // An idle conversation moves to the account in place, limited or not.
        // Only a running turn falls through to a fresh session.
        if !current.busy {
            if current.summary.has_messages || current.account_recovery_limit(cx).is_some() {
                view.update(cx, |v, cx| v.switch_account(profile, cx));
            } else {
                view.update(cx, |v, cx| v.set_blank_account(profile, cx));
                self.update_usage_provider(cx);
            }
            return;
        }
        let mut settings = current.settings.clone();
        settings.account = profile;
        settings.mono = None;
        settings.usage_limit = None;
        settings.handoff = None;
        settings.turns.clear();
        settings.operator_enabled = false;
        settings.revision = 0;
        let cwd = current.summary.cwd.clone();
        let store = self.store.clone();
        let revision = self.navigation_revision;
        let work = cx.background_executor().spawn(async move {
            let store = locked(&store)?;
            let summary = store.create(kind.id(), &cwd)?;
            store.save_agent_settings(summary.id, &settings)?;
            Ok::<_, String>((summary, settings))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok((summary, settings)) => {
                    let id = summary.id;
                    this.add(summary, cx);
                    this.sessions[&id].update(cx, |v, cx| v.install_transfer(settings, None, cx));
                    if this.navigation_revision == revision {
                        this.activate_session(id, window, cx);
                    } else {
                        this.invalidate_chrome(cx);
                        cx.notify();
                    }
                }
                Err(error) => {
                    this.error = Some(error);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account_switch(cx: &mut gpui::TestAppContext, carry: bool, cold: bool, limited: bool) {
        use monocode_core::{accounts::Profile, transcript::Row};
        let (window, _, id) = crate::shell::tests::window(cx);
        let (target, destination, original_count, active, tab_count) = window
            .update(cx, |shell, _, cx| {
                let center = crate::accounts::get(cx).unwrap();
                let (source, target) = center.update(cx, |center, _| {
                    (
                        center.book.add(Kind::Claude, "Limited").unwrap(),
                        center.book.add(Kind::Claude, "Available").unwrap(),
                    )
                });
                let root = center.read(cx).root.clone();
                let source = Profile::new(&source.id, &root);
                let destination = Profile::new(&target.id, &root)
                    .home(Kind::Claude)
                    .unwrap()
                    .join("projects/-fixture/saved-thread.jsonl");
                if carry {
                    let transcript = source
                        .home(Kind::Claude)
                        .unwrap()
                        .join("projects/-fixture/saved-thread.jsonl");
                    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
                    std::fs::write(transcript, "{\"conversation\":\"original\"}\n").unwrap();
                }
                let mut rows = vec![
                    Row {
                        role: "user".into(),
                        text: "Finish the integration".into(),
                        first: true,
                        sent_at: None,
                    },
                    Row {
                        role: "assistant".into(),
                        text: "The integration is partly implemented".into(),
                        first: true,
                        sent_at: None,
                    },
                ];
                if limited {
                    // Older native saves persisted this as a system notice and
                    // have no structured usage_limit setting.
                    rows.push(Row {
                        role: "system".into(),
                        text: "rate_limit".into(),
                        first: true,
                        sent_at: None,
                    });
                }
                {
                    let mut store = locked(&shell.store).unwrap();
                    store.save_provider_accounts(&center.read(cx).book).unwrap();
                    store.provider_id(id, "saved-thread").unwrap();
                    store.write_rows(id, 0, &rows).unwrap();
                }
                shell.sessions[&id].update(cx, |view, cx| {
                    let mut settings = view.settings.clone();
                    settings.account = source;
                    // Block provider launch at preflight: these tests exercise the
                    // account transfer and routing without making a live request.
                    settings.worktree_removed = true;
                    view.install_transfer(settings, None, cx);
                    view.summary.has_messages = true;
                    view.summary.provider_id = Some("saved-thread".into());
                    view.transcript.update(cx, |v, cx| {
                        if cold {
                            v.visible(false, cx);
                        }
                        v.patch(0, rows, cx);
                    });
                    assert!(view.settings.usage_limit.is_none());
                    if cold {
                        assert!(view.unload_history(cx));
                    }
                });
                let state = (
                    target.id.clone(),
                    destination,
                    shell.sessions.len(),
                    shell.workspace.active,
                    shell.workspace.tabs.len(),
                );
                center.update(cx, |center, cx| {
                    center.choose(
                        Kind::Claude,
                        target.id,
                        "/work/monocode".into(),
                        Some(id),
                        cx,
                    );
                });
                state
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(shell.sessions.len(), original_count);
                assert_eq!(
                    locked(&shell.store).unwrap().sessions().unwrap().len(),
                    original_count
                );
                assert_eq!(shell.workspace.active, active);
                assert_eq!(shell.workspace.tabs.len(), tab_count);
                let view = shell.sessions[&id].read(cx);
                assert_eq!(view.summary.id, id);
                assert_eq!(view.settings.account.id, target);
                assert!(
                    view.transcript
                        .read(cx)
                        .source_rows()
                        .iter()
                        .any(|row| row.text == "The integration is partly implemented")
                );
                if carry {
                    assert_eq!(view.summary.provider_id.as_deref(), Some("saved-thread"));
                    assert!(view.settings.pending_provider_switch.is_none());
                    assert!(view.settings.handoff.is_none());
                    assert_eq!(
                        std::fs::read_to_string(&destination).unwrap(),
                        "{\"conversation\":\"original\"}\n"
                    );
                } else {
                    assert!(view.summary.provider_id.is_none());
                    let pending = view.settings.pending_provider_switch.as_ref().unwrap();
                    assert_eq!(pending.skip_outgoing_recap, limited);
                    assert_eq!(pending.provider_id.as_deref(), Some("saved-thread"));
                    assert_ne!(pending.account.id, target);
                }
            })
            .unwrap();
    }

    #[gpui::test]
    fn footer_account_switch_carries_a_limited_session_in_place(cx: &mut gpui::TestAppContext) {
        account_switch(cx, true, false, true);
    }

    #[gpui::test]
    fn footer_account_switch_restores_a_saved_limit_before_choosing_a_session(
        cx: &mut gpui::TestAppContext,
    ) {
        account_switch(cx, true, true, true);
    }

    #[gpui::test]
    fn footer_account_switch_keeps_the_local_session_when_native_history_is_missing(
        cx: &mut gpui::TestAppContext,
    ) {
        account_switch(cx, false, false, true);
    }

    #[gpui::test]
    fn footer_account_switch_carries_an_unlimited_session_in_place(cx: &mut gpui::TestAppContext) {
        account_switch(cx, true, false, false);
    }

    #[gpui::test]
    fn footer_account_switch_hands_off_an_unlimited_session_without_native_history(
        cx: &mut gpui::TestAppContext,
    ) {
        account_switch(cx, false, false, false);
    }

    #[gpui::test]
    fn switching_accounts_reuses_the_session_with_or_without_messages(
        cx: &mut gpui::TestAppContext,
    ) {
        let (window, _, id) = crate::shell::tests::window(cx);
        let count = window
            .update(cx, |shell, _, cx| {
                let center = crate::accounts::get(cx).unwrap();
                let account = center.update(cx, |center, _| {
                    center.book.add(Kind::Claude, "Work").unwrap()
                });
                locked(&shell.store)
                    .unwrap()
                    .save_provider_accounts(&center.read(cx).book)
                    .unwrap();
                let count = shell.sessions.len();
                shell.select_provider_account(
                    Kind::Claude,
                    &account.id,
                    "/work/monocode",
                    Some(id),
                    cx,
                );
                assert_eq!(shell.sessions.len(), count);
                assert_eq!(shell.sessions[&id].read(cx).settings.account.id, account.id);
                shell.sessions[&id].update(cx, |v, _| v.summary.has_messages = true);
                shell.select_provider_account(
                    Kind::Claude,
                    monocode_core::accounts::DEFAULT,
                    "/work/monocode",
                    Some(id),
                    cx,
                );
                count
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                let view = shell.sessions[&id].read(cx);
                assert_eq!(view.settings.account.id, monocode_core::accounts::DEFAULT);
                assert!(view.settings.usage_limit.is_none());
                assert_eq!(shell.sessions.len(), count);
            })
            .unwrap();
    }
}
