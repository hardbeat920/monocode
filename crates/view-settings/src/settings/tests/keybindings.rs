//! Ports of the keybinding cases of SettingsView.test.ts and of
//! QuickComposerShortcutEditor.test.ts.

use gpui::TestAppContext;
use monocode_core::Platform;
use monocode_core::settings::{
    KEYBINDING_OVERRIDES_KEY, KeybindingOverride, QUICK_COMPOSER_ENABLED_KEY,
    QUICK_COMPOSER_SHORTCUT_KEY, SettingsSectionId,
};
use monocode_settings::settings_store as ss;

use super::*;
use crate::settings::shortcut_editor::ShortcutEditor;

fn editor(
    page: &Entity<SettingsPage>,
    command: &str,
    cx: &mut VisualTestContext,
) -> Entity<ShortcutEditor> {
    let SectionBody::Keybindings(section) = body(page, cx) else {
        panic!("keybindings section");
    };
    section.read_with(cx, |section, _| section.editor(command).cloned().unwrap())
}

fn value(editor: &Entity<ShortcutEditor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.value())
}

fn error(editor: &Entity<ShortcutEditor>, cx: &mut VisualTestContext) -> Option<String> {
    editor.read_with(cx, |editor, _| editor.error().map(str::to_string))
}

fn command_modifiers() -> Modifiers {
    Modifiers {
        platform: true,
        ..Modifiers::default()
    }
}

// SettingsView.test.ts, on a platform without ⌘ labels.

#[gpui::test]
fn records_a_custom_keybinding_from_the_key_cell(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Linux);
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    click(cx, "shortcut:App: Search");
    keys(cx, "ctrl-shift-m");
    assert_eq!(
        setup.kv.get_item(KEYBINDING_OVERRIDES_KEY).as_deref(),
        Some(r#"{"App: Search":{"shortcut":"Control+Shift+KeyM"}}"#)
    );
    assert_eq!(value(&editor(&page, "App: Search", cx), cx), "Ctrl+Shift+M");
}

#[gpui::test]
fn lets_tab_leave_the_recorder_and_keeps_cmd_delete_recordable(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Linux);
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    let search = editor(&page, "App: Search", cx);
    click(cx, "shortcut:App: Search");
    assert!(exists(cx, "shortcut-hint"));
    keys(cx, "tab");
    assert!(!search.read_with(cx, |editor, _| editor.is_recording()));
    assert!(!exists(cx, "shortcut-hint"));

    click(cx, "shortcut:App: Search");
    keys(cx, "cmd-delete");
    assert_eq!(
        setup.kv.get_item(KEYBINDING_OVERRIDES_KEY).as_deref(),
        Some(r#"{"App: Search":{"shortcut":"Command+Delete"}}"#)
    );
}

#[gpui::test]
fn surfaces_a_failed_menu_update_instead_of_dropping_it(cx: &mut TestAppContext) {
    // The TypeScript case made localStorage throw. Kv cannot fail a write,
    // so this checks the other failure the row reports: the macOS menus.
    let setup = Setup::new(Platform::Mac);
    *setup.host.overrides_failure.borrow_mut() = Some("Could not update the menus".into());
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    click(cx, "shortcut:App: Search");
    keys(cx, "cmd-shift-y");
    assert_eq!(
        error(&editor(&page, "App: Search", cx), cx).as_deref(),
        Some("Could not update the menus")
    );
    assert!(exists(cx, "shortcut-error:App: Search"));
    assert_eq!(setup.host.overrides.borrow().len(), 1);
}

#[gpui::test]
fn records_an_alt_shortcut_on_a_keybinding_row(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Linux);
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    click(cx, "shortcut:App: Search");
    keys(cx, "alt-m");
    assert_eq!(
        setup.kv.get_item(KEYBINDING_OVERRIDES_KEY).as_deref(),
        Some(r#"{"App: Search":{"shortcut":"Option+KeyM"}}"#)
    );
    assert_eq!(value(&editor(&page, "App: Search", cx), cx), "Alt+M");
}

#[gpui::test]
fn disables_and_restores_an_individual_keybinding(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Linux);
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    click(cx, "shortcut:App: Search");
    keys(cx, "backspace");
    assert_eq!(
        setup.kv.get_item(KEYBINDING_OVERRIDES_KEY).as_deref(),
        Some(r#"{"App: Search":{"disabled":true}}"#)
    );
    assert_eq!(value(&editor(&page, "App: Search", cx), cx), "Disabled");
    click(cx, "shortcut-reset:App: Search");
    assert_eq!(setup.kv.get_item(KEYBINDING_OVERRIDES_KEY), None);
    assert_eq!(value(&editor(&page, "App: Search", cx), cx), "Ctrl+K");
}

#[gpui::test]
fn rejects_a_chord_another_command_owns(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Linux);
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    click(cx, "shortcut:App: Search");
    // Ctrl+P is App: Go to File's default.
    keys(cx, "ctrl-p");
    assert_eq!(
        error(&editor(&page, "App: Search", cx), cx).as_deref(),
        Some("Error: Already used by App: Go to File")
    );
    assert_eq!(setup.kv.get_item(KEYBINDING_OVERRIDES_KEY), None);
}

#[gpui::test]
fn filters_the_table(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Linux);
    let (_, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    assert!(exists(cx, "shortcut:App: Search"));
    click(cx, "filter-keybindings");
    cx.simulate_input("terminal");
    draw(cx);
    assert!(!exists(cx, "shortcut:App: Search"));
    assert!(exists(cx, "shortcut:Terminal: New"));
}

// QuickComposerShortcutEditor.test.ts, on macOS.

const QUICK: &str = "shortcut:quick composer";

