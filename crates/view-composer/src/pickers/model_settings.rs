//! Port of src/features/sessions/ui/ModelSettings.tsx: a model's options as
//! standalone pills, toggles that light up when on and selects with a
//! listbox popover.

use std::rc::Rc;

use gpui::{
    App, Context, FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyDownEvent,
    MouseDownEvent, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _,
};
use monocode_core::models::{ModelSetting, ModelSettingKind};
use monocode_core::{HarnessId, ModelSettings};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::anchor::{BoundsCell, Side, anchored_popover, popover_layer, popover_surface};
use super::model_logic::{model_settings_controls, setting_value, with_setting};
use super::model_picker::{CloseFn, SettingsFn};
use super::model_source::ModelSource;
use super::style::{pill_chevron, pill_label, toolbar_pill};

/// `MENU_WIDTH`.
const MENU_WIDTH: f32 = 220.0;

/// The toggle pill's glyph.
pub fn toggle_icon(setting: &ModelSetting) -> IconName {
    match setting.id.as_str() {
        "fast" => IconName::Zap,
        "thinking" => IconName::AiIdea,
        _ => IconName::Gauge,
    }
}

/// The select pill's glyph.
pub fn select_icon(setting: &ModelSetting) -> IconName {
    if setting.id == "context" {
        IconName::Maximize2
    } else {
        IconName::Gauge
    }
}

fn option_index(setting: &ModelSetting, value: &str) -> usize {
    setting
        .options
        .iter()
        .position(|option| option.value == value)
        .unwrap_or(0)
}

pub struct ModelSettingsView {
    harness: HarnessId,
    model: String,
    values: ModelSettings,
    source: Rc<dyn ModelSource>,
    /// The open select and its highlighted option.
    open: Option<(ModelSetting, usize)>,
    focus: FocusHandle,
    trigger_bounds: BoundsCell,
    on_change: Option<SettingsFn>,
    on_close: Option<CloseFn>,
}

impl ModelSettingsView {
    pub fn new(
        harness: HarnessId,
        model: impl Into<String>,
        values: ModelSettings,
        source: Rc<dyn ModelSource>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            harness,
            model: model.into(),
            values,
            source,
            open: None,
            focus: cx.focus_handle(),
            trigger_bounds: BoundsCell::default(),
            on_change: None,
            on_close: None,
        }
    }

    pub fn on_change(
        mut self,
        f: impl Fn(&ModelSettings, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    pub fn set_selection(
        &mut self,
        harness: HarnessId,
        model: impl Into<String>,
        values: ModelSettings,
        cx: &mut Context<Self>,
    ) {
        self.harness = harness;
        self.model = model.into();
        self.values = values;
        cx.notify();
    }

    /// The settings shown, in order.
    pub fn settings(&self) -> Vec<ModelSetting> {
        let model = Some(self.model.as_str()).filter(|model| !model.is_empty());
        model_settings_controls(self.harness, &self.source.resolve(self.harness, model))
    }

    pub fn open_setting(&self) -> Option<(&str, usize)> {
        self.open
            .as_ref()
            .map(|(setting, active)| (setting.id.as_str(), *active))
    }

    fn set_value(&self, id: &str, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let next = with_setting(&self.values, id, value);
        if let Some(f) = self.on_change.clone() {
            window.defer(cx, move |window, cx| f(&next, window, cx));
        }
    }

    pub fn toggle(&mut self, setting: &ModelSetting, window: &mut Window, cx: &mut Context<Self>) {
        let on = setting_value(setting, &self.values) == "true";
        self.set_value(&setting.id, if on { "false" } else { "true" }, window, cx);
    }

    fn dismiss(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = None;
        if restore && let Some(f) = self.on_close.clone() {
            window.defer(cx, move |window, cx| f(window, cx));
        }
        cx.notify();
    }

    pub fn toggle_select(
        &mut self,
        setting: &ModelSetting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .open
            .as_ref()
            .is_some_and(|(open, _)| open.id == setting.id)
        {
            self.dismiss(true, window, cx);
            return;
        }
        let active = option_index(setting, setting_value(setting, &self.values));
        self.open = Some((setting.clone(), active));
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn pick(
        &mut self,
        setting: &ModelSetting,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_value(&setting.id, value, window, cx);
        self.dismiss(true, window, cx);
    }

    pub fn key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((setting, active)) = self.open.clone() else {
            return false;
        };
        match key {
            "down" => {
                self.open = Some((
                    setting.clone(),
                    (active + 1).min(setting.options.len().saturating_sub(1)),
                ))
            }
            "up" => self.open = Some((setting.clone(), active.saturating_sub(1))),
            "enter" => {
                if let Some(option) = setting.options.get(active) {
                    let value = option.value.clone();
                    self.pick(&setting, &value, window, cx);
                }
            }
            "escape" => self.dismiss(true, window, cx),
            _ => return false,
        }
        cx.notify();
        true
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        if self.key(event.keystroke.key.as_str(), window, cx) {
            cx.stop_propagation();
        }
    }

    fn render_menu(
        &self,
        setting: &ModelSetting,
        active: usize,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let value = setting_value(setting, &self.values).to_string();
        let hover = theme.content(0.05);
        let mut list = div().flex().flex_col().p(u(4.));
        for (index, option) in setting.options.iter().enumerate() {
            let selected = option.value == value;
            let picked = setting.clone();
            let option_value = option.value.clone();
            list = list.child(
                div()
                    .id(("model-settings-option", index))
                    .flex()
                    .w_full()
                    .items_center()
                    .px(u(8.))
                    .py(u(6.))
                    .rounded(u(theme.radius.lg))
                    .text_px(theme.text.body)
                    .leading(theme.leading.normal)
                    .text_color(theme.colors.content)
                    .map(|row| {
                        if index == active || selected {
                            row.bg(theme.colors.selection)
                        } else {
                            row.hover(move |s| s.bg(hover))
                        }
                    })
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if let (true, Some((_, active))) = (*hovered, this.open.as_mut()) {
                            *active = index;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick(&picked, &option_value, window, cx)
                    }))
                    .child(option.label.clone()),
            );
        }
        let outside = cx.listener(|this, event: &MouseDownEvent, window, cx| {
            if !this.trigger_bounds.contains(event.position) {
                this.dismiss(false, window, cx);
            }
        });
        popover_surface(
            "model-settings-menu",
            Some(MENU_WIDTH),
            None,
            outside,
            list.font_family(theme.fonts.sans.clone()),
        )
    }
}

