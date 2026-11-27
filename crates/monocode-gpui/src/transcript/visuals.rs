//! Visuals are persistent transcript messages with lazily created native views.
use super::*;
use crate::visuals::{VisualChanged, VisualView};
use monocode_core::visual::VisualDocument;

// Each view owns a bounded script interpreter. Long conversations must not
// keep one interpreter alive for every visual that has ever been painted.
const RETAINED_VISUALS: usize = 8;

pub(super) struct CachedVisual {
    document: Arc<VisualDocument>,
    view: gpui::Entity<VisualView>,
    touched: Instant,
    _changes: gpui::Subscription,
}

/// Collects the visual ids an answer places with `![caption](visual:id)`.
pub(crate) fn placements(text: &str, out: &mut HashSet<String>) {
    for (at, _) in text.match_indices("](visual:") {
        let id: String = text[at + 9..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            .collect();
        if !id.is_empty() && text[at + 9 + id.len()..].starts_with(')') {
            out.insert(id);
        }
    }
}

impl TranscriptView {
    fn visual_view(
        &mut self,
        message: usize,
        document: Arc<VisualDocument>,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<VisualView> {
        let unchanged = self.visuals.get(&message).is_some_and(|cached| {
            cached.document.id == document.id
                && cached.document.title == document.title
                && cached.document.source == document.source
        });
        if !unchanged {
            self.visuals.remove(&message);
            if self.visuals.len() >= RETAINED_VISUALS
                && let Some(oldest) = self
                    .visuals
                    .iter()
                    .min_by_key(|(_, cached)| cached.touched)
                    .map(|(&key, _)| key)
            {
                self.visuals.remove(&oldest);
            }
            let view = cx.new(|cx| VisualView::new((*document).clone(), cx));
            let changes = cx.subscribe(&view, |this, _, _: &VisualChanged, cx| {
                this.remeasure_visual_rows();
                this.list
                    .remeasure_items(this.footer_ix()..this.footer_ix() + 1);
                cx.notify();
            });
            self.visuals.insert(
                message,
                CachedVisual {
                    document,
                    view,
                    touched: Instant::now(),
                    _changes: changes,
                },
            );
        }
        let cached = self.visuals.get_mut(&message).unwrap();
        cached.touched = Instant::now();
        cached.view.clone()
    }

    /// A visual's own row, for one no answer placed in its prose.
    pub(super) fn visual_row(
        &mut self,
        message: usize,
        document: Arc<VisualDocument>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let view = self.visual_view(message, document, cx);
        view.update(cx, |view, cx| view.set_inline(None, cx));
        div()
            .w_full()
            .min_w_0()
            .py_3()
            .child(view)
            .into_any_element()
    }

    /// The published visual `id`, by its message's start, from the source:
    /// placed visuals have no row of their own.
    fn published(&mut self, id: &str) -> Option<(usize, Arc<VisualDocument>)> {
        if let Some(found) = self.published.get(id) {
            return Some(found.clone());
        }
        let mut at = self.source.len();
        while at > 0 {
            at -= 1;
            if self.source[at].role != "visual" || !self.source[at].first {
                continue;
            }
            let end = (at + 1..self.source.len())
                .find(|&i| self.source[i].first)
                .unwrap_or(self.source.len());
            let text: String = self.source[at..end]
                .iter()
                .map(|r| r.text.as_str())
                .collect();
            if !text.contains(id) {
                continue;
            }
            let Some(document) = serde_json::from_str::<VisualDocument>(&text)
                .ok()
                .filter(|d| d.id == id && d.validate().is_ok())
            else {
                continue;
            };
            let found = (at, Arc::new(document));
            self.published.insert(id.to_owned(), found.clone());
            return Some(found);
        }
        None
    }

    /// `![caption](visual:id)` in an answer: the published visual, in place.
    pub(super) fn placed_visual(
        &mut self,
        id: &str,
        caption: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some((message, document)) = self.published(id) else {
            return div()
                .text_size(px(13.))
                .text_color(content(0.5))
                .child(
                    if caption.is_empty() {
                        "Visual"
                    } else {
                        caption
                    }
                    .to_owned(),
                )
                .into_any_element();
        };
        let view = self.visual_view(message, document, cx);
        let caption = caption.to_owned();
        view.update(cx, |view, cx| view.set_inline(Some(caption), cx));
        div()
            .w_full()
            .min_w_0()
            .py_2()
            .child(view)
            .into_any_element()
    }

    /// Rows holding a visual, in its own row or placed in prose, change size
    /// when it does, when an answer places it and when the turn ends.
    pub(super) fn remeasure_visual_rows(&mut self) {
        for ix in 0..self.rows.len() {
            let visual = match &self.rows[ix].body {
                Body::Visual(_) => true,
                Body::Markdown(md) => matches!(md.part, markdown::Part::Visual { .. }),
                _ => false,
            };
            if visual {
                self.list.remeasure_items(ix..ix + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn row(role: &str, text: String) -> Row {
        Row {
            role: role.into(),
            text,
            first: true,
            sent_at: None,
        }
    }

    fn document(id: &str) -> VisualDocument {
        VisualDocument {
            id: id.into(),
            title: "Interactive result".into(),
            source: "return () => visual.text('Ready');".into(),
        }
    }

    #[gpui::test]
    fn visuals_remain_visible_when_work_is_folded_and_history_is_reloaded(cx: &mut TestAppContext) {
        let view = cx.new(|_| TranscriptView::new());
        let doc = document("plot-1");
        let mut rows = vec![row("user", "Show the results".into())];
        let tool = ToolCall::new("read", "Read");
        rows.push(row("tool", serde_json::to_string(&tool).unwrap()));
        let mut visual = monocode_core::transcript::Transcript::default();
        visual.begin("visual");
        visual.append(&serde_json::to_string(&doc).unwrap());
        rows.extend(visual.rows);
        rows.push(row("assistant", "The results are ready.".into()));
        view.update(cx, |view, cx| {
            view.patch(0, rows.clone(), cx);
            for reveal in view.reveals.values_mut() {
                reveal.settle(Instant::now());
            }
            view.rebuild(0, cx);
            assert!(view.rows.iter().any(|row| {
                matches!(&row.body, Body::Visual(value) if value.id == doc.id) && !row.work
            }));
            view.replace_history(rows, cx);
            assert_eq!(
                view.rows
                    .iter()
                    .filter(|row| matches!(row.body, Body::Visual(_)))
                    .count(),
                1
            );
        });
    }

    #[gpui::test]
    fn malformed_visuals_show_a_readable_error_without_exposing_the_payload(
        cx: &mut TestAppContext,
    ) {
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.patch(0, vec![row("visual", "{malformed".into())], cx);
            assert!(view.rows.iter().any(|row| {
                matches!(&row.body, Body::Plain(text) if text.as_ref() == "This visual could not be loaded.")
            }));
        });
    }

    #[gpui::test]
    fn retained_script_heaps_are_bounded_and_charged_to_history_cache(cx: &mut TestAppContext) {
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            let initial = view.history_weight();
            for index in 0..RETAINED_VISUALS + 2 {
                let document = Arc::new(document(&format!("visual-{index}")));
                let _ = view.visual_row(index, document, cx);
                assert!(view.visuals.len() <= RETAINED_VISUALS);
            }
            assert_eq!(view.visuals.len(), RETAINED_VISUALS);
            assert!(!view.visuals.contains_key(&0));
            assert_eq!(
                view.history_weight() - initial,
                RETAINED_VISUALS * monocode_core::visual::MEMORY_LIMIT_BYTES,
            );
            view.replace_history(Vec::new(), cx);
            assert!(view.visuals.is_empty());
        });
    }
}
