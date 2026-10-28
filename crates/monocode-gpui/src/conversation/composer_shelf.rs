//! Queued messages and tool approvals share the ledge above the composer.
//! Entrances grow upward from its edge without scaling the clipped contents.
use crate::{pane_motion::PaneSlide, reveal::Disclosure, theme::content};
use gpui::{AnyElement, Div, IntoElement, Pixels, Stateful, Window, div, prelude::*, px};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    time::Instant,
};

#[derive(Default)]
pub(super) struct Motion {
    card: RefCell<Option<PaneSlide>>,
    rows: RefCell<HashMap<String, PaneSlide>>,
}

pub(super) fn shelf(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .text_size(px(12.))
        .text_color(content(0.55))
}

pub(super) fn card(id: &'static str, classic_layout: bool) -> Stateful<Div> {
    let border_alpha = if classic_layout {
        0.1
    } else if content(1.).l > 0.5 {
        0.06
    } else {
        0.08
    };
    div()
        .id(id)
        .rounded_t(px(10.))
        .border_1()
        .border_b_0()
        .border_color(content(border_alpha))
        .bg(content(0.03))
        .px(px(10.))
        .py(px(2.))
}

impl Motion {
    pub(super) fn insert(&self, id: &str, first: bool, animate: bool) {
        if !animate {
            return;
        }
        let slide = PaneSlide::new(0., 1., 40.);
        if first {
            *self.card.borrow_mut() = Some(slide);
            self.rows.borrow_mut().clear();
        } else {
            self.rows.borrow_mut().insert(id.to_owned(), slide);
        }
    }

    pub(super) fn retain<'a>(&self, ids: impl Iterator<Item = &'a str>, animate: bool) {
        let ids: HashSet<_> = ids.collect();
        self.rows
            .borrow_mut()
            .retain(|id, _| animate && ids.contains(id.as_str()));
        if !animate || ids.is_empty() {
            *self.card.borrow_mut() = None;
        }
    }

    pub(super) fn row(
        &self,
        id: &str,
        child: impl IntoElement,
        width: Pixels,
        window: &Window,
    ) -> AnyElement {
        let mut rows = self.rows.borrow_mut();
        let Some(slide) = rows.get(id) else {
            return child.into_any_element();
        };
        let (progress, done) = slide.at(Instant::now());
        if done {
            rows.remove(id);
            child.into_any_element()
        } else {
            reveal(child, width, progress, window)
        }
    }

    pub(super) fn card(
        &self,
        child: impl IntoElement,
        width: Pixels,
        window: &Window,
    ) -> AnyElement {
        let mut card = self.card.borrow_mut();
        let Some(slide) = card.as_ref() else {
            return child.into_any_element();
        };
        let (progress, done) = slide.at(Instant::now());
        if done {
            *card = None;
            child.into_any_element()
        } else {
            reveal(child, width, progress, window)
        }
    }

    #[cfg(test)]
    pub(super) fn elapsed(&self, elapsed: std::time::Duration) {
        if let Some(card) = self.card.borrow().as_ref() {
            card.elapsed(elapsed);
        }
        for row in self.rows.borrow().values() {
            row.elapsed(elapsed);
        }
    }
}

fn reveal(child: impl IntoElement, width: Pixels, progress: f32, window: &Window) -> AnyElement {
    window.request_animation_frame();
    div()
        .w_full()
        .flex_shrink_0()
        .overflow_hidden()
        .child(Disclosure::new(child, width, progress))
        .into_any_element()
}