impl Focusable for ModelSettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ModelSettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut row = div()
            .id("model-settings")
            .flex()
            .items_center()
            .gap(u(4.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down));
        for setting in self.settings() {
            let value = setting_value(&setting, &self.values).to_string();
            let title = setting
                .description
                .clone()
                .unwrap_or_else(|| setting.label.clone());
            let clicked = setting.clone();
            let id = format!("model-settings-{}", setting.id);
            if setting.kind == ModelSettingKind::Toggle {
                let on = value == "true";
                let emphasis = theme.colors.selection_emphasis;
                let hover = theme.colors.selection_hover;
                let ink = theme.colors.content;
                let group: gpui::SharedString = id.clone().into();
                let glyph_ink = if on { ink } else { theme.content(0.50) };
                let pill = div()
                    .id(gpui::ElementId::Name(id.clone().into()))
                    .group(group.clone())
                    .debug_selector(move || id)
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(4.))
                    .h(u(26.))
                    .px(u(6.))
                    .rounded(u(theme.radius.md))
                    .map(|pill| {
                        if on {
                            pill.bg(emphasis).text_color(ink)
                        } else {
                            pill.bg(theme.colors.selection)
                                .text_color(theme.content(0.50))
                                .hover(move |s| s.bg(hover).text_color(ink))
                        }
                    })
                    .tooltip(monocode_ui::widgets::tooltip(title))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.toggle(&clicked, window, cx)),
                    )
                    .child(
                        icon(toggle_icon(&setting))
                            .size(u(14.))
                            .text_color(glyph_ink)
                            .group_hover(group, move |s| s.text_color(ink)),
                    )
                    .child(
                        div()
                            .text_px(theme.text.caption)
                            .leading(theme.leading.normal)
                            .child(setting.label.clone()),
                    );
                row = row.child(pill);
                continue;
            }
            let open = self
                .open
                .as_ref()
                .filter(|(open, _)| open.id == setting.id)
                .map(|(_, active)| *active);
            let label = setting
                .options
                .iter()
                .find(|option| option.value == value)
                .or(setting.options.first())
                .map(|option| option.label.clone())
                .unwrap_or_else(|| setting.label.clone());
            let mut trigger =
                toolbar_pill(id.clone(), open.is_some(), &theme)
                    .max_w(u(144.))
                    .debug_selector(move || id)
                    .tooltip(monocode_ui::widgets::tooltip(title))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle_select(&clicked, window, cx)
                    }))
                    .child(
                        icon(select_icon(&setting))
                            .size(u(14.))
                            .text_color(theme.colors.content),
                    )
                    .child(pill_label(label, None, &theme))
                    .child(pill_chevron(open.is_some(), &theme));
            if open.is_some() {
                trigger = trigger.child(self.trigger_bounds.probe());
            }
            let mut cell = div().relative().child(trigger);
            if let Some(active) = open {
                let menu = self
                    .render_menu(&setting, active, &theme, cx)
                    .into_any_element();
                cell = cell.child(anchored_popover(
                    Side::Top,
                    super::anchor::DEFAULT_GAP,
                    popover_layer(cx),
                    window,
                    menu,
                ));
            }
            row = row.child(cell);
        }
        row
    }
}
