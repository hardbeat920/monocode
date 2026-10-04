//! Grouped Settings navigation in place of the project rail.

use super::rail_action::rail_action;
use super::{Shell, WhenMac as _, drag_region};
use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div,
};
use monocode_app::boot::AppServices;
use monocode_core::settings::{SettingsSectionId, settings_sections_by_group};
use monocode_ui::{IconName, Theme, UiStyled as _, u};

fn section_icon(section: SettingsSectionId) -> IconName {
    match section {
        SettingsSectionId::General => IconName::SlidersHorizontal,
        SettingsSectionId::Connections => IconName::Internet,
        SettingsSectionId::Appearance => IconName::Palette,
        SettingsSectionId::Keybindings => IconName::Keyboard,
        SettingsSectionId::Chat => IconName::MessageSquare,
        SettingsSectionId::Providers => IconName::Bot,
        SettingsSectionId::Mcp => IconName::Globe,
        SettingsSectionId::Skills => IconName::Sparkles,
        SettingsSectionId::Inbox => IconName::Inbox,
        SettingsSectionId::Worktrees => IconName::FolderTree,
        SettingsSectionId::Archive => IconName::Archive,
    }
}

pub fn view(shell: WeakEntity<Shell>, width: f32, _: &mut Window, cx: &mut App) -> AnyElement {
    let theme = Theme::of(cx).clone();
    let selected = AppServices::try_global(cx)
        .map(|services| monocode_settings::settings_store::load_settings_section(&services.kv))
        .unwrap_or(SettingsSectionId::General);
    let header = drag_region(
        div()
            .id("settings-rail-header")
            .flex()
            .flex_none()
            .h(u(theme.metrics.title_bar_height))
            .when_mac(|element| element.child(div().w(u(theme.metrics.traffic_light_inset)))),
        shell.clone(),
    );
    let mut nav = div()
        .id("settings-rail-sections")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .gap(u(20.))
        .px(u(8.))
        .py(u(12.));
    for group in settings_sections_by_group() {
        let mut rows = div().flex().flex_col().gap(gpui::px(1.)).child(
            div()
                .px(u(8.))
                .pb(u(4.))
                .text_px(12.)
                .medium()
                .text_color(theme.content(0.35))
                .child(group.label),
        );
        for section in group.sections {
            let shell = shell.clone();
            rows = rows.child(
                rail_action(
                    section.id.as_str(),
                    section.label,
                    section_icon(section.id),
                    selected == section.id,
                    None,
                    &theme,
                )
                .on_click(move |_, window, cx| {
                    crate::pages::settings::reveal_section(section.id, window, cx);
                    shell.update(cx, |_, cx| cx.notify()).ok();
                }),
            );
        }
        nav = nav.child(rows);
    }
    div()
        .id("settings-rail")
        .flex()
        .flex_col()
        .flex_none()
        .h_full()
        .w(u(width))
        .bg(theme.colors.sidebar_glass)
        .border_r_1()
        .border_color(theme.colors.stroke)
        .child(header)
        .child(nav)
        .child(
            div().p(u(8.)).child(
                rail_action(
                    "settings-back",
                    "Back",
                    IconName::ArrowLeft,
                    false,
                    None,
                    &theme,
                )
                .on_click(move |_, _, cx| {
                    shell.update(cx, |shell, cx| shell.close_settings(cx)).ok();
                }),
            ),
        )
        .into_any_element()
}
