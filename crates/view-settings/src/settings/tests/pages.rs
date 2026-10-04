//! Ports of the "settings pages" cases of SettingsView.test.ts.

use std::time::Duration;

use gpui::{AppContext as _, Focusable as _, SharedString, TestAppContext, px};
use monocode_core::Platform;
use monocode_core::appearance::{
    CHAT_BACKGROUND_EMPTY_OPACITY_KEY, CHAT_BACKGROUND_PATH_KEY, NEW_THREAD_BACKGROUND_EFFECT_KEY,
    NewThreadBackgroundEffect, UI_SCALE_KEY,
};
use monocode_core::settings::{
    COLLAPSED_PROJECT_RAIL_MODE_KEY, CollapsedProjectRailMode, FILE_TAB_MODE_KEY,
    SETTINGS_SECTIONS, SettingsGroupId, SettingsSectionId, TAB_ANIMATIONS_ENABLED_KEY,
    settings_index,
};

use super::*;
use crate::settings::store::{SESSION_FILTERS_KEY, SOUNDS_KEY};

const LINUX: Platform = Platform::Linux;

fn rendered_ids(page: &Entity<SettingsPage>, cx: &mut VisualTestContext) -> Vec<String> {
    page.read_with(cx, |page, _| {
        page.anchors()
            .ids()
            .into_iter()
            .map(|id| id.to_string())
            .collect()
    })
}

fn revealed(page: &Entity<SettingsPage>, cx: &mut VisualTestContext) -> Option<SharedString> {
    page.read_with(cx, |page, cx| page.revealed(cx))
}

#[test]
fn gives_every_section_a_rail_group() {
    let mut groups: Vec<SettingsGroupId> = Vec::new();
    for section in SETTINGS_SECTIONS {
        if !groups.contains(&section.group) {
            groups.push(section.group);
        }
    }
    assert_eq!(
        groups,
        vec![
            SettingsGroupId::App,
            SettingsGroupId::Agents,
            SettingsGroupId::Workspace
        ]
    );
}

#[test]
fn indexes_each_setting_once() {
    for platform in [Platform::Mac, Platform::Windows, Platform::Linux] {
        let ids: Vec<&str> = settings_index(platform)
            .iter()
            .map(|entry| entry.id)
            .collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), unique.len());
    }
}

#[gpui::test]
fn lets_files_open_as_normal_top_bar_tabs(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (_, cx) = mount(cx, SettingsSectionId::General, &setup);
    assert!(exists(cx, "radio:File tabs:pane"));
    click(cx, "radio:File tabs:workspace");
    assert_eq!(
        setup.kv.get_item(FILE_TAB_MODE_KEY).as_deref(),
        Some("workspace")
    );
}

#[gpui::test]
fn offers_tab_animations_as_an_opt_in(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (_, cx) = mount(cx, SettingsSectionId::General, &setup);
    assert_eq!(setup.kv.get_item(TAB_ANIMATIONS_ENABLED_KEY), None);
    click(cx, "switch:Tab animations");
    assert_eq!(
        setup.kv.get_item(TAB_ANIMATIONS_ENABLED_KEY).as_deref(),
        Some("1")
    );
}

#[gpui::test]
fn sounds_switch_saves_and_plays_on_flip(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (_, cx) = mount(cx, SettingsSectionId::General, &setup);
    click(cx, "switch:Sounds");
    assert_eq!(setup.kv.get_item(SOUNDS_KEY).as_deref(), Some("0"));
}

#[gpui::test]
fn defaults_to_the_icon_rail_and_lets_users_hide_it(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (page, cx) = mount(cx, SettingsSectionId::Appearance, &setup);
    let mode = |page: &Entity<SettingsPage>, cx: &mut VisualTestContext| {
        page.read_with(cx, |page, cx| {
            page.appearance().read(cx).collapsed_project_rail_mode()
        })
    };
    assert_eq!(mode(&page, cx), CollapsedProjectRailMode::Compact);
    click(cx, "radio:Collapsed project rail:hidden");
    assert_eq!(mode(&page, cx), CollapsedProjectRailMode::Hidden);
    assert_eq!(
        setup
            .kv
            .get_item(COLLAPSED_PROJECT_RAIL_MODE_KEY)
            .as_deref(),
        Some("hidden")
    );

    // A fresh page reads the stored mode.
    let (kv, platform, hosts, props) = (
        setup.kv.clone(),
        setup.platform,
        super::hosts(&setup.host),
        setup.props.clone(),
    );
    let reopened = cx.update(|window, cx| {
        cx.new(|cx| {
            SettingsPage::new(
                kv,
                platform,
                hosts,
                SettingsSectionId::Appearance,
                props,
                Default::default(),
                window,
                cx,
            )
        })
    });
    assert_eq!(mode(&reopened, cx), CollapsedProjectRailMode::Hidden);
}

