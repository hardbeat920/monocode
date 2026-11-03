//! The changes pill beside the commit pill: `+a -d` and a five-square bar.
//! A click opens the latest turn's review card on the composer's ledge, where
//! background tasks and queued messages sit, so it can be read without
//! scrolling back to the reply.
use super::*;
use crate::pane_motion::PaneSlide;
use crate::right_panel::{DIFF_ADD, DIFF_DEL, format_integer, tabular};
use gpui::rgb;
use std::{cell::RefCell, time::Instant};

const SQUARES: usize = 5;

#[derive(Default)]
pub(super) struct ChangesShelf {
    pub open: bool,
    /// The card growing up from the composer or folding back down into it.
    slide: RefCell<Option<PaneSlide>>,
}

impl ChangesShelf {
    /// Springs toward open or shut, from wherever an earlier slide got to.
    fn slide_to(&self, open: bool, animate: bool) {
        let mut slide = self.slide.borrow_mut();
        if !animate {
            *slide = None;
            return;
        }
        let to = if open { 1. } else { 0. };
        let now = Instant::now();
        *slide = Some(match slide.as_ref() {
            Some(current) => current.retarget(to, now),
            None => PaneSlide::new(1. - to, to, 40.),
        });
    }

    /// How far open the card is while a slide runs (`Some`), or whether it
    /// rests open (`None` inside) — `None` overall once it has folded away.
    /// A running slide may sit at exactly 0 or 1 on its first frame, so the
    /// caller keeps drawing frames until the spring reports done.
    fn progress(&self) -> Option<Option<f32>> {
        let mut slide = self.slide.borrow_mut();
        let Some((progress, done)) = slide.as_ref().map(|s| s.at(Instant::now())) else {
            return self.open.then_some(None);
        };
        if done {
            *slide = None;
            return self.open.then_some(None);
        }
        Some(Some(progress))
    }
}

impl Conversation {
    /// The turn's totals while the pill has something to show.
    fn change_totals(&self, cx: &gpui::App) -> Option<(usize, i64, i64)> {
        if self.busy || self.is_remote() || self.settings.worktree_removed {
            return None;
        }
        self.review.read(cx).totals()
    }

    fn toggle_changes(&mut self, cx: &mut Context<Self>) {
        self.changes.open = !self.changes.open;
        self.changes
            .slide_to(self.changes.open, !cx.reduce_motion());
        cx.notify();
    }

