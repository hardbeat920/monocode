use super::*;

const FEED: &[u8] = include_bytes!("fixtures/feed.json");
const KEY: &str = include_str!("fixtures/public-key.txt");
const PACKAGE: &[u8] = include_bytes!("fixtures/package.txt");
fn config(version: &str) -> Config {
    Config::new(
        "https://updates.example.test/native-preview/latest.json",
        KEY,
        version,
    )
    .unwrap()
}

#[test]
fn signed_tauri_key_and_packages_work_for_each_native_target() {
    for target in [
        "darwin-aarch64-app",
        "darwin-x86_64-app",
        "windows-x86_64-nsis",
        "linux-x86_64-appimage",
        "linux-x86_64-deb",
        "linux-x86_64-rpm",
    ] {
        let update = verify_feed(FEED, &config("0.16.0-preview.1"), target)
            .unwrap()
            .unwrap();
        assert_eq!(update.version.to_string(), "0.16.0-preview.2");
        verify(PACKAGE, &update.artifact.signature, KEY).unwrap();
        assert!(verify(b"tampered package", &update.artifact.signature, KEY).is_err());
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(PACKAGE).unwrap();
        update.verify_file(file.path()).unwrap();
        file.as_file_mut().set_len(0).unwrap();
        assert!(update.verify_file(file.path()).is_err());
    }
}
#[test]
fn modified_feed_cannot_redirect_a_valid_package_or_invent_a_new_version() {
    let mut envelope: serde_json::Value = serde_json::from_slice(FEED).unwrap();
    let mut payload: serde_json::Value = serde_json::from_slice(
        &STANDARD
            .decode(envelope["payload"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    payload["version"] = serde_json::json!("99.0.0");
    envelope["payload"] = serde_json::json!(STANDARD.encode(serde_json::to_vec(&payload).unwrap()));
    assert!(verify_feed(
        &serde_json::to_vec(&envelope).unwrap(),
        &config("0.16.0-preview.1"),
        "darwin-aarch64-app"
    )
    .unwrap_err()
    .contains("signature verification failed"));
}
#[test]
fn equal_older_and_final_versions_are_never_downgraded_to_a_preview() {
    for current in ["0.16.0-preview.2", "0.16.0-preview.10", "0.16.0", "0.17.0"] {
        assert!(verify_feed(FEED, &config(current), "darwin-aarch64-app")
            .unwrap()
            .is_none());
    }
}
#[test]
fn another_architecture_is_not_used_when_a_target_is_missing() {
    assert!(
        verify_feed(FEED, &config("0.16.0-preview.1"), "windows-aarch64-nsis")
            .unwrap_err()
            .contains("No native update package")
    );
}
#[test]
fn wrong_application_and_channel_are_rejected_even_in_a_signed_payload() {
    let envelope: Envelope = serde_json::from_slice(FEED).unwrap();
    let payload = STANDARD.decode(envelope.payload).unwrap();
    for field in ["app_id", "channel"] {
        let mut manifest: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        manifest[field] = serde_json::json!("production-tauri");
        assert!(select(
            serde_json::from_value(manifest).unwrap(),
            &config("0.16.0-preview.1"),
            "darwin-aarch64-app"
        )
        .unwrap_err()
        .contains("different application or release channel"));
    }
}
#[test]
fn update_endpoints_require_https_and_a_valid_embedded_key() {
    for endpoint in [
        "http://updates.example.test/latest.json",
        "file:///tmp/update",
        "https://user:password@example.test/latest.json",
    ] {
        assert!(Config::new(endpoint, KEY, "0.16.0").is_err());
    }
    assert!(Config::new(
        "https://updates.example.test/latest.json",
        "invalid key",
        "0.16.0"
    )
    .is_err());
}
#[test]
fn unsigned_tauri_production_feed_is_never_accepted_as_native() {
    assert!(verify_feed(
        br#"{"version":"99.0.0","platforms":{}}"#,
        &config("0.16.0-preview.1"),
        "darwin-aarch64-app"
    )
    .is_err());
}
