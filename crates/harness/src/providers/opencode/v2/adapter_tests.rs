use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::core::registry::{event_sink, ignore_events};
use crate::providers::opencode::test_support::{FakeHost, path_of, wait_for};

const THREAD: &str = "native-v2-owned";
const PROVIDER: &str = "ses_owned";

fn host() -> FakeHost {
    let host = FakeHost::v2();
    host.respond_with(|request| match (request.method.as_str(), path_of(&request.url).as_str()) {
        ("POST", "/api/session") => (200, json!({"data":{"id":PROVIDER,"location":{"directory":"/owned/work"}}}).to_string()),
        ("GET", "/api/session/ses_owned") => (200, json!({"data":{"id":PROVIDER,"location":{"directory":"/owned/work"}}}).to_string()),
        ("GET", "/api/session/ses_owned/message") => (200, json!({"data":[{"id":"msg_old","type":"user"},{"id":"msg_latest","type":"user"}],"cursor":{}}).to_string()),
        ("GET", "/api/permission/request") | ("GET", "/api/session/ses_owned/form") => (200, json!({"data":[]}).to_string()),
        _ => (204, String::new()),
    });
    host
}

fn input() -> SendTurnInput {
    SendTurnInput {
        session: HarnessSessionInput {
            session_id: THREAD.into(),
            cwd: "/owned/work".into(),
            model: "opencode:fixture/free".into(),
            model_settings: None,
            provider_account_id: None,
            runtime_mode: RuntimeMode::Supervised,
            intent: None,
            controls_agents: None,
            app_access: None,
        },
        text: "one owned fixture turn".into(),
        attachments: None,
    }
}

fn adapter(host: &FakeHost) -> Adapter {
    Adapter::new(host.children(), SharedCatalog::new(), host.spawner(), None)
}

fn stream(host: &FakeHost) -> String {
    host.sse_opens()
        .into_iter()
        .rev()
        .find(|(_, url)| path_of(url) == "/api/event")
        .unwrap()
        .0
}

fn emit(host: &FakeHost, kind: &str, data: Value) {
    host.sse(&stream(host), json!({"type":kind,"data":data}));
}

async fn start(adapter: &Adapter, host: &FakeHost, sink: EventSink) -> smol::Task<Result<()>> {
    let previous = host.calls_to("/prompt").len();
    let adapter = adapter.clone();
    let turn = smol::spawn(async move { adapter.send_turn(input(), sink, None).await });
    wait_for("version 2 prompt", || {
        host.calls_to("/prompt").len() > previous
    })
    .await;
    turn
}

#[test]
fn preserves_turn_acceptance_native_control_identity_and_stop_cleanup() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        let events = Arc::new(Mutex::new(Vec::new()));
        let accepted = Arc::new(AtomicUsize::new(0));
        let turn = {
            let adapter = adapter.clone();
            let events = events.clone();
            let accepted = accepted.clone();
            smol::spawn(async move {
                adapter
                    .send_turn(
                        input(),
                        event_sink(move |event| events.lock().push(event)),
                        Some(Arc::new(move || {
                            accepted.fetch_add(1, Ordering::SeqCst);
                        })),
                    )
                    .await
            })
        };
        wait_for("accepted prompt", || accepted.load(Ordering::SeqCst) == 1).await;
        emit(
            &host,
            "session.text.delta",
            json!({"sessionID":PROVIDER,"assistantMessageID":"msg_answer","ordinal":0,"delta":"hel"}),
        );
        emit(
            &host,
            "session.text.ended",
            json!({"sessionID":PROVIDER,"assistantMessageID":"msg_answer","ordinal":0,"text":"hello"}),
        );
        emit(
            &host,
            "session.execution.succeeded",
            json!({"sessionID":PROVIDER}),
        );
        turn.await.unwrap();
        let events = events.lock().clone();
        let text: String = events
            .iter()
            .filter_map(|event| match event {
                HarnessEvent::MessageDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "hello");
        assert!(events.iter().any(|event| matches!(event, HarnessEvent::SessionProviderBound { provider_session_id } if provider_session_id == PROVIDER)));
        assert!(events.iter().any(|event| matches!(event, HarnessEvent::TurnStarted { provider_turn_id } if provider_turn_id.starts_with("msg_"))));
        let spawned = host.spawns();
        assert_eq!(spawned[0].session_id, THREAD);
        assert!(spawned[0].args.contains(&"--stdio".into()));
        assert_eq!(spawned[0].environment.len(), 2);
        assert!(
            !format!("{:?}", spawned[0]).contains(&spawned[0].environment["OPENCODE_PASSWORD"])
        );
        adapter.stop_session(THREAD.into()).await.unwrap();
        assert_eq!(host.watched_children(), 0);
        assert_eq!(host.watched_streams(), 0);
    });
}

