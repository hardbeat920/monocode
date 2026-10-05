//! The text runner (codexText.ts), titles (codexTitle.ts), git text
//! (codexGit.ts), and model discovery (codexCatalog.ts) over an app-server
//! that answers at once. The TypeScript had no tests for these modules
//! beyond `parseCodexModelList`, which tests/protocol.rs covers.

use std::sync::Arc;

use futures::FutureExt;
use parking_lot::Mutex;
use serde_json::{Value, json};

use monocode_core::block::ModelSettings;
use monocode_core::harness::HarnessId;

use crate::core::registry::{HarnessAdapter, TextPromptInput, TitleInput};
use crate::core::task::{AbortSignal, BoxFuture};

use super::super::git::{GitContexts, GitRangeContext, GitStagedContext};
use super::super::text::{pick_effort_for_test, pick_model_for_test};
use super::fake::*;

const TEXT_ID: &str = "monocode-codex-text";

fn run(test: impl std::future::Future<Output = ()>) {
    smol::block_on(test);
}

/// An app-server that answers every request, and replies to turn/start
/// with `reply` streamed as two deltas.
fn text_harness(reply: &'static str, options: HarnessOptions) -> Harness {
    let h = Harness::with(options);
    *h.fake.on_write.lock() = Some(Box::new(move |wire, session_id, message| {
        let Some(id) = message
            .get("id")
            .filter(|_| message.get("method").is_some())
        else {
            return;
        };
        let method = message["method"].as_str().unwrap_or("");
        let result = match method {
            "thread/start" | "thread/resume" => json!({ "thread": { "id": "thr_text" } }),
            "turn/start" => json!({ "turn": { "id": "turn_text" } }),
            _ => json!({}),
        };
        wire.push(session_id, json!({ "id": id, "result": result }));
        if method == "turn/start" {
            let half = reply.len() / 2;
            for delta in [&reply[..half], &reply[half..]] {
                wire.push(
                    session_id,
                    json!({ "method": "item/agentMessage/delta", "params": { "delta": delta } }),
                );
            }
            wire.push(
                session_id,
                json!({ "method": "turn/completed", "params": { "turn": { "id": "turn_text", "status": "completed" } } }),
            );
        }
    }));
    h
}

fn prompt(text: &str) -> TextPromptInput {
    TextPromptInput {
        cwd: "/repo".into(),
        prompt: text.into(),
        ..Default::default()
    }
}

#[test]
fn runs_a_text_prompt_on_its_own_app_server() {
    run(async {
        let h = text_harness("Hello world", HarnessOptions::default());
        let output = h
            .adapter
            .run_text_prompt(prompt("Say hello"))
            .await
            .unwrap();
        assert_eq!(output, "Hello world");

        let sent: Vec<(String, Value)> = h.fake.sent.lock().clone();
        assert!(sent.iter().all(|(session, _)| session == TEXT_ID));
        let initialize = h.find_method("initialize").unwrap();
        assert_eq!(initialize["params"]["clientInfo"]["name"], "monocode-text");
        let turn = h.find_method("turn/start").unwrap();
        let model = pick_model_for_test(h.adapter.text(), None);
        assert_eq!(turn["params"]["model"], json!(model));
        assert_eq!(turn["params"]["approvalPolicy"], "untrusted");
        assert_eq!(
            turn["params"]["input"],
            json!([{ "type": "text", "text": "Say hello" }])
        );
        // The TypeScript drops the process after each prompt.
        assert!(h.fake.kills.lock().iter().any(|id| id == TEXT_ID));
    });
}

#[test]
fn refuses_a_blank_model_and_an_aborted_signal() {
    run(async {
        let h = text_harness("unused", HarnessOptions::default());
        let error = h
            .adapter
            .run_text_prompt(TextPromptInput {
                model: Some("  ".into()),
                ..prompt("x")
            })
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "The selected Codex model is unavailable."
        );

        let signal = AbortSignal::new();
        signal.abort();
        let error = h
            .adapter
            .run_text_prompt(TextPromptInput {
                signal: Some(signal),
                ..prompt("x")
            })
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "This operation was aborted");
        assert!(h.find_method("turn/start").is_none());
    });
}

#[test]
fn picks_the_requested_model_and_a_supported_effort() {
    let h = Harness::new();
    assert_eq!(
        pick_model_for_test(h.adapter.text(), Some(" gpt-x ")),
        "gpt-x"
    );
    let settings: ModelSettings = [("reasoningEffort".to_string(), "high".to_string())].into();
    // An unknown model has no options, so any requested effort stands.
    assert_eq!(
        pick_effort_for_test(h.adapter.text(), "unknown-model", Some(&settings)),
        "high"
    );
    assert_eq!(
        pick_effort_for_test(h.adapter.text(), "unknown-model", None),
        "low"
    );
}

#[test]
fn generates_a_session_title() {
    run(async {
        let h = text_harness(
            r#"{"title":"Fix the login redirect"}"#,
            HarnessOptions::default(),
        );
        let title = h
            .adapter
            .generate_title(TitleInput {
                session_id: "s".into(),
                cwd: "/repo".into(),
                message: "the login page redirects forever".into(),
                provider_account_id: None,
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(title.title, "Fix the login redirect");
    });
}

struct FakeGit {
    staged: Mutex<Vec<String>>,
}

impl GitContexts for FakeGit {
    fn staged_context(&self, cwd: String) -> BoxFuture<'static, Result<GitStagedContext, String>> {
        self.staged.lock().push(cwd);
        async {
            Ok(GitStagedContext {
                branch: Some("main".into()),
                summary: "M src/login.rs".into(),
                patch: "@@ -1 +1 @@\n-old\n+new".into(),
            })
        }
        .boxed()
    }

    fn range_context(&self, _cwd: String) -> BoxFuture<'static, Result<GitRangeContext, String>> {
        async {
            Ok(GitRangeContext {
                base: "main".into(),
                head: "fix-login".into(),
                commit_summary: "Fix login redirect\nMore".into(),
                diff_summary: "1 file".into(),
                diff_patch: "diff".into(),
            })
        }
        .boxed()
    }
}

