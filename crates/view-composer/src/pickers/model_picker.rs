//! Port of `ModelPicker` in src/features/sessions/ui/ModelPicker.tsx: the
//! composer's model trigger, its "Model and settings" menu with the option
//! submenus and the model flyout, and the recent-models quick switch.
//!
//! The picker takes its data as plain values: a [`ModelSource`] for the
//! catalog and availability, [`ModelPrefs`] for favorites, recents, and
//! hidden providers, and [`ProjectProviders`] for per-project hidden
//! providers. It reports through callbacks, which run deferred so an owner
//! may update the picker from inside one.
//!
//! The `App: Switch Model` hotkey is the [`SwitchModel`] action. The picker
//! handles it while focus is inside it; the composer forwards it from its
//! own key context with [`ModelPicker::toggle_from_hotkey`], after checking
//! that focus is not in a terminal or another picker.

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement as _, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    actions, div, prelude::FluentBuilder as _,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_core::models::{
    AgentModel, ModelPickerTab, ModelPrefs, ModelSetting, ModelSettingKind, coerce_model_picker_tab,
};
use monocode_core::{HarnessId, ModelSettings, ProjectProviders};
use monocode_ui::{IconName, ProviderLogo, Theme, UiStyled as _, icon, provider_logo, u};

use super::anchor::{
    BoundsCell, Side, anchored_popover, popover_layer, popover_surface, submenu_layer,
};
use super::effort_tiles::{effort_glow, effort_tile_tone, effort_tiles};
use super::model_flyout::render_flyout;
use super::model_logic::{
    self, MENU_WIDTH, SETTING_MENU_WIDTH, SUBMENU_OVERLAP, setting_label, setting_value,
    setting_value_label,
};
use super::model_source::ModelSource;
use super::style::{check_mark, menu_row, pill_chevron, pill_label, toolbar_pill};

actions!(
    model_picker,
    [
        /// `App: Switch Model`: toggle the recent-models menu.
        SwitchModel
    ]
);

pub(crate) type ChangeFn = Rc<dyn Fn(HarnessId, &str, &mut Window, &mut App)>;
pub(crate) type SettingsFn = Rc<dyn Fn(&ModelSettings, &mut Window, &mut App)>;
pub(crate) type CloseFn = Rc<dyn Fn(&mut Window, &mut App)>;
pub(crate) type FavoritesFn = Rc<dyn Fn(&[String], &mut Window, &mut App)>;

/// A provider logo for a harness id.
pub(crate) fn harness_logo(harness: HarnessId) -> ProviderLogo {
    ProviderLogo::from_id(harness.as_str()).unwrap_or(ProviderLogo::Claude)
}

/// One row of the "Model and settings" menu.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Setting(ModelSetting),
    Model,
}

/// The flyout off a menu row.
#[derive(Clone, Debug, PartialEq)]
pub enum Submenu {
    Setting(ModelSetting),
    Models,
}

/// What the picker shows: the session's provider, model, and option
/// values, plus where it sits.
#[derive(Clone, Debug, Default)]
pub struct ModelPickerProps {
    pub harness: Option<HarnessId>,
    pub model: String,
    pub values: ModelSettings,
    /// Project whose disabled providers are hidden from the picker.
    pub project: Option<String>,
    /// Hide option rows from the menu when they render as pills beside the
    /// picker.
    pub hide_settings: bool,
    /// Limit provider tabs for surfaces that only support some harnesses.
    pub allowed_harnesses: Option<Vec<HarnessId>>,
    pub hotkeys: bool,
}

pub struct ModelPicker {
    pub(crate) props: ModelPickerProps,
    pub(crate) source: Rc<dyn ModelSource>,
    pub(crate) prefs: ModelPrefs,
    pub(crate) projects: ProjectProviders,