#[test]
fn rejects_a_bad_attachment_before_startup_or_an_active_turn() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        let mut request = input();
        request.attachments = Some(vec![Attachment {
            name: "missing.png".into(),
            mime_type: "image/png".into(),
            ..Default::default()
        }]);
        assert!(
            adapter
                .send_turn(request, ignore_events(), None)
                .await
                .is_err()
        );
        assert!(host.spawns().is_empty());
        assert!(adapter.live(THREAD).is_none());
        let turn = start(&adapter, &host, ignore_events()).await;
        emit(
            &host,
            "session.execution.succeeded",
            json!({"sessionID":PROVIDER}),
        );
        turn.await.unwrap();
        let live = adapter.live(THREAD).unwrap();
        adapter.event(&live, &json!({"type":"permission.asked","data":{"sessionID":PROVIDER,"id":"per_stale","action":"bash","resources":[]}}));
        assert!(live.state.lock().approvals.is_empty());
        adapter.stop_session(THREAD.into()).await.unwrap();
    });
}

#[test]
fn startup_log_failure_releases_the_child_and_both_stream_watchers() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        host.next_log_error("fixture log HTTP failure");
        let result = adapter.send_turn(input(), ignore_events(), None).await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("fixture log HTTP failure")
        );
        assert!(host.calls_to("/prompt").is_empty());
        assert!(adapter.live(THREAD).is_none());
        assert!(adapter.inner.starting.lock().is_empty());
        assert_eq!(host.watched_children(), 0);
        assert_eq!(host.watched_streams(), 0);
    });
}

#[test]
fn resume_reuses_only_the_bound_provider_id_and_rewind_commits_without_files() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        adapter.bind_session(THREAD, PROVIDER, "/owned/work", None);
        let turn = start(&adapter, &host, ignore_events()).await;
        assert!(
            host.http_calls()
                .iter()
                .all(|call| call.method != "POST" || path_of(&call.url) != "/api/session")
        );
        emit(
            &host,
            "session.execution.succeeded",
            json!({"sessionID":PROVIDER}),
        );
        turn.await.unwrap();
        let result = adapter
            .rewind_last_turn(
                RewindLastTurnInput {
                    session: input().session,
                    provider_turn_id: None,
                    text: None,
                    attachments: None,
                },
                ignore_events(),
            )
            .await
            .unwrap();
        assert!(!result.submitted);
        let calls = host.calls_to("/revert/");
        assert_eq!(calls.len(), 2);
        assert_eq!(
            serde_json::from_str::<Value>(calls[0].body.as_ref().unwrap()).unwrap(),
            json!({"messageID":"msg_latest","files":false})
        );
        assert!(calls[1].url.ends_with("/revert/commit"));
        adapter.stop_session(THREAD.into()).await.unwrap();
    });
}

#[test]
fn routes_a_descendant_approval_and_deduplicates_a_ui_click() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        let events = Arc::new(Mutex::new(Vec::new()));
        let turn = start(&adapter, &host, {
            let events = events.clone();
            event_sink(move |event| events.lock().push(event))
        })
        .await;
        emit(
            &host,
            "session.created",
            json!({"sessionID":"ses_child","parentID":PROVIDER}),
        );
        emit(
            &host,
            "permission.asked",
            json!({"id":"per_owned","sessionID":"ses_child","action":"bash","resources":["echo fixture"],"source":{"type":"tool","id":"call_owned","messageID":"msg_owned"}}),
        );
        wait_for("approval", || {
            events
                .lock()
                .iter()
                .any(|event| matches!(event, HarnessEvent::ApprovalRequested { .. }))
        })
        .await;
        let ui = events
            .lock()
            .iter()
            .find_map(|event| match event {
                HarnessEvent::ApprovalRequested { request_id, .. } => Some(*request_id),
                _ => None,
            })
            .unwrap();
        adapter.respond_approval(THREAD, ui, ApprovalDecision::Deny);
        adapter.respond_approval(THREAD, ui, ApprovalDecision::Allow);
        wait_for("approval reply", || {
            !host.calls_to("/permission/per_owned/reply").is_empty()
        })
        .await;
        let calls = host.calls_to("/permission/per_owned/reply");
        assert_eq!(calls.len(), 1);
        assert!(calls[0].url.contains("/ses_child/"));
        assert_eq!(
            serde_json::from_str::<Value>(calls[0].body.as_ref().unwrap()).unwrap(),
            json!({"decision":"reject"})
        );
        emit(
            &host,
            "session.execution.succeeded",
            json!({"sessionID":PROVIDER}),
        );
        turn.await.unwrap();
        adapter.stop_session(THREAD.into()).await.unwrap();
    });
}