#[test]
fn generates_a_commit_message_from_staged_changes() {
    run(async {
        let git = Arc::new(FakeGit {
            staged: Mutex::new(Vec::new()),
        });
        let h = text_harness(
            r#"{"subject":"Fix login redirect","body":"Stop the loop."}"#,
            HarnessOptions {
                git: Some(git.clone()),
                ..Default::default()
            },
        );
        let message = h
            .adapter
            .generate_commit_message("/repo".into(), None)
            .await
            .unwrap();
        assert_eq!(message, "Fix login redirect\n\nStop the loop.");
        assert_eq!(*git.staged.lock(), vec!["/repo".to_string()]);
        let turn = h.find_method("turn/start").unwrap();
        assert!(
            turn["params"]["input"][0]["text"]
                .as_str()
                .unwrap()
                .contains("M src/login.rs")
        );
    });
}

#[test]
fn reports_what_the_model_said_when_a_commit_message_does_not_parse() {
    run(async {
        let h = text_harness(
            "I cannot   do\nthat",
            HarnessOptions {
                git: Some(Arc::new(FakeGit {
                    staged: Mutex::new(Vec::new()),
                })),
                ..Default::default()
            },
        );
        let error = h
            .adapter
            .generate_commit_message("/repo".into(), None)
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Could not generate a commit message. Model replied: I cannot do that"
        );
    });
}

#[test]
fn falls_back_to_the_commit_summary_for_pull_request_text() {
    run(async {
        let h = text_harness(
            "not json",
            HarnessOptions {
                git: Some(Arc::new(FakeGit {
                    staged: Mutex::new(Vec::new()),
                })),
                ..Default::default()
            },
        );
        let content = h
            .adapter
            .generate_pr_content("/repo".into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(content.title, "Fix login redirect");
        assert_eq!(content.body, "Fix login redirect\nMore");
        assert_eq!(
            (content.base.as_str(), content.head.as_str()),
            ("main", "fix-login")
        );
    });
}

#[test]
fn fails_git_text_without_a_git_context() {
    run(async {
        let h = Harness::new();
        let error = h
            .adapter
            .generate_commit_message("/repo".into(), None)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "Git context is not available");
    });
}

/// An app-server for the model probe.
fn probe_harness(account: Value) -> Harness {
    let h = Harness::new();
    *h.fake.on_write.lock() = Some(Box::new(move |wire, session_id, message| {
        let Some(id) = message
            .get("id")
            .filter(|_| message.get("method").is_some())
        else {
            return;
        };
        assert!(
            session_id.starts_with("monocode-codex-probe-"),
            "{session_id}"
        );
        let result = match message["method"].as_str().unwrap_or("") {
            "account/read" => account.clone(),
            "model/list" if message["params"]["cursor"] == "page2" => json!({
                "data": [{ "model": "gpt-5.6-terra", "displayName": "gpt-5.6-terra", "isDefault": true }],
                "nextCursor": null,
            }),
            "model/list" => json!({
                "data": [
                    { "model": "gpt-5.6-luna", "displayName": "gpt-5.6-luna",
                      "supportedReasoningEfforts": ["low", "high"] },
                    { "model": "hidden-model", "hidden": true },
                ],
                "nextCursor": "page2",
            }),
            _ => json!({}),
        };
        wire.push(session_id, json!({ "id": id, "result": result }));
    }));
    h
}

#[test]
fn refreshes_the_catalog_from_every_model_list_page() {
    run(async {
        let h =
            probe_harness(json!({ "account": { "type": "chatgpt" }, "requiresOpenaiAuth": true }));
        h.adapter.refresh_catalog().await.unwrap();
        let catalog = h.catalog.read();
        assert!(catalog.has_live_catalog(HarnessId::Codex));
        let ids: Vec<&str> = catalog
            .models_for(HarnessId::Codex)
            .iter()
            .map(|model| model.id.as_str())
            .collect();
        assert_eq!(ids, ["codex:gpt-5.6-terra", "codex:gpt-5.6-luna"]);
        drop(catalog);
        let spawn = h.fake.spawns.lock()[0].clone();
        assert_eq!(spawn.cwd, "/home/test");
        assert_eq!(spawn.args, ["app-server"]);
        assert!(h.fake.kills.lock()[0].starts_with("monocode-codex-probe-"));
    });
}

#[test]
fn reports_an_unauthenticated_cli() {
    run(async {
        let h = probe_harness(json!({ "account": null, "requiresOpenaiAuth": true }));
        let error = h
            .adapter
            .catalog()
            .discover_codex_models(Some("/repo"))
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Codex CLI is not authenticated. Run `codex login` and try again."
        );
        // A refresh logs the failure and leaves the bundled list in place.
        h.adapter.refresh_catalog().await.unwrap();
        assert!(!h.catalog.has_live_catalog(HarnessId::Codex));
    });
}
