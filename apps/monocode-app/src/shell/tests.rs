//! Shell transitions over the real workspace and history models.

use super::*;
use gpui::TestAppContext;
use monocode_engine::runtime::testing::init_test_engine;
use monocode_settings::Kv;

#[gpui::test]
fn pages_and_sidebar_changes_follow_workspace_and_saved_preferences(cx: &mut TestAppContext) {
    cx.skip_drawing();
    init_test_engine(cx);
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
    });
    let shell_window = cx.add_window(|window, cx| {
        let mut shell = Shell::new(ShellOptions::full(), window, cx);
        let history = cx.new(|cx| History::new(Kv::in_memory(), cx));
        shell.attach(WorkspaceConfig::fresh(Some("/repo")), history, window, cx);
        shell
    });
    shell_window
        .update(cx, |shell, window, cx| {
            let workspace = shell.workspace().unwrap().clone();
            let history = shell.history().unwrap().clone();
            assert!(history.read(cx).sidebar().sessions_tab_active);
            history.update(cx, |history, cx| {
                history.set_search_query("saved query", cx)
            });
            shell.set_sidebar_tab(SidebarTab::Files, cx);
            assert!(!history.read(cx).sidebar().sessions_tab_active);
            assert!(history.read(cx).sidebar().search_query.is_empty());
            shell.set_sidebar_tab(SidebarTab::Sessions, cx);
            assert!(history.read(cx).sidebar().sessions_tab_active);
            shell.open_page(Page::Settings, cx);
            assert_eq!(shell.layout().page, Some(Page::Settings));
            assert!(workspace.read(cx).full_page_open());
            shell.open_page(Page::Inbox, cx);
            assert_eq!(shell.layout().page, Some(Page::Inbox));
            assert!(workspace.read(cx).full_page_open());
            shell.close_page(cx);
            assert_eq!(shell.layout().page, None);
            assert!(!workspace.read(cx).full_page_open());
            shell.toggle_session_sidebar(cx);
            assert!(!history.read(cx).sidebar().sessions_tab_active);
            shell.toggle_session_sidebar(cx);
            assert!(history.read(cx).sidebar().sessions_tab_active);
            shell.toggle_project_rail(cx);
            shell.set_session_sidebar_open(false, cx);
            let kv = history.read(cx).kv().clone();
            let restored = ShellOptions::from_preferences(&kv);
            assert!(!restored.project_rail_open);
            assert!(!restored.session_sidebar_open);
            shell.start_resize(ResizeTarget::ProjectRail, gpui::px(200.));
            shell.on_mouse_move(
                &MouseMoveEvent {
                    position: gpui::point(gpui::px(278.), gpui::px(100.)),
                    pressed_button: Some(MouseButton::Left),
                    modifiers: gpui::Modifiers::none(),
                },
                window,
                cx,
            );
            shell.on_mouse_up(
                &MouseUpEvent {
                    position: gpui::point(gpui::px(278.), gpui::px(100.)),
                    button: MouseButton::Left,
                    modifiers: gpui::Modifiers::none(),
                    click_count: 1,
                },
                window,
                cx,
            );
            assert_eq!(
                kv.get_item(monocode_core::appearance::PROJECT_RAIL_WIDTH_KEY)
                    .as_deref(),
                Some("278")
            );
            shell.reset_width(ResizeTarget::ProjectRail, cx);
            assert_eq!(
                kv.get_item(monocode_core::appearance::PROJECT_RAIL_WIDTH_KEY)
                    .as_deref(),
                Some("200")
            );
        })
        .unwrap();
    cx.run_until_parked();
}

