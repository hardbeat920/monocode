//! Port of src/features/sessions/ui/SecondOpinionButton.tsx: the turn
//! footer's Second opinion button, the Handoff button, and the plan card's
//! Build target button. Each opens a provider menu, a model flyout beside
//! the hovered provider, and an effort flyout beside the hovered model, so
//! a model and its effort are picked together.
//!
//! The provider list is `secondOpinionTargets` over what [`ModelMenuSource`]
//! reports: the installer probe, the picker's hidden providers, and the
//! model catalogs. Picks come out as [`SecondOpinionEvent::Pick`].

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use monocode_core::block::{ModelSettings, ModelTarget};
use monocode_core::harness::HARNESSES;
use monocode_core::models::{
    AgentModel, HarnessAvailability, ModelCatalog, ModelEnv, ModelPrefs, ModelSetting,
    model_effort_setting,
};
use monocode_core::{HarnessId, ProjectProviders};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use monocode_view_composer::pickers::anchor::{BoundsCell, popover_surface};

use super::parts::{Placement, anchored_popover, flip, harness_icon};

const MENU_WIDTH: f32 = 240.;
const SUBMENU_WIDTH: f32 = 240.;
const SUBMENU_MAX_HEIGHT: f32 = 288.;
const EFFORT_MENU_WIDTH: f32 = 200.;
/// The flyout tucks under the parent menu's edge rather than floating free.
const SUBMENU_OVERLAP: f32 = -4.;

/// What the menus read from the model catalogs, the installer probe, and
/// the model picker's saved preferences.
pub trait ModelMenuSource: 'static {
    /// `modelsFor`.
    fn models_for(&self, harness: HarnessId) -> Vec<AgentModel>;
    /// `hasLiveCatalog`.
    fn has_live_catalog(&self, harness: HarnessId) -> bool;
    /// `isHarnessAvailable`.
    fn available(&self, harness: HarnessId) -> bool;
    /// `hasProbedHarnessAvailability`.
    fn probed(&self) -> bool;
    /// `isPickerProviderVisible`.
    fn visible(&self, harness: HarnessId) -> bool;
    /// `preferredModelId`.
    fn preferred_model_id(&self, harness: HarnessId) -> String;
    /// `mergeModelSettings`.
    fn merge_model_settings(&self, model: &AgentModel, current: &ModelSettings) -> ModelSettings;
    /// `probeHarnessAvailability`, when the menu opens.
    fn probe_availability(&self) {}
    /// `refreshHarnessCatalogs`, when a provider is highlighted.
    fn refresh_catalogs(&self, _harnesses: &[HarnessId]) {}
}

type RefreshFn = Rc<dyn Fn(Option<&[HarnessId]>)>;

/// A [`ModelMenuSource`] over core's values. The owner makes a new one when
/// any of them changes.
#[derive(Clone, Default)]
pub struct CatalogMenuSource {
    pub catalog: ModelCatalog,
    pub prefs: ModelPrefs,
    pub availability: HarnessAvailability,
    pub projects: ProjectProviders,
    /// Called with `None` to probe availability and `Some` to refresh
    /// catalogs.
    refresh: Option<RefreshFn>,
}

impl CatalogMenuSource {
    pub fn new(
        catalog: ModelCatalog,
        prefs: ModelPrefs,
        availability: HarnessAvailability,
    ) -> Self {
        Self {
            catalog,
            prefs,
            availability,
            projects: ProjectProviders::default(),
            refresh: None,
        }
    }

    pub fn on_refresh(mut self, refresh: impl Fn(Option<&[HarnessId]>) + 'static) -> Self {
        self.refresh = Some(Rc::new(refresh));
        self
    }
}

impl ModelMenuSource for CatalogMenuSource {
    fn models_for(&self, harness: HarnessId) -> Vec<AgentModel> {
        self.catalog.models_for(harness).to_vec()
    }

    fn has_live_catalog(&self, harness: HarnessId) -> bool {
        self.catalog.has_live_catalog(harness)
    }

    fn available(&self, harness: HarnessId) -> bool {
        self.availability.is_available(harness)
    }

    fn probed(&self) -> bool {
        self.availability.probed
    }

    fn visible(&self, harness: HarnessId) -> bool {
        self.prefs.is_picker_provider_visible(harness)
    }

    fn preferred_model_id(&self, harness: HarnessId) -> String {
        ModelEnv {
            catalog: &self.catalog,
            prefs: &self.prefs,
            availability: &self.availability,
            projects: &self.projects,
        }
        .preferred_model_id(harness)
    }

    fn merge_model_settings(&self, model: &AgentModel, current: &ModelSettings) -> ModelSettings {
        self.catalog.merge_model_settings(model, Some(current))
    }

    fn probe_availability(&self) {
        if let Some(refresh) = &self.refresh {
            refresh(None);
        }
    }

    fn refresh_catalogs(&self, harnesses: &[HarnessId]) {
        if let Some(refresh) = &self.refresh {
            refresh(Some(harnesses));
        }
    }
}

