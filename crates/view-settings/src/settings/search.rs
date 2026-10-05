//! Port of `SettingsSearch` in SettingsView.tsx: jumps to any setting by
//! name, including ones on another page.

use std::rc::Rc;

use gpui::{
    Anchor, App, AppContext as _, Context, ElementId, Entity, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, anchored, deferred, div, px, relative,
};
use gpui_component::input::{Enter, InputEvent, InputState, MoveDown, MoveUp};
use monocode_core::Platform;
use monocode_core::settings::{SettingsSearchResult, SettingsSectionId, search_settings};
use monocode_ui::widgets::{POPOVER_GAP, POPOVER_PADDING, popover_frame};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::controls::search_field;

/// Called with the section and the row a result points at.
pub type RevealHandler = Rc<dyn Fn(SettingsSectionId, Option<&'static str>, &mut Window, &mut App)>;

pub struct SettingsSearch {
    platform: Platform,
    query: String,
    active: usize,
    input: Entity<InputState>,
    on_reveal: Option<RevealHandler>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsSearch {
    pub fn new(platform: Platform, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search settings"));
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, input, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.query = input.read(cx).value().to_string();
                    this.active = 0;
                    cx.notify();
                }
            }),
        ];
        Self {
            platform,
            query: String::new(),
            active: 0,
            input,
            on_reveal: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn on_reveal(mut self, f: RevealHandler) -> Self {
        self.on_reveal = Some(f);
        self
    }

    pub fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn active(&self) -> usize {
        self.active
    }

    pub fn is_open(&self) -> bool {
        !monocode_core::js::trim(&self.query).is_empty()
    }

    /// `searchSettings(query)` with its default limit of 8.
    pub fn results(&self) -> Vec<SettingsSearchResult> {
        search_settings(&self.query, 8, self.platform)
    }

    /// Types `query` into the field, as the user would.
    pub fn set_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.query = query.to_string();
        self.active = 0;
        let value = query.to_string();
        self.input
            .update(cx, |input, cx| input.set_value(value, window, cx));
        cx.notify();
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.clear();
        self.active = 0;
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    /// The clear button: empty the field and keep focus in it.
    pub fn clear_and_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear(window, cx);
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// `go`: reveal a result, then clear and leave the field.
    pub fn go(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(result) = self.results().get(index).cloned() else {
            return;
        };
        if let Some(on_reveal) = self.on_reveal.clone() {
            window.defer(cx, move |window, cx| {
                on_reveal(result.section, result.setting_id, window, cx)
            });
        }
        self.clear(window, cx);
        window.blur();
    }

    fn move_active(&mut self, delta: i64, cx: &mut Context<Self>) {
        let len = self.results().len() as i64;
        let next = (self.active as i64 + delta).clamp(0, (len - 1).max(0));
        self.active = next as usize;
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // The popover's Escape dismissal: clear the results and keep focus.
        if event.keystroke.key == "escape" && self.is_open() {
            self.clear_and_focus(window, cx);
            cx.stop_propagation();
        }
    }

    fn render_results(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = Theme::of(cx).clone();
        let results = self.results();
        let mut list = div()
            .id("settings-search-results")
            .flex()
            .flex_col()
            .p(u(4.))
            .max_h(u(320.))
            .overflow_y_scroll()
            .debug_selector(|| "listbox:Settings search results".into());
        if results.is_empty() {
            list = list.child(
                div()
                    .px(u(8.))
                    .py(u(6.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child("No matching settings"),
            );
        }
        for (index, result) in results.into_iter().enumerate() {
            let active = index == self.active;
            let meta = if result.setting_id.is_some() {
                result.section_label
            } else {
                "Page"
            };
            let selector = format!("search-result:{}:{meta}", result.label);
            let hover = theme.content(0.05);
            let mut item = div()
                .id(ElementId::from(index))
                .flex()
                .w_full()
                .items_center()
                .gap(u(8.))
                .px(u(8.))
                .py(u(6.))
                .rounded(u(theme.radius.lg))
                .text_px(theme.text.label)
                .text_color(theme.colors.content)
                .debug_selector(move || selector)
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.active != index {
                        this.active = index;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.go(index, window, cx)))
                .child(div().min_w_0().flex_1().truncate().child(result.label))
                .child(
                    div()
                        .flex_none()
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.40))
                        .child(meta),
                );
            item = if active {
                item.bg(theme.colors.selection)
            } else {
                item.hover(move |s| s.bg(hover))
            };
            list = list.child(item);
        }
        let gap = u(POPOVER_GAP).to_pixels(window.rem_size());
        div().absolute().top(relative(1.)).right_0().size_0().child(
            deferred(
                anchored()
                    .anchor(Anchor::TopRight)
                    .offset(gpui::point(px(0.), gap))
                    .snap_to_window_with_margin(px(POPOVER_PADDING))
                    .child(
                        div()
                            .occlude()
                            .on_mouse_down_out(
                                cx.listener(|this, _, window, cx| this.clear(window, cx)),
                            )
                            .child(
                                popover_frame("settings-search-popover")
                                    .width(300.)
                                    .child(list),
                            ),
                    ),
            )
            .with_priority(theme.layer.popover),
        )
    }
}

impl Render for SettingsSearch {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let clear = (!self.query.is_empty()).then(|| {
            let hover = theme.colors.content;
            div()
                .id("clear-settings-search")
                .flex()
                .flex_none()
                .size(u(16.))
                .items_center()
                .justify_center()
                .rounded(u(theme.radius.sm))
                .text_color(theme.content(0.45))
                .hover(move |s| s.text_color(hover))
                .debug_selector(|| "button:clear-settings-search".into())
                .on_click(cx.listener(|this, _, window, cx| this.clear_and_focus(window, cx)))
                .child(
                    icon(IconName::X)
                        .size(u(12.))
                        .text_color(theme.content(0.45)),
                )
                .into_any_element()
        });
        let field = search_field("search-settings", &self.input, 192., clear, window, cx);
        let results = self.is_open().then(|| self.render_results(window, cx));
        div()
            .relative()
            .flex_none()
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                if this.is_open() {
                    this.move_active(1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                if this.is_open() {
                    this.move_active(-1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                if this.is_open() {
                    let active = this.active;
                    this.go(active, window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(Self::on_key_down))
            .child(field)
            .children(results)
    }
}
