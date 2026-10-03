//! Ports of the Agent CLI and provider scope cases of SettingsView.test.ts.

use gpui::{Focusable as _, TestAppContext};
use monocode_core::Platform;
use monocode_core::harness::HARNESSES;
use monocode_core::models::{HIDDEN_PICKER_PROVIDERS_KEY, LAST_MODEL_KEY};
use monocode_core::settings::SettingsSectionId;

use super::*;
use crate::settings::binary_control::BinaryControl;
use crate::settings::providers::ProvidersSection;
use crate::settings::store::PROVIDER_BINARY_PATHS_KEY;

const LINUX: Platform = Platform::Linux;

fn section(page: &Entity<SettingsPage>, cx: &mut VisualTestContext) -> Entity<ProvidersSection> {
    let SectionBody::Providers(section) = body(page, cx) else {
        panic!("providers section");
    };
    section
}

fn control(
    page: &Entity<SettingsPage>,
    harness: HarnessId,
    cx: &mut VisualTestContext,
) -> Entity<BinaryControl> {
    let section = section(page, cx);
    section.read_with(cx, |section, _| section.binary(harness).cloned().unwrap())
}

fn stored_paths(setup: &Setup) -> serde_json::Value {
    serde_json::from_str(
        &setup
            .kv
            .get_item(PROVIDER_BINARY_PATHS_KEY)
            .unwrap_or_else(|| "{}".into()),
    )
    .unwrap()
}

/// Opens the CLI details for `title` (unless open) and saves `path`.
fn save_path(cx: &mut VisualTestContext, control: &Entity<BinaryControl>, title: &str, path: &str) {
    let id = control.read_with(cx, |control, _| control.is_open());
    if !id {
        click(cx, &format!("binary:{title}"));
    }
    if !control.read_with(cx, |control, _| control.is_editing()) {
        let provider = title.to_lowercase();
        click(cx, &format!("button:edit-path-{provider}"));
    }
    let draft = control.read_with(cx, |control, _| control.draft().clone());
    cx.update(|window, cx| draft.update(cx, |draft, cx| draft.set_value(path, window, cx)));
    let provider = title.to_lowercase();
    click(cx, &format!("button:save-path-{provider}"));
}

#[gpui::test]
fn validates_and_stores_codex_and_opencode_binary_overrides(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let fail_auto_codex = Rc::new(RefCell::new(false));
    let fail = fail_auto_codex.clone();
    *setup.host.inspect.borrow_mut() = Some(Box::new(move |provider, path| {
        if path == Some("/bad/codex") {
            return Err("Codex failed to start".into());
        }
        if provider == HarnessId::Codex && path.is_none() && *fail.borrow() {
            return Err("Codex auto-detection failed".into());
        }
        let path = path.map(str::to_string).unwrap_or_else(|| match provider {
            HarnessId::Opencode => "/auto/opencode".into(),
            _ => "/auto/codex".into(),
        });
        let version = if path.contains("opencode") {
            "opencode 1.18.32"
        } else {
            "codex-cli 0.156.1"
        };
        Ok(BinaryInspection {
            path,
            version: Some(version.into()),
            error: None,
        })
    }));
    let (page, cx) = mount(cx, SettingsSectionId::Providers, &setup);
    let codex = control(&page, HarnessId::Codex, cx);
    let opencode = control(&page, HarnessId::Opencode, cx);

    click(cx, "binary:Codex");
    click(cx, "button:open-location-codex");
    assert_eq!(
        setup.host.revealed.borrow().as_slice(),
        &["/auto/codex".to_string()]
    );

    save_path(cx, &codex, "Codex", "/opt/codex/bin/codex");
    save_path(cx, &opencode, "OpenCode", "/opt/opencode/bin/opencode");
    assert_eq!(
        stored_paths(&setup),
        serde_json::json!({
            "codex": "/opt/codex/bin/codex",
            "opencode": "/opt/opencode/bin/opencode",
        })
    );
    click(cx, "binary:Codex");
    codex.read_with(cx, |control, _| {
        let inspection = control.inspection().unwrap();
        assert_eq!(inspection.path, "/opt/codex/bin/codex");
        assert_eq!(inspection.version.as_deref(), Some("codex-cli 0.156.1"));
        assert_eq!(control.status(), "Restart required");
    });
    click(cx, "binary:Codex");
    click(cx, "binary:OpenCode");
    opencode.read_with(cx, |control, _| {
        let inspection = control.inspection().unwrap();
        assert_eq!(inspection.path, "/opt/opencode/bin/opencode");
        assert_eq!(inspection.version.as_deref(), Some("opencode 1.18.32"));
    });
    click(cx, "binary:OpenCode");

    save_path(cx, &codex, "Codex", "/bad/codex");
    assert_eq!(
        codex.read_with(cx, |control, _| control.error().map(str::to_string)),
        Some("Codex failed to start".into())
    );
    assert_eq!(stored_paths(&setup)["codex"], "/opt/codex/bin/codex");
    click(cx, "button:cancel-path-codex");
    assert!(exists(cx, "button:retry-codex"));

    *fail_auto_codex.borrow_mut() = true;
    save_path(cx, &codex, "Codex", "");
    assert_eq!(
        codex.read_with(cx, |control, _| control.error().map(str::to_string)),
        Some("Codex auto-detection failed".into())
    );
    assert_eq!(stored_paths(&setup)["codex"], "/opt/codex/bin/codex");

    *fail_auto_codex.borrow_mut() = false;
    save_path(cx, &codex, "Codex", "");
    assert_eq!(stored_paths(&setup).get("codex"), None);
    click(cx, "binary:Codex");
    codex.read_with(cx, |control, _| {
        assert_eq!(control.inspection().unwrap().path, "/auto/codex");
        assert_eq!(control.status(), "Auto-detected");
    });
}