/// `secondOpinionTargets`: every other installed provider the picker shows,
/// with `from` first when `include_current`.
// TODO(port): a copy of monocode_engine::submit::second_opinion's version,
// because view crates may not depend on the engine. Delete it when they can.
pub fn second_opinion_targets(
    from: HarnessId,
    installed: &dyn Fn(HarnessId) -> bool,
    visible: &dyn Fn(HarnessId) -> bool,
    probed: bool,
    include_current: bool,
) -> Vec<HarnessId> {
    let others: Vec<HarnessId> = HARNESSES
        .into_iter()
        .filter(|id| *id != from && visible(*id) && (!probed || installed(*id)))
        .collect();
    if include_current && (!probed || installed(from)) {
        return std::iter::once(from).chain(others).collect();
    }
    others
}

/// How the trigger looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TriggerStyle {
    /// A quiet icon button in the turn footer.
    #[default]
    Plain,
    /// The plan card's split Build button's chevron half.
    BuildTarget,
}

/// The button's props.
#[derive(Clone, Debug, PartialEq)]
pub struct SecondOpinionProps {
    pub from: HarnessId,
    pub from_model: Option<String>,
    pub from_settings: Option<ModelSettings>,
    pub icon: IconName,
    pub title: SharedString,
    pub disabled_title: SharedString,
    pub description: SharedString,
    pub menu_label: SharedString,
    pub include_current: bool,
    /// Hide `from_model` from `from`'s own model list instead of offering it
    /// back as a target. Only the plain Second opinion button sets this;
    /// the Build target button still needs to reselect the same model to
    /// change its effort.
    pub exclude_from_model: bool,
    pub disabled: bool,
    pub trigger: TriggerStyle,
}

impl SecondOpinionProps {
    /// `SecondOpinionButton` with its defaults.
    pub fn second_opinion(from: HarnessId) -> Self {
        Self {
            from,
            from_model: None,
            from_settings: None,
            icon: IconName::MessageMultiple,
            title: "Second opinion".into(),
            disabled_title: "No different model available for a second opinion".into(),
            description: "Send this turn to another agent to review the work.".into(),
            menu_label: "Send this turn to another agent".into(),
            include_current: false,
            exclude_from_model: false,
            disabled: false,
            trigger: TriggerStyle::Plain,
        }
    }

    /// `HandoffButton`.
    pub fn handoff(from: HarnessId) -> Self {
        Self {
            icon: IconName::Replace,
            title: "Handoff".into(),
            disabled_title: "Install another provider to hand off".into(),
            description: "Hand this session to another agent to continue the work.".into(),
            menu_label: "Hand this session to another agent".into(),
            ..Self::second_opinion(from)
        }
    }

    /// `BuildTargetButton`.
    pub fn build_target(
        from: HarnessId,
        model: Option<String>,
        settings: Option<ModelSettings>,
        disabled: bool,
    ) -> Self {
        Self {
            from_model: model,
            from_settings: settings,
            icon: IconName::ChevronDown,
            title: "Build with another model".into(),
            disabled_title: "No build providers are available".into(),
            description: "Choose the model and provider that should build this plan.".into(),
            menu_label: "Build this plan with another model or provider".into(),
            include_current: true,
            disabled,
            trigger: TriggerStyle::BuildTarget,
            ..Self::second_opinion(from)
        }
    }
}

/// Which menu the keyboard is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuLevel {
    #[default]
    Providers,
    Models,
    Effort,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SecondOpinionEvent {
    /// `onPick`.
    Pick(ModelTarget),
}

/// The button and its menus.
pub struct SecondOpinionButton {
    props: SecondOpinionProps,
    source: Rc<dyn ModelMenuSource>,
    open: bool,
    active: usize,
    model_active: usize,
    effort_active: usize,
    level: MenuLevel,
    focus: FocusHandle,
    trigger_bounds: BoundsCell,
    submenu_bounds: BoundsCell,
    effort_bounds: BoundsCell,
    menu_bounds: BoundsCell,
    provider_row_bounds: BoundsCell,
    model_row_bounds: BoundsCell,
    animate: bool,
}

impl EventEmitter<SecondOpinionEvent> for SecondOpinionButton {}

impl Focusable for SecondOpinionButton {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SecondOpinionButton {
    pub fn new(
        props: SecondOpinionProps,
        source: Rc<dyn ModelMenuSource>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            props,
            source,
            open: false,
            active: 0,
            model_active: 0,
            effort_active: 0,
            level: MenuLevel::Providers,
            focus: cx.focus_handle(),
            trigger_bounds: BoundsCell::default(),
            submenu_bounds: BoundsCell::default(),
            effort_bounds: BoundsCell::default(),
            menu_bounds: BoundsCell::default(),
            provider_row_bounds: BoundsCell::default(),
            model_row_bounds: BoundsCell::default(),
            animate: true,
        }
    }

