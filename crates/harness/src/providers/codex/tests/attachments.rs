//! Port of src/integrations/harness/providers/codex/codexAttachments.test.ts.
//! The TypeScript built attachments with `prepareAttachments`; here they are
//! built the way it would: images up to the embed limit carry their bytes,
//! everything else carries only its path.

use serde_json::{Value, json};

use monocode_core::attachment::{
    ATTACHMENT_ONLY_PROMPT, Attachment, PromptContentBlock, kind_from_mime, mime_from_name,
    prompt_blocks,
};
use monocode_core::harness::RuntimeMode;
use monocode_core::harness_event::{SendTurnInput, SteerTurnInput};

use super::fake::*;

const S: &str = "issue174";

fn run(test: impl std::future::Future<Output = ()>) {
    smol::block_on(test);
}

/// `prepareAttachments(await attachmentsFromPaths([`/tmp/issue174/${name}`]))`.
fn prepared(name: &str) -> Vec<Attachment> {
    let path = format!("/tmp/issue174/{name}");
    let mime_type = mime_from_name(name);
    let large = name.ends_with("large.png");
    let kind = kind_from_mime(&mime_type);
    let embeds = !large && monocode_core::attachment::is_vision_image(&mime_type);
    vec![Attachment {
        id: name.into(),
        name: name.into(),
        kind,
        size: if large { 21 * 1024 * 1024 } else { 100 },
        path: Some(path),
        data: embeds.then(|| "aW1hZ2U=".to_string()),
        mime_type,
        ..Default::default()
    }]
}

/// A fake app-server that answers every request at once, like the
/// TypeScript `writeChild` mock.
fn auto_harness(keep_turn_open: bool) -> Harness {
    let h = Harness::new();
    *h.fake.on_write.lock() = Some(Box::new(move |wire, session_id, message| {
        let Some(id) = message
            .get("id")
            .filter(|_| message.get("method").is_some())
        else {
            return;
        };
        let method = message["method"].as_str().unwrap_or("");
        let result = match method {
            "thread/start" => json!({ "thread": { "id": "thr_repro" } }),
            "turn/start" => json!({ "turn": { "id": "turn_repro", "status": "inProgress" } }),
            _ => json!({}),
        };
        wire.push(session_id, json!({ "id": id, "result": result }));
        if method == "turn/start" {
            wire.push(
                session_id,
                json!({ "method": "turn/started", "params": { "turn": { "id": "turn_repro" } } }),
            );
            if !keep_turn_open {
                wire.push(
                    session_id,
                    json!({ "method": "turn/completed", "params": { "turn": { "id": "turn_repro", "status": "completed" } } }),
                );
            }
        }
    }));
    h
}

fn turn_input(text: &str, attachments: Vec<Attachment>) -> SendTurnInput {
    SendTurnInput {
        session: session_input(S, RuntimeMode::Supervised),
        text: text.into(),
        attachments: Some(attachments),
    }
}

/// The `input` the adapter sent with `method`, or the error it failed with.
async fn outbound(method: &str, text: &str, attachments: Vec<Attachment>) -> Result<Value, String> {
    let steer = method == "turn/steer";
    let h = auto_harness(steer);
    let events = Events::default();
    let result = if !steer {
        send(&h, turn_input(text, attachments), &events, None)
            .await
            .map_err(|error| error.to_string())
    } else {
        let running = send(&h, turn_input("Initial request", Vec::new()), &events, None);
        wait_for("turn", || events.has("turn.started")).await;
        let steered = h
            .adapter
            .sessions()
            .steer_turn(SteerTurnInput {
                session_id: S.into(),
                cwd: "/repo".into(),
                model: "codex:gpt-5.4".into(),
                model_settings: None,
                text: text.into(),
                attachments: Some(attachments),
            })
            .await
            .map_err(|error| error.to_string());
        complete_turn(&h, S, "turn_repro");
        running.await.unwrap();
        steered
    };
    h.adapter.sessions().stop_session(S).await.unwrap();
    result?;
    Ok(h.find_method(method)
        .map(|message| message["params"]["input"].clone())
        .unwrap_or(Value::Null))
}

#[test]
fn forwards_each_document_and_image_path() {
    for method in ["turn/start", "turn/steer"] {
        for name in [
            "report.pdf",
            "small.md",
            "server.log",
            "large.png",
            "drawing.svg",
        ] {
            run(async {
                let files = prepared(name);
                assert_eq!(files.len(), 1);
                let blocks = prompt_blocks("Read attached", &files).unwrap();
                assert!(
                    matches!(blocks[1], PromptContentBlock::ResourceLink { .. }),
                    "{name}"
                );
                let input = outbound(method, "Read attached", files.clone())
                    .await
                    .unwrap();
                assert!(
                    input
                        .to_string()
                        .contains(files[0].path.as_deref().unwrap()),
                    "{method} {name}: {input}"
                );
            });
        }
    }
}

#[test]
fn sends_an_attachment_only_pdf_message() {
    for method in ["turn/start", "turn/steer"] {
        run(async {
            let files = prepared("report.pdf");
            let input = outbound(method, "", files.clone()).await.unwrap();
            assert!(!input.is_null(), "{method}");
            assert!(
                input
                    .to_string()
                    .contains(files[0].path.as_deref().unwrap())
            );
        });
    }
}

#[test]
fn preserves_a_normal_png() {
    for method in ["turn/start", "turn/steer"] {
        run(async {
            let input = outbound(method, "Read attached", prepared("screenshot.png"))
                .await
                .unwrap();
            assert_eq!(
                input,
                json!([
                    { "type": "text", "text": "Read attached" },
                    { "type": "image", "url": "data:image/png;base64,aW1hZ2U=" },
                ]),
                "{method}"
            );
        });
    }
}

#[test]
fn uses_native_local_image_inputs_for_images_without_embedded_bytes() {
    for method in ["turn/start", "turn/steer"] {
        run(async {
            let input = outbound(method, "", prepared("large.png")).await.unwrap();
            assert_eq!(
                input,
                json!([
                    { "type": "text", "text": ATTACHMENT_ONLY_PROMPT },
                    { "type": "localImage", "path": "/tmp/issue174/large.png" },
                ]),
                "{method}"
            );
        });
    }
}

#[test]
fn preserves_mixed_image_and_document_inputs() {
    for method in ["turn/start", "turn/steer"] {
        run(async {
            let mut files = prepared("screenshot.png");
            files.extend(prepared("report.pdf"));
            let input = outbound(method, "Review both", files).await.unwrap();
            assert_eq!(
                input,
                json!([
                    { "type": "text", "text": "Review both" },
                    { "type": "image", "url": "data:image/png;base64,aW1hZ2U=" },
                    { "type": "text", "text": "Attached file (read from disk): \"/tmp/issue174/report.pdf\"" },
                ]),
                "{method}"
            );
        });
    }
}

#[test]
fn rejects_an_attachment_with_no_source() {
    let pattern = regex::Regex::new(r"report\.pdf.*no local file path").unwrap();
    for method in ["turn/start", "turn/steer"] {
        run(async {
            let files: Vec<Attachment> = prepared("report.pdf")
                .into_iter()
                .map(|file| Attachment { path: None, ..file })
                .collect();
            let error = outbound(method, "Review", files).await.unwrap_err();
            assert!(pattern.is_match(&error), "{method}: {error}");
        });
    }
}
