//! Port of src/features/search/ui/SearchView.tsx: the search field in the
//! title bar, the scope tabs, the empty state, and the result list with
//! keyboard and pointer highlight.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, HighlightStyle,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseMoveEvent, ParentElement as _, Pixels,
    Point, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _,
    StyledImage as _, StyledText, Subscription, Window, canvas, div, fill, point,
    prelude::FluentBuilder as _, px, size,
};
use gpui_component::input::{Enter, InputEvent, InputState, MoveDown, MoveUp};
use monocode_core::js;
use monocode_ui::{
    IconName, ProviderLogo, Theme, UiStyled as _, file_type_icon, icon, provider_logo, u,
};
use monocode_view_composer::pickers::match_text;

use super::data::SearchData;
use super::model::{AppSearchHit, SearchScope, SearchState, highlight_range, name_positions};
use crate::data::ProjectsData;
use crate::widgets::{PageChrome, page_title_bar, plain_input, spinner_icon};

const EMPTY_DOT_COLS: usize = 27;
const EMPTY_DOT_ROWS: usize = 19;

type CloseFn = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct SearchView {
    data: Rc<dyn SearchData>,
    projects: Rc<dyn ProjectsData>,
    chrome: PageChrome,
    on_close: Option<CloseFn>,
    state: SearchState,
    focus: FocusHandle,
    input: Entity<InputState>,
    scroll: ScrollHandle,
    /// `pointer`: hover may move the highlight only after the pointer moved
    /// since the results changed.
    pointer: Option<Point<Pixels>>,
    pointer_allowed: bool,
    hit_ids: Vec<String>,
    last_active: usize,
    _subscriptions: Vec<Subscription>,
}