#[test]
fn authoritative_form_rejection_reasks_only_the_bad_field_with_prior_answers() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        let replies = Arc::new(AtomicUsize::new(0));
        let replies_for_http = replies.clone();
        host.respond_with(move |request| {
            let path = path_of(&request.url);
            if path == "/api/session" { return (200, json!({"data":{"id":PROVIDER}}).to_string()); }
            if path.ends_with("/form/frm_owned/reply") && replies_for_http.fetch_add(1, Ordering::SeqCst) == 0 { return (400, json!({"_tag":"FormInvalidAnswerError","message":"Expected email for form field: address"}).to_string()); }
            (204, String::new())
        });
        let turn = start(&adapter, &host, ignore_events()).await;
        emit(
            &host,
            "form.created",
            json!({"form":{"id":"frm_owned","sessionID":PROVIDER,"title":"Typed question","fields":[{"key":"enabled","type":"boolean","required":true},{"key":"address","type":"string","format":"email","required":true}]}}),
        );
        let live = adapter.live(THREAD).unwrap();
        wait_for("first form field", || {
            live.state.lock().visible_form.is_some()
        })
        .await;
        let ui = live.state.lock().visible_form.as_ref().unwrap().0;
        adapter.respond_question(THREAD, ui, answered("enabled", "true"));
        let ui = live.state.lock().visible_form.as_ref().unwrap().0;
        adapter.respond_question(THREAD, ui, answered("address", "invalid"));
        wait_for("invalid field retry", || {
            replies.load(Ordering::SeqCst) == 1 && live.state.lock().visible_form.is_some()
        })
        .await;
        let (ui, value) = {
            let state = live.state.lock();
            let (ui, form) = state.visible_form.as_ref().unwrap();
            (*ui, form.answer())
        };
        assert_eq!(value, json!({"enabled":true}));
        adapter.respond_question(THREAD, ui, answered("address", "owner@example.test"));
        wait_for("valid form reply", || replies.load(Ordering::SeqCst) == 2).await;
        let calls = host.calls_to("/form/frm_owned/reply");
        assert_eq!(
            serde_json::from_str::<Value>(calls[1].body.as_ref().unwrap()).unwrap(),
            json!({"answer":{"enabled":true,"address":"owner@example.test"}})
        );
        emit(
            &host,
            "session.execution.succeeded",
            json!({"sessionID":PROVIDER}),
        );
        turn.await.unwrap();
        adapter.stop_session(THREAD.into()).await.unwrap();
    });
}

fn answered(key: &str, value: &str) -> UserQuestionReply {
    UserQuestionReply::Answered {
        answers: BTreeMap::from([(key.into(), vec![value.into()])]),
        custom: None,
    }
}

#[test]
fn hidden_default_form_is_submitted_and_skip_uses_cancel_route() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        let turn = start(&adapter, &host, ignore_events()).await;
        emit(
            &host,
            "form.created",
            json!({"form":{"id":"frm_hidden","sessionID":PROVIDER,"fields":[{"key":"mode","type":"string","hidden":true,"default":"owned"}]}}),
        );
        wait_for("hidden defaults submitted", || {
            !host.calls_to("/form/frm_hidden/reply").is_empty()
        })
        .await;
        emit(
            &host,
            "form.created",
            json!({"form":{"id":"frm_skip","sessionID":PROVIDER,"fields":[{"key":"mode","type":"string","required":true}]}}),
        );
        let live = adapter.live(THREAD).unwrap();
        wait_for("visible form", || live.state.lock().visible_form.is_some()).await;
        let ui = live.state.lock().visible_form.as_ref().unwrap().0;
        adapter.respond_question(THREAD, ui, UserQuestionReply::Skipped);
        wait_for("skipped form cancelled", || {
            host.http_calls()
                .iter()
                .any(|call| call.method == "DELETE" && call.url.ends_with("/form/frm_skip"))
        })
        .await;
        emit(
            &host,
            "session.execution.succeeded",
            json!({"sessionID":PROVIDER}),
        );
        turn.await.unwrap();
        adapter.stop_session(THREAD.into()).await.unwrap();
    });
}

