//! Port of the layout pieces of SettingsView.tsx: `PageHeader`, `Group` (a
//! titled card), `Row` (a label, a description, and controls), and the
//! `RevealedSetting` context that flashes the row search landed on.
//!
//! Rows with an id record their bounds each frame in [`Anchors`], so the page
//! can scroll a search result into view (`scrollIntoView({ block: "center" })`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Div, InteractiveElement as _, IntoElement, ParentElement, Pixels,
    RenderOnce, SharedString, Styled as _, Window, canvas, div, prelude::FluentBuilder as _, px,
};
use monocode_ui::{Theme, UiStyled as _, u};

/// The content width under which rows stack (`@container settings (width < 560px)`).
pub const NARROW_WIDTH: f32 = 560.0;

/// Bounds of every element with a settings id, from the last frame.
#[derive(Clone, Default)]
pub struct Anchors(Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>);

impl Anchors {
    pub fn get(&self, id: &str) -> Option<Bounds<Pixels>> {
        self.0.borrow().get(id).copied()
    }

    pub fn ids(&self) -> Vec<SharedString> {
        let mut ids: Vec<SharedString> = self.0.borrow().keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    /// An absolute, full-size probe that records its parent's bounds.
    pub fn probe(&self, id: SharedString) -> impl IntoElement + use<> {
        let map = self.0.clone();
        canvas(
            move |bounds, _, _| {
                map.borrow_mut().insert(id, bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

/// `RevealedSetting` plus the container query: what each row needs from
/// the page.
#[derive(Clone, Default)]
pub struct Reveal {
    /// The row or group Settings just jumped to, so it can flash.
    pub revealed: Option<SharedString>,
    /// The page is narrower than 560px, so rows stack.
    pub narrow: bool,
    pub anchors: Anchors,
}

impl Reveal {
    pub fn is(&self, id: &str) -> bool {
        self.revealed.as_deref() == Some(id)
    }
}

/// The DOM id SettingsView gave a row (`settingDomId`).
pub fn setting_dom_id(id: &str) -> String {
    format!("setting-{id}")
}

/// `PageHeader`: the section title and description. The skills page in
/// monocode-view-pages draws its own copy through this function.
pub fn page_header(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    cx: &App,
) -> Div {
    let theme = Theme::of(cx);
    let description: SharedString = description.into();
    div()
        .flex()
        .flex_col()
        .pb(u(16.))
        .child(
            div()
                .text_px(20.)
                .semibold()
                .leading(theme.leading.tight)
                .text_color(theme.colors.content)
                .child(title.into()),
        )
        .when(!description.is_empty(), |el| {
            el.child(
                div()
                    .mt(u(6.))
                    .max_w(u(576.))
                    .text_px(theme.text.body)
                    .leading(theme.leading.relaxed)
                    .text_color(theme.content(0.45))
                    .child(description),
            )
        })
}

/// `Group`: a titled card of related settings.
#[derive(IntoElement)]
pub struct Group {
    id: Option<SharedString>,
    title: AnyElement,
    description: Option<SharedString>,
    action: Option<AnyElement>,
    first: bool,
    reveal: Reveal,
    children: Vec<AnyElement>,
}

/// A card. `first` drops the top padding (`first:pt-0`).
pub fn group(reveal: &Reveal, title: impl IntoElement) -> Group {
    Group {
        id: None,
        title: title.into_any_element(),
        description: None,
        action: None,
        first: false,
        reveal: reveal.clone(),
        children: Vec::new(),
    }
}

impl Group {
    /// Matches a `SETTINGS_INDEX` id when the whole card is the search target.
    pub fn id(mut self, id: impl Into<SharedString>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }

    pub fn first(mut self, first: bool) -> Self {
        self.first = first;
        self
    }
}

impl ParentElement for Group {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Group {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let flash = self.id.as_deref().is_some_and(|id| self.reveal.is(id));
        let mut section = div().relative().flex().flex_col();
        if !self.first {
            section = section.pt(u(32.));
        }
        if let Some(id) = self.id.clone() {
            section = section
                .debug_selector(|| format!("setting-id:{id}"))
                .child(self.reveal.anchors.probe(id));
        }
        let mut heading = div().min_w_0().flex_1().child(
            div()
                .text_px(theme.text.body)
                .semibold()
                .text_color(theme.colors.content)
                .child(self.title),
        );
        if let Some(description) = self.description {
            heading = heading.child(
                div()
                    .mt(u(4.))
                    .text_px(theme.text.label)
                    .leading(theme.leading.relaxed)
                    .text_color(theme.content(0.45))
                    .child(description),
            );
        }
        let header = div()
            .flex()
            .items_end()
            .gap(u(16.))
            .pb(u(10.))
            .child(heading)
            .when_some(self.action, |el, action| {
                el.child(div().flex_none().pb(u(2.)).child(action))
            });
        let border = if flash {
            theme.accent(0.60)
        } else {
            theme.content(0.10)
        };
        // Each row draws its bottom border. The inner column pulls the last
        // one under the card's edge, which clips it (`last:border-b-0`).
        let card = div()
            .overflow_hidden()
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(border)
            .bg(theme.content(0.03))
            .when(flash, |el| el.debug_selector(|| "flash-group".into()))
            .child(div().flex().flex_col().mb(px(-1.)).children(self.children));
        section.child(header).child(card)
    }
}

/// A row's bottom divider (`border-b border-content/5`).
pub fn divider_color(cx: &App) -> gpui::Hsla {
    Theme::of(cx).content(0.05)
}

/// `Row`: a label and description beside the controls.
#[derive(IntoElement)]
pub struct Row {
    id: Option<SharedString>,
    selector: Option<SharedString>,
    label: AnyElement,
    description: Option<SharedString>,
    reveal: Reveal,
    controls: Vec<AnyElement>,
    switch_only: bool,
}

pub fn row(reveal: &Reveal, label: impl IntoElement) -> Row {
    Row {
        id: None,
        selector: None,
        label: label.into_any_element(),
        description: None,
        reveal: reveal.clone(),
        controls: Vec::new(),
        switch_only: false,
    }
}

impl Row {
    /// Matches a `SETTINGS_INDEX` id so search can scroll here.
    pub fn id(mut self, id: impl Into<SharedString>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// A test selector for a row without a settings id.
    pub fn selector(mut self, selector: impl Into<SharedString>) -> Self {
        self.selector = Some(selector.into());
        self
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// The only control is a switch, which stays beside the label even on a
    /// narrow page.
    pub fn switch_only(mut self) -> Self {
        self.switch_only = true;
        self
    }
}

impl ParentElement for Row {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.controls.extend(elements);
    }
}

impl RenderOnce for Row {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let flash = self.id.as_deref().is_some_and(|id| self.reveal.is(id));
        let stacked = self.reveal.narrow && !self.switch_only;
        let mut text = div().min_w_0().flex_1().child(
            div()
                .text_px(theme.text.body)
                .medium()
                .text_color(theme.colors.content)
                .child(self.label),
        );
        if let Some(description) = self.description {
            text = text.child(
                div()
                    .mt(u(4.))
                    .text_px(theme.text.label)
                    .leading(theme.leading.relaxed)
                    .text_color(theme.content(0.45))
                    .child(description),
            );
        }
        let mut controls = div()
            .flex()
            .min_w_0()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(u(8.))
            .children(self.controls);
        controls = if stacked {
            controls.max_w_full().justify_start()
        } else {
            controls.max_w(gpui::relative(0.6)).justify_end()
        };
        let mut el = div()
            .relative()
            .flex()
            .px(u(16.))
            .py(u(14.))
            .border_b_1()
            .border_color(theme.content(0.05));
        el = if stacked {
            el.flex_col().gap(u(12.))
        } else {
            el.items_start().gap(u(24.))
        };
        if flash {
            // A marker child: an element keeps only its last debug selector.
            el = el.bg(theme.accent(0.10)).child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .debug_selector(|| "flash-row".into()),
            );
        }
        if let Some(id) = self.id.clone() {
            el = el
                .debug_selector(|| format!("setting-id:{id}"))
                .child(self.reveal.anchors.probe(id));
        }
        if let Some(selector) = self.selector {
            el = el.debug_selector(|| selector.to_string());
        }
        el.child(text).child(controls)
    }
}

/// `<p className="border-b border-content/5 px-4 pb-3 text-[12px] text-red-400/90">`:
/// an error under a row.
pub fn row_error(message: impl Into<SharedString>, cx: &App) -> Div {
    let theme = Theme::of(cx);
    div()
        .px(u(16.))
        .pb(u(12.))
        .border_b_1()
        .border_color(theme.content(0.05))
        .text_px(theme.text.label)
        .text_color(gpui::Hsla {
            a: 0.9,
            ..theme.colors.danger
        })
        .child(message.into())
}

/// `<p className="px-4 py-3.5 text-[12px] text-content/45">`: a quiet line
/// in a card, such as an empty state.
pub fn card_note(message: impl Into<SharedString>, cx: &App) -> Div {
    let theme = Theme::of(cx);
    div()
        .px(u(16.))
        .py(u(14.))
        .border_b_1()
        .border_color(theme.content(0.05))
        .text_px(theme.text.label)
        .text_color(theme.content(0.45))
        .child(message.into())
}