    pub fn set_props(&mut self, props: SecondOpinionProps, cx: &mut Context<Self>) {
        if self.props == props {
            return;
        }
        let targets = self.targets();
        self.props = props;
        if self.targets() != targets {
            self.reset_targets();
        }
        self.sync_model_active();
        cx.notify();
    }

    /// A new catalog or probe result (the `useSyncExternalStore` versions).
    pub fn set_source(&mut self, source: Rc<dyn ModelMenuSource>, cx: &mut Context<Self>) {
        let targets = self.targets();
        self.source = source;
        if self.targets() != targets {
            self.reset_targets();
        }
        self.sync_model_active();
        cx.notify();
    }

    /// Turns popover animations off, for screenshots.
    pub fn set_animate(&mut self, animate: bool) {
        self.animate = animate;
    }

    // Derived state.

    pub fn targets(&self) -> Vec<HarnessId> {
        let source = &self.source;
        second_opinion_targets(
            self.props.from,
            &|id| source.available(id),
            &|id| source.visible(id),
            source.probed(),
            self.props.include_current,
        )
    }

    pub fn active_harness(&self) -> Option<HarnessId> {
        self.targets().get(self.active).copied()
    }

    /// The highlighted provider's models. The current model is not a
    /// second opinion on itself, so it is not an option once the picker
    /// lets the turn's own harness back in.
    pub fn models(&self) -> Vec<AgentModel> {
        let Some(harness) = self.active_harness() else {
            return Vec::new();
        };
        self.models_for(harness)
    }

    fn models_for(&self, harness: HarnessId) -> Vec<AgentModel> {
        let list = self.source.models_for(harness);
        match &self.props.from_model {
            Some(from_model) if self.props.exclude_from_model && harness == self.props.from => list
                .into_iter()
                .filter(|model| &model.id != from_model)
                .collect(),
            _ => list,
        }
    }

    fn preferred(&self) -> Option<String> {
        let harness = self.active_harness()?;
        Some(match &self.props.from_model {
            Some(model) if harness == self.props.from && !self.props.exclude_from_model => {
                model.clone()
            }
            _ => self.source.preferred_model_id(harness),
        })
    }

    fn active_model(&self) -> Option<AgentModel> {
        self.models().get(self.model_active).cloned()
    }

    fn effort_of(model: &AgentModel) -> Option<ModelSetting> {
        model_effort_setting(model)
            .filter(|setting| !setting.options.is_empty())
            .cloned()
    }

    fn selected_effort_value(&self, model: &AgentModel, effort: &ModelSetting) -> String {
        if Some(&model.id) == self.props.from_model.as_ref() {
            self.props
                .from_settings
                .as_ref()
                .and_then(|settings| settings.get(&effort.id))
                .cloned()
                .unwrap_or_else(|| effort.value.clone())
        } else {
            effort.value.clone()
        }
    }

    /// With the current model hidden, a target harness only counts as
    /// usable if it still has a model left. A harness whose catalog has not
    /// loaded yet (Codex has no bundled list) cannot be confirmed empty, so
    /// it gets the benefit of the doubt.
    fn has_selectable_model(&self) -> bool {
        let (true, Some(from_model)) = (self.props.exclude_from_model, &self.props.from_model)
        else {
            return true;
        };
        self.targets().into_iter().any(|harness| {
            if !self.source.has_live_catalog(harness) {
                return true;
            }
            let list = self.source.models_for(harness);
            if harness == self.props.from {
                list.iter().any(|model| &model.id != from_model)
            } else {
                !list.is_empty()
            }
        })
    }

    fn no_targets(&self) -> bool {
        self.targets().is_empty() || !self.has_selectable_model()
    }

    pub fn is_disabled(&self) -> bool {
        self.props.disabled || self.no_targets()
    }