#[gpui::test]
fn sets_interface_scale_from_a_menu_instead_of_a_live_slider(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (page, cx) = mount(cx, SettingsSectionId::Appearance, &setup);
    assert!(!exists(cx, "slider:Interface scale"));
    let SectionBody::Appearance(section) = body(&page, cx) else {
        panic!("appearance section");
    };
    let select = section.read_with(cx, |section, _| section.ui_scale_select().clone());
    let label =
        |cx: &mut VisualTestContext| select.read_with(cx, |select, _| select.trigger_label());
    assert_eq!(label(cx), "Interface scale: 100%");
    click(cx, "select:Interface scale");
    assert!(exists(cx, "option:Interface scale:100%"));
    // 150% is five rows below the selected 100%.
    keys(cx, "down down down down down enter");
    assert_eq!(setup.kv.get_item(UI_SCALE_KEY).as_deref(), Some("1.5"));
    assert_eq!(label(cx), "Interface scale: 150%");
    // The theme follows at once.
    cx.read(|cx| assert_eq!(monocode_ui::Theme::of(cx).ui_scale(), 1.5));
}

#[gpui::test]
fn reports_collapsed_project_rail_changes_to_the_app_shell(cx: &mut TestAppContext) {
    let mut setup = Setup::new(LINUX);
    let calls = Calls::new();
    setup.callbacks.on_collapsed_project_rail_mode_change = calls.callback();
    setup.props.collapsed_project_rail_mode = Some(CollapsedProjectRailMode::Compact);
    let (_, cx) = mount(cx, SettingsSectionId::Appearance, &setup);
    click(cx, "radio:Collapsed project rail:hidden");
    assert_eq!(calls.all(), vec![CollapsedProjectRailMode::Hidden]);
}

#[gpui::test]
fn shows_background_effect_choices_above_scope_when_artwork_is_available(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    setup.kv.set_item(
        CHAT_BACKGROUND_PATH_KEY,
        "/app-data/backgrounds/chat-background.png",
    );
    let (page, cx) = mount(cx, SettingsSectionId::Appearance, &setup);
    let effect = bounds(cx, "new-thread-background-effect-dither");
    let scope = bounds(cx, "radiogroup:Show background on");
    assert!(effect.top() < scope.top());
    click(cx, "new-thread-background-effect-dither");
    assert_eq!(
        setup
            .kv
            .get_item(NEW_THREAD_BACKGROUND_EFFECT_KEY)
            .as_deref(),
        Some("dither")
    );
    let effect = page.read_with(cx, |page, cx| {
        page.appearance()
            .read(cx)
            .settings
            .new_thread_background_effect
    });
    assert_eq!(effect, NewThreadBackgroundEffect::Dither);
    assert_eq!(
        effect.description(),
        "Rebuilds the artwork with a dithered color palette."
    );
}

#[gpui::test]
fn previews_and_restores_haze_with_the_existing_empty_chat_visibility(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    setup
        .kv
        .set_item(CHAT_BACKGROUND_PATH_KEY, "/background.png");
    setup.kv.set_item(CHAT_BACKGROUND_EMPTY_OPACITY_KEY, "0.4");
    let (page, cx) = mount(cx, SettingsSectionId::Appearance, &setup);
    assert_eq!(NewThreadBackgroundEffect::GradientBlur.label(), "Haze");
    assert!(!exists(cx, "gradient-blur-background"));
    click(cx, "new-thread-background-effect-gradient-blur");
    assert!(exists(cx, "gradient-blur-background"));
    assert_eq!(
        setup
            .kv
            .get_item(NEW_THREAD_BACKGROUND_EFFECT_KEY)
            .as_deref(),
        Some("gradient-blur")
    );
    let opacity = page.read_with(cx, |page, cx| {
        page.appearance()
            .read(cx)
            .settings
            .chat_background_empty_opacity
    });
    assert_eq!(opacity, 0.4);

    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.set_section(SettingsSectionId::Providers, window, cx);
            page.set_section(SettingsSectionId::Appearance, window, cx);
        })
    });
    draw(cx);
    assert!(exists(cx, "gradient-blur-background"));
}