/// Port of 8fae5666 (`settingsReturnViewRef` in App.tsx).
#[gpui::test]
fn closing_settings_returns_to_the_page_it_replaced(cx: &mut TestAppContext) {
    cx.skip_drawing();
    init_test_engine(cx);
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
    });
    let shell_window = cx.add_window(|window, cx| {
        let mut shell = Shell::new(ShellOptions::full(), window, cx);
        let history = cx.new(|cx| History::new(Kv::in_memory(), cx));
        shell.attach(WorkspaceConfig::fresh(Some("/repo")), history, window, cx);
        shell
    });
    shell_window
        .update(cx, |shell, _, cx| {
            let workspace = shell.workspace().unwrap().clone();
            shell.open_page(Page::Inbox, cx);
            shell.toggle_page(Page::Settings, cx);
            assert_eq!(shell.layout().page, Some(Page::Settings));
            shell.toggle_page(Page::Settings, cx);
            assert_eq!(shell.layout().page, Some(Page::Inbox));
            assert!(workspace.read(cx).full_page_open());

            // Settings opened over the workspace closes to the workspace.
            shell.close_page(cx);
            shell.open_page(Page::Settings, cx);
            shell.close_settings(cx);
            assert_eq!(shell.layout().page, None);
            assert!(!workspace.read(cx).full_page_open());

            // Opening Settings again while it shows keeps the first return page.
            shell.open_page(Page::Automations, cx);
            shell.open_page(Page::Settings, cx);
            shell.open_page(Page::Settings, cx);
            shell.close_settings(cx);
            assert_eq!(shell.layout().page, Some(Page::Automations));

            // Notes stays closed when Settings turned it off.
            shell.open_page(Page::Notes, cx);
            shell.open_page(Page::Settings, cx);
            let kv = shell.history().unwrap().read(cx).kv().clone();
            monocode_settings::settings_store::save_notes_enabled(&kv, false);
            shell.close_settings(cx);
            assert_eq!(shell.layout().page, None);
        })
        .unwrap();
    cx.run_until_parked();
}

struct MenuFocusChild {
    focus: FocusHandle,
}

impl Render for MenuFocusChild {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().track_focus(&self.focus)
    }
}

#[gpui::test]
fn window_menu_actions_work_before_child_focus_and_after_it_is_removed(cx: &mut TestAppContext) {
    init_test_engine(cx);
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
    });
    struct MenuUsageHost;
    impl monocode_view_settings::accounts::host::UsageHost for MenuUsageHost {}
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let mut shell = Shell::new(ShellOptions::full(), window, cx);
        shell.usage_footer = Some(cx.new(|cx| {
            monocode_view_settings::accounts::UsageFooter::new(
                std::rc::Rc::new(MenuUsageHost),
                Default::default(),
                Default::default(),
                cx,
            )
        }));
        shell
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear();
        assert!(window.is_action_available(&keymap::OpenSettings, cx));
        assert!(window.is_action_available(&keymap::ToggleSidebar, cx));
        window.dispatch_action(Box::new(keymap::OpenSettings), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        shell.read_with(cx, |shell, _| shell.layout.page),
        Some(Page::Settings)
    );
    let child = cx.new(|cx| MenuFocusChild {
        focus: cx.focus_handle(),
    });
    cx.update(|_, cx| {
        let view = child.clone();
        cx.set_global(AppSlots {
            workspace: Some(std::rc::Rc::new(move |_, _| view.clone().into())),
            ..Default::default()
        });
        shell.update(cx, |shell, cx| shell.close_page(cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear();
        let focus = child.read(cx).focus.clone();
        focus.focus(window, cx);
        window.draw(cx).clear();
        assert!(child.read(cx).focus.is_focused(window));
        assert!(window.is_action_available(&keymap::OpenSettings, cx));
        window.draw(cx).clear();
        assert!(child.read(cx).focus.is_focused(window));
        cx.set_global(AppSlots::default());
        shell.update(cx, |_, cx| cx.notify());
        window.draw(cx).clear();
        assert!(shell.read(cx).focus.is_focused(window));
        assert!(window.is_action_available(&keymap::OpenSettings, cx));
        assert!(window.is_action_available(&keymap::ToggleSidebar, cx));
        window.dispatch_action(Box::new(keymap::ToggleSidebar), cx);
    });
    cx.run_until_parked();
    assert!(!shell.read_with(cx, |shell, _| shell.layout.project_rail_open));
}