    /// The trigger's title and accessible name.
    pub fn label(&self) -> SharedString {
        if self.no_targets() {
            self.props.disabled_title.clone()
        } else {
            self.props.title.clone()
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn menu_level(&self) -> MenuLevel {
        self.level
    }

    /// The model flyout shows.
    pub fn shows_submenu(&self) -> bool {
        self.open
            && self.level != MenuLevel::Providers
            && self.active_harness().is_some()
            && !self.models().is_empty()
    }

    /// The model flyout's accessible name.
    pub fn submenu_label(&self) -> Option<String> {
        if !self.shows_submenu() {
            return None;
        }
        Some(format!("{} models", self.active_harness()?.title()))
    }

    /// The effort flyout's model and options, when it shows.
    pub fn effort_menu(&self) -> Option<(AgentModel, ModelSetting)> {
        if !self.shows_submenu() || self.level != MenuLevel::Effort {
            return None;
        }
        let model = self.active_model()?;
        let effort = Self::effort_of(&model)?;
        Some((model, effort))
    }

    // Effects.

    /// `[open, targets]`: back to the first provider.
    fn reset_targets(&mut self) {
        self.active = 0;
        self.level = MenuLevel::Providers;
        self.sync_model_active();
    }

    /// `[activeHarness, preferred, models]`: highlight the preferred model.
    fn sync_model_active(&mut self) {
        let preferred = self.preferred();
        self.model_active = self
            .models()
            .iter()
            .position(|model| Some(&model.id) == preferred.as_ref())
            .unwrap_or(0);
        self.sync_effort_active();
    }

    /// `[activeEffort, selectedEffortValue]`: highlight the selected effort.
    fn sync_effort_active(&mut self) {
        self.effort_active = self
            .active_model()
            .and_then(|model| {
                let effort = Self::effort_of(&model)?;
                let selected = self.selected_effort_value(&model, &effort);
                effort
                    .options
                    .iter()
                    .position(|option| option.value == selected)
            })
            .unwrap_or(0);
    }

    fn set_active(&mut self, index: usize) {
        if self.active != index {
            self.active = index;
            self.sync_model_active();
            if let Some(harness) = self.active_harness() {
                self.source.refresh_catalogs(&[harness]);
            }
        }
    }

    fn set_model_active(&mut self, index: usize) {
        if self.model_active != index {
            self.model_active = index;
            self.sync_effort_active();
        }
    }

    // Actions.

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_disabled() {
            return;
        }
        if self.open {
            self.dismiss(false, window, cx);
            return;
        }
        self.open = true;
        self.reset_targets();
        self.source.probe_availability();
        if let Some(harness) = self.active_harness() {
            self.source.refresh_catalogs(&[harness]);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn dismiss(&mut self, restore_focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.level = MenuLevel::Providers;
        if restore_focus {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// The pointer entered provider row `index`.
    pub fn hover_provider(&mut self, index: usize, cx: &mut Context<Self>) {
        self.set_active(index);
        self.level = MenuLevel::Models;
        cx.notify();
    }

    /// A click on provider row `index`.
    pub fn click_provider(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(harness) = self.targets().get(index).copied() else {
            return;
        };
        if !self.source.available(harness) && self.source.probed() {
            return;
        }
        self.set_active(index);
        if !self.models_for(harness).is_empty() {
            self.level = MenuLevel::Models;
        }
        cx.notify();
    }

    /// The pointer entered the model row `index`.
    pub fn hover_model(&mut self, index: usize, cx: &mut Context<Self>) {
        self.set_model_active(index);
        let has_effort = self
            .models()
            .get(index)
            .is_some_and(|model| Self::effort_of(model).is_some());
        self.level = if has_effort {
            MenuLevel::Effort
        } else {
            MenuLevel::Models
        };
        cx.notify();
    }

    /// A click on model row `index`: its effort menu, or the pick.
    pub fn click_model(&mut self, index: usize, cx: &mut Context<Self>) {
        self.set_model_active(index);
        if let Some(model) = self.models().get(index).cloned() {
            self.open_effort_or_pick(&model, cx);
        }
        cx.notify();
    }

    pub fn hover_effort(&mut self, index: usize, cx: &mut Context<Self>) {
        self.effort_active = index;
        cx.notify();
    }

    /// Picks the highlighted model with effort option `index`.
    pub fn pick_effort(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((model, effort)) = self.effort_menu() else {
            return;
        };
        if let Some(option) = effort.options.get(index) {
            self.pick(&model, Some(&option.value), cx);
        }
    }

    fn open_effort_or_pick(&mut self, model: &AgentModel, cx: &mut Context<Self>) {
        if Self::effort_of(model).is_some() {
            self.level = MenuLevel::Effort;
        } else {
            self.pick(model, None, cx);
        }
    }

    fn pick(&mut self, model: &AgentModel, effort_value: Option<&str>, cx: &mut Context<Self>) {
        let mut current = ModelSettings::new();
        if Some(&model.id) == self.props.from_model.as_ref()
            && let Some(settings) = &self.props.from_settings
        {
            current.extend(settings.clone());
        }
        if let (Some(effort), Some(value)) = (model_effort_setting(model), effort_value) {
            current.insert(effort.id.clone(), value.to_string());
        }
        let model_settings = self.source.merge_model_settings(model, &current);
        self.open = false;
        self.level = MenuLevel::Providers;
        cx.emit(SecondOpinionEvent::Pick(ModelTarget {
            harness: model.harness,
            model: model.id.clone(),
            model_settings,
        }));
        cx.notify();
    }

    fn open_models(&mut self) {
        if !self.models().is_empty() {
            self.level = MenuLevel::Models;
        }
    }

    fn move_by(&mut self, step: isize) {
        let wrap = |index: usize, len: usize| {
            ((index as isize + step + len as isize) % len as isize) as usize
        };
        match self.level {
            MenuLevel::Effort => {
                if let Some((_, effort)) = self.effort_menu() {
                    self.effort_active = wrap(self.effort_active, effort.options.len());
                }
            }
            MenuLevel::Models => {
                let len = self.models().len();
                if len > 0 {
                    self.set_model_active(wrap(self.model_active, len));
                }
            }
            MenuLevel::Providers => {
                let len = self.targets().len();
                if len > 0 {
                    self.level = MenuLevel::Providers;
                    self.set_active(wrap(self.active, len));
                }
            }
        }
    }

    /// `onMenuKey`.
    pub fn key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.open {
            return false;
        }
        match key {
            "down" => self.move_by(1),
            "up" => self.move_by(-1),
            "right" => match self.level {
                MenuLevel::Providers => self.open_models(),
                MenuLevel::Models => {
                    if self
                        .active_model()
                        .is_some_and(|model| Self::effort_of(&model).is_some())
                    {
                        self.level = MenuLevel::Effort;
                    }
                }
                MenuLevel::Effort => {}
            },
            "left" => {
                self.level = if self.level == MenuLevel::Effort {
                    MenuLevel::Models
                } else {
                    MenuLevel::Providers
                };
            }
            "enter" => {
                if self.active_harness().is_none() {
                    return true;
                }
                match self.level {
                    MenuLevel::Effort => {
                        let index = self.effort_active;
                        self.pick_effort(index, cx);
                    }
                    MenuLevel::Models => {
                        if let Some(model) = self.active_model() {
                            self.open_effort_or_pick(&model, cx);
                        }
                    }
                    MenuLevel::Providers => self.open_models(),
                }
            }
            "escape" => self.dismiss(true, window, cx),
            _ => return false,
        }
        cx.notify();
        true
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.key(&event.keystroke.key, window, cx) {
            cx.stop_propagation();
        }
    }

    fn press_is_inside(&self, event: &MouseDownEvent) -> bool {
        let position = event.position;
        self.trigger_bounds.contains(position)
            || self.submenu_bounds.contains(position)
            || self.effort_bounds.contains(position)
    }

    // Drawing.

    fn render_trigger(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let disabled = self.is_disabled();
        let label = self.label();
        let glyph = self.props.icon;
        let el = match self.props.trigger {
            TriggerStyle::Plain => {
                let hover = theme.content(0.08);
                let hover_ink = theme.content(0.70);
                let ink = if self.open {
                    theme.content(0.70)
                } else {
                    theme.content(0.40)
                };
                div()
                    .id("second-opinion-trigger")
                    .group("second-opinion-trigger")
                    .rounded(u(theme.radius.md))
                    .p(u(4.))
                    .when(self.open, |el| el.bg(hover))
                    .when(!self.open && !disabled, |el| {
                        el.hover(move |style| style.bg(hover))
                    })
                    .when(disabled, |el| el.opacity(0.4))
                    .child(
                        icon(glyph)
                            .size(u(14.))
                            .text_color(ink)
                            .when(!disabled, |svg| {
                                svg.group_hover("second-opinion-trigger", move |style| {
                                    style.text_color(hover_ink)
                                })
                            }),
                    )
            }
            TriggerStyle::BuildTarget => {
                let fill = theme.colors.content;
                let hover = theme.content(0.90);
                div()
                    .id("build-target-trigger")
                    .flex()
                    .flex_none()
                    .size(u(24.))
                    .items_center()
                    .justify_center()
                    .rounded_r(u(theme.radius.md))
                    .border_l_1()
                    .border_color(monocode_ui::color::with_alpha(
                        theme.colors.background_base,
                        0.2,
                    ))
                    .bg(fill)
                    .when(!disabled, |el| el.hover(move |style| style.bg(hover)))
                    .when(disabled, |el| el.opacity(0.4))
                    .child(
                        icon(glyph)
                            .size(u(14.))
                            .text_color(theme.colors.background_base),
                    )
            }
        };
        el.tooltip(tooltip(label))
            .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx)))
            .child(self.trigger_bounds.probe())
            .into_any_element()
    }

    fn row(
        id: impl Into<gpui::ElementId>,
        highlighted: bool,
        theme: &Theme,
    ) -> gpui::Stateful<gpui::Div> {
        let hover = theme.content(0.05);
        div()
            .id(id)
            .flex()
            .h(u(32.))
            .w_full()
            .items_center()
            .gap(u(8.))
            .rounded(u(theme.radius.lg))
            .px(u(8.))
            .text_px(13.)
            .leading(theme.leading.none)
            .text_color(theme.colors.content)
            .when(highlighted, |el| el.bg(theme.colors.selection))
            .when(!highlighted, |el| el.hover(move |style| style.bg(hover)))
    }

    fn render_menu(&self, theme: &Theme, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let targets = self.targets();
        let mut list = div()
            .flex()
            .flex_col()
            .p(u(4.))
            .font_family(theme.fonts.sans.clone())
            .child(
                div().px(u(6.)).pb(u(8.)).pt(u(6.)).child(
                    div()
                        .text_px(11.)
                        .line_height(u(12.))
                        .text_color(theme.content(0.50))
                        .child(self.props.description.clone()),
                ),
            )
            .child(div().mx(u(4.)).mb(u(4.)).h(px(1.)).bg(theme.content(0.10)));
        if targets.is_empty() {
            list = list.child(
                div()
                    .px(u(10.))
                    .py(u(8.))
                    .text_px(12.)
                    .line_height(u(16.))
                    .text_color(theme.content(0.50))
                    .child(self.props.disabled_title.clone()),
            );
        }
        let probed = self.source.probed();
        for (index, harness) in targets.iter().copied().enumerate() {
            let highlighted = index == self.active;
            let unavailable = !self.source.available(harness) && probed;
            let has_models = !self.models_for(harness).is_empty();
            let mut row = Self::row(
                ("second-opinion-provider", index),
                highlighted && !unavailable,
                theme,
            )
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hover_provider(index, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.click_provider(index, cx)))
            .child(harness_icon(harness, 14.))
            .child(div().min_w_0().flex_1().truncate().child(harness.title()));
            if unavailable {
                row = row.text_color(theme.content(0.30));
            }
            if has_models {
                row = row.child(
                    icon(IconName::ChevronRight)
                        .size(u(14.))
                        .text_color(theme.content(0.40)),
                );
            }
            let mut cell = div().relative().child(row);
            if highlighted {
                cell = cell.child(self.provider_row_bounds.probe());
            }
            if highlighted && self.shows_submenu() {
                let side = flip(
                    Placement::RightStart,
                    self.provider_row_bounds.get(),
                    gpui::size(u(SUBMENU_WIDTH).to_pixels(window.rem_size()), px(0.)),
                    SUBMENU_OVERLAP,
                    window,
                );
                cell = cell.child(anchored_popover(
                    side,
                    SUBMENU_OVERLAP,
                    theme.layer.submenu,
                    window,
                    self.render_models(theme, window, cx),
                ));
            }
            list = list.child(cell);
        }
        let outside = cx.listener(|this, event: &MouseDownEvent, window, cx| {
            if !this.press_is_inside(event) {
                this.dismiss(false, window, cx);
            }
        });
        popover_surface(
            "second-opinion-menu",
            Some(MENU_WIDTH),
            None,
            outside,
            list.relative().child(self.menu_bounds.probe()),
        )
        .into_any_element()
    }