#[test]
fn reconnect_recovers_lost_full_text_and_the_terminal_event_from_the_durable_log() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        let events = Arc::new(Mutex::new(Vec::new()));
        let turn = start(&adapter, &host, {
            let events = events.clone();
            event_sink(move |event| events.lock().push(event))
        })
        .await;
        emit(
            &host,
            "session.text.delta",
            json!({"sessionID":PROVIDER,"assistantMessageID":"msg_answer","ordinal":0,"delta":"hel"}),
        );
        wait_for("initial text", || {
            events
                .lock()
                .iter()
                .any(|event| matches!(event, HarnessEvent::MessageDelta { text } if text == "hel"))
        })
        .await;
        host.next_log(vec![json!({"id":"evt_full","type":"session.text.ended","durable":{"seq":7},"data":{"sessionID":PROVIDER,"assistantMessageID":"msg_answer","ordinal":0,"text":"hello"}}), json!({"id":"evt_done","type":"session.execution.succeeded","durable":{"seq":8},"data":{"sessionID":PROVIDER}})]);
        host.sse_end(&stream(&host), Some("owned connection interruption"));
        turn.await.unwrap();
        let text: String = events
            .lock()
            .iter()
            .filter_map(|event| match event {
                HarnessEvent::MessageDelta { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "hello");
        assert!(
            host.sse_opens()
                .iter()
                .filter(|(_, url)| path_of(url) == "/api/event")
                .count()
                >= 2
        );
        adapter.stop_session(THREAD.into()).await.unwrap();
    });
}

#[test]
fn failed_reconnect_releases_the_last_opened_stream_and_child() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        let turn = start(&adapter, &host, ignore_events()).await;
        for _ in 0..3 {
            host.next_log_error("owned recovery log failure");
        }
        host.sse_end(&stream(&host), Some("owned interruption"));
        assert!(
            turn.await
                .unwrap_err()
                .to_string()
                .contains("could not recover")
        );
        wait_for("failed recovery cleanup", || {
            host.watched_children() == 0 && host.watched_streams() == 0
        })
        .await;
        adapter.stop_session(THREAD.into()).await.unwrap();
    });
}

#[test]
fn steering_and_manual_compaction_use_the_same_session_and_wait_for_execution() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        let turn = start(&adapter, &host, ignore_events()).await;
        adapter
            .steer_turn(SteerTurnInput {
                session_id: THREAD.into(),
                cwd: "/owned/work".into(),
                model: input().session.model,
                model_settings: None,
                text: "follow up".into(),
                attachments: None,
            })
            .await
            .unwrap();
        assert_eq!(host.calls_to("/prompt").len(), 2);
        adapter.cancel_turn(THREAD.into()).await.unwrap();
        turn.await.unwrap();
        let compact = {
            let adapter = adapter.clone();
            smol::spawn(async move {
                adapter
                    .compact_context(input().session, ignore_events())
                    .await
            })
        };
        wait_for("manual compaction admission", || {
            !host.calls_to("/compact").is_empty()
        })
        .await;
        emit(
            &host,
            "session.compaction.ended",
            json!({"sessionID":PROVIDER,"reason":"manual","text":"summary"}),
        );
        assert!(adapter.live(THREAD).unwrap().state.lock().turn.is_some());
        emit(
            &host,
            "session.execution.succeeded",
            json!({"sessionID":PROVIDER}),
        );
        compact.await.unwrap();
        adapter.stop_session(THREAD.into()).await.unwrap();
    });
}

