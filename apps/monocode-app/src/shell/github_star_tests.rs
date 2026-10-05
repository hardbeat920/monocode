//! GitHub prompt requests use a controlled backend, never the real API.

use super::*;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use gpui::{AppContext as _, TestAppContext, VisualTestContext};
use monocode_engine::inbox::backend::InboxBackend;
use monocode_engine::inbox::client::InboxClient;
use monocode_settings::Kv;
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::sync::Arc;

type Reply = Result<Value, String>;
struct Request {
    command: String,
    reply: oneshot::Sender<Reply>,
}

#[derive(Default)]
struct FakeGithub {
    calls: Mutex<Vec<String>>,
    requests: Mutex<Vec<Request>>,
}

impl FakeGithub {
    fn count(&self, command: &str) -> usize {
        self.calls
            .lock()
            .iter()
            .filter(|call| *call == command)
            .count()
    }

    fn answer(&self, command: &str, result: Reply) {
        let mut requests = self.requests.lock();
        let index = requests
            .iter()
            .position(|request| request.command == command)
            .expect("the fixture request should be pending");
        let request = requests.remove(index);
        request.reply.send(result).expect("request still awaited");
    }
}

impl InboxBackend for FakeGithub {
    fn invoke(&self, command: &str, args: Value) -> BoxFuture<'static, Reply> {
        assert!(matches!(
            command,
            "github_monocode_star_status" | "github_star_monocode"
        ));
        assert_eq!(args, json!({}));
        self.calls.lock().push(command.to_owned());
        let (reply, response) = oneshot::channel();
        self.requests.lock().push(Request {
            command: command.to_owned(),
            reply,
        });
        Box::pin(async move {
            response
                .await
                .unwrap_or_else(|_| Err("fixture request dropped".into()))
        })
    }

    fn fetch_media(&self, _: &str) -> BoxFuture<'static, Result<Vec<u8>, String>> {
        panic!("the GitHub prompt must not request media")
    }
}

fn mount(
    cx: &mut TestAppContext,
    kv: Kv,
    backend: Arc<FakeGithub>,
) -> (Entity<Prompt>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
        Inbox::init(
            InboxClient::new(backend, kv.clone(), cx.background_executor().clone()),
            cx,
        );
    });
    let (prompt, cx) = cx.add_window_view(|_, cx| Prompt::new(kv, cx));
    draw(cx);
    (prompt, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
}

#[gpui::test]
fn star_status_is_read_only_and_deduplicates_pending_refreshes(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeGithub::default());
    let (prompt, cx) = mount(cx, Kv::in_memory(), backend.clone());
    for _ in 0..3 {
        prompt.update(cx, |prompt, cx| prompt.refresh(cx));
    }
    cx.run_until_parked();
    assert_eq!(backend.count("github_monocode_star_status"), 1);
    assert_eq!(backend.count("github_star_monocode"), 0);
    backend.answer("github_monocode_star_status", Ok(json!("notStarred")));
    draw(cx);
    assert_eq!(
        prompt.read_with(cx, |prompt, _| prompt.phase),
        Phase::Visible
    );
    assert!(cx.debug_bounds("github-star-action").is_some());
    prompt.update(cx, |prompt, cx| prompt.refresh(cx));
    cx.run_until_parked();
    backend.answer("github_monocode_star_status", Ok(json!("starred")));
    draw(cx);
    assert_eq!(
        prompt.read_with(cx, |prompt, _| prompt.phase),
        Phase::Hidden
    );
    assert!(cx.debug_bounds("github-star-action").is_none());
    assert_eq!(backend.count("github_monocode_star_status"), 2);
    assert_eq!(backend.count("github_star_monocode"), 0);
}

#[gpui::test]
fn a_dismissed_prompt_ignores_late_status_and_new_mounts(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeGithub::default());
    let kv = Kv::in_memory();
    let (prompt, cx) = mount(cx, kv.clone(), backend.clone());
    prompt.update(cx, |prompt, cx| prompt.dismiss(cx));
    assert_eq!(kv.get_item(DISMISSED).as_deref(), Some("1"));
    backend.answer("github_monocode_star_status", Ok(json!("notStarred")));
    draw(cx);
    assert_eq!(
        prompt.read_with(cx, |prompt, _| prompt.phase),
        Phase::Hidden
    );
    prompt.update(cx, |prompt, cx| prompt.refresh(cx));
    let remounted = cx.update(|_, cx| cx.new(|cx| Prompt::new(kv, cx)));
    cx.run_until_parked();
    assert_eq!(
        remounted.read_with(cx, |prompt, _| prompt.phase),
        Phase::Hidden
    );
    assert_eq!(backend.count("github_monocode_star_status"), 1);
    assert_eq!(backend.count("github_star_monocode"), 0);
}