impl SearchView {
    pub fn new(
        data: Rc<dyn SearchData>,
        projects: Rc<dyn ProjectsData>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search everything..."));
        let weak = cx.weak_entity();
        let changes = data.subscribe(
            Box::new(move |cx| {
                weak.update(cx, |this, cx| this.refresh(cx)).ok();
            }),
            cx,
        );
        let input_events = cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                this.data.set_query(&value, cx);
            }
        });
        let state = data.state(cx);
        Self {
            data,
            projects,
            chrome: PageChrome::default(),
            on_close: None,
            state,
            focus: cx.focus_handle(),
            input,
            scroll: ScrollHandle::new(),
            pointer: None,
            pointer_allowed: false,
            hit_ids: Vec::new(),
            last_active: 0,
            _subscriptions: vec![changes, input_events],
        }
    }

    pub fn chrome(mut self, chrome: PageChrome) -> Self {
        self.chrome = chrome;
        self
    }

    /// `onClose`: Escape and opening a hit leave the page.
    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    pub fn state(&self) -> &SearchState {
        &self.state
    }

    pub fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    /// Show the page for `cwd`: a fresh query and the focused field
    /// (`open` and `focusToken` in React).
    pub fn open(
        &mut self,
        cwd: &str,
        recents: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.data.open(cwd, recents, cx);
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.refresh(cx);
    }

    /// Put `query` in the field and search for it, as typing does.
    pub fn set_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = query.to_string();
        self.input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.data.set_query(query, cx);
        self.refresh(cx);
    }

    /// Focus the search field again.
    pub fn focus_input(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.state = self.data.state(cx);
        let ids: Vec<String> = self
            .state
            .hits
            .iter()
            .map(|hit| hit.id().to_string())
            .collect();
        if ids != self.hit_ids {
            self.hit_ids = ids;
            self.pointer_allowed = false;
        }
        cx.notify();
    }

    fn close(&self, window: &mut Window, cx: &mut App) {
        self.data.close(cx);
        if let Some(close) = self.on_close.clone() {
            close(window, cx);
        }
    }

    /// `openHit`.
    pub fn open_hit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = self.state.hits.get(index).cloned() else {
            return;
        };
        let query = js::trim(&self.state.query).to_string();
        self.data.open_hit(&hit, &query, window, cx);
        self.close(window, cx);
    }

    fn step(&mut self, delta: i64, cx: &mut Context<Self>) {
        if self.state.hits.is_empty() {
            return;
        }
        self.pointer_allowed = false;
        self.data.move_active(delta, cx);
    }

    fn render_scopes(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut row = div()
            .flex()
            .flex_none()
            .h(u(theme.metrics.toolbar_height))
            .items_center()
            .gap(px(1.))
            .px(u(12.))
            .border_b_1()
            .border_color(theme.colors.stroke);
        for (scope, label) in SearchScope::ALL_SCOPES {
            let selected = self.state.scope == scope;
            let hover = theme.content(0.05);
            let ink = theme.colors.content;
            row = row.child(
                div()
                    .id(label)
                    .debug_selector(move || format!("search-scope {label}"))
                    .px(u(8.))
                    .py(u(4.))
                    .rounded(u(theme.radius.md))
                    .text_px(theme.text.label)
                    .leading(theme.leading.normal)
                    .map(|tab| {
                        if selected {
                            tab.bg(theme.colors.selection).text_color(ink)
                        } else {
                            tab.text_color(theme.content(0.50))
                                .hover(move |s| s.bg(hover).text_color(ink))
                        }
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.data.set_scope(scope, cx)))
                    .child(label),
            );
        }
        row
    }

    fn row_icon(&self, hit: &AppSearchHit, theme: &Theme, cx: &App) -> gpui::AnyElement {
        match hit {
            AppSearchHit::Conversation(hit) => ProviderLogo::from_id(hit.harness.as_str())
                .map(|logo| {
                    provider_logo(logo)
                        .size(14.)
                        .color(theme.colors.content)
                        .into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element()),
            AppSearchHit::Message(_) => icon(IconName::MessageSquare)
                .size(u(14.))
                .text_color(theme.content(0.55))
                .into_any_element(),
            AppSearchHit::File(hit) => file_type_icon(hit.name.clone())
                .size(16.)
                .into_any_element(),
            AppSearchHit::Content(hit) => file_type_icon(hit.name.clone())
                .size(16.)
                .into_any_element(),
            AppSearchHit::Project(hit) => match self.projects.mark(&hit.path, cx).logo {
                Some(logo) => div()
                    .size(u(14.))
                    .rounded(u(2.))
                    .overflow_hidden()
                    .child(
                        gpui::img(std::path::PathBuf::from(logo))
                            .size_full()
                            .object_fit(gpui::ObjectFit::Cover),
                    )
                    .into_any_element(),
                None => icon(IconName::Folder)
                    .size(u(14.))
                    .text_color(theme.colors.content)
                    .into_any_element(),
            },
        }
    }

    fn row_title(&self, hit: &AppSearchHit, theme: &Theme) -> gpui::AnyElement {
        match hit {
            AppSearchHit::Conversation(hit) => {
                match_text(hit.title.clone(), hit.positions.clone(), true).into_any_element()
            }
            AppSearchHit::File(hit) => match_text(
                hit.name.clone(),
                name_positions(&hit.name, &hit.relative, &hit.positions),
                true,
            )
            .into_any_element(),
            AppSearchHit::Project(hit) => {
                match_text(hit.name.clone(), hit.positions.clone(), true).into_any_element()
            }
            AppSearchHit::Message(_) | AppSearchHit::Content(_) => {
                let text: SharedString = hit.title().to_string().into();
                let highlights = highlight_range(&text, &self.state.query)
                    .map(|range| {
                        vec![(
                            range,
                            HighlightStyle {
                                color: Some(theme.colors.accent),
                                ..Default::default()
                            },
                        )]
                    })
                    .unwrap_or_default();
                StyledText::new(text)
                    .with_highlights(highlights)
                    .into_any_element()
            }
        }
    }

    fn render_results(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let trimmed = js::trim(&self.state.query).to_string();
        let mut list = div()
            .id("search-results")
            .debug_selector(|| "search-results".into())
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, _| {
                if this.pointer != Some(event.position) {
                    this.pointer = Some(event.position);
                    this.pointer_allowed = true;
                }
            }));
        if trimmed.is_empty() {
            return list
                .items_center()
                .justify_center()
                .child(empty_state(theme))
                .into_any_element();
        }
        list = list.px(u(6.)).py(u(6.));
        let notice = || {
            div()
                .px(u(10.))
                .py(u(4.))
                .text_px(theme.text.caption)
                .text_color(theme.content(0.45))
                .child("Results limited to the first matches")
        };
        let message = |text: String, color| {
            div()
                .px(u(8.))
                .py(u(6.))
                .text_px(theme.text.label)
                .text_color(color)
                .child(text)
        };
        let hits = &self.state.hits;
        if let (Some(error), true) = (self.state.error.clone(), hits.is_empty()) {
            return list
                .child(message(error, theme.colors.danger))
                .into_any_element();
        }
        if hits.is_empty() && !self.state.loading {
            list = list.child(message("No results".into(), theme.content(0.50)));
            if self.state.truncated {
                list = list.child(notice());
            }
            return list.into_any_element();
        }
        if self.state.truncated {
            list = list.child(notice());
        }
        for (index, hit) in hits.iter().enumerate() {
            let highlighted = index == self.state.active;
            let meta = hit.meta();
            let selector = hit.id().to_string();
            list = list.child(
                div()
                    .id(("search-hit", index))
                    .debug_selector(move || format!("search-hit {selector}"))
                    .flex()
                    .flex_none()
                    .w_full()
                    .h(u(32.))
                    .items_center()
                    .gap(u(8.))
                    .px(u(8.))
                    .rounded(u(theme.radius.md))
                    .text_px(theme.text.body)
                    .leading(theme.leading.none)
                    .text_color(theme.colors.content)
                    .when(highlighted, |row| row.bg(theme.colors.selection))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered && this.pointer_allowed && this.state.active != index {
                            this.last_active = index;
                            this.data.set_active(index, cx);
                        }
                    }))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.open_hit(index, window, cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .size(u(16.))
                            .items_center()
                            .justify_center()
                            .child(self.row_icon(hit, theme, cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(self.row_title(hit, theme)),
                    )
                    .when(!meta.is_empty(), |row| {
                        row.child(
                            div()
                                .min_w_0()
                                .max_w(gpui::relative(0.45))
                                .truncate()
                                .font_family(theme.fonts.mono.clone())
                                .text_px(theme.text.caption)
                                .text_color(theme.content(0.40))
                                .child(meta),
                        )
                    }),
            );
        }
        list.into_any_element()
    }
}

