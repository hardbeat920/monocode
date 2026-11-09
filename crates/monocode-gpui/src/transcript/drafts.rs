use super::*;
use gpui::{PathBuilder, point};
use monocode_core::follow_up::Message as Draft;

impl TranscriptView {
    pub(crate) fn set_saved_draft(&mut self, draft: Option<Draft>, cx: &mut Context<Self>) {
        if self.saved_draft == draft {
            return;
        }
        self.saved_draft = draft;
        if self.saved_draft.is_none() && self.anchor_turn == Some(SAVED_DRAFT_START) {
            self.anchor_turn = self.source.iter().rposition(|row| row.is_prompt());
            self.prompt_rise = None;
            self.prompt_rise_pending = false;
        }
        if self.source.is_empty() {
            self.rows.clear();
            self.push_saved_draft();
            self.list.reset(self.rows.len() + 1);
            self.list.scroll_to_end();
        } else {
            self.rebuild(0, cx);
        }
        cx.notify();
    }
    pub(super) fn push_saved_draft(&mut self) {
        if let Some(draft) = &self.saved_draft {
            if self
                .rows
                .iter()
                .any(|row| matches!(row.body, Body::SavedDraft))
            {
                return;
            }
            let index = self
                .rows
                .iter()
                .position(|row| row.key.0 >= self.base + draft.at)
                .unwrap_or(self.rows.len());
            self.rows.insert(
                index,
                UiRow {
                    key: (SAVED_DRAFT_START, 0),
                    body: Body::SavedDraft,
                    message: usize::MAX,
                    first: false,
                    last: false,
                    work: false,
                    work_tail: false,
                    thought_phase: None,
                },
            );
        }
    }
    pub(super) fn saved_draft_row(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(draft) = &self.saved_draft else {
            return div().into_any_element();
        };
        let (mut rise, mut opacity) = (0., 1.);
        if let Some((SAVED_DRAFT_START, born)) = self.prompt_rise
            && !cx.reduce_motion()
        {
            let elapsed = born.elapsed().as_secs_f32();
            let progress = crate::reveal::bezier((elapsed / 0.56).min(1.), 0.22, 1., 0.36, 1.);
            rise =
                (f32::from(self.viewport.size.height) * 0.3 - TOP_INSET).max(0.) * (1. - progress);
            opacity = crate::reveal::bezier((elapsed / 0.48).min(1.), 0., 0., 0.58, 1.);
        }
        div()
            .id("saved-message-draft")
            .w_full()
            .flex()
            .justify_center()
            .opacity(opacity)
            .child(
                div()
                    .relative()
                    .top(px(rise))
                    .w_full()
                    .max_w(px(COLUMN))
                    .px(px(32.))
                    .pt(px(12.))
                    .pb(px(20.))
                    .flex()
                    .justify_end()
                    .child(
                        div()
                            .relative()
                            .min_w_0()
                            .max_w(px(576.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(content(0.))
                            .bg(crate::theme::draft_bubble_fill())
                            .px_3()
                            .py_2()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .child(draft_dashes(Some(12.), crate::theme::draft_bubble_edge()))
                            .children(draft.attachments.iter().map(|path| {
                                div()
                                    .mb_1()
                                    .text_size(px(12.))
                                    .text_color(content(0.65))
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(icon("file", 14.))
                                    .child(
                                        path.file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .to_string(),
                                    )
                            }))
                            .child(
                                div()
                                    .id("saved-draft-text")
                                    .role(Role::Label)
                                    .aria_label(draft.text.clone())
                                    .child(draft.text.clone()),
                            )
                            .child(
                                div()
                                    .relative()
                                    .mt_2()
                                    .pt_2()
                                    .border_t_1()
                                    .border_color(content(0.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(draft_dashes(None, content(0.2)))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .text_size(px(12.))
                                            .text_color(content(0.5))
                                            .child(icon("circle-dashed-hi", 14.))
                                            .child("Draft"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                draft_button(
                                                    "remove-message-draft",
                                                    "Remove draft",
                                                )
                                                .px_2()
                                                .text_color(content(0.55))
                                                .child(icon("trash", 14.))
                                                .child("Remove")
                                                .on_click(cx.listener(|_, _, _, cx| {
                                                    cx.emit(ResponseRequest::Draft { send: false })
                                                })),
                                            )
                                            .child(
                                                draft_button("send-message-draft", "Send draft")
                                                    .px(px(10.))
                                                    .bg(white(1.))
                                                    .text_color(black(1.))
                                                    .child("Send")
                                                    .child(icon("send", 14.).text_color(black(1.)))
                                                    .on_click(cx.listener(|_, _, _, cx| {
                                                        cx.emit(ResponseRequest::Draft {
                                                            send: true,
                                                        })
                                                    })),
                                            ),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// Browser dashed 1px borders use approximately 3px dashes with 3px gaps.
/// GPUI's built-in 2px/1px pattern is visibly tighter. Keep the browser pattern
/// local to draft bubbles and their divider, including the rounded corners.
fn draft_dashes(radius: Option<f32>, color: gpui::Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let left = bounds.left() + px(0.5);
            let right = bounds.right() - px(0.5);
            let top = bounds.top() + px(0.5);
            let bottom = bounds.bottom() - px(0.5);
            let mut path = PathBuilder::stroke(px(1.)).dash_array(&[px(3.), px(3.)]);
            if let Some(radius) = radius {
                let r = px((radius - 0.5).max(0.))
                    .min((right - left) / 2.)
                    .min((bottom - top) / 2.);
                let radii = point(r, r);
                path.move_to(point(left + r, top));
                path.line_to(point(right - r, top));
                path.arc_to(radii, px(0.), false, true, point(right, top + r));
                path.line_to(point(right, bottom - r));
                path.arc_to(radii, px(0.), false, true, point(right - r, bottom));
                path.line_to(point(left + r, bottom));
                path.arc_to(radii, px(0.), false, true, point(left, bottom - r));
                path.line_to(point(left, top + r));
                path.arc_to(radii, px(0.), false, true, point(left + r, top));
                path.close();
            } else {
                path.move_to(point(left, top));
                path.line_to(point(right, top));
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
fn draft_button(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .h(px(28.))
        .rounded_md()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .hover(|s| s.bg(content(0.1)))
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn restored_drafts_do_not_replay_submission(cx: &mut TestAppContext) {
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.set_saved_draft(
                Some(Draft {
                    id: "restored".into(),
                    text: "already saved".into(),
                    ..Default::default()
                }),
                cx,
            );
            assert!(view.anchor_turn.is_none());
            assert!(view.prompt_rise.is_none());
        });
    }

    #[gpui::test]
    fn saved_draft_keeps_its_position_as_history_grows_and_removal_leaves_history_intact(
        cx: &mut TestAppContext,
    ) {
        let view = cx.new(|_| TranscriptView::new());
        let row = |role: &str, text: &str| Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        };
        view.update(cx, |view, cx| {
            view.patch(
                0,
                vec![row("user", "first"), row("assistant", "answer")],
                cx,
            );
            view.set_saved_draft(
                Some(Draft {
                    id: "draft".into(),
                    text: "for later".into(),
                    at: 2,
                    ..Default::default()
                }),
                cx,
            );
            view.patch(
                2,
                vec![
                    row("user", "another message"),
                    row("assistant", "another answer"),
                ],
                cx,
            );
            let draft = view
                .rows
                .iter()
                .position(|r| matches!(r.body, Body::SavedDraft))
                .unwrap();
            let next = view.rows.iter().position(|r| r.key.0 == 2).unwrap();
            assert!(draft < next);
            assert_eq!(
                view.rows
                    .iter()
                    .filter(|r| matches!(r.body, Body::SavedDraft))
                    .count(),
                1
            );
            let history = view.source.clone();
            view.set_saved_draft(None, cx);
            assert!(
                view.rows
                    .iter()
                    .all(|r| !matches!(r.body, Body::SavedDraft))
            );
            assert_eq!(view.source, history);
        });
    }
}
