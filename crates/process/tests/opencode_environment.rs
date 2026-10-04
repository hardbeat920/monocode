use std::collections::HashMap;

use monocode_process::harness::{HarnessHost, harness_spawn_with_env};

#[test]
fn provider_environment_overrides_reject_other_providers_unknown_names_and_nul_values() {
    let host = HarnessHost::default();
    for (provider, name, value) in [
        ("codex", "OPENCODE_PASSWORD", "owned-fixture-password"),
        ("opencode", "PATH", "owned-fixture-password"),
        ("opencode", "OPENCODE_PASSWORD", "owned-fixture-password\0"),
    ] {
        let result = harness_spawn_with_env(
            &host,
            &std::env::temp_dir(),
            None,
            "env-rejection-owned".into(),
            "not-an-executable".into(),
            Vec::new(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            None,
            Some(provider.into()),
            None,
            HashMap::from([(name.into(), value.into())]),
        );
        assert_eq!(
            result.unwrap_err(),
            "Unsupported provider environment override"
        );
    }
}