    pub(super) fn changes_pill(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some((count, additions, deletions)) = self.change_totals(cx) else {
            // Nothing left to show: Keep, Undo or a new turn took it.
            self.changes.open = false;
            *self.changes.slide.borrow_mut() = None;
            return None;
        };
        let open = self.changes.open;
        let label: SharedString = format!(
            "{count} changed {}: {additions} additions, {deletions} deletions",
            if count == 1 { "file" } else { "files" }
        )
        .into();
        let squares = squares(additions, deletions);
        Some(
            div()
                .id("changes-pill")
                .debug_selector(|| "changes-pill".into())
                .role(Role::Button)
                .aria_label(label)
                .aria_expanded(open)
                .relative()
                .h(px(30.))
                .flex()
                .items_stretch()
                .rounded_full()
                .border_1()
                .border_color(content(0.1))
                .p(px(3.))
                .child(crate::transcript::floating_surface())
                .child(
                    div()
                        .h_full()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .pl(px(10.))
                        .pr(px(7.))
                        .rounded_full()
                        .text_color(content(0.55))
                        .cursor_pointer()
                        .when(open, |d| d.bg(content(0.06)).text_color(content(1.)))
                        .hover(|s| s.bg(content(0.06)).text_color(content(1.)))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                // The Commit pill's label: 12px, regular weight.
                                .text_size(px(12.))
                                .font_features(tabular())
                                .child(
                                    div()
                                        .text_color(rgb(DIFF_ADD))
                                        .child(format!("+{}", format_integer(additions))),
                                )
                                .child(
                                    div()
                                        .text_color(rgb(DIFF_DEL))
                                        .child(format!("-{}", format_integer(deletions))),
                                ),
                        )
                        .child(div().flex().items_center().gap(px(1.5)).children(
                            squares.into_iter().map(|square| {
                                div().size(px(6.)).rounded(px(1.5)).bg(match square {
                                    Square::Add => rgb(DIFF_ADD).into(),
                                    Square::Del => rgb(DIFF_DEL).into(),
                                    Square::Empty => content(0.12),
                                })
                            }),
                        ))
                        .child(icon(if open { "chevron-up" } else { "chevron-down" }, 14.)),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, _, cx| this.toggle_changes(cx)))
                .into_any_element(),
        )
    }

    /// SessionReview's card on the ledge, while the pill holds it open.
    pub(super) fn changes_shelf(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if self.change_totals(cx).is_none() {
            return div().into_any_element();
        }
        let Some(progress) = self.changes.progress() else {
            return div().into_any_element();
        };
        let width = self.composer_anchor.get().map(|bounds| bounds.size.width);
        // Over a queued message or background tasks it tucks in behind them.
        let inset = if self.follow_ups.state.messages.is_empty() && self.background.tasks.is_empty()
        {
            px(8.)
        } else {
            px(20.)
        };
        let card_width = (width.unwrap_or_default() - inset * 2.).max(px(0.));
        let frame = composer_shelf::frame(self.classic_layout).overflow_hidden();
        let card = self
            .review
            .update(cx, |review, cx| review.contents(frame, cx));
        composer_shelf::shelf("changes-shelf")
            .debug_selector(|| "changes-shelf".into())
            .px(inset)
            .child(match progress {
                None => card,
                Some(progress) => {
                    composer_shelf::reveal(card, card_width, progress.clamp(0., 1.), window)
                }
            })
            .into_any_element()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Square {
    Add,
    Del,
    Empty,
}

/// GitHub's diffstat bar: five squares split by share of changed lines, each
/// side keeping at least one square when it changed anything.
fn squares(additions: i64, deletions: i64) -> [Square; SQUARES] {
    let mut out = [Square::Empty; SQUARES];
    let total = additions.max(0) + deletions.max(0);
    if total == 0 {
        return out;
    }
    let share = |n: i64| ((n.max(0) as f64 / total as f64) * SQUARES as f64).round() as usize;
    let mut add = share(additions).max(usize::from(additions > 0));
    let mut del = share(deletions).max(usize::from(deletions > 0));
    while add + del > SQUARES {
        if add >= del {
            add -= 1;
        } else {
            del -= 1;
        }
    }
    for (ix, square) in out.iter_mut().enumerate() {
        *square = if ix < add {
            Square::Add
        } else if ix < add + del {
            Square::Del
        } else {
            Square::Empty
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{ChangesShelf, Square::*, squares};

    #[test]
    fn a_slide_keeps_running_from_its_resting_end_until_done() {
        let mut shelf = ChangesShelf {
            open: true,
            ..Default::default()
        };
        shelf.slide_to(true, true);
        // Opened and settled: rests without a slide.
        shelf
            .slide
            .borrow()
            .as_ref()
            .unwrap()
            .elapsed(std::time::Duration::from_secs(5));
        assert_eq!(shelf.progress(), Some(None));
        // Closing starts at fully open, yet still counts as running.
        shelf.open = false;
        shelf.slide_to(false, true);
        assert!(matches!(shelf.progress(), Some(Some(p)) if p >= 0.99));
        shelf
            .slide
            .borrow()
            .as_ref()
            .unwrap()
            .elapsed(std::time::Duration::from_secs(5));
        assert_eq!(shelf.progress(), None, "folded away");
    }

    #[test]
    fn squares_split_by_share_and_keep_a_minority() {
        assert_eq!(squares(0, 0), [Empty; 5]);
        assert_eq!(squares(10, 0), [Add; 5]);
        assert_eq!(squares(0, 3), [Del; 5]);
        assert_eq!(squares(482, 37), [Add, Add, Add, Add, Del]);
        assert_eq!(squares(1, 1), [Add, Add, Del, Del, Del]);
        assert_eq!(squares(3, 1), [Add, Add, Add, Add, Del]);
    }
}