fn quick(page: &Entity<SettingsPage>, cx: &mut VisualTestContext) -> Entity<ShortcutEditor> {
    editor(page, "App: Quick Composer", cx)
}

#[gpui::test]
fn records_a_global_shortcut_persists_it_and_restores_the_default(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Mac);
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    let editor = quick(&page, cx);
    assert_eq!(value(&editor, cx), "⌘⇧Space");
    click(cx, QUICK);
    assert_eq!(value(&editor, cx), "Record…");
    modifiers(cx, command_modifiers());
    assert_eq!(value(&editor, cx), "⌘");
    keys(cx, "q");
    assert_eq!(
        setup.host.quick_composer.borrow().as_slice(),
        &[(true, Some("Command+KeyQ".to_string()))]
    );
    assert_eq!(
        setup.kv.get_item(QUICK_COMPOSER_SHORTCUT_KEY).as_deref(),
        Some("Command+KeyQ")
    );
    assert_eq!(value(&editor, cx), "⌘Q");
    modifiers(cx, Modifiers::default());

    click(cx, "shortcut-reset:quick composer");
    assert_eq!(
        setup.kv.get_item(QUICK_COMPOSER_SHORTCUT_KEY).as_deref(),
        Some("Command+Shift+Space")
    );
    assert_eq!(value(&editor, cx), "⌘⇧Space");
}

#[gpui::test]
fn keeps_the_previous_shortcut_when_native_registration_fails(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Mac);
    setup
        .host
        .quick_composer_failures
        .borrow_mut()
        .push_back("Shortcut is in use".into());
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    let editor = quick(&page, cx);
    click(cx, QUICK);
    keys(cx, "cmd-q");
    assert_eq!(value(&editor, cx), "⌘⇧Space");
    assert_eq!(setup.kv.get_item(QUICK_COMPOSER_SHORTCUT_KEY), None);
    assert_eq!(error(&editor, cx).as_deref(), Some("Shortcut is in use"));
}

#[gpui::test]
fn accepts_control_plus_one_key(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Mac);
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    let editor = quick(&page, cx);
    click(cx, QUICK);
    modifiers(
        cx,
        Modifiers {
            control: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(value(&editor, cx), "⌃");
    keys(cx, "y");
    assert_eq!(
        setup.host.quick_composer.borrow().as_slice(),
        &[(true, Some("Control+KeyY".to_string()))]
    );
    assert_eq!(value(&editor, cx), "⌃Y");
}

#[gpui::test]
fn refuses_a_chord_another_command_already_owns(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Mac);
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    let editor = quick(&page, cx);
    click(cx, QUICK);
    // Command+K is App: Search's default.
    keys(cx, "cmd-k");
    assert!(
        error(&editor, cx)
            .unwrap()
            .contains("Already used by App: Search")
    );
    assert_eq!(setup.kv.get_item(QUICK_COMPOSER_SHORTCUT_KEY), None);
    // A rejected chord never reaches native registration.
    assert!(setup.host.quick_composer.borrow().is_empty());
}

#[gpui::test]
fn reserves_its_live_custom_chord_so_no_other_command_can_claim_it(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Mac);
    let (_, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    click(cx, QUICK);
    keys(cx, "cmd-shift-q");
    assert_eq!(
        setup.kv.get_item(QUICK_COMPOSER_SHORTCUT_KEY).as_deref(),
        Some("Command+Shift+KeyQ")
    );
    assert_eq!(
        ss::save_keybinding_override(
            &setup.kv,
            "App: Search",
            &KeybindingOverride {
                disabled: None,
                shortcut: Some("Command+Shift+KeyQ".into()),
            },
            Platform::Mac,
        ),
        Err("Already used by App: Quick Composer".into())
    );
}

#[gpui::test]
fn re_enables_and_re_registers_the_default_when_a_disabled_row_is_reset(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Mac);
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    let editor = quick(&page, cx);
    click(cx, QUICK);
    keys(cx, "backspace");
    assert_eq!(value(&editor, cx), "Disabled");
    assert_eq!(
        setup.kv.get_item(QUICK_COMPOSER_ENABLED_KEY).as_deref(),
        Some("0")
    );
    click(cx, "shortcut-reset:quick composer");
    assert_eq!(
        setup.host.quick_composer.borrow().last(),
        Some(&(true, Some("Command+Shift+Space".to_string())))
    );
    assert_eq!(
        setup.kv.get_item(QUICK_COMPOSER_ENABLED_KEY).as_deref(),
        Some("1")
    );
    assert_eq!(value(&editor, cx), "⌘⇧Space");
}

#[gpui::test]
fn refuses_an_alt_only_global_shortcut_without_registering_it(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Mac);
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    let editor = quick(&page, cx);
    click(cx, QUICK);
    keys(cx, "alt-k");
    assert!(error(&editor, cx).unwrap().contains("Quick Composer needs"));
    assert!(setup.host.quick_composer.borrow().is_empty());
    assert_eq!(setup.kv.get_item(QUICK_COMPOSER_SHORTCUT_KEY), None);
}

#[gpui::test]
fn shows_pressed_keys_without_an_error_and_escape_cancels_recording(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Mac);
    let (page, cx) = mount(cx, SettingsSectionId::Keybindings, &setup);
    let editor = quick(&page, cx);
    click(cx, QUICK);
    assert_eq!(value(&editor, cx), "Record…");
    assert!(exists(cx, "shortcut-hint"));
    keys(cx, "k");
    assert_eq!(value(&editor, cx), "K");
    assert_eq!(error(&editor, cx), None);
    keys(cx, "escape");
    assert_eq!(value(&editor, cx), "⌘⇧Space");
    assert!(!exists(cx, "shortcut-hint"));
    assert!(setup.host.quick_composer.borrow().is_empty());
}