    fn render_models(&self, theme: &Theme, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let preferred = self.preferred();
        let effort_menu = self.effort_menu();
        let mut list = div()
            .id("second-opinion-models")
            .flex()
            .flex_col()
            .p(u(4.))
            .max_h(u(SUBMENU_MAX_HEIGHT))
            .overflow_y_scroll()
            .font_family(theme.fonts.sans.clone());
        for (index, model) in self.models().into_iter().enumerate() {
            let highlighted = index == self.model_active;
            let has_effort = Self::effort_of(&model).is_some();
            let row = Self::row(("second-opinion-model", index), highlighted, theme)
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered {
                        this.hover_model(index, cx);
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.click_model(index, cx)))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .child(model.name.clone()),
                )
                .when(Some(&model.id) == preferred.as_ref(), |row| {
                    row.child(
                        icon(IconName::Check)
                            .size(u(12.))
                            .text_color(theme.content(0.45)),
                    )
                })
                .when(has_effort, |row| {
                    row.child(
                        icon(IconName::ChevronRight)
                            .size(u(14.))
                            .text_color(theme.content(0.40)),
                    )
                });
            let mut cell = div().relative().child(row);
            if highlighted {
                cell = cell.child(self.model_row_bounds.probe());
            }
            if highlighted && let Some((model, effort)) = &effort_menu {
                let side = flip(
                    Placement::RightStart,
                    self.model_row_bounds.get(),
                    gpui::size(u(EFFORT_MENU_WIDTH).to_pixels(window.rem_size()), px(0.)),
                    SUBMENU_OVERLAP,
                    window,
                );
                cell = cell.child(anchored_popover(
                    side,
                    SUBMENU_OVERLAP,
                    theme.layer.submenu + 1,
                    window,
                    self.render_effort(model, effort, theme, cx),
                ));
            }
            list = list.child(cell);
        }
        popover_surface(
            "second-opinion-models-surface",
            Some(SUBMENU_WIDTH),
            None,
            |_, _, _| {},
            div()
                .relative()
                .child(list)
                .child(self.submenu_bounds.probe()),
        )
        .into_any_element()
    }

    fn render_effort(
        &self,
        model: &AgentModel,
        effort: &ModelSetting,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selected_effort_value(model, effort);
        let mut list = div()
            .relative()
            .flex()
            .flex_col()
            .p(u(4.))
            .font_family(theme.fonts.sans.clone());
        for (index, option) in effort.options.iter().enumerate() {
            let highlighted = index == self.effort_active;
            list = list.child(
                Self::row(("second-opinion-effort", index), highlighted, theme)
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.hover_effort(index, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_effort(index, cx)))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .child(option.label.clone()),
                    )
                    .when(option.value == selected, |row| {
                        row.child(
                            icon(IconName::Check)
                                .size(u(14.))
                                .text_color(theme.content(0.50)),
                        )
                    }),
            );
        }
        popover_surface(
            "second-opinion-effort-surface",
            Some(EFFORT_MENU_WIDTH),
            None,
            |_, _, _| {},
            list.child(self.effort_bounds.probe()),
        )
        .into_any_element()
    }
}

