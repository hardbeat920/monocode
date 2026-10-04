//! Port of src/features/quick-composer/ui/QuickModelSelector.tsx: an inline
//! model browser. Providers share the full width above the search box and
//! results; reasoning effort and fast mode sit under the list. Permissions
//! have their own picker ([`crate::QuickPermissions`]).

use gpui::{
    AnyElement, App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement as _, Pixels, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, canvas, div, px, relative,
    svg,
};
use monocode_core::HarnessId;
use monocode_core::block::ModelSettings;
use monocode_core::models::{AgentModel, ModelCatalog, ModelPickerTab, ModelPrefs, ModelSetting};
use monocode_ui::{IconName, ProviderLogo, Theme, UiStyled as _, icon, provider_logo, u};
use monocode_view_composer::composer::prompt_input::{self, PromptInput, PromptInputEvent};

use crate::colors;
use crate::field::search_field;
use crate::model::selector::{
    FastToggle, active_row, change_setting, effort_index, effort_setting, empty_label, fast_toggle,
    filter_quick_models, model_enabled, reset_settings, selector_pool, selector_providers,
    selector_tabs, toggle_favorite, visible_tab,
};

/// The data `QuickModelSelector` shows. The owner pushes a new value with
/// [`QuickModelSelector::set_props`] when its state changes.
#[derive(Clone, Debug)]
pub struct SelectorProps {
    pub model: AgentModel,
    pub values: ModelSettings,
    pub available: Option<Vec<HarnessId>>,
    pub catalog: ModelCatalog,
    pub prefs: ModelPrefs,
}

/// What the selector reports.
#[derive(Debug, Clone, PartialEq)]
pub enum QuickModelSelectorEvent {
    /// `onChange`: a model was picked. The selector stays open.
    Change(AgentModel),
    /// `onSettingsChange`.
    Settings(ModelSettings),
    /// `onClose`.
    Close,
    /// `saveFavoriteModels`.
    Favorites(Vec<String>),
    /// `QUICK_COMPOSER_CATALOG_REQUEST_EVENT` for a provider tab.
    RequestCatalog(HarnessId),
}

/// A harness logo (`HarnessIcon`).
pub fn harness_icon(harness: HarnessId, size: f32, ink: Hsla) -> AnyElement {
    match ProviderLogo::from_id(harness.as_str()) {
        Some(logo) => provider_logo(logo).size(size).color(ink).into_any_element(),
        None => div().size(u(size)).into_any_element(),
    }
}

/// gpui-component's lucide `star-fill`, for favorites.
fn star(filled: bool, size: f32, ink: Hsla) -> AnyElement {
    if filled {
        svg()
            .path("icons/star-fill.svg")
            .flex_none()
            .size(u(size))
            .text_color(ink)
            .into_any_element()
    } else {
        icon(IconName::Star)
            .size(u(size))
            .text_color(ink)
            .into_any_element()
    }
}

/// The thumb is 20px, so its center travels 10px in from each end.
const THUMB: f32 = 20.0;

pub struct QuickModelSelector {
    focus_handle: FocusHandle,
    tabs_focus: FocusHandle,
    slider_focus: FocusHandle,
    search: Entity<PromptInput>,
    props: SelectorProps,
    tab: ModelPickerTab,
    query: String,
    active: usize,
    favorites: Vec<String>,
    list_scroll: ScrollHandle,
    slider_bounds: Option<Bounds<Pixels>>,
    dragging_slider: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<QuickModelSelectorEvent> for QuickModelSelector {}

impl Focusable for QuickModelSelector {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl QuickModelSelector {
    /// Mounting focuses the search box and asks for the first provider's
    /// live list.
    pub fn new(props: SelectorProps, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = search_field("Search models\u{2026}", window, cx);
        let subscriptions =
            vec![
                cx.subscribe(&search, |this, search, event: &PromptInputEvent, cx| {
                    if *event == PromptInputEvent::Changed {
                        this.query = search.read(cx).text().to_string();
                        this.active = 0;
                        this.sync_active(cx);
                        cx.notify();
                    }
                }),
            ];
        let favorites = props.prefs.favorite_models.clone();
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            tabs_focus: cx.focus_handle(),
            slider_focus: cx.focus_handle(),
            search,
            tab: ModelPickerTab::Harness(props.model.harness),
            props,
            query: String::new(),
            active: 0,
            favorites,
            list_scroll: ScrollHandle::new(),
            slider_bounds: None,
            dragging_slider: false,
            _subscriptions: subscriptions,
        };
        this.sync_active(cx);
        this.request_catalog(cx);
        let search = this.search.clone();
        search.update(cx, |search, cx| search.focus(window, cx));
        this
    }