    pub(crate) open: bool,
    pub(crate) tab: ModelPickerTab,
    pub(crate) active: usize,
    pub(crate) active_model: usize,
    pub(crate) active_setting: usize,
    pub(crate) recent_menu: Option<Vec<AgentModel>>,
    pub(crate) recent_active: usize,
    pub(crate) submenu: Option<Submenu>,
    pub(crate) query: String,
    pub(crate) favorites: Vec<String>,
    pub(crate) search: Entity<InputState>,
    focus: FocusHandle,
    /// Whether the last frame showed the model flyout, to focus its search
    /// once the frame that shows it is drawn.
    models_shown: bool,
    last_hotkey: Option<Instant>,

    trigger_bounds: BoundsCell,
    pub(crate) flyout_bounds: BoundsCell,
    submenu_bounds: BoundsCell,
    pub(crate) flyout_scroll: ScrollHandle,

    on_change: Option<ChangeFn>,
    on_settings_change: Option<SettingsFn>,
    on_close: Option<CloseFn>,
    on_favorites_change: Option<FavoritesFn>,
    _search_events: Subscription,
}

impl ModelPicker {
    pub fn new(
        props: ModelPickerProps,
        source: Rc<dyn ModelSource>,
        prefs: ModelPrefs,
        projects: ProjectProviders,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search models"));
        let search_events = cx.subscribe_in(&search, window, |this, search, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = search.read(cx).value().to_string();
                this.sync_active_model();
                cx.notify();
            }
        });
        let tab = props
            .harness
            .map(ModelPickerTab::Harness)
            .unwrap_or_default();
        let favorites = prefs.favorite_models.clone();
        Self {
            props,
            source,
            prefs,
            projects,
            open: false,
            tab,
            active: 0,
            active_model: 0,
            active_setting: 0,
            recent_menu: None,
            recent_active: 0,
            submenu: None,
            query: String::new(),
            favorites,
            search,
            focus: cx.focus_handle(),
            models_shown: false,
            last_hotkey: None,
            trigger_bounds: BoundsCell::default(),
            flyout_bounds: BoundsCell::default(),
            submenu_bounds: BoundsCell::default(),
            flyout_scroll: ScrollHandle::new(),
            on_change: None,
            on_settings_change: None,
            on_close: None,
            on_favorites_change: None,
            _search_events: search_events,
        }
    }

    /// Called with the picked provider and model key.
    pub fn on_change(
        mut self,
        f: impl Fn(HarnessId, &str, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// Called with every option value after one changes.
    pub fn on_settings_change(
        mut self,
        f: impl Fn(&ModelSettings, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_settings_change = Some(Rc::new(f));
        self
    }

    /// Called when the picker closes in a way that should return focus to
    /// the composer (Escape, a pick).
    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    /// Called with the new favorites, to save as `monocode.favoriteModels`.
    pub fn on_favorites_change(
        mut self,
        f: impl Fn(&[String], &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_favorites_change = Some(Rc::new(f));
        self
    }

    pub fn set_props(&mut self, props: ModelPickerProps, cx: &mut Context<Self>) {
        self.props = props;
        self.clamp_active();
        cx.notify();
    }

    /// The session's selection changed.
    pub fn set_selection(
        &mut self,
        harness: HarnessId,
        model: impl Into<String>,
        values: ModelSettings,
        cx: &mut Context<Self>,
    ) {
        self.props.harness = Some(harness);
        self.props.model = model.into();
        self.props.values = values;
        self.clamp_active();
        cx.notify();
    }

    /// A new catalog or availability probe arrived.
    pub fn set_source(&mut self, source: Rc<dyn ModelSource>, cx: &mut Context<Self>) {
        self.source = source;
        self.clamp_active();
        cx.notify();
    }

    pub fn set_prefs(&mut self, prefs: ModelPrefs, cx: &mut Context<Self>) {
        self.prefs = prefs;
        cx.notify();
    }

    pub fn set_projects(&mut self, projects: ProjectProviders, cx: &mut Context<Self>) {
        self.projects = projects;
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_recent_menu_open(&self) -> bool {
        self.recent_menu.is_some()
    }

    fn harness(&self) -> HarnessId {
        self.props.harness.unwrap_or(HarnessId::Claude)
    }

    /// `current`: the model the session uses, resolved against the source.
    pub fn current(&self) -> AgentModel {
        let model = Some(self.props.model.as_str()).filter(|model| !model.is_empty());
        self.source.resolve(self.harness(), model)
    }

    /// `entries`: option rows, then the Model row.
    pub fn entries(&self) -> Vec<MenuEntry> {
        let settings = if self.props.hide_settings {
            Vec::new()
        } else {
            model_logic::picker_settings(&self.current())
        };
        settings
            .into_iter()
            .map(MenuEntry::Setting)
            .chain(std::iter::once(MenuEntry::Model))
            .collect()
    }

    /// `pickerHarnesses`.
    pub fn picker_harnesses(&self) -> Vec<HarnessId> {
        model_logic::picker_harnesses(
            self.source.as_ref(),
            &self.prefs,
            &self.projects,
            self.props.project.as_deref(),
            self.props.allowed_harnesses.as_deref(),
        )
    }

    /// `visibleTab`.
    pub fn visible_tab(&self) -> ModelPickerTab {
        let harnesses = self.picker_harnesses();
        coerce_model_picker_tab(self.tab, |id| harnesses.contains(&id))
    }

    /// `visibleModels`.
    pub fn visible_models(&self) -> Vec<AgentModel> {
        model_logic::visible_models(
            self.visible_tab(),
            &self.favorites,
            &self.picker_harnesses(),
            self.source.as_ref(),
            &self.query,
        )
    }

    pub fn favorites(&self) -> &[String] {
        &self.favorites
    }

    pub fn active_model_index(&self) -> usize {
        self.active_model
    }

    pub fn recent_models(&self) -> Option<&[AgentModel]> {
        self.recent_menu.as_deref()
    }

    pub fn recent_active(&self) -> usize {
        self.recent_active
    }

    pub fn submenu(&self) -> Option<&Submenu> {
        self.submenu.as_ref()
    }

    pub fn active_entry(&self) -> usize {
        self.active
    }

    fn clamp_active(&mut self) {
        if !self.open {
            return;
        }
        let len = self.entries().len();
        self.active = self.active.min(len.saturating_sub(1));
    }

    /// The models list effect: highlight the current model, else the first.
    pub(crate) fn sync_active_model(&mut self) {
        if !self.open || self.submenu != Some(Submenu::Models) {
            return;
        }
        let current = self.current().id;
        self.active_model = self
            .visible_models()
            .iter()
            .position(|item| item.id == current)
            .unwrap_or(0);
        self.flyout_scroll_to_active();
    }

    fn sync_active_setting(&mut self) {
        if let Some(Submenu::Setting(setting)) = &self.submenu {
            let value = setting_value(setting, &self.props.values);
            self.active_setting = setting
                .options
                .iter()
                .position(|option| option.value == value)
                .unwrap_or(0);
        }
    }

    pub(crate) fn flyout_scroll_to_active(&self) {
        let tab = self.visible_tab();
        let models = self.visible_models();
        let mut child = 0;
        for group in model_logic::model_groups(tab, &models) {
            if group.name.is_some() {
                child += 1;
            }
            for (_, index) in &group.models {
                if *index == self.active_model {
                    self.flyout_scroll.scroll_to_item(child);
                    return;
                }
                child += 1;
            }
        }
    }

    fn fire_close(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.on_close.clone() {
            window.defer(cx, move |window, cx| f(window, cx));
        }
    }

    /// `dismiss`.
    pub fn dismiss(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.recent_menu = None;
        self.submenu = None;
        self.flyout_bounds.clear();
        self.submenu_bounds.clear();
        if restore {
            self.fire_close(window, cx);
        }
        cx.notify();
    }

    /// `togglePicker`: the trigger's click.
    pub fn toggle_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.dismiss(true, window, cx);
        } else {
            self.recent_menu = None;
            self.open_menu(window, cx);
        }
    }

    /// The effect that runs when the menu opens.
    fn open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        let current = self.current();
        self.source.refresh(&[current.harness]);
        let harnesses = self.picker_harnesses();
        self.tab = coerce_model_picker_tab(ModelPickerTab::Harness(current.harness), |id| {
            harnesses.contains(&id)
        });
        self.active = 0;
        self.submenu = self.props.hide_settings.then_some(Submenu::Models);
        self.query.clear();
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.favorites = self.prefs.favorite_models.clone();
        self.sync_active_model();
        if self.props.hide_settings {
            self.search
                .update(cx, |search, cx| search.focus(window, cx));
        } else {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// `openRecentMenu`: the trigger's right-click.
    pub fn open_recent_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.current();
        let models = model_logic::recent_menu_models(
            &selected,
            &self.prefs.recent_models,
            self.source.as_ref(),
        );
        self.recent_active = models
            .iter()
            .position(|item| item.id == selected.id)
            .unwrap_or(0);
        self.open = false;
        self.submenu = None;
        self.recent_menu = Some(models);
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// `toggleRecentMenu`.
    pub fn toggle_recent_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recent_menu.is_some() {
            self.recent_menu = None;
            self.fire_close(window, cx);
            cx.notify();
        } else {
            self.open_recent_menu(window, cx);
        }
    }

    /// `toggleFromHotkey`: the switch-model hotkey and the app menu item.
    /// Repeats within 80ms are ignored, as key auto-repeat would flicker.
    pub fn toggle_from_hotkey(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self
            .last_hotkey
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(80))
        {
            return;
        }
        self.last_hotkey = Some(now);
        self.toggle_recent_menu(window, cx);
    }

    fn set_setting(
        &mut self,
        setting: &ModelSetting,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = model_logic::with_setting(&self.props.values, &setting.id, value);
        if let Some(f) = self.on_settings_change.clone() {
            window.defer(cx, move |window, cx| f(&next, window, cx));
        }
    }

    /// `pickModel`: unavailable providers cannot be picked.
    pub fn pick_model(&mut self, item: &AgentModel, window: &mut Window, cx: &mut Context<Self>) {
        if !self.source.available(item.harness) {
            return;
        }
        if let Some(f) = self.on_change.clone() {
            let (harness, id) = (item.harness, item.id.clone());
            window.defer(cx, move |window, cx| f(harness, &id, window, cx));
        }
        self.dismiss(true, window, cx);
    }

    fn pick_setting(
        &mut self,
        setting: &ModelSetting,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_setting(setting, value, window, cx);
        self.dismiss(true, window, cx);
    }

    /// `toggleFavorite`.
    pub fn toggle_favorite(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.favorites.iter().any(|item| item == id) {
            self.favorites.retain(|item| item != id);
        } else {
            self.favorites.push(id.to_string());
        }
        self.prefs.favorite_models = self.favorites.clone();
        if let Some(f) = self.on_favorites_change.clone() {
            let next = self.favorites.clone();
            window.defer(cx, move |window, cx| f(&next, window, cx));
        }
        self.sync_active_model();
        cx.notify();
    }

    /// `selectTab`.
    pub fn select_tab(
        &mut self,
        next: ModelPickerTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tab = next;
        self.query.clear();
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.active_model = 0;
        if self.open
            && let ModelPickerTab::Harness(harness) = self.visible_tab()
        {
            self.source.refresh(&[harness]);
        }
        self.sync_active_model();
        cx.notify();
    }

    /// `showEntrySubmenu`.
    pub fn show_entry_submenu(&mut self, entry: &MenuEntry, cx: &mut Context<Self>) {
        self.submenu = match entry {
            MenuEntry::Model => Some(Submenu::Models),
            MenuEntry::Setting(setting) if setting.kind == ModelSettingKind::Select => {
                Some(Submenu::Setting(setting.clone()))
            }
            MenuEntry::Setting(_) => None,
        };
        self.sync_active_model();
        self.sync_active_setting();
        cx.notify();
    }

    fn move_entry(&mut self, direction: isize, cx: &mut Context<Self>) {
        let len = self.entries().len() as isize;
        self.submenu = None;
        self.active = ((self.active as isize + direction + len) % len) as usize;
        cx.notify();
    }

    pub(crate) fn set_active_model(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.active_model != index {
            self.active_model = index;
            cx.notify();
        }
    }

    /// `onMenuKey`: keys while the menu itself has focus. Returns whether
    /// the key was handled.
    pub fn menu_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let entries = self.entries();
        match key {
            "down" | "up" => {
                let down = key == "down";
                match &self.submenu {
                    Some(Submenu::Models) => {
                        let len = self.visible_models().len();
                        self.active_model = if down {
                            (self.active_model + 1).min(len.saturating_sub(1))
                        } else {
                            self.active_model.saturating_sub(1)
                        };
                        self.flyout_scroll_to_active();
                    }
                    Some(Submenu::Setting(setting)) => {
                        let len = setting.options.len();
                        self.active_setting = if down {
                            (self.active_setting + 1).min(len.saturating_sub(1))
                        } else {
                            self.active_setting.saturating_sub(1)
                        };
                    }
                    None => self.move_entry(if down { 1 } else { -1 }, cx),
                }
                cx.notify();
                true
            }
            "right" => {
                if let Some(entry) = entries.get(self.active) {
                    self.show_entry_submenu(&entry.clone(), cx);
                }
                true
            }
            "left" => {
                self.submenu = None;
                cx.notify();
                true
            }
            "enter" => {
                match self.submenu.clone() {
                    Some(Submenu::Models) => {
                        if let Some(item) = self.visible_models().get(self.active_model).cloned() {
                            self.pick_model(&item, window, cx);
                        }
                    }
                    Some(Submenu::Setting(setting)) => {
                        if let Some(option) = setting.options.get(self.active_setting).cloned() {
                            self.pick_setting(&setting, &option.value, window, cx);
                        }
                    }
                    None => match entries.get(self.active).cloned() {
                        Some(MenuEntry::Setting(setting))
                            if setting.kind == ModelSettingKind::Toggle =>
                        {
                            let value = setting_value(&setting, &self.props.values);
                            let next = if value == "true" { "false" } else { "true" };
                            self.set_setting(&setting, next, window, cx);
                        }
                        Some(entry) => self.show_entry_submenu(&entry, cx),
                        None => {}
                    },
                }
                true
            }
            _ => false,
        }
    }

    /// Keys while the recent menu is open: arrows wrap, Enter and Space pick.
    pub fn recent_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(models) = self.recent_menu.clone() else {
            return false;
        };
        match key {
            "down" | "up" => {
                let len = models.len();
                if len > 0 {
                    let step = if key == "down" { 1 } else { len - 1 };
                    self.recent_active = (self.recent_active + step) % len;
                    cx.notify();
                }
                true
            }
            "enter" | "space" => {
                if let Some(item) = models.get(self.recent_active) {
                    self.pick_model(item, window, cx);
                }
                true
            }
            _ => false,
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        let key = keystroke.key.as_str();
        if key == "escape" {
            if self.open {
                self.dismiss(true, window, cx);
                cx.stop_propagation();
            } else if self.recent_menu.is_some() {
                self.recent_menu = None;
                cx.notify();
                cx.stop_propagation();
            }
            return;
        }
        if self.recent_menu.is_some() {
            if self.recent_key(key, window, cx) {
                cx.stop_propagation();
            }
            return;
        }
        if !self.open || self.search.focus_handle(cx).is_focused(window) {
            return;
        }
        if self.menu_key(key, window, cx) {
            cx.stop_propagation();
        }
    }

    pub(crate) fn press_is_on_trigger(&self, event: &MouseDownEvent) -> bool {
        self.trigger_bounds.contains(event.position)
    }

    /// The menu's outside-press rule: presses on the trigger and on the
    /// picker's own flyouts are inside.
    fn press_is_inside(&self, event: &MouseDownEvent) -> bool {
        let at = event.position;
        self.trigger_bounds.contains(at)
            || self.flyout_bounds.contains(at)
            || self.submenu_bounds.contains(at)
    }
}