impl Render for SecondOpinionButton {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut root = div()
            .id("second-opinion")
            .relative()
            .flex_none()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.render_trigger(&theme, cx));
        if self.open {
            if !self.shows_submenu() {
                self.submenu_bounds.clear();
            }
            if self.effort_menu().is_none() {
                self.effort_bounds.clear();
            }
            let menu = self.render_menu(&theme, window, cx);
            let side = flip(
                Placement::TopCenter,
                self.trigger_bounds.get(),
                self.menu_bounds
                    .get()
                    .map_or(gpui::Size::default(), |bounds| bounds.size),
                monocode_ui::widgets::POPOVER_GAP,
                window,
            );
            root = root.child(anchored_popover(
                side,
                monocode_ui::widgets::POPOVER_GAP,
                theme.layer.popover,
                window,
                menu,
            ));
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};
    use monocode_core::models::{ModelSettingChoice, ModelSettingKind};
    use std::cell::RefCell;

    fn grok_models() -> Vec<AgentModel> {
        let mut review = AgentModel::new("grok:review", HarnessId::Grok, "Review Model");
        review.settings = Some(vec![ModelSetting {
            id: "effort".into(),
            label: "Reasoning".into(),
            kind: ModelSettingKind::Select,
            value: "high".into(),
            options: [("xhigh", "Extra High"), ("high", "High"), ("low", "Low")]
                .iter()
                .map(|(value, label)| ModelSettingChoice {
                    value: (*value).into(),
                    label: (*label).into(),
                })
                .collect(),
            description: None,
        }]);
        vec![
            review,
            AgentModel::new("grok:quick", HarnessId::Grok, "Quick Model"),
        ]
    }

    fn source(installed: &[HarnessId], grok: Vec<AgentModel>) -> Rc<dyn ModelMenuSource> {
        let mut catalog = ModelCatalog::new();
        catalog.set_harness_models(HarnessId::Grok, grok);
        Rc::new(CatalogMenuSource::new(
            catalog,
            ModelPrefs::default(),
            HarnessAvailability {
                installed: installed.iter().copied().collect(),
                probed: true,
            },
        ))
    }

    type Picks = Rc<RefCell<Vec<ModelTarget>>>;

    fn mount(
        cx: &mut TestAppContext,
        props: SecondOpinionProps,
        source: Rc<dyn ModelMenuSource>,
    ) -> (
        gpui::Entity<SecondOpinionButton>,
        &mut VisualTestContext,
        Picks,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
        });
        let picks: Picks = Rc::default();
        let sink = picks.clone();
        let (button, cx) = cx.add_window_view(|_, cx| SecondOpinionButton::new(props, source, cx));
        cx.update(|_, cx| {
            cx.subscribe(&button, move |_, event: &SecondOpinionEvent, _| {
                let SecondOpinionEvent::Pick(target) = event;
                sink.borrow_mut().push(target.clone());
            })
            .detach();
        });
        (button, cx, picks)
    }

    fn grok_index(button: &SecondOpinionButton) -> usize {
        button
            .targets()
            .iter()
            .position(|harness| harness.title() == "Grok Build")
            .expect("Grok Build row")
    }

    #[gpui::test]
    fn opens_effort_beside_a_hovered_model_and_selects_both_atomically(cx: &mut TestAppContext) {
        let (button, cx, picks) = mount(
            cx,
            SecondOpinionProps::second_opinion(HarnessId::Cursor),
            source(&[HarnessId::Grok], grok_models()),
        );
        cx.update(|window, cx| button.update(cx, |button, cx| button.toggle(window, cx)));
        cx.update(|_, cx| {
            button.update(cx, |button, cx| {
                assert_eq!(button.label(), "Second opinion");
                let grok = grok_index(button);
                button.hover_provider(grok, cx);
                assert_eq!(button.submenu_label().as_deref(), Some("Grok Build models"));
                let review = button
                    .models()
                    .iter()
                    .position(|model| model.name == "Review Model")
                    .unwrap();
                button.hover_model(review, cx);
            })
        });
        assert!(picks.borrow().is_empty());
        cx.update(|_, cx| {
            button.update(cx, |button, cx| {
                let (model, effort) = button.effort_menu().expect("effort menu");
                assert_eq!(format!("{} effort", model.name), "Review Model effort");
                let extra_high = effort
                    .options
                    .iter()
                    .position(|option| option.label == "Extra High")
                    .unwrap();
                button.pick_effort(extra_high, cx);
            })
        });
        assert_eq!(
            *picks.borrow(),
            [ModelTarget {
                harness: HarnessId::Grok,
                model: "grok:review".into(),
                model_settings: [("effort".to_string(), "xhigh".to_string())]
                    .into_iter()
                    .collect(),
            }]
        );
    }

    #[gpui::test]
    fn selects_a_model_without_effort_directly(cx: &mut TestAppContext) {
        let (button, cx, picks) = mount(
            cx,
            SecondOpinionProps::second_opinion(HarnessId::Cursor),
            source(&[HarnessId::Grok], grok_models()),
        );
        cx.update(|window, cx| button.update(cx, |button, cx| button.toggle(window, cx)));
        cx.update(|_, cx| {
            button.update(cx, |button, cx| {
                let grok = grok_index(button);
                button.hover_provider(grok, cx);
                let quick = button
                    .models()
                    .iter()
                    .position(|model| model.name == "Quick Model")
                    .unwrap();
                button.click_model(quick, cx);
            })
        });
        assert_eq!(
            *picks.borrow(),
            [ModelTarget {
                harness: HarnessId::Grok,
                model: "grok:quick".into(),
                model_settings: ModelSettings::new(),
            }]
        );
    }

    #[gpui::test]
    fn hides_the_current_model_from_a_same_harness_second_opinion(cx: &mut TestAppContext) {
        let props = SecondOpinionProps {
            from_model: Some("grok:review".into()),
            include_current: true,
            exclude_from_model: true,
            ..SecondOpinionProps::second_opinion(HarnessId::Grok)
        };
        let (button, cx, _) = mount(cx, props, source(&[HarnessId::Grok], grok_models()));
        cx.update(|window, cx| button.update(cx, |button, cx| button.toggle(window, cx)));
        cx.update(|_, cx| {
            button.update(cx, |button, cx| {
                let grok = grok_index(button);
                button.hover_provider(grok, cx);
                let names: Vec<String> = button
                    .models()
                    .into_iter()
                    .map(|model| model.name)
                    .collect();
                assert!(!names.iter().any(|name| name == "Review Model"));
                assert!(names.iter().any(|name| name == "Quick Model"));
            })
        });
    }

    #[gpui::test]
    fn disables_the_second_opinion_button_once_the_current_model_is_the_only_one_left(
        cx: &mut TestAppContext,
    ) {
        let props = SecondOpinionProps {
            from_model: Some("grok:review".into()),
            include_current: true,
            exclude_from_model: true,
            ..SecondOpinionProps::second_opinion(HarnessId::Grok)
        };
        let only = vec![AgentModel::new(
            "grok:review",
            HarnessId::Grok,
            "Review Model",
        )];
        let (button, cx, _) = mount(cx, props, source(&[HarnessId::Grok], only));
        cx.update(|_, cx| {
            let button = button.read(cx);
            assert_eq!(
                button.label(),
                "No different model available for a second opinion"
            );
            assert!(button.is_disabled());
        });
    }

    #[gpui::test]
    fn keeps_the_button_enabled_for_an_installed_provider_whose_catalog_has_not_loaded_yet(
        cx: &mut TestAppContext,
    ) {
        // Codex has no bundled list, so its models are empty until the live
        // catalog loads. The button must stay enabled so the menu can open
        // and ask for that catalog.
        let props = SecondOpinionProps {
            from_model: Some("cursor:main".into()),
            include_current: true,
            exclude_from_model: true,
            ..SecondOpinionProps::second_opinion(HarnessId::Cursor)
        };
        let (button, cx, _) = mount(cx, props, source(&[HarnessId::Codex], grok_models()));
        cx.update(|_, cx| {
            let button = button.read(cx);
            assert_eq!(button.label(), "Second opinion");
            assert!(!button.is_disabled());
        });
    }

    #[gpui::test]
    fn walks_the_menus_with_the_keyboard(cx: &mut TestAppContext) {
        let (button, cx, picks) = mount(
            cx,
            SecondOpinionProps::second_opinion(HarnessId::Cursor),
            source(&[HarnessId::Grok], grok_models()),
        );
        cx.update(|window, cx| button.update(cx, |button, cx| button.toggle(window, cx)));
        cx.update(|window, cx| {
            button.update(cx, |button, cx| {
                button.key("right", window, cx);
                assert_eq!(button.menu_level(), MenuLevel::Models);
                // The preferred model (the catalog default) starts highlighted.
                button.key("enter", window, cx);
                assert_eq!(button.menu_level(), MenuLevel::Effort);
                button.key("up", window, cx);
                button.key("enter", window, cx);
            })
        });
        assert_eq!(picks.borrow().len(), 1);
        assert_eq!(picks.borrow()[0].model, "grok:review");
        assert_eq!(
            picks.borrow()[0]
                .model_settings
                .get("effort")
                .map(String::as_str),
            Some("xhigh")
        );
    }
}
