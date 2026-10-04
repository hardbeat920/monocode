use base64::Engine;
use serde_json::json;

use super::client::Client;
use super::protocol::{MajorVersion, model_ref, permission_rules, version};
use crate::providers::opencode::test_support::FakeHost;
use monocode_core::harness::RuntimeMode;

#[test]
fn dispatch_rejects_unknown_majors_and_old_v1() {
    assert_eq!(version("opencode v2.0.20").unwrap(), MajorVersion::Two);
    assert_eq!(version("1.14.19").unwrap(), MajorVersion::One);
    assert!(version("1.14.18").is_err());
    assert!(version("3.0.0").is_err());
    assert!(version("not a version").is_err());
}

#[test]
fn uses_pinned_v2_model_and_permission_shapes() {
    assert_eq!(
        model_ref("opencode:openai/gpt-5", Some("high")).unwrap(),
        json!({"providerID":"openai","id":"gpt-5","variant":"high"})
    );
    let supervised = permission_rules(RuntimeMode::Supervised);
    assert_eq!(
        supervised[0],
        json!({"action":"*","resource":"*","effect":"ask"})
    );
    assert_eq!(
        permission_rules(RuntimeMode::FullAccess),
        json!([{"action":"*","resource":"*","effect":"allow"}])
    );
}

#[test]
fn authenticates_http_without_exposing_credentials_in_debug() {
    let host = FakeHost::new();
    host.respond_with(|_| (200, json!({"data":{"id":"ses_fixture"}}).to_string()));
    let client = Client::new(
        "http://127.0.0.1:4096",
        "/owned/work",
        "fixture-password",
        host.children(),
    )
    .unwrap();
    smol::block_on(client.create_session(json!({"location":{"directory":"/owned/work"}}))).unwrap();
    let call = host.http_calls().pop().unwrap();
    assert_eq!(call.url, "http://127.0.0.1:4096/api/session");
    assert_eq!(
        call.headers.as_ref().unwrap()["Authorization"],
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("opencode:fixture-password")
        )
    );
    assert!(!format!("{call:?}").contains("fixture-password"));
    assert!(
        !format!("{call:?}").contains(
            &base64::engine::general_purpose::STANDARD.encode("opencode:fixture-password")
        )
    );
    assert!(!format!("{client:?}").contains("fixture-password"));
    assert!(!call.headers.unwrap().contains_key("x-opencode-directory"));
}

#[test]
fn uses_location_scoped_models_and_pinned_prompt_form_reply() {
    let host = FakeHost::new();
    host.respond_with(|_| (200, json!({"data":[]}).to_string()));
    let client = Client::new(
        "http://127.0.0.1:4096",
        "/owned/work",
        "fixture-password",
        host.children(),
    )
    .unwrap();
    smol::block_on(async {
        client.models().await.unwrap();
        client.prompt("ses_fixture", json!({"text":"hello","files":[{"uri":"data:image/png;base64,AAAA","name":"picture.png"}]})).await.unwrap();
        client
            .reply_form(
                "ses_fixture",
                "frm_fixture",
                json!({"choice":["one","two"],"count":3}),
            )
            .await
            .unwrap();
        client
            .reply_permission("ses_child", "per_fixture", "once")
            .await
            .unwrap();
    });
    let calls = host.http_calls();
    let query: Vec<_> = url::Url::parse(&calls[0].url)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    assert_eq!(
        query,
        vec![("location[directory]".into(), "/owned/work".into())]
    );
    assert!(calls[1].url.ends_with("/api/session/ses_fixture/prompt"));
    let body: serde_json::Value = serde_json::from_str(calls[1].body.as_ref().unwrap()).unwrap();
    assert_eq!(body["text"], "hello");
    assert!(body.get("model").is_none());
    assert!(
        calls[2]
            .url
            .ends_with("/api/session/ses_fixture/form/frm_fixture/reply")
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(calls[2].body.as_ref().unwrap()).unwrap(),
        json!({"answer":{"choice":["one","two"],"count":3}})
    );
    assert!(
        calls[3]
            .url
            .ends_with("/api/session/ses_child/permission/per_fixture/reply")
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(calls[3].body.as_ref().unwrap()).unwrap(),
        json!({"decision":"once"})
    );
}

#[test]
fn refuses_non_loopback_servers_and_preserves_native_errors() {
    let host = FakeHost::new();
    assert!(
        Client::new(
            "https://127.0.0.1:4096",
            "/owned",
            "password",
            host.children()
        )
        .is_err()
    );
    assert!(Client::new("http://example.com", "/owned", "password", host.children()).is_err());
    assert!(
        Client::new(
            "http://user:password@127.0.0.1",
            "/owned",
            "password",
            host.children()
        )
        .is_err()
    );
    host.respond_with(|_| {
        (
            400,
            json!({"_tag":"InvalidRequestError","message":"Invalid selected model"}).to_string(),
        )
    });
    let client = Client::new(
        "http://127.0.0.1:4096",
        "/owned",
        "password",
        host.children(),
    )
    .unwrap();
    let error = smol::block_on(client.create_session(json!({}))).unwrap_err();
    assert!(error.to_string().contains("Invalid selected model"));
}