#[test]
fn steering_honors_changed_model_settings_and_rejects_an_idle_session() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        let turn = start(&adapter, &host, ignore_events()).await;
        let request = SteerTurnInput {
            session_id: THREAD.into(),
            cwd: "/owned/work".into(),
            model: "opencode:fixture/other".into(),
            model_settings: Some(
                [
                    (String::from("variant"), String::from("high")),
                    (String::from("agent"), String::from("plan")),
                ]
                .into(),
            ),
            text: "owned steering".into(),
            attachments: None,
        };
        adapter.steer_turn(request.clone()).await.unwrap();
        let model = host.calls_to("/model").pop().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(model.body.as_ref().unwrap()).unwrap(),
            json!({"model":{"providerID":"fixture","id":"other","variant":"high"}})
        );
        let agent = host.calls_to("/agent").pop().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(agent.body.as_ref().unwrap()).unwrap(),
            json!({"agent":"plan"})
        );
        let prompt = host.calls_to("/prompt").pop().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(prompt.body.as_ref().unwrap()).unwrap()["delivery"],
            "steer"
        );
        emit(
            &host,
            "session.execution.succeeded",
            json!({"sessionID":PROVIDER}),
        );
        turn.await.unwrap();
        assert!(adapter.steer_turn(request).await.is_err());
        assert_eq!(host.calls_to("/prompt").len(), 2);
        adapter.stop_session(THREAD.into()).await.unwrap();
    });
}

#[test]
fn text_prompt_reuses_warm_server_denies_tools_and_cleans_owned_state() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        adapter.warmup_text("/owned/work".into()).await.unwrap();
        assert_eq!(host.spawns().len(), 1);
        let bound = Arc::new(Mutex::new(None));
        let prompt = {
            let adapter = adapter.clone();
            let bound = bound.clone();
            smol::spawn(async move {
                adapter
                    .run_text_prompt(TextPromptInput {
                        cwd: "/owned/work".into(),
                        model: Some("opencode:fixture/free".into()),
                        prompt: "one fixture".into(),
                        on_thread_id: Some(Arc::new(move |id| *bound.lock() = Some(id))),
                        ..Default::default()
                    })
                    .await
            })
        };
        wait_for("text prompt", || !host.calls_to("/prompt").is_empty()).await;
        emit(
            &host,
            "session.text.ended",
            json!({"sessionID":PROVIDER,"assistantMessageID":"msg_answer","ordinal":0,"text":"owned text"}),
        );
        emit(
            &host,
            "session.execution.succeeded",
            json!({"sessionID":PROVIDER}),
        );
        assert_eq!(prompt.await.unwrap(), "owned text");
        assert_eq!(host.spawns().len(), 1);
        assert_eq!(bound.lock().as_deref(), Some(PROVIDER));
        assert!(
            host.http_calls().iter().any(|call| call.method == "PATCH"
                && serde_json::from_str::<Value>(call.body.as_deref().unwrap_or("null")).unwrap()
                    ["permissions"][0]["effect"]
                    == "deny")
        );
        assert!(adapter.inner.text_threads.lock().is_empty());
        assert!(adapter.inner.resumes.lock().is_empty());
        assert_eq!(host.watched_children(), 0);
        assert_eq!(host.watched_streams(), 0);
    });
}

#[test]
fn text_prompt_rejects_empty_output_and_stop_releases_an_active_text_turn() {
    smol::block_on(async {
        let host = host();
        let adapter = adapter(&host);
        let prompt = {
            let adapter = adapter.clone();
            smol::spawn(async move {
                adapter
                    .run_text_prompt(TextPromptInput {
                        cwd: "/owned/work".into(),
                        model: Some("opencode:fixture/free".into()),
                        prompt: "fixture".into(),
                        ..Default::default()
                    })
                    .await
            })
        };
        wait_for("empty text prompt", || !host.calls_to("/prompt").is_empty()).await;
        emit(
            &host,
            "session.execution.succeeded",
            json!({"sessionID":PROVIDER}),
        );
        assert!(
            prompt
                .await
                .unwrap_err()
                .to_string()
                .contains("no assistant text")
        );
        let count = host.calls_to("/prompt").len();
        let prompt = {
            let adapter = adapter.clone();
            smol::spawn(async move {
                adapter
                    .run_text_prompt(TextPromptInput {
                        cwd: "/owned/work".into(),
                        model: Some("opencode:fixture/free".into()),
                        prompt: "fixture".into(),
                        ..Default::default()
                    })
                    .await
            })
        };
        wait_for("active text prompt", || {
            host.calls_to("/prompt").len() > count
        })
        .await;
        adapter.stop_text_prompt().await.unwrap();
        assert!(prompt.await.is_err());
        assert_eq!(host.watched_children(), 0);
        assert_eq!(host.watched_streams(), 0);
        assert!(adapter.inner.live.lock().is_empty());
    });
}
