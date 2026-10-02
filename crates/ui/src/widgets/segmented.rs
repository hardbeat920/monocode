//! The segmented control from SettingsView.tsx `Segmented`: a bordered strip
//! of equal options, the picked one on `bg-selection`.

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::styled::UiStyled as _;
use crate::{Theme, u};

type PickHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    options: Vec<SharedString>,
    selected: usize,
    on_pick: Option<PickHandler>,
}

pub fn segmented(
    id: impl Into<ElementId>,
    options: impl IntoIterator<Item = impl Into<SharedString>>,
    selected: usize,
) -> Segmented {
    Segmented {
        id: id.into(),
        options: options.into_iter().map(Into::into).collect(),
        selected,
        on_pick: None,
    }
}

impl Segmented {
    /// Called with the picked option's index.
    pub fn on_pick(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_pick = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = theme.colors;
        let mut row = div()
            .id(self.id)
            .flex()
            .flex_none()
            .gap(u(2.))
            .p(u(2.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .text_px(theme.text.label)
            .leading(theme.leading.label);
        for (index, label) in self.options.into_iter().enumerate() {
            let picked = index == self.selected;
            let mut option = div()
                .id(index)
                .flex_1()
                .flex()
                .justify_center()
                .px(u(10.))
                .py(u(4.))
                .rounded(u(5.))
                .whitespace_nowrap()
                .child(label);
            if picked {
                option = option.bg(c.selection).text_color(c.content);
            } else {
                let ink = c.content;
                option = option
                    .text_color(theme.content(0.50))
                    .hover(move |s| s.text_color(ink));
            }
            if let Some(handler) = self.on_pick.clone() {
                option = option.on_click(move |_, window, cx| handler(index, window, cx));
            }
            row = row.child(option);
        }
        row
    }
}