#[gpui::test]
fn offers_manual_auto_detect_retry_when_a_cli_is_missing(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (_, cx) = mount(cx, SettingsSectionId::Providers, &setup);
    click(cx, "binary:Codex");
    assert!(exists(cx, "button:retry-codex"));
    click(cx, "button:retry-codex");
    assert_eq!(
        setup.host.inspections.borrow().as_slice(),
        &[(HarnessId::Codex, None), (HarnessId::Codex, None)]
    );
}

#[gpui::test]
fn shows_path_details_for_every_agent_cli(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (_, cx) = mount(cx, SettingsSectionId::Providers, &setup);
    // Nothing is inspected until a popover opens.
    assert!(setup.host.inspections.borrow().is_empty());
    for harness in HARNESSES {
        assert!(
            exists(cx, &format!("binary:{}", harness.title())),
            "{harness}"
        );
    }
}

#[gpui::test]
fn returns_focus_to_the_cli_trigger_when_the_details_popover_closes(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (page, cx) = mount(cx, SettingsSectionId::Providers, &setup);
    let codex = control(&page, HarnessId::Codex, cx);
    click(cx, "binary:Codex");
    assert!(codex.read_with(cx, |control, _| control.is_open()));
    keys(cx, "escape");
    assert!(!codex.read_with(cx, |control, _| control.is_open()));
    let trigger = codex.read_with(cx, |control, cx| control.focus_handle(cx));
    assert!(cx.update(|window, _| trigger.is_focused(window)));
}

#[gpui::test]
fn keeps_location_failures_separate_from_cli_check_failures(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    *setup.host.inspect.borrow_mut() = Some(Box::new(|_, _| {
        Ok(BinaryInspection {
            path: "/auto/codex".into(),
            version: Some("codex-cli 0.156.1".into()),
            error: None,
        })
    }));
    *setup.host.reveal_failure.borrow_mut() = Some("File manager unavailable".into());
    let (page, cx) = mount(cx, SettingsSectionId::Providers, &setup);
    let codex = control(&page, HarnessId::Codex, cx);
    click(cx, "binary:Codex");
    click(cx, "button:open-location-codex");
    codex.read_with(cx, |control, _| {
        assert_eq!(control.error(), None);
        assert_eq!(control.reveal_error(), Some("File manager unavailable"));
    });
}

#[gpui::test]
fn inherits_the_global_default_provider_and_picker_visibility_in_project_scope(
    cx: &mut TestAppContext,
) {
    let setup = Setup::new(LINUX);
    *setup.host.installed.borrow_mut() = vec![HarnessId::Claude, HarnessId::Cursor];
    setup.kv.set_item(
        LAST_MODEL_KEY,
        r#"{"harness":"claude","model":"claude:opus-5"}"#,
    );
    setup
        .kv
        .set_item(HIDDEN_PICKER_PROVIDERS_KEY, r#"["cursor"]"#);
    let (page, cx) = mount(cx, SettingsSectionId::Providers, &setup);
    click(cx, "select:Provider defaults scope");
    click(cx, "option:Provider defaults scope:repo");
    let section = section(&page, cx);
    assert_eq!(
        section.read_with(cx, |section, _| section.scope().to_string()),
        "/repo"
    );
    let rows = section.read_with(cx, |section, cx| section.rows(cx));
    let row = |harness| {
        rows.iter()
            .find(|row| row.harness == harness)
            .unwrap()
            .clone()
    };
    // A project with no overrides shows the inherited global default.
    assert!(row(HarnessId::Claude).is_default);
    assert!(exists(cx, "button:use-default-claude"));
    // Picker visibility also inherits the global setting.
    assert!(row(HarnessId::Claude).in_picker);
    assert!(!row(HarnessId::Cursor).in_picker);
    // The project toggle cannot turn a globally hidden provider back on.
    assert!(row(HarnessId::Cursor).picker_locked);
    assert!(exists(cx, "switch:Show Cursor in the model picker"));
}

#[gpui::test]
fn sets_the_global_default_provider_and_hides_a_provider(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    *setup.host.installed.borrow_mut() = vec![HarnessId::Claude, HarnessId::Cursor];
    let (_, cx) = mount(cx, SettingsSectionId::Providers, &setup);
    click(cx, "button:use-default-cursor");
    let last = setup.kv.get_item(LAST_MODEL_KEY).unwrap();
    assert!(last.starts_with(r#"{"harness":"cursor""#), "{last}");
    click(cx, "switch:Show Claude Code in the model picker");
    assert_eq!(
        setup.kv.get_item(HIDDEN_PICKER_PROVIDERS_KEY).as_deref(),
        Some(r#"["claude"]"#)
    );
}
