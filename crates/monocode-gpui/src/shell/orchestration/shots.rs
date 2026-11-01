use super::*;
use gpui::{HeadlessAppContext, size};
use monocode_core::{
    orchestration::{Assignment, Plan, Task},
    store::Store,
    tool::{ToolCall, ToolState},
    transcript::Row,
};
use std::sync::{Arc, Mutex};

fn row(role: &str, text: impl Into<String>) -> Row {
    Row {
        role: role.into(),
        text: text.into(),
        first: true,
        sent_at: None,
    }
}
fn task(id: &str, title: &str, files: &[&str], after: &[&str]) -> Task {
    Task {
        id: id.into(),
        title: title.into(),
        prompt: format!("{title}. Run the focused checks and report what changed."),
        files: files.iter().map(|f| (*f).into()).collect(),
        depends_on: after.iter().map(|f| (*f).into()).collect(),
        provider: None,
        model: None,
        difficulty: None,
    }
}

#[test]
#[ignore = "Requires Metal; writes orchestration card screenshots using an in-memory fixture"]
fn orchestration_ui_shots() {
    let directory = std::env::var("ORCHESTRATION_UI_SHOTS")
        .unwrap_or_else(|_| "/private/tmp/monocode-orchestration-shots".into());
    std::fs::create_dir_all(&directory).unwrap();
    let plan = Plan {
        title: "Split the settings page".into(),
        summary: "Move each settings section into its own module, then wire the pages back together and verify the build.".into(),
        tasks: vec![
            task("models", "Extract the models section", &["src/settings/models"], &[]),
            task("keys", "Extract the keybindings section", &["src/settings/keys"], &[]),
            task("mcp", "Extract the MCP section", &["src/settings/mcp"], &[]),
            task("wire", "Wire the sections into the page", &["src/settings/page.rs"], &["models", "keys", "mcp"]),
            task("docs", "Update the settings docs", &["docs/settings.md"], &["wire"]),
        ],
    };
    let mut store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let mut lead = store.create("claude", "/orchestration-fixture").unwrap();
    lead.title = "Split the settings page".into();
    lead.has_messages = true;
    store.title(lead.id, &lead.title).unwrap();
    let mut call = ToolCall::new("plan-call", "mcp__monocode__orchestration_plan");
    call.input = serde_json::to_string(&plan).unwrap();
    call.state = ToolState::Completed;
    store
        .write_rows(
            lead.id,
            0,
            &[
                row(
                    "user",
                    "Split the settings page into modules, one agent per section.",
                ),
                row("tool", serde_json::to_string(&call).unwrap()),
                row(
                    "assistant",
                    "Three extractions run in parallel; wiring waits for them, docs come last.",
                ),
            ],
        )
        .unwrap();
    let mut workers = Vec::new();
    for (title, reply) in [
        (
            "Agent · Extract the models section",
            "Moved the models section into src/settings/models. cargo check passes.",
        ),
        (
            "Agent · Extract the keybindings section",
            "Extracting the keybinding table.",
        ),
        (
            "Agent · Extract the MCP section",
            "Provider error: usage limit reached",
        ),
    ] {
        let mut worker = store.create("codex", "/orchestration-fixture").unwrap();
        worker.title = title.into();
        worker.has_messages = true;
        store.title(worker.id, title).unwrap();
        store
            .write_rows(
                worker.id,
                0,
                &[row("user", "Assignment"), row("assistant", reply)],
            )
            .unwrap();
        workers.push(worker);
    }
    let mut run = Run::new(
        plan.clone(),
        Some("plan-call".into()),
        "/orchestration-fixture".into(),
        "codex".into(),
        Settings {
            model: Some("gpt-6.1".into()),
            ..Default::default()
        },
    );
    let state = |session: i64, state: State, report: &str| Assignment {
        session: Some(session),
        state,
        report: report.into(),
        outside_scope: Vec::new(),
        launched: true,
        ..Default::default()
    };
    run.tasks[0] = state(
        workers[0].id,
        State::Done,
        "Moved the models section. cargo check passes.",
    );
    run.tasks[0].outside_scope = vec!["src/settings/mod.rs".into()];
    run.tasks[1] = state(workers[1].id, State::Running, "");
    run.tasks[2] = state(
        workers[2].id,
        State::Failed,
        "Provider error: usage limit reached",
    );
    run.dispatch();
    let lead_id = lead.id;
    let mut workspace = Workspace::default();
    workspace.add_project(&lead.cwd);
    workspace.open(lead.id);
    let saved = serde_json::to_string(&workspace).unwrap();
    let mut sessions = vec![lead];
    sessions.extend(workers);
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
        .open_window(size(px(1280.), px(860.)), move |_, cx| {
            cx.new(|cx| {
                Shell::new(
                    store,
                    sessions,
                    "/orchestration-fixture".into(),
                    Some(saved),
                    cx,
                )
            })
        })
        .unwrap();
    let shot = |cx: &mut HeadlessAppContext, name: &str| {
        for _ in 0..4 {
            cx.run_until_parked();
            cx.advance_clock(Duration::from_millis(400));
            cx.update_window(window.into(), |_, w, _| w.refresh())
                .unwrap();
        }
        cx.capture_screenshot(window.into())
            .unwrap()
            .save(std::path::Path::new(&directory).join(format!("{name}.png")))
            .unwrap();
    };
    // Review: the plan as the planning turn left it.
    window
        .update(&mut cx, |shell, _, cx| {
            shell.sessions[&lead_id].update(cx, |v, cx| {
                v.orchestration_plan = Some((Some("plan-call".into()), plan.clone()));
                cx.notify();
            });
            shell.push_orchestration_views(lead_id, cx);
        })
        .unwrap();
    shot(&mut cx, "review");
    // Running: one done (with an edit outside its scope), one working, one
    // failed, the rest waiting on them.
    window
        .update(&mut cx, |shell, _, cx| {
            shell.sessions[&lead_id].update(cx, |v, _| v.orchestration_plan = None);
            shell
                .orchestrations
                .by_lead
                .insert(lead_id, vec![run.clone()]);
            shell.push_orchestration_views(lead_id, cx);
            let transcript = shell.sessions[&lead_id].read(cx).transcript.clone();
            transcript.update(cx, |t, cx| t.toggle_orchestration_task("plan-call", 0, cx));
        })
        .unwrap();
    shot(&mut cx, "running");
    // After a restart the run waits to be resumed.
    window
        .update(&mut cx, |shell, _, cx| {
            shell
                .orchestrations
                .by_lead
                .insert(lead_id, vec![run.clone().restored()]);
            shell.push_orchestration_views(lead_id, cx);
        })
        .unwrap();
    shot(&mut cx, "paused");
}