    /// New data from the owner (the `useLayoutEffect` on the catalog and
    /// the model).
    pub fn set_props(&mut self, props: SelectorProps, cx: &mut Context<Self>) {
        self.props = props;
        self.sync_active(cx);
    }

    pub fn props(&self) -> &SelectorProps {
        &self.props
    }

    pub fn search(&self) -> &Entity<PromptInput> {
        &self.search
    }

    fn providers(&self) -> Vec<HarnessId> {
        selector_providers(&self.props.prefs, self.props.available.as_deref())
    }

    /// The tabs: Favorites, then each shown provider.
    pub fn tabs(&self) -> Vec<ModelPickerTab> {
        selector_tabs(&self.providers())
    }

    /// `visibleTab`.
    pub fn visible_tab(&self) -> ModelPickerTab {
        visible_tab(self.tab, &self.tabs())
    }

    /// The listed models.
    pub fn models(&self) -> Vec<AgentModel> {
        let pool = selector_pool(
            &self.props.catalog,
            self.visible_tab(),
            &self.favorites,
            &self.providers(),
        );
        filter_quick_models(&pool, &self.query)
    }

    pub fn active(&self) -> usize {
        self.active
    }

    pub fn favorites(&self) -> &[String] {
        &self.favorites
    }

    fn sync_active(&mut self, cx: &mut Context<Self>) {
        let models = self.models();
        self.active = active_row(&models, &self.props.model.id);
        self.list_scroll.scroll_to_item(self.active);
        cx.notify();
    }

    fn request_catalog(&self, cx: &mut Context<Self>) {
        if let ModelPickerTab::Harness(harness) = self.visible_tab() {
            cx.emit(QuickModelSelectorEvent::RequestCatalog(harness));
        }
    }

    /// `selectTab`.
    pub fn select_tab(&mut self, tab: ModelPickerTab, window: &mut Window, cx: &mut Context<Self>) {
        let previous = self.visible_tab();
        self.tab = tab;
        self.query.clear();
        self.search
            .update(cx, |search, cx| search.reset_text("", cx));
        self.active = 0;
        if self.visible_tab() != previous {
            self.request_catalog(cx);
        }
        self.sync_active(cx);
        let _ = window;
        cx.notify();
    }

    /// `pick`: only models whose provider is available.
    pub fn pick(&mut self, model: &AgentModel, cx: &mut Context<Self>) {
        if model_enabled(model, self.props.available.as_deref()) {
            cx.emit(QuickModelSelectorEvent::Change(model.clone()));
        }
    }

    /// The star beside a row.
    pub fn toggle_favorite(&mut self, id: &str, cx: &mut Context<Self>) {
        self.favorites = toggle_favorite(&self.favorites, id);
        cx.emit(QuickModelSelectorEvent::Favorites(self.favorites.clone()));
        self.sync_active(cx);
    }

    fn effort(&self) -> Option<ModelSetting> {
        effort_setting(&self.props.model)
            .filter(|effort| !effort.options.is_empty())
            .cloned()
    }

    fn fast(&self) -> Option<FastToggle> {
        fast_toggle(&self.props.model)
    }

    /// The reasoning slider moved to `index`.
    pub fn set_effort_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(effort) = self.effort() else {
            return;
        };
        let Some(option) = effort.options.get(index.min(effort.options.len() - 1)) else {
            return;
        };
        if effort_index(&effort, &self.props.values) == index
            && self.props.values.contains_key(&effort.id)
        {
            return;
        }
        let next = change_setting(&self.props.values, &effort.id, &option.value);
        cx.emit(QuickModelSelectorEvent::Settings(next));
    }

    /// The lightning button.
    pub fn toggle_fast(&mut self, cx: &mut Context<Self>) {
        if let Some(fast) = self.fast() {
            cx.emit(QuickModelSelectorEvent::Settings(
                fast.toggled(&self.props.values),
            ));
        }
    }

