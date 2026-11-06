use super::*;
use gpui::{HeadlessAppContext, size};
use monocode_core::{
    agent::Settings,
    store::Store,
    tool::{ToolCall, ToolState},
    transcript::Row,
};

fn row(role: &str, text: impl Into<String>) -> Row {
    Row {
        role: role.into(),
        text: text.into(),
        first: true,
        sent_at: None,
    }
}
fn tool(name: &str, input: Value, output: Value) -> Row {
    let mut call = ToolCall::new(name, &format!("monocode:{name}"));
    call.input = input.to_string();
    call.output = output.to_string();
    call.state = ToolState::Completed;
    row("tool", serde_json::to_string(&call).unwrap())
}

#[test]
#[ignore = "Requires Metal; writes subagent UI screenshots using an in-memory fixture"]
fn thread_ui_shots() {
    let directory = std::env::var("THREAD_UI_SHOTS")
        .unwrap_or_else(|_| "/private/tmp/monocode-thread-shots".into());
    std::fs::create_dir_all(&directory).unwrap();
    let mut store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let cwd = "/thread-ui-fixture";
    let mut lead = store.create("claude", cwd).unwrap();
    let mut review = store.create("codex", cwd).unwrap();
    let mut tests = store.create("claude", cwd).unwrap();
    let mut thread = store.create("claude", cwd).unwrap();
    lead.title = "Review live thread ownership".into();
    review.title = "Ownership review".into();
    tests.title = "Write the tests".into();
    thread.title = "Refactor sidebar rows".into();
    for session in [&mut lead, &mut review, &mut tests, &mut thread] {
        store.title(session.id, &session.title).unwrap();
        session.has_messages = true;
    }
    store.write_rows(lead.id,0,&[
        row("user","Audit thread ownership. Start a Codex subagent for the review and one to write tests, and open a separate thread for the sidebar refactor so I can take it over."),
        tool("thread_start",json!({"prompt":"Review child ownership in shell/threads.rs and report problems.","title":review.title,"harness":"codex"}),json!({"threadId":review.id,"harness":"codex","model":"gpt-6.1"})),
        tool("thread_start",json!({"prompt":"Write tests for thread ownership.","title":tests.title}),json!({"threadId":tests.id,"harness":"claude","model":"claude-opus-5"})),
        tool("new_thread",json!({"prompt":"Refactor the sidebar rows.","title":thread.title}),json!({"sessionId":thread.id})),
        row("assistant","Two subagents report back to me here; the sidebar refactor is its own thread for you to continue."),
    ]).unwrap();
    store.write_rows(review.id,0,&[
        row("user","Review child ownership in shell/threads.rs and report problems."),
        row("reasoning","Reading shell/threads.rs and app_commands.rs."),
        row("assistant","Two problems so far: a stopped lead can still queue a send, and closing a child doesn't release its waiters. Checking the restart path next."),
    ]).unwrap();
    store
        .write_rows(
            tests.id,
            0,
            &[
                row("user", "Write tests for thread ownership."),
                row(
                    "assistant",
                    "Added four tests covering ownership, stop and close.",
                ),
            ],
        )
        .unwrap();
    store
        .write_rows(
            thread.id,
            0,
            &[
                row("user", "Refactor the sidebar rows."),
                row("assistant", "Started on the row layout."),
            ],
        )
        .unwrap();
    for (child, model) in [(&review, "gpt-6.1"), (&tests, "claude-opus-5")] {
        store.save_started_link(child.id, lead.id).unwrap();
        store.save_thread_parent(child.id, lead.id).unwrap();
        store
            .save_agent_settings(
                child.id,
                &Settings {
                    model: Some(model.into()),
                    turns: [(
                        1,
                        monocode_core::agent::TurnInfo {
                            provider: child.provider.clone(),
                            sender: Some(lead.id),
                            ..Default::default()
                        },
                    )]
                    .into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    // new_thread: started by the lead, but the user's own.
    store.save_started_link(thread.id, lead.id).unwrap();
    store
        .save_agent_settings(
            lead.id,
            &Settings {
                model: Some("claude-opus-5".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let mut workspace = Workspace::default();
    workspace.add_project(&lead.cwd);
    workspace.open(lead.id);
    let saved = serde_json::to_string(&workspace).unwrap();
    let (lead_id, review_id, tests_id, thread_id) = (lead.id, review.id, tests.id, thread.id);
    let store = Arc::new(Mutex::new(store));
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(gpui_apple::AppleTextSystem::new()),
        Arc::new(crate::assets::Assets),
        || {
            Ok(Some(Box::new(
                gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
            )))
        },
    );
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        crate::popover::bind(cx);
    });
    let window = cx
        .open_window(size(px(1280.), px(820.)), move |_, cx| {
            cx.new(|cx| {
                Shell::new(
                    store,
                    vec![lead, review, tests, thread],
                    cwd.into(),
                    Some(saved),
                    cx,
                )
            })
        })
        .unwrap();
    window
        .update(&mut cx, |shell, _, cx| {
            for id in [review_id, tests_id] {
                shell
                    .ensure_session(id, cx)
                    .unwrap()
                    .update(cx, |v, cx| v.load(cx));
            }
            // The review is still working; the tests are done.
            shell.sessions[&review_id].update(cx, |v, _| v.busy = true);
            shell.thread_session_changed(review_id, cx);
            shell.thread_session_changed(tests_id, cx);
        })
        .unwrap();
    let shot = |cx: &mut HeadlessAppContext, name: &str| {
        for _ in 0..4 {
            cx.run_until_parked();
            cx.advance_clock(Duration::from_millis(400));
            cx.update_window(window.into(), |_, w, _| w.refresh())
                .unwrap();
        }
        cx.run_until_parked();
        cx.update_window(window.into(), |_, w, _| w.refresh())
            .unwrap();
        cx.capture_screenshot(window.into())
            .unwrap()
            .save(std::path::Path::new(&directory).join(format!("{name}.png")))
            .unwrap();
    };
    shot(&mut cx, "lead");
    window
        .update(&mut cx, |shell, window, cx| {
            shell.open(review_id, window, cx)
        })
        .unwrap();
    shot(&mut cx, "subagent-open");
    // Another chat in front: the lead's tree folds back to its summary.
    window
        .update(&mut cx, |shell, window, cx| {
            shell.open(thread_id, window, cx)
        })
        .unwrap();
    shot(&mut cx, "other-thread");
    // Full two-line cards, the lead in front again.
    window
        .update(&mut cx, |shell, window, cx| {
            shell.workspace.compact_session_cards = false;
            shell.open(lead_id, window, cx);
            shell.invalidate_chrome(cx);
        })
        .unwrap();
    shot(&mut cx, "lead-full-cards");
}