impl Focusable for ModelPicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ModelPicker {
    fn render_trigger(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let current = self.current();
        let effort = model_logic::trigger_effort_label(
            &current,
            &self.props.values,
            self.props.hide_settings,
        );
        let title = model_logic::trigger_title(&current, effort.as_deref());
        let tooltip = format!("{title} · Recent models: right-click or {}.", mod_key());
        let open = self.open;
        // The pill keeps its natural width so the model name stays whole on
        // one line.
        toolbar_pill("model-picker-trigger", open, theme)
            .debug_selector(|| "model-picker-trigger".into())
            .child(self.trigger_bounds.probe())
            .child(provider_logo(harness_logo(current.harness)).size(16.))
            .child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .text_px(theme.text.caption)
                    .leading(theme.leading.normal)
                    .child(current.name.clone()),
            )
            .when_some(effort, |el, effort| {
                el.child(pill_label(effort, Some(theme.content(0.50)), theme).flex_none())
            })
            .child(pill_chevron(open, theme))
            .tooltip(monocode_ui::widgets::tooltip(tooltip))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_recent_menu(window, cx);
                }),
            )
            .on_click(cx.listener(|this, _, window, cx| this.toggle_picker(window, cx)))
    }

    fn render_menu(
        &self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let current = self.current();
        let entries = self.entries();
        let mut list = div()
            .flex()
            .flex_col()
            .p(u(4.))
            .debug_selector(|| "model-menu".into());
        for (index, entry) in entries.into_iter().enumerate() {
            let highlighted = index == self.active;
            let row = menu_row(("model-menu-row", index), 36., highlighted, theme);
            let hover_entry = entry.clone();
            let row = row.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.active = index;
                    this.show_entry_submenu(&hover_entry, cx);
                }
            }));
            let row = match &entry {
                MenuEntry::Model => {
                    row.debug_selector(|| "model-menu-model".into())
                        .child(div().flex_1().min_w_0().child("Model"))
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .min_w_0()
                                .max_w(u(144.))
                                .items_center()
                                .gap(u(4.))
                                .text_color(theme.content(0.55))
                                .child(
                                    provider_logo(harness_logo(current.harness))
                                        .size(14.)
                                        .color(theme.content(0.55)),
                                )
                                .child(div().min_w_0().truncate().child(current.name.clone())),
                        )
                        .child(
                            icon(IconName::ChevronRight)
                                .size(u(14.))
                                .text_color(theme.content(0.45)),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_entry_submenu(&MenuEntry::Model, cx)
                        }))
                }
                MenuEntry::Setting(setting) => {
                    let value = setting_value(setting, &self.props.values).to_string();
                    let label = setting_label(setting);
                    let selector = format!("model-menu-{}", setting.id);
                    let row = row
                        .debug_selector(move || selector)
                        .child(div().flex_1().min_w_0().child(label));
                    let clicked = setting.clone();
                    if setting.kind == ModelSettingKind::Toggle {
                        let on = value == "true";
                        row.child(toggle_switch(on, theme)).on_click(cx.listener(
                            move |this, _, window, cx| {
                                let next = if on { "false" } else { "true" };
                                this.set_setting(&clicked, next, window, cx);
                            },
                        ))
                    } else {
                        row.child(
                            div()
                                .min_w_0()
                                .max_w(u(112.))
                                .truncate()
                                .text_color(theme.content(0.55))
                                .child(setting_value_label(setting, &self.props.values)),
                        )
                        .child(
                            icon(IconName::ChevronRight)
                                .size(u(14.))
                                .text_color(theme.content(0.45)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.show_entry_submenu(&MenuEntry::Setting(clicked.clone()), cx)
                        }))
                    }
                }
            };
            let mut cell = div().relative().child(row);
            if highlighted {
                match &self.submenu {
                    Some(Submenu::Setting(setting)) => {
                        cell = cell.child(anchored_popover(
                            Side::Right,
                            SUBMENU_OVERLAP,
                            submenu_layer(cx),
                            window,
                            self.render_setting_submenu(setting, theme, cx),
                        ));
                    }
                    Some(Submenu::Models) => {
                        let flyout = render_flyout(self, false, theme, window, cx);
                        cell = cell.child(anchored_popover(
                            Side::Right,
                            SUBMENU_OVERLAP,
                            submenu_layer(cx),
                            window,
                            flyout,
                        ));
                    }
                    None => {}
                }
            }
            list = list.child(cell);
        }
        let outside = cx.listener(|this, event: &MouseDownEvent, window, cx| {
            if !this.press_is_inside(event) {
                this.dismiss(false, window, cx);
            }
        });
        popover_surface(
            "model-menu",
            Some(MENU_WIDTH),
            None,
            outside,
            list.font_family(theme.fonts.sans.clone()),
        )
    }

    fn render_setting_submenu(
        &self,
        setting: &ModelSetting,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let value = setting_value(setting, &self.props.values).to_string();
        let harness = self.current().harness;
        let reduced = cx.reduce_motion();
        let mut list = div().relative().flex().flex_col().p(u(4.));
        for (index, option) in setting.options.iter().enumerate() {
            let selected = option.value == value;
            let highlighted = index == self.active_setting;
            let picked = setting.clone();
            let option_value = option.value.clone();
            let selector = format!("model-setting-option-{}", option.label);
            let tone = effort_tile_tone(harness, setting, &option.value).filter(|_| highlighted);
            let shimmer_id = format!("model-effort-{}-{}", setting.id, option.value);
            list = list.child(
                menu_row(("model-setting-option", index), 32., highlighted, theme)
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
                        if *hovered && this.active_setting != index {
                            this.active_setting = index;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_setting(&picked, &option_value, window, cx)
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
        let id: SharedString = format!("model-setting-menu-{}", setting.id).into();
        popover_surface(
            id,
            Some(SETTING_MENU_WIDTH),
            None,
            |_, _, _| {},
            list.child(self.submenu_bounds.probe()),
        )
    }

    fn render_recent_menu(
        &self,
        models: &[AgentModel],
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let current = self.current().id;
        let mut list = div()
            .flex()
            .flex_col()
            .p(u(4.))
            .debug_selector(|| "model-recent-menu".into());
        for (index, item) in models.iter().enumerate() {
            let selected = item.id == current;
            let highlighted = index == self.recent_active;
            let disabled = !self.source.available(item.harness);
            let picked = item.clone();
            let selector = format!("model-recent-{}", item.name);
            let caption = match &item.provider {
                Some(provider) => format!("{} · {}", item.harness.title(), provider.name),
                None => item.harness.title().to_string(),
            };
            let mut row = menu_row(
                ("model-recent-row", index),
                40.,
                highlighted && !disabled,
                theme,
            )
            .debug_selector(move || selector);
            if disabled {
                row = row
                    .text_color(theme.content(0.30))
                    .tooltip(monocode_ui::widgets::tooltip(
                        self.source.unavailable_hint(item.harness),
                    ));
            } else {
                row = row
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered && this.recent_active != index {
                            this.recent_active = index;
                            cx.notify();
                        }
                    }))
                    .on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.pick_model(&picked, window, cx)
                        }),
                    );
            }
            list = list.child(
                row.child(provider_logo(harness_logo(item.harness)).size(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_px(theme.text.body)
                                    .line_height(u(16.))
                                    .child(item.name.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_px(theme.text.caption)
                                    .line_height(u(16.))
                                    .text_color(theme.content(0.45))
                                    .child(caption),
                            ),
                    )
                    .when(selected, |row| row.child(check_mark(0.55, theme))),
            );
        }
        let outside = cx.listener(|this, event: &MouseDownEvent, _, cx| {
            if !this.trigger_bounds.contains(event.position) {
                this.recent_menu = None;
                cx.notify();
            }
        });
        popover_surface(
            "model-recent-menu",
            Some(MENU_WIDTH),
            None,
            outside,
            list.font_family(theme.fonts.sans.clone()),
        )
    }
}