/// `EmptyState`: a faded dot field behind a search tile, and the hint.
fn empty_state(theme: &Theme) -> impl IntoElement + use<> {
    let dot = theme.colors.content;
    let dots = canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let rem = window.rem_size();
            let cell = u(3.).to_pixels(rem);
            let gap = u(7.).to_pixels(rem);
            let width = cell * EMPTY_DOT_COLS as f32 + gap * (EMPTY_DOT_COLS - 1) as f32;
            let height = cell * EMPTY_DOT_ROWS as f32 + gap * (EMPTY_DOT_ROWS - 1) as f32;
            let origin = point(
                bounds.origin.x + (bounds.size.width - width) / 2.,
                bounds.origin.y + (bounds.size.height - height) / 2.,
            );
            // `radial-gradient(ellipse 72% 68% at 50% 50%, #000 18%,
            // transparent 76%)` over the grid, times `opacity-[0.14]`.
            let rx = f32::from(width) * 0.72;
            let ry = f32::from(height) * 0.68;
            for row in 0..EMPTY_DOT_ROWS {
                for col in 0..EMPTY_DOT_COLS {
                    let x = origin.x + (cell + gap) * col as f32;
                    let y = origin.y + (cell + gap) * row as f32;
                    let dx = f32::from(x + cell / 2. - origin.x) - f32::from(width) / 2.;
                    let dy = f32::from(y + cell / 2. - origin.y) - f32::from(height) / 2.;
                    let t = ((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt();
                    let mask = ((0.76 - t) / 0.58).clamp(0., 1.);
                    if mask <= 0. {
                        continue;
                    }
                    window.paint_quad(
                        fill(
                            Bounds::new(point(x, y), size(cell, cell)),
                            dot.opacity(0.14 * mask),
                        )
                        .corner_radii(cell / 2.),
                    );
                }
            }
        },
    )
    .size_full();
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .px(u(24.))
        .pb(u(96.))
        .debug_selector(|| "search-empty".into())
        .child(
            div()
                .relative()
                .mb(u(8.))
                .h(u(192.))
                .w(u(288.))
                .flex()
                .items_center()
                .justify_center()
                .child(div().absolute().top_0().left_0().size_full().child(dots))
                .child(
                    div()
                        .flex()
                        .size(u(56.))
                        .items_center()
                        .justify_center()
                        .rounded(u(theme.radius.xxl))
                        .bg(theme.content(0.06))
                        .child(
                            icon(IconName::Search)
                                .size(u(24.))
                                .text_color(theme.content(0.50)),
                        ),
                ),
        )
        .child(
            div()
                .max_w(u(320.))
                .text_center()
                .text_px(theme.text.body)
                .text_color(theme.content(0.45))
                .child("Find files, conversations, messages, and projects."),
        )
}

impl Focusable for SearchView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SearchView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        if self.state.active != self.last_active {
            self.last_active = self.state.active;
            if !self.pointer_allowed {
                let offset = usize::from(self.state.truncated);
                self.scroll.scroll_to_item(self.state.active + offset);
            }
        }
        let field = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(u(8.))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h(u(20.))
                    .items_center()
                    .text_px(theme.text.body)
                    .debug_selector(|| "search-input".into())
                    .capture_action(cx.listener(|this: &mut Self, _: &MoveDown, _, cx| {
                        this.step(1, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this: &mut Self, _: &MoveUp, _, cx| {
                        this.step(-1, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this: &mut Self, _: &Enter, window, cx| {
                        cx.stop_propagation();
                        let active = this.state.active;
                        this.open_hit(active, window, cx);
                    }))
                    .child(plain_input(&self.input, None, cx)),
            )
            .when(self.state.loading, |row| {
                row.child(spinner_icon("search-loading", 14., theme.content(0.35)))
            });
        div()
            .id("search-view")
            .key_context("SearchView")
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && !event.keystroke.modifiers.modified() {
                    cx.stop_propagation();
                    this.close(window, cx);
                }
            }))
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .min_h_0()
            .min_w_0()
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal))
            .child(page_title_bar(
                &self.chrome,
                IconName::Search,
                field,
                &theme,
            ))
            .child(self.render_scopes(&theme, cx))
            .child(self.render_results(&theme, cx))
    }
}