#[gpui::test]
fn renders_every_indexed_setting_on_its_page(cx: &mut TestAppContext) {
    let index = settings_index(LINUX);
    let mut sections: Vec<SettingsSectionId> = Vec::new();
    for entry in &index {
        if !sections.contains(&entry.section) {
            sections.push(entry.section);
        }
    }
    let setup = Setup::new(LINUX);
    let (page, cx) = mount(cx, SettingsSectionId::General, &setup);
    for section in sections {
        cx.update(|window, cx| page.update(cx, |page, cx| page.set_section(section, window, cx)));
        draw(cx);
        let mut expected: Vec<String> = index
            .iter()
            .filter(|entry| entry.section == section)
            .map(|entry| entry.id.to_string())
            .collect();
        expected.sort();
        assert_eq!(rendered_ids(&page, cx), expected, "{section:?}");
    }
}

#[gpui::test]
fn shows_mac_and_windows_only_rows_on_their_platforms(cx: &mut TestAppContext) {
    let setup = Setup::new(Platform::Mac);
    let (page, cx) = mount(cx, SettingsSectionId::General, &setup);
    let ids = rendered_ids(&page, cx);
    assert!(ids.contains(&"quick-composer".to_string()));
    assert!(!ids.contains(&"close-to-tray".to_string()));
}

#[gpui::test]
fn only_tags_rows_that_search_can_find(cx: &mut TestAppContext) {
    let index = settings_index(LINUX);
    let setup = Setup::new(LINUX);
    let (page, cx) = mount(cx, SettingsSectionId::General, &setup);
    for section in SETTINGS_SECTIONS.iter().map(|section| section.id) {
        if section == SettingsSectionId::Skills {
            continue;
        }
        cx.update(|window, cx| page.update(cx, |page, cx| page.set_section(section, window, cx)));
        draw(cx);
        for id in rendered_ids(&page, cx) {
            assert!(
                index.iter().any(|entry| entry.id == id),
                "{section:?}: {id}"
            );
        }
    }
}

#[gpui::test]
fn reveals_the_project_notifications_anchor_and_clears_the_highlight(cx: &mut TestAppContext) {
    let mut setup = Setup::new(LINUX);
    setup.props.notification_project_path = Some("/repo".into());
    setup.props.notification_settings_request = 1;
    let (page, cx) = mount(cx, SettingsSectionId::Inbox, &setup);
    page.update(cx, |page, cx| {
        page.set_anchor(Some("project-notifications"), cx)
    });
    draw(cx);
    assert_eq!(
        revealed(&page, cx).as_deref(),
        Some("project-notifications")
    );
    assert!(exists(cx, "flash-row"));

    cx.executor().advance_clock(Duration::from_millis(1800));
    draw(cx);
    assert_eq!(revealed(&page, cx), None);
    assert!(!exists(cx, "flash-row"));

    // A repeated quick action reveals the same project again.
    let mut props = setup.props.clone();
    props.notification_settings_request = 2;
    page.update(cx, |page, cx| page.set_props(props, cx));
    draw(cx);
    assert_eq!(
        revealed(&page, cx).as_deref(),
        Some("project-notifications")
    );
    cx.executor().advance_clock(Duration::from_millis(1800));
    draw(cx);
    assert_eq!(revealed(&page, cx), None);
}

#[gpui::test]
fn scrolls_a_revealed_row_into_view(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (page, cx) = mount_sized(cx, SettingsSectionId::General, &setup, 900., 500.);
    page.update(cx, |page, cx| page.reveal(Some("update".into()), cx));
    draw(cx);
    let offset = page.read_with(cx, |page, _| page.scroll_handle().offset());
    assert!(offset.y < px(0.), "{offset:?}");
    let row = bounds(cx, "setting-id:update");
    let window = cx.update(|window, _| window.viewport_size());
    assert!(row.top() >= px(0.) && row.bottom() <= window.height);
}

#[gpui::test]
fn shows_the_version_and_opens_whats_new(cx: &mut TestAppContext) {
    let mut setup = Setup::new(LINUX);
    *setup.host.version.borrow_mut() = Some("0.7.1".into());
    let calls = Calls::new();
    setup.callbacks.on_open_whats_new = calls.callback();
    let (page, cx) = mount(cx, SettingsSectionId::General, &setup);
    let SectionBody::General(general) = body(&page, cx) else {
        panic!("general section");
    };
    general.read_with(cx, |general, _| {
        assert_eq!(general.snapshot().current_version, "0.7.1");
        assert_eq!(
            general.update_status(),
            "MonoCode updates itself from the release feed."
        );
    });
    click(cx, "button:whats-new");
    assert_eq!(calls.all(), vec!["0.7.1".to_string()]);
}