/// The menu's on/off switch: `h-5 w-9 rounded-full`, `bg-content/35` on,
/// `bg-content/15` off, with a `size-4 bg-content` knob.
fn toggle_switch(on: bool, theme: &Theme) -> impl IntoElement + use<> {
    div()
        .relative()
        .flex_none()
        .w(u(36.))
        .h(u(20.))
        .rounded_full()
        .bg(theme.content(if on { 0.35 } else { 0.15 }))
        .child(
            div()
                .absolute()
                .top(u(2.))
                .left(u(if on { 18. } else { 2. }))
                .size(u(16.))
                .rounded_full()
                .bg(theme.colors.content)
                .shadow_sm(),
        )
}

/// `MOD`: the platform's command key name.
pub(crate) fn mod_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    }
}

impl ModelPicker {
    /// `autoFocusSearch`: the search takes focus once the frame that first
    /// shows the model flyout is drawn, from the Model row as well as beside
    /// the picker. When the flyout goes while the menu stays open, focus
    /// returns to the menu so its keys keep working. Upstream waits one
    /// animation frame because browsers drop focus on a hidden element. GPUI
    /// keeps focus on an unpainted handle, so deferring past this draw is
    /// enough.
    fn sync_search_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.open && matches!(self.submenu, Some(Submenu::Models));
        if shown == self.models_shown {
            return;
        }
        self.models_shown = shown;
        cx.defer_in(window, move |this, window, cx| {
            let showing = this.open && matches!(this.submenu, Some(Submenu::Models));
            if showing && shown {
                this.search
                    .update(cx, |search, cx| search.focus(window, cx));
            } else if this.open && !showing && this.search.focus_handle(cx).is_focused(window) {
                this.focus.focus(window, cx);
            }
        });
    }
}

