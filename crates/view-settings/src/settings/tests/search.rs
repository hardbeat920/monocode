//! Ports of the "settings search" cases of SettingsView.test.ts.

use gpui::TestAppContext;
use monocode_core::Platform;
use monocode_core::settings::SettingsSectionId;

use super::*;

const LINUX: Platform = Platform::Linux;

/// Types into the header search field, replacing what it held.
fn search(page: &Entity<SettingsPage>, cx: &mut VisualTestContext, query: &str) {
    let field = page.read_with(cx, |page, _| page.search().clone());
    cx.update(|window, cx| field.update(cx, |field, cx| field.set_query("", window, cx)));
    click(cx, "search-settings");
    cx.simulate_input(query);
    draw(cx);
}

fn labels(page: &Entity<SettingsPage>, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    page.read_with(cx, |page, cx| {
        page.search()
            .read(cx)
            .results()
            .into_iter()
            .map(|result| {
                let meta = if result.setting_id.is_some() {
                    result.section_label
                } else {
                    "Page"
                };
                (result.label.to_string(), meta.to_string())
            })
            .collect()
    })
}

#[gpui::test]
fn finds_a_setting_that_lives_on_another_page(cx: &mut TestAppContext) {
    let mut setup = Setup::new(LINUX);
    let selected = Calls::new();
    setup.callbacks.on_select_section = selected.callback();
    let (page, cx) = mount(cx, SettingsSectionId::General, &setup);
    search(&page, cx, "pacman");
    assert_eq!(
        labels(&page, cx),
        vec![("Empty session games".to_string(), "Chat".to_string())]
    );
    assert!(exists(cx, "search-result:Empty session games:Chat"));
    click(cx, "search-result:Empty session games:Chat");
    assert_eq!(selected.all(), vec![SettingsSectionId::Chat]);
}

#[gpui::test]
fn finds_and_reveals_project_notifications_separately_from_global_notifications(
    cx: &mut TestAppContext,
) {
    let mut setup = Setup::new(LINUX);
    let selected = Calls::new();
    setup.callbacks.on_select_section = selected.callback();
    let (page, cx) = mount(cx, SettingsSectionId::General, &setup);
    search(&page, cx, "project notifications");
    assert_eq!(
        labels(&page, cx),
        vec![("Project notifications".to_string(), "Inbox".to_string())]
    );
    click(cx, "search-result:Project notifications:Inbox");
    assert_eq!(selected.all(), vec![SettingsSectionId::Inbox]);
    // The owner switches the page.
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.set_section(SettingsSectionId::Inbox, window, cx)
        })
    });
    draw(cx);
    assert!(exists(cx, "setting-id:project-notifications"));
    assert!(exists(cx, "flash-row"));
}

#[gpui::test]
fn ranks_a_page_against_the_settings_that_mention_it(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (page, cx) = mount(cx, SettingsSectionId::General, &setup);
    search(&page, cx, "archive");
    let names: Vec<String> = labels(&page, cx)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    assert_eq!(names, vec!["Archive", "Show archived in the sidebar"]);
    search(&page, cx, "notification");
    let names: Vec<String> = labels(&page, cx)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    assert_eq!(
        names,
        vec![
            "Notifications",
            "Project notifications",
            "Claude Code hooks",
            "General",
            "Inbox"
        ]
    );
}

#[gpui::test]
fn closes_the_results_without_touching_the_page_when_cleared(cx: &mut TestAppContext) {
    let mut setup = Setup::new(LINUX);
    let selected = Calls::new();
    setup.callbacks.on_select_section = selected.callback();
    let (page, cx) = mount(cx, SettingsSectionId::General, &setup);
    search(&page, cx, "sounds");
    assert!(exists(cx, "listbox:Settings search results"));
    click(cx, "button:clear-settings-search");
    assert!(!exists(cx, "listbox:Settings search results"));
    let query = page.read_with(cx, |page, cx| page.search().read(cx).query().to_string());
    assert_eq!(query, "");
    assert!(selected.all().is_empty());
}

#[gpui::test]
fn reveals_a_setting_on_the_current_page(cx: &mut TestAppContext) {
    let mut setup = Setup::new(LINUX);
    let selected = Calls::new();
    setup.callbacks.on_select_section = selected.callback();
    let (page, cx) = mount(cx, SettingsSectionId::General, &setup);
    search(&page, cx, "sounds");
    keys(cx, "enter");
    assert!(selected.all().is_empty());
    let revealed = page.read_with(cx, |page, cx| page.revealed(cx));
    assert_eq!(revealed.as_deref(), Some("sounds"));
    assert!(exists(cx, "flash-row"));
}

#[gpui::test]
fn arrow_keys_move_through_results_and_escape_clears_them(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (page, cx) = mount(cx, SettingsSectionId::General, &setup);
    search(&page, cx, "notification");
    keys(cx, "down down");
    let active = page.read_with(cx, |page, cx| page.search().read(cx).active());
    assert_eq!(active, 2);
    keys(cx, "up");
    let active = page.read_with(cx, |page, cx| page.search().read(cx).active());
    assert_eq!(active, 1);
    keys(cx, "escape");
    assert!(!exists(cx, "listbox:Settings search results"));
}

#[gpui::test]
fn switches_pages_itself_without_an_owner(cx: &mut TestAppContext) {
    let setup = Setup::new(LINUX);
    let (page, cx) = mount(cx, SettingsSectionId::General, &setup);
    search(&page, cx, "pacman");
    keys(cx, "enter");
    let section = page.read_with(cx, |page, _| page.section());
    assert_eq!(section, SettingsSectionId::Chat);
    assert!(exists(cx, "flash-row"));
}
