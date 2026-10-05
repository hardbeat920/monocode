//! Port of `ModelControlPills`, `TogglePill`, and `SelectPill` in
//! src/features/sessions/ui/ModelPicker.tsx: the model options as toolbar
//! pills beside a picker opened with `hide_settings`. Fast mode and the
//! service tier ride inside the effort pill's popover.

use std::rc::Rc;

use gpui::{
    App, Context, FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyDownEvent,
    MouseDownEvent, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _,
};
use monocode_core::models::{AgentModel, ModelSetting};
use monocode_core::{HarnessId, ModelSettings};
use monocode_ui::{IconName, Theme, icon, u};

use super::anchor::{BoundsCell, Side, anchored_popover, popover_layer, popover_surface};
use super::effort_tiles::{effort_glow, effort_tile_tone, effort_tiles};
use super::model_logic::{
    ControlPill, SETTING_MENU_WIDTH, control_pills, is_effort_setting, select_menu_label,
    select_menu_options, setting_label, setting_value, setting_value_label, with_setting,
};
use super::model_picker::{CloseFn, SettingsFn};
use super::model_source::ModelSource;
use super::style::{
    check_mark, group_caption, menu_row, menu_separator, pill_chevron, pill_label, toolbar_pill,
};

/// The glyph a select pill leads with: a gauge for reasoning levels, a
/// bolt for the service tier.
pub fn select_pill_icon(setting: &ModelSetting) -> Option<IconName> {
    if is_effort_setting(setting) {
        Some(IconName::Gauge)
    } else if setting.id == "serviceTier" {
        Some(IconName::Zap)
    } else {
        None
    }
}

/// The open select pill.
#[derive(Clone, Debug, PartialEq)]
struct OpenPill {
    setting: ModelSetting,
    grouped: Vec<ModelSetting>,
    active: usize,
}

impl OpenPill {
    fn menu_settings(&self) -> Vec<ModelSetting> {
        std::iter::once(self.setting.clone())
            .chain(self.grouped.iter().cloned())
            .collect()
    }
}

pub struct ModelControlPills {
    harness: HarnessId,
    model: String,
    values: ModelSettings,
    source: Rc<dyn ModelSource>,
    open: Option<OpenPill>,
    focus: FocusHandle,
    trigger_bounds: BoundsCell,
    on_settings_change: Option<SettingsFn>,
    on_close: Option<CloseFn>,
}

impl ModelControlPills {
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
            on_settings_change: None,
            on_close: None,
        }
    }

    pub fn on_settings_change(
        mut self,
        f: impl Fn(&ModelSettings, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_settings_change = Some(Rc::new(f));
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

    pub fn set_source(&mut self, source: Rc<dyn ModelSource>, cx: &mut Context<Self>) {
        self.source = source;
        cx.notify();
    }

    fn current(&self) -> AgentModel {
        let model = Some(self.model.as_str()).filter(|model| !model.is_empty());
        self.source.resolve(self.harness, model)
    }

    /// The pills, in order.
    pub fn pills(&self) -> Vec<ControlPill> {
        control_pills(&self.current())
    }

    /// The open select pill's setting id and highlighted option.
    pub fn open_menu(&self) -> Option<(String, usize)> {
        self.open
            .as_ref()
            .map(|open| (open.setting.id.clone(), open.active))
    }

    /// The open popover's accessible name, such as "Effort and Fast".
    pub fn open_menu_label(&self) -> Option<String> {
        self.open
            .as_ref()
            .map(|open| select_menu_label(&open.menu_settings()))
    }

    fn change(&self, next: ModelSettings, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.on_settings_change.clone() {
            window.defer(cx, move |window, cx| f(&next, window, cx));
        }
    }

    /// `TogglePill`'s click.
    pub fn toggle(&mut self, setting: &ModelSetting, window: &mut Window, cx: &mut Context<Self>) {
        let on = setting_value(setting, &self.values) == "true";
        let next = with_setting(&self.values, &setting.id, if on { "false" } else { "true" });
        self.change(next, window, cx);
    }

    fn dismiss(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = None;
        if restore && let Some(f) = self.on_close.clone() {
            window.defer(cx, move |window, cx| f(window, cx));
        }
        cx.notify();
    }

    /// `SelectPill`'s click: open with the current value highlighted, or
    /// close.
    pub fn toggle_select(
        &mut self,
        setting: &ModelSetting,
        grouped: &[ModelSetting],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .open
            .as_ref()
            .is_some_and(|open| open.setting.id == setting.id)
        {
            self.dismiss(true, window, cx);
            return;
        }
        let pill = OpenPill {
            setting: setting.clone(),
            grouped: grouped.to_vec(),
            active: 0,
        };
        let value = setting_value(setting, &self.values);
        let active = select_menu_options(&pill.menu_settings())
            .iter()
            .position(|(item, option, _)| item.id == setting.id && option == value)
            .unwrap_or(0);
        self.open = Some(OpenPill { active, ..pill });
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
        let next = with_setting(&self.values, &setting.id, value);
        self.change(next, window, cx);
        self.dismiss(true, window, cx);
    }

    /// Keys while a popover is open: arrows wrap, Enter and Space pick,
    /// Escape closes.
    pub fn key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(open) = self.open.clone() else {
            return false;
        };
        let options = select_menu_options(&open.menu_settings());
        match key {
            "down" | "up" => {
                let len = options.len();
                if len > 0 {
                    let step = if key == "down" { 1 } else { len - 1 };
                    if let Some(open) = &mut self.open {
                        open.active = (open.active + step) % len;
                    }
                    cx.notify();
                }
                true
            }
            "enter" | "space" => {
                if let Some((setting, value, _)) = options.get(open.active) {
                    self.pick(setting, value, window, cx);
                }
                true
            }
            "escape" => {
                self.dismiss(true, window, cx);
                true
            }
            _ => false,
        }
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
        open: &OpenPill,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let settings = open.menu_settings();
        let grouped = settings.len() > 1;
        let reduced = cx.reduce_motion();
        let mut list = div()
            .flex()
            .flex_col()
            .p(u(4.))
            .font_family(theme.fonts.sans.clone())
            .debug_selector(|| "model-pill-menu".into());
        let mut index = 0;
        for (group_index, setting) in settings.iter().enumerate() {
            if group_index > 0 {
                list = list.child(menu_separator(theme));
            }
            if grouped {
                list = list.child(group_caption(&setting_label(setting), 4., theme));
            }
            let value = setting_value(setting, &self.values).to_string();
            for option in &setting.options {
                let row_index = index;
                index += 1;
                let selected = option.value == value;
                let picked = setting.clone();
                let option_value = option.value.clone();
                let selector = format!("model-pill-option-{}-{}", setting.id, option.label);
                let highlighted = row_index == open.active;
                let tone =
                    effort_tile_tone(self.harness, setting, &option.value).filter(|_| highlighted);
                let shimmer_id = format!("model-pill-effort-{}-{}", setting.id, option.value);
                list = list.child(
                    menu_row(("model-pill-option", row_index), 32., highlighted, theme)
                        .relative()
                        .debug_selector(move || selector)
                        .when_some(tone, |row, tone| {
                            row.child(effort_tiles(
                                SharedString::from(format!("{shimmer_id}-tiles")),
                                tone,
                                reduced,
                            ))
                        })
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if let (true, Some(open)) = (*hovered, this.open.as_mut())
                                && open.active != row_index
                            {
                                open.active = row_index;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.pick(&picked, &option_value, window, cx)
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(option.label.clone()),
                        )
                        .when(selected, |row| row.child(check_mark(0.50, theme)))
                        .when_some(tone, |row, tone| {
                            row.child(effort_glow(
                                SharedString::from(format!("{shimmer_id}-glow")),
                                tone,
                                u(theme.radius.lg),
                                reduced,
                            ))
                        }),
                );
            }
        }
        let outside = cx.listener(|this, event: &MouseDownEvent, window, cx| {
            if !this.trigger_bounds.contains(event.position) {
                this.dismiss(false, window, cx);
            }
        });
        popover_surface(
            "model-pill-menu",
            Some(SETTING_MENU_WIDTH),
            None,
            outside,
            list,
        )
    }
}