impl Render for ModelPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        self.sync_search_focus(window, cx);
        let mut root = div()
            .id("model-picker")
            .relative()
            .flex_none()
            .track_focus(&self.focus)
            .key_context("ModelPicker")
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(|this, _: &SwitchModel, window, cx| {
                if this.props.hotkeys {
                    this.toggle_from_hotkey(window, cx);
                }
            }))
            .child(self.render_trigger(&theme, cx));
        if self.open && self.props.hide_settings {
            let flyout = render_flyout(self, true, &theme, window, cx);
            root = root.child(anchored_popover(
                Side::Top,
                super::anchor::DEFAULT_GAP,
                submenu_layer(cx),
                window,
                flyout,
            ));
        }
        if self.open && !self.props.hide_settings {
            let menu = self.render_menu(&theme, window, cx).into_any_element();
            root = root.child(anchored_popover(
                Side::Top,
                super::anchor::DEFAULT_GAP,
                popover_layer(cx),
                window,
                menu,
            ));
        }
        if let Some(models) = self.recent_menu.clone() {
            root = root.child(anchored_popover(
                Side::Top,
                super::anchor::DEFAULT_GAP,
                popover_layer(cx),
                window,
                self.render_recent_menu(&models, &theme, cx),
            ));
        }
        root
    }
}