#[gpui::test]
fn real_star_button_clicks_share_one_pending_mock_mutation(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeGithub::default());
    let kv = Kv::in_memory();
    let (prompt, cx) = mount(cx, kv.clone(), backend.clone());
    backend.answer("github_monocode_star_status", Ok(json!("notStarred")));
    draw(cx);
    let bounds = cx.debug_bounds("github-star-action").expect("star button");
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        prompt.read_with(cx, |prompt, _| prompt.phase),
        Phase::Starring
    );
    assert_eq!(backend.count("github_star_monocode"), 1);
    prompt.update(cx, |prompt, cx| prompt.refresh(cx));
    assert_eq!(backend.count("github_monocode_star_status"), 1);
    backend.answer("github_star_monocode", Ok(Value::Null));
    draw(cx);
    assert_eq!(
        prompt.read_with(cx, |prompt, _| prompt.phase),
        Phase::Hidden
    );
    assert!(cx.debug_bounds("github-star-action").is_none());
    assert!(kv.get_item(DISMISSED).is_none());
}

#[gpui::test]
fn an_older_status_reply_cannot_unlock_a_pending_star_click(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeGithub::default());
    let (prompt, cx) = mount(cx, Kv::in_memory(), backend.clone());
    backend.answer("github_monocode_star_status", Ok(json!("notStarred")));
    draw(cx);
    prompt.update(cx, |prompt, cx| prompt.refresh(cx));
    cx.run_until_parked();
    assert_eq!(backend.count("github_monocode_star_status"), 2);
    let bounds = cx.debug_bounds("github-star-action").expect("star button");
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    backend.answer("github_monocode_star_status", Ok(json!("notStarred")));
    draw(cx);
    assert_eq!(
        prompt.read_with(cx, |prompt, _| prompt.phase),
        Phase::Starring
    );
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(backend.count("github_star_monocode"), 1);
    backend.answer("github_star_monocode", Ok(Value::Null));
    draw(cx);
    assert_eq!(
        prompt.read_with(cx, |prompt, _| prompt.phase),
        Phase::Hidden
    );
}

#[gpui::test]
fn an_older_status_reply_cannot_reopen_a_successfully_starred_prompt(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeGithub::default());
    let (prompt, cx) = mount(cx, Kv::in_memory(), backend.clone());
    backend.answer("github_monocode_star_status", Ok(json!("notStarred")));
    draw(cx);
    prompt.update(cx, |prompt, cx| prompt.refresh(cx));
    cx.run_until_parked();
    let bounds = cx.debug_bounds("github-star-action").expect("star button");
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    backend.answer("github_star_monocode", Ok(Value::Null));
    draw(cx);
    assert_eq!(
        prompt.read_with(cx, |prompt, _| prompt.phase),
        Phase::Hidden
    );
    backend.answer("github_monocode_star_status", Ok(json!("notStarred")));
    draw(cx);
    assert_eq!(
        prompt.read_with(cx, |prompt, _| prompt.phase),
        Phase::Hidden
    );
    assert!(cx.debug_bounds("github-star-action").is_none());
    assert_eq!(backend.count("github_star_monocode"), 1);
}

#[gpui::test]
fn dismissal_during_a_failed_mock_star_request_keeps_the_prompt_hidden(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeGithub::default());
    let kv = Kv::in_memory();
    let (prompt, cx) = mount(cx, kv.clone(), backend.clone());
    backend.answer("github_monocode_star_status", Ok(json!("notStarred")));
    draw(cx);
    let bounds = cx.debug_bounds("github-star-action").expect("star button");
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    prompt.update(cx, |prompt, cx| prompt.dismiss(cx));
    backend.answer("github_star_monocode", Err("mock permission denied".into()));
    draw(cx);
    assert_eq!(
        prompt.read_with(cx, |prompt, _| prompt.phase),
        Phase::Hidden
    );
    assert_eq!(kv.get_item(DISMISSED).as_deref(), Some("1"));
    assert_eq!(backend.count("github_star_monocode"), 1);
}

#[gpui::test]
fn an_unavailable_initial_status_hides_the_prompt_without_mutation(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeGithub::default());
    let (prompt, cx) = mount(cx, Kv::in_memory(), backend.clone());
    backend.answer("github_monocode_star_status", Ok(json!("unavailable")));
    draw(cx);
    assert_eq!(
        prompt.read_with(cx, |prompt, _| prompt.phase),
        Phase::Hidden
    );
    assert_eq!(backend.count("github_star_monocode"), 0);
    assert!(cx.debug_bounds("github-star-action").is_none());
}