    /// `resetSettings`.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        let next = reset_settings(
            &self.props.catalog,
            &self.props.model,
            &self.props.values,
            &self.props.prefs.last_model_settings,
        );
        cx.emit(QuickModelSelectorEvent::Settings(next));
    }

    /// Keys in the search box: the arrows move through the list, Enter
    /// picks.
    fn search_key(&mut self, down: bool, cx: &mut Context<Self>) {
        let len = self.models().len();
        if len == 0 {
            return;
        }
        self.active = if down {
            (self.active + 1) % len
        } else {
            (self.active + len - 1) % len
        };
        self.list_scroll.scroll_to_item(self.active);
        cx.notify();
    }

    fn search_enter(&mut self, cx: &mut Context<Self>) {
        if let Some(model) = self.models().get(self.active).cloned() {
            self.pick(&model, cx);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        let key = keystroke.key.as_str();
        if key == "escape" {
            cx.stop_propagation();
            cx.emit(QuickModelSelectorEvent::Close);
            return;
        }
        if self.tabs_focus.is_focused(window) {
            let tabs = self.tabs();
            let index = tabs
                .iter()
                .position(|tab| *tab == self.visible_tab())
                .unwrap_or(0);
            let next = match key {
                "right" => (index + 1) % tabs.len(),
                "left" => (index + tabs.len() - 1) % tabs.len(),
                "home" => 0,
                "end" => tabs.len() - 1,
                _ => return,
            };
            cx.stop_propagation();
            self.select_tab(tabs[next], window, cx);
            return;
        }
        if self.slider_focus.is_focused(window)
            && let Some(effort) = self.effort()
        {
            let index = effort_index(&effort, &self.props.values);
            let last = effort.options.len() - 1;
            let next = match key {
                "right" | "up" => (index + 1).min(last),
                "left" | "down" => index.saturating_sub(1),
                "home" => 0,
                "end" => last,
                _ => return,
            };
            cx.stop_propagation();
            self.set_effort_index(next, cx);
        }
    }

    fn slider_index_at(&self, x: Pixels, count: usize) -> Option<usize> {
        let bounds = self.slider_bounds?;
        let thumb = px(THUMB);
        let travel = (bounds.size.width - thumb).max(px(1.));
        let offset = (x - bounds.origin.x - thumb / 2.).clamp(px(0.), travel);
        let fraction = offset / travel;
        Some(((fraction * (count.saturating_sub(1)) as f32).round() as usize).min(count - 1))
    }

    fn slider_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.slider_focus, cx);
        self.dragging_slider = true;
        if let Some(effort) = self.effort()
            && let Some(index) = self.slider_index_at(event.position.x, effort.options.len())
        {
            self.set_effort_index(index, cx);
        }
    }

    fn slider_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging_slider || !event.dragging() {
            self.dragging_slider = false;
            return;
        }
        if let Some(effort) = self.effort()
            && let Some(index) = self.slider_index_at(event.position.x, effort.options.len())
        {
            self.set_effort_index(index, cx);
        }
    }

    fn render_tabs(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let visible = self.visible_tab();
        let mut strip = div()
            .id("quick-model-tabs")
            .track_focus(&self.tabs_focus)
            .flex()
            .flex_none()
            .h(u(44.))
            .items_center()
            .gap(u(4.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(8.));
        for tab in self.tabs() {
            let selected = visible == tab;
            let ink = if selected {
                theme.colors.content
            } else {
                theme.content(0.40)
            };
            let glyph = match tab {
                ModelPickerTab::Favorites => star(selected, 16., ink),
                ModelPickerTab::Harness(harness) => harness_icon(harness, 16., ink),
            };
            let hover_bg = theme.colors.selection_hover;
            let hover_ink = theme.colors.content;
            let mut button = div()
                .id(SharedString::from(format!("quick-model-tab-{tab}")))
                .flex()
                .flex_1()
                .h(u(32.))
                .min_w_0()
                .items_center()
                .justify_center()
                .rounded(u(theme.radius.lg))
                .text_color(ink)
                .tooltip(monocode_ui::widgets::tooltip(match tab {
                    ModelPickerTab::Favorites => "Favorites",
                    ModelPickerTab::Harness(harness) => harness.title(),
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| window.focus(&this.tabs_focus, cx)),
                )
                .on_click(cx.listener(move |this, _, window, cx| this.select_tab(tab, window, cx)))
                .child(glyph);
            button = if selected {
                button.bg(theme.colors.selection_emphasis)
            } else {
                button.hover(move |style| style.bg(hover_bg).text_color(hover_ink))
            };
            strip = strip.child(button);
        }
        strip.into_any_element()
    }

    fn render_list(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let models = self.models();
        let visible = self.visible_tab();
        let mut list = div()
            .id("quick-model-list")
            .track_scroll(&self.list_scroll)
            .h(u(240.))
            .min_h_0()
            .overflow_y_scroll()
            .p(u(8.))
            .flex()
            .flex_col();
        if models.is_empty() {
            return list
                .child(
                    div()
                        .px(u(8.))
                        .py(u(24.))
                        .text_center()
                        .text_px(12.)
                        .text_color(theme.content(0.45))
                        .child(empty_label(&self.query, visible)),
                )
                .into_any_element();
        }
        for (index, model) in models.into_iter().enumerate() {
            let enabled = model_enabled(&model, self.props.available.as_deref());
            let selected = model.id == self.props.model.id;
            let favorite = self.favorites.contains(&model.id);
            let hover = theme.colors.selection_hover;
            let mut row = div()
                .id(SharedString::from(format!(
                    "quick-model-{}-{}",
                    model.harness, model.id
                )))
                .flex()
                .flex_none()
                .h(u(32.))
                .items_center()
                .rounded(u(theme.radius.lg))
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.active != index {
                        this.active = index;
                        cx.notify();
                    }
                }));
            row = if index == self.active {
                row.bg(theme.colors.selection_emphasis)
            } else {
                row.hover(move |style| style.bg(hover))
            };
            let mut button = div()
                .id(SharedString::from(format!("quick-model-pick-{index}")))
                .flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .items_center()
                .gap(u(8.))
                .px(u(8.))
                .text_px(13.)
                .text_color(theme.colors.content);
            if !enabled {
                button = button.opacity(0.35);
            }
            if visible == ModelPickerTab::Favorites {
                button = button.child(harness_icon(model.harness, 14., theme.colors.content));
            }
            button = button.child(div().min_w_0().truncate().child(model.name.clone()));
            if let Some(provider) = &model.provider {
                button = button.child(
                    div()
                        .ml_auto()
                        .min_w_0()
                        .truncate()
                        .text_px(11.)
                        .text_color(theme.content(0.40))
                        .child(provider.name.clone()),
                );
            }
            if selected {
                button = button.child(
                    div().ml_auto().child(
                        icon(IconName::Check)
                            .size(u(14.))
                            .text_color(theme.colors.accent),
                    ),
                );
            }
            let picked = model.clone();
            button = button.on_click(cx.listener(move |this, _, _, cx| this.pick(&picked, cx)));
            let id = model.id.clone();
            let star_ink = theme.content(0.35);
            let star_hover = theme.colors.content;
            let star_button = div()
                .id(SharedString::from(format!("quick-model-star-{index}")))
                .flex()
                .flex_none()
                .size(u(32.))
                .items_center()
                .justify_center()
                .text_color(star_ink)
                .tooltip(monocode_ui::widgets::tooltip(if favorite {
                    "Remove from favorites"
                } else {
                    "Add to favorites"
                }))
                .hover(move |style| style.text_color(star_hover))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_favorite(&id, cx);
                }))
                .child(star(favorite, 14., star_ink));
            list = list.child(row.child(button).child(star_button));
        }
        list.into_any_element()
    }

    fn render_effort(&mut self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let effort = self.effort()?;
        let index = effort_index(&effort, &self.props.values);
        let count = effort.options.len();
        let fraction = index as f32 / (count.saturating_sub(1)).max(1) as f32;
        let fast = self.fast();
        let fast_button: AnyElement = match &fast {
            Some(fast) => {
                let on = fast.enabled(&self.props.values);
                let (bg, ink, hover_bg) = if on {
                    (
                        colors::amber(theme, 0.2),
                        colors::amber(theme, 1.0),
                        colors::amber(theme, 0.3),
                    )
                } else {
                    (
                        gpui::transparent_black(),
                        theme.content(0.40),
                        theme.colors.selection_hover,
                    )
                };
                let hover_ink = if on { ink } else { theme.colors.content };
                div()
                    .id("quick-fast-mode")
                    .flex()
                    .size(u(28.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.md))
                    .bg(bg)
                    .text_color(ink)
                    .hover(move |style| style.bg(hover_bg).text_color(hover_ink))
                    .tooltip(monocode_ui::widgets::tooltip(if on {
                        "Turn off fast mode"
                    } else {
                        "Turn on fast mode"
                    }))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_fast(cx)))
                    .child(icon(IconName::Zap).size(u(16.)).text_color(ink))
                    .into_any_element()
            }
            None => div().size(u(28.)).into_any_element(),
        };
        let hover_bg = theme.colors.selection_hover;
        let hover_ink = theme.colors.content;
        let reset = div()
            .id("quick-reset-settings")
            .flex()
            .size(u(28.))
            .items_center()
            .justify_center()
            .rounded(u(theme.radius.md))
            .text_color(theme.content(0.40))
            .hover(move |style| style.bg(hover_bg).text_color(hover_ink))
            .tooltip(monocode_ui::widgets::tooltip("Reset to saved defaults"))
            .on_click(cx.listener(|this, _, _, cx| this.reset(cx)))
            .child(
                icon(IconName::RotateCcw)
                    .size(u(16.))
                    .text_color(theme.content(0.40)),
            );
        let label = effort
            .options
            .get(index)
            .map(|option| option.label.clone())
            .unwrap_or_default();
        let header = div()
            .mb(u(4.))
            .flex()
            .items_center()
            .child(fast_button)
            .child(
                div()
                    .flex_1()
                    .text_center()
                    .text_px(13.)
                    .medium()
                    .text_color(theme.colors.accent)
                    .child(label),
            )
            .child(reset);
        let track = div()
            .absolute()
            .left_0()
            .right_0()
            .top(u(6.))
            .h(u(12.))
            .rounded_full()
            .overflow_hidden()
            .bg(theme.content(0.15))
            .child(div().h_full().w(relative(fraction)).bg(theme.colors.accent));
        let mut dots = div()
            .absolute()
            .left(u(10.))
            .right(u(10.))
            .top(u(12.))
            .h(u(0.));
        for dot in 0..count {
            let at = dot as f32 / (count.saturating_sub(1)).max(1) as f32;
            let color = if dot <= index {
                colors::white_alpha(0.45)
            } else {
                theme.content(0.25)
            };
            dots = dots.child(
                div()
                    .absolute()
                    .left(relative(at))
                    .ml(u(-2.))
                    .top(u(-2.))
                    .size(u(4.))
                    .rounded_full()
                    .bg(color),
            );
        }
        let thumb = div()
            .absolute()
            .top(u(2.))
            .left(relative(fraction))
            .ml(u(-THUMB * fraction))
            .size(u(THUMB))
            .rounded_full()
            .border_1()
            .border_color(colors::black_alpha(0.05))
            .bg(colors::white())
            .shadow_sm();
        let entity = cx.entity().downgrade();
        let slider = div()
            .id("quick-reasoning-slider")
            .track_focus(&self.slider_focus)
            .relative()
            .h(u(24.))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::slider_down))
            .on_mouse_move(cx.listener(Self::slider_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging_slider = false),
            )
            .child(track)
            .child(dots)
            .child(thumb)
            .child(
                canvas(
                    move |bounds, _, cx| {
                        if let Some(entity) = entity.upgrade() {
                            entity.update(cx, |this, _| this.slider_bounds = Some(bounds));
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        Some(
            div()
                .flex_none()
                .border_t_1()
                .border_color(theme.colors.stroke)
                .px(u(16.))
                .pb(u(12.))
                .pt(u(8.))
                .child(header)
                .child(slider)
                .into_any_element(),
        )
    }
}

impl Render for QuickModelSelector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let tabs = self.render_tabs(&theme, cx);
        let list = self.render_list(&theme, cx);
        let effort = self.render_effort(&theme, cx);
        let search_row = div()
            .flex()
            .flex_none()
            .h(u(40.))
            .items_center()
            .gap(u(8.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(16.))
            .child(
                icon(IconName::Search)
                    .size(u(14.))
                    .text_color(theme.content(0.40)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_px(13.)
                    .line_height(u(20.))
                    .text_color(theme.colors.content)
                    .key_context("QuickSearch")
                    .capture_action(cx.listener(|this, _: &prompt_input::MoveDown, _, cx| {
                        cx.stop_propagation();
                        this.search_key(true, cx);
                    }))
                    .capture_action(cx.listener(|this, _: &prompt_input::MoveUp, _, cx| {
                        cx.stop_propagation();
                        this.search_key(false, cx);
                    }))
                    .capture_action(cx.listener(|this, _: &prompt_input::Enter, _, cx| {
                        cx.stop_propagation();
                        if !this.search.read(cx).is_composing() {
                            this.search_enter(cx);
                        }
                    }))
                    .capture_action(cx.listener(|_, _: &prompt_input::Newline, _, cx| {
                        cx.stop_propagation();
                    }))
                    .child(self.search.clone()),
            );
        let mut body = div()
            .flex()
            .flex_col()
            .min_h_0()
            .min_w_0()
            .flex_1()
            .child(search_row)
            .child(list);
        if let Some(effort) = effort {
            body = body.child(effort);
        }
        div()
            .id("quick-model-selector")
            .key_context("QuickModelSelector")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .flex_col()
            .min_h_0()
            .border_t_1()
            .border_color(theme.colors.stroke)
            .child(tabs)
            .child(body)
    }
}