impl Focusable for ModelControlPills {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ModelControlPills {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut row = div()
            .id("model-control-pills")
            .flex()
            .items_center()
            .gap(u(4.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down));
        for pill in self.pills() {
            match pill {
                ControlPill::Toggle(setting) => {
                    let on = setting_value(&setting, &self.values) == "true";
                    let label = format!("{}: {}", setting.label, if on { "On" } else { "Off" });
                    let selector = format!("model-pill-{}", setting.id);
                    let clicked = setting.clone();
                    row = row.child(
                        toolbar_pill(selector.clone(), false, &theme)
                            .max_w(u(112.))
                            .debug_selector(move || selector)
                            .tooltip(monocode_ui::widgets::tooltip(label))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.toggle(&clicked, window, cx)
                            }))
                            .child(pill_label(
                                setting.label.clone(),
                                (!on).then(|| theme.content(0.50)),
                                &theme,
                            )),
                    );
                }
                ControlPill::Select { setting, grouped } => {
                    let open = self
                        .open
                        .as_ref()
                        .filter(|open| open.setting.id == setting.id)
                        .cloned();
                    let value_label = setting_value_label(&setting, &self.values);
                    let title = format!("{}: {}", setting_label(&setting), value_label);
                    let glyph = select_pill_icon(&setting);
                    let selector = format!("model-pill-{}", setting.id);
                    let clicked = setting.clone();
                    let clicked_group = grouped.clone();
                    let mut trigger = toolbar_pill(selector.clone(), open.is_some(), &theme)
                        .max_w(u(112.))
                        .debug_selector(move || selector)
                        .tooltip(monocode_ui::widgets::tooltip(title))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.toggle_select(&clicked, &clicked_group, window, cx)
                        }));
                    if open.is_some() {
                        trigger = trigger.child(self.trigger_bounds.probe());
                    }
                    if let Some(glyph) = glyph {
                        trigger = trigger
                            .child(icon(glyph).size(u(14.)).text_color(theme.colors.content));
                    }
                    trigger = trigger
                        .child(pill_label(value_label, None, &theme))
                        .child(pill_chevron(open.is_some(), &theme));
                    let mut cell = div().relative().child(trigger);
                    if let Some(open) = open {
                        cell = cell.child(anchored_popover(
                            Side::Top,
                            super::anchor::DEFAULT_GAP,
                            popover_layer(cx),
                            window,
                            self.render_menu(&open, &theme, cx),
                        ));
                    }
                    row = row.child(cell);
                }
            }
        }
        row
    }
}