#[gpui::test]
fn archive_lists_archived_sessions_and_saves_the_sidebar_filter(cx: &mut TestAppContext) {
    let mut setup = Setup::new(LINUX);
    setup.props.sessions = vec![
        SessionSummary {
            id: "old".into(),
            title: "Old".into(),
            harness: HarnessId::Claude,
            updated_at: 1,
            archived: true,
        },
        SessionSummary {
            id: "new".into(),
            title: "codex · New".into(),
            harness: HarnessId::Codex,
            updated_at: 2,
            archived: true,
        },
        SessionSummary {
            id: "live".into(),
            title: "Live".into(),
            harness: HarnessId::Claude,
            updated_at: 3,
            archived: false,
        },
    ];
    let opened = Calls::new();
    let archived = Calls::new();
    setup.callbacks.on_open_session = opened.callback();
    setup.callbacks.on_archive_session = archived.callback();
    let (page, cx) = mount(cx, SettingsSectionId::Archive, &setup);
    let SectionBody::Archive(archive) = body(&page, cx) else {
        panic!("archive section");
    };
    let ids: Vec<String> = archive.read_with(cx, |archive, _| {
        archive
            .archived()
            .into_iter()
            .map(|session| session.id)
            .collect()
    });
    assert_eq!(ids, vec!["new", "old"]);
    assert!(!exists(cx, "archived-session:live"));
    click(cx, "archived-session:new");
    assert_eq!(opened.all(), vec!["new".to_string()]);
    click(cx, "button:unarchive-old");
    assert_eq!(archived.all(), vec![("old".to_string(), false)]);
    click(cx, "switch:Show archived in the sidebar");
    let filters = setup.kv.get_item(SESSION_FILTERS_KEY).unwrap();
    assert!(filters.starts_with(r#"{"showArchived":true"#), "{filters}");
}

#[gpui::test]
fn deletes_an_archived_project_after_confirming(cx: &mut TestAppContext) {
    let mut setup = Setup::new(LINUX);
    setup.host.archived.borrow_mut().push(ArchivedProject {
        path: "/Users/ada/src/old".into(),
        label: "old".into(),
    });
    let deleted = Calls::new();
    let restored = Calls::new();
    setup.callbacks.on_delete_project = deleted.callback();
    setup.callbacks.on_restore_project = restored.callback();
    let (_, cx) = mount(cx, SettingsSectionId::Archive, &setup);
    click(cx, "button:restore-/Users/ada/src/old");
    assert_eq!(restored.all(), vec!["/Users/ada/src/old".to_string()]);
    click(cx, "button:delete-project-/Users/ada/src/old");
    assert!(exists(cx, "remove-project-dialog"));
    keys(cx, "escape");
    assert!(!exists(cx, "remove-project-dialog"));
    click(cx, "button:delete-project-/Users/ada/src/old");
    click(cx, "button:remove-project-confirm");
    assert!(!exists(cx, "remove-project-dialog"));
    assert_eq!(deleted.all(), vec!["/Users/ada/src/old".to_string()]);
}

#[gpui::test]
fn escape_closes_the_page(cx: &mut TestAppContext) {
    let mut setup = Setup::new(LINUX);
    let closed = Calls::new();
    setup.callbacks.on_close = closed.callback();
    let (page, cx) = mount(cx, SettingsSectionId::Chat, &setup);
    cx.update(|window, cx| {
        let focus = page.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    });
    keys(cx, "escape");
    assert_eq!(closed.all().len(), 1);
}

#[gpui::test]
fn restore_defaults_resets_appearance(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    setup
        .kv
        .set_item(monocode_core::appearance::THEME_HUE_KEY, "120");
    setup.kv.set_item(UI_SCALE_KEY, "1.3");
    let (page, cx) = mount(cx, SettingsSectionId::Appearance, &setup);
    click(cx, "button:restore-defaults");
    assert_eq!(
        setup
            .kv
            .get_item(monocode_core::appearance::THEME_HUE_KEY)
            .as_deref(),
        Some("240")
    );
    assert_eq!(setup.kv.get_item(UI_SCALE_KEY).as_deref(), Some("1"));
    let hue = page.read_with(cx, |page, cx| page.appearance().read(cx).settings.theme_hue);
    assert_eq!(hue, 240);
}

#[gpui::test]
fn appearance_changes_update_the_theme_live(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (_, cx) = mount(cx, SettingsSectionId::Appearance, &setup);
    click(cx, "radio:Theme:light");
    assert_eq!(
        setup
            .kv
            .get_item(monocode_core::appearance::SCHEME_KEY)
            .as_deref(),
        Some("light")
    );
    cx.read(|cx| assert!(!monocode_ui::Theme::of(cx).is_dark()));

    click(cx, "swatch:Violet");
    assert_eq!(
        setup
            .kv
            .get_item(monocode_core::appearance::ACCENT_COLOR_KEY)
            .as_deref(),
        Some("#8b5cf6")
    );
    cx.read(|cx| assert!(monocode_ui::Theme::of(cx).colors.user_accent.is_some()));
    click(cx, "swatch:Default");
    assert_eq!(
        setup
            .kv
            .get_item(monocode_core::appearance::ACCENT_COLOR_KEY),
        None
    );
}

#[gpui::test]
fn diff_colors_save_the_palette_and_swap_the_theme_tokens(cx: &mut TestAppContext) {
    use monocode_core::appearance::DIFF_PALETTE_KEY;
    use monocode_ui::DiffPalette;
    let setup = Setup::new(LINUX);
    let (_, cx) = mount(cx, SettingsSectionId::Appearance, &setup);
    let palette = |cx: &mut gpui::VisualTestContext| {
        cx.read(|cx| monocode_ui::Theme::of(cx).appearance.diff_palette)
    };
    assert_eq!(palette(cx), DiffPalette::Default);

    click(cx, "radio:Diff colors:colorblind");
    assert_eq!(
        setup.kv.get_item(DIFF_PALETTE_KEY).as_deref(),
        Some("colorblind")
    );
    assert_eq!(palette(cx), DiffPalette::Colorblind);

    click(cx, "radio:Diff colors:high-contrast");
    assert_eq!(
        setup.kv.get_item(DIFF_PALETTE_KEY).as_deref(),
        Some("high-contrast")
    );
    assert_eq!(palette(cx), DiffPalette::HighContrast);

    click(cx, "button:restore-defaults");
    assert_eq!(
        setup.kv.get_item(DIFF_PALETTE_KEY).as_deref(),
        Some("default")
    );
    assert_eq!(palette(cx), DiffPalette::Default);
}

#[gpui::test]
fn sliders_save_the_value_under_the_pointer(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (_, cx) = mount(cx, SettingsSectionId::Appearance, &setup);
    let track = bounds(cx, "slider:Hue");
    cx.simulate_click(track.center(), gpui::Modifiers::none());
    draw(cx);
    assert_eq!(
        setup
            .kv
            .get_item(monocode_core::appearance::THEME_HUE_KEY)
            .as_deref(),
        Some("180")
    );
    // Dragging past the end clamps to the maximum.
    cx.simulate_mouse_down(
        track.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_move(
        gpui::point(track.right() + px(40.), track.center().y),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_up(
        gpui::point(track.right() + px(40.), track.center().y),
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    draw(cx);
    assert_eq!(
        setup
            .kv
            .get_item(monocode_core::appearance::THEME_HUE_KEY)
            .as_deref(),
        Some("360")
    );
    cx.read(|cx| assert_eq!(monocode_ui::Theme::of(cx).appearance.theme_hue, 360.0));
}

#[gpui::test]
fn the_custom_accent_picker_saves_a_typed_hex(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (page, cx) = mount(cx, SettingsSectionId::Appearance, &setup);
    click(cx, "swatch:Custom color");
    assert!(exists(cx, "color-square"));
    let SectionBody::Appearance(section) = body(&page, cx) else {
        panic!("appearance section");
    };
    let accent = section.read_with(cx, |section, _| section.accent_picker().clone());
    let picker = accent.read_with(cx, |accent, _| accent.picker().cloned().unwrap());
    let hex = picker.read_with(cx, |picker, _| picker.hex_input().clone());
    cx.update(|window, cx| {
        hex.update(cx, |hex, cx| {
            hex.focus(window, cx);
            hex.select_all(window, cx);
        })
    });
    cx.simulate_input("#123456");
    draw(cx);
    assert_eq!(
        setup
            .kv
            .get_item(monocode_core::appearance::ACCENT_COLOR_KEY)
            .as_deref(),
        Some("#123456")
    );
}
