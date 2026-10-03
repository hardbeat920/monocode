//! Port of `ModelFlyout` and `ProviderTabButton` in
//! src/features/sessions/ui/ModelPicker.tsx: the provider tab rail, the
//! model search, and the model list with favorites.

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, MouseDownEvent, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, svg,
};
use gpui_component::input::{Enter, MoveDown, MoveUp};
use monocode_core::models::ModelPickerTab;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, provider_logo, u};

use super::anchor::popover_surface;
use super::field::plain_input;
use super::model_logic::{
    MODEL_MENU_HEIGHT, MODEL_MENU_WIDTH, empty_models_message, model_groups, provenance,
};
use super::model_picker::{ModelPicker, harness_logo};
use super::style::check_mark;

/// The Hugeicons star, or gpui-component's filled star when `filled`.
pub(crate) fn star(filled: bool, size: f32) -> gpui::Svg {
    if filled {
        svg().path("icons/star-fill.svg").flex_none().size(u(size))
    } else {
        icon(IconName::Star).size(u(size))
    }
}

/// The flyout. `standalone` when settings live beside the picker and the
/// flyout opens straight from the trigger; as a submenu, the menu owns
/// dismissal.
pub(crate) fn render_flyout(
    picker: &ModelPicker,
    standalone: bool,
    theme: &Theme,
    _window: &mut Window,
    cx: &mut Context<ModelPicker>,
) -> AnyElement {
    let tab = picker.visible_tab();
    let models = picker.visible_models();
    let current = picker.current().id;

    // Provider tab rail.
    let mut rail = div()
        .flex()
        .flex_col()
        .flex_none()
        .items_center()
        .gap(u(4.))
        .w(u(44.))
        .p(u(6.))
        .border_r_1()
        .border_color(theme.colors.stroke);
    let favorites_selected = tab == ModelPickerTab::Favorites;
    rail = rail.child(
        tab_button(
            "model-tab-favorites",
            favorites_selected,
            theme,
            cx,
            ModelPickerTab::Favorites,
        )
        .debug_selector(|| "model-tab-favorites".into())
        .child(
            star(favorites_selected, 16.)
                .text_color(tab_ink(favorites_selected, theme))
                .group_hover("model-tab-favorites", {
                    let ink = theme.colors.content;
                    move |s| s.text_color(ink)
                }),
        ),
    );
    for harness in picker.picker_harnesses() {
        let selected = tab == ModelPickerTab::Harness(harness);
        let id = format!("model-tab-{}", harness.as_str());
        let selector = id.clone();
        rail = rail.child(
            tab_button(id, selected, theme, cx, ModelPickerTab::Harness(harness))
                .debug_selector(move || selector)
                .tooltip(monocode_ui::widgets::tooltip(harness.title()))
                .child(
                    provider_logo(harness_logo(harness))
                        .size(16.)
                        .color(tab_ink(selected, theme)),
                ),
        );
    }

    // Search field.
    let search = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(u(8.))
        .px(u(12.))
        .py(u(10.))
        .border_b_1()
        .border_color(theme.colors.stroke)
        .text_color(theme.content(0.50))
        .child(
            icon(IconName::Search)
                .size(u(14.))
                .text_color(theme.content(0.50)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(u(20.))
                .text_px(theme.text.body)
                .text_color(theme.colors.content)
                .debug_selector(|| "model-search".into())
                .capture_action(cx.listener(|this: &mut ModelPicker, _: &MoveDown, _, cx| {
                    let len = this.visible_models().len();
                    let next = (this.active_model + 1).min(len.saturating_sub(1));
                    this.set_active_model(next, cx);
                    this.flyout_scroll_to_active();
                    cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this: &mut ModelPicker, _: &MoveUp, _, cx| {
                    let next = this.active_model.saturating_sub(1);
                    this.set_active_model(next, cx);
                    this.flyout_scroll_to_active();
                    cx.stop_propagation();
                }))
                .capture_action(
                    cx.listener(|this: &mut ModelPicker, _: &Enter, window, cx| {
                        cx.stop_propagation();
                        if let Some(item) = this.visible_models().get(this.active_model).cloned() {
                            this.pick_model(&item, window, cx);
                        }
                    }),
                )
                .flex()
                .items_center()
                .child(plain_input(&picker.search, cx)),
        );

    // Model list.
    let mut list = div()
        .id("model-list")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&picker.flyout_scroll)
        .p(u(4.));
    if models.is_empty() {
        list = list.child(
            div()
                .px(u(8.))
                .py(u(12.))
                .text_px(theme.text.label)
                .leading(theme.leading.normal)
                .text_color(theme.content(0.50))
                .child(empty_models_message(
                    tab,
                    &picker.query,
                    picker.source.as_ref(),
                )),
        );
    } else {
        for group in model_groups(tab, &models) {
            if let Some(name) = &group.name {
                list = list.child(super::style::group_caption(name, 8., theme));
            }
            for (item, index) in group.models {
                let selected = item.id == current;
                let highlighted = index == picker.active_model;
                let favorited = picker.favorites.contains(&item.id);
                let disabled = !picker.source.available(item.harness);
                let group_name = format!("model-row-{index}");
                let ink = if disabled {
                    theme.content(0.30)
                } else {
                    theme.colors.content
                };
                let hover = theme.content(0.05);
                let mut row = div()
                    .id(("model-row", index))
                    .group(group_name.clone())
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(u(32.))
                    .px(u(4.))
                    .rounded(u(theme.radius.lg))
                    .text_color(ink)
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.set_active_model(index, cx);
                        }
                    }));
                if !disabled {
                    row = if highlighted {
                        row.bg(theme.colors.selection)
                    } else {
                        row.hover(move |s| s.bg(hover))
                    };
                }
                let picked = item.clone();
                let mut name = div()
                    .id(("model-option", index))
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .px(u(6.))
                    .text_px(theme.text.body)
                    .debug_selector({
                        let name = item.name.clone();
                        move || format!("model-option-{name}")
                    })
                    .child(div().min_w_0().truncate().child(item.name.clone()));
                if disabled {
                    name = name.tooltip(monocode_ui::widgets::tooltip(
                        picker.source.unavailable_hint(item.harness),
                    ));
                } else {
                    name =
                        name.on_click(cx.listener(move |this, _, window, cx| {
                            this.pick_model(&picked, window, cx)
                        }));
                }
                row = row.child(name);
                if tab == ModelPickerTab::Favorites {
                    row = row.child(
                        div()
                            .flex_none()
                            .max_w(u(96.))
                            .truncate()
                            .text_px(theme.text.micro)
                            .text_color(theme.content(0.40))
                            .child(provenance(&item)),
                    );
                }
                let favorite_id = item.id.clone();
                let star_ink = if favorited {
                    theme.content(0.60)
                } else {
                    theme.content(0.35)
                };
                let mut star_button = div()
                    .id(("model-favorite", index))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(u(24.))
                    .rounded(u(theme.radius.md))
                    .debug_selector({
                        let name = item.name.clone();
                        move || format!("model-favorite-{name}")
                    })
                    .tooltip(monocode_ui::widgets::tooltip(if favorited {
                        "Remove from favorites"
                    } else {
                        "Add to favorites"
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.toggle_favorite(&favorite_id, window, cx);
                    }))
                    .child(star(favorited, 14.).text_color(star_ink));
                if !favorited {
                    star_button = star_button
                        .opacity(0.)
                        .group_hover(group_name, |s| s.opacity(1.));
                }
                row = row.child(star_button);
                if selected {
                    row = row.child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(u(24.))
                            .child(check_mark(0.55, theme)),
                    );
                }
                list = list.child(row);
            }
        }
    }

    let content = div()
        .relative()
        .flex()
        .h(u(MODEL_MENU_HEIGHT))
        .min_h_0()
        .overflow_hidden()
        .font_family(theme.fonts.sans.clone())
        .debug_selector(|| "model-flyout".into())
        .child(picker.flyout_bounds.probe())
        .child(rail)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .child(search)
                .child(list),
        );

    let outside = cx.listener(
        move |this: &mut ModelPicker, event: &MouseDownEvent, window, cx| {
            // `onDismiss(reason)` restores focus only for Escape.
            if standalone && !this.press_is_on_trigger(event) {
                this.dismiss(false, window, cx);
            }
        },
    );
    popover_surface(
        "model-flyout",
        Some(MODEL_MENU_WIDTH),
        None,
        outside,
        content,
    )
    .into_any_element()
}

fn tab_ink(selected: bool, theme: &Theme) -> gpui::Hsla {
    if selected {
        theme.colors.content
    } else {
        theme.content(0.45)
    }
}

/// `ProviderTabButton`: hovering an unselected tab selects it.
fn tab_button(
    id: impl Into<gpui::SharedString>,
    selected: bool,
    theme: &Theme,
    cx: &mut Context<ModelPicker>,
    tab: ModelPickerTab,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let hover = theme.content(0.08);
    let button = div()
        .id(gpui::ElementId::Name(id.clone()))
        .group(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(u(32.))
        .rounded(u(theme.radius.md))
        .on_click(cx.listener(move |this, _, window, cx| this.select_tab(tab, window, cx)));
    if selected {
        button.bg(theme.colors.selection_strong)
    } else {
        button.hover(move |s| s.bg(hover)).on_hover(cx.listener(
            move |this, hovered: &bool, window, cx| {
                if *hovered {
                    this.select_tab(tab, window, cx);
                }
            },
        ))
    }
}
