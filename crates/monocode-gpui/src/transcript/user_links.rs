//! A prompt's GitHub issue or pull request link as a chip with a hover card,
//! ported from src/features/sessions/ui/UserLinkPreview.tsx.
use super::*;
use crate::popover::Align;
use gpui::{AnyElement, HighlightStyle, Hsla, StyledText, Task};
use monocode_core::inbox::{Details, Item};
use monocode_core::user_links::{GithubWorkItemLink, plain_text_summary};
use std::{
    ops::Range,
    path::Path,
    sync::{LazyLock, Mutex},
    time::Duration,
};

const HOVER_OPEN_DELAY: Duration = Duration::from_millis(220);
const HOVER_CLOSE_DELAY: Duration = Duration::from_millis(100);
const CARD_WIDTH: f32 = 360.;
const VIOLET: u32 = 0xa78bfa;
const EMERALD: u32 = 0x34d399;
const ROSE: u32 = 0xfb7185;

/// githubTasks.ts's workItemByKey and detailsByKey: one cache for the app.
type Key = (String, &'static str, u64);
static ITEMS: LazyLock<Mutex<HashMap<Key, Item>>> = LazyLock::new(Default::default);
static DETAILS: LazyLock<Mutex<HashMap<Key, Details>>> = LazyLock::new(Default::default);

fn key(work: &GithubWorkItemLink) -> Key {
    (work.repo.to_lowercase(), work.kind, work.number)
}
fn peek_item(key: &Key) -> Option<Item> {
    ITEMS.lock().ok()?.get(key).cloned()
}
fn peek_details(key: &Key) -> Option<Details> {
    DETAILS.lock().ok()?.get(key).cloned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Load {
    Loading,
    Ready,
    Unavailable,
}

/// The open hover card, keyed by its prompt's source index.
#[derive(Default)]
pub(super) struct Cards {
    open: Option<(usize, GithubWorkItemLink)>,
    anchors: HashMap<usize, AnchorSlot>,
    opening: Option<Task<()>>,
    closing: Option<Task<()>>,
    /// Each chip requests its details once, as React's `requestStarted`.
    loads: HashMap<Key, Load>,
}

impl TranscriptView {
    /// A text chunk with its link drawn as a chip. Lines before and after the
    /// link stay one text run; only the link's line flows word by word.
    pub(super) fn user_link_text(
        &mut self,
        text: &SharedString,
        link: &UserMessageLink,
        row: (usize, usize),
        message_start: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // One search pass and one selection handle for every piece of the chunk.
        let pieces = Pieces {
            text: text.clone(),
            highlights: markdown::mark_styles(
                text,
                vec![],
                self.search_match.as_ref().map_or("", |(_, q)| q.as_str()),
                self.search_match
                    .as_ref()
                    .is_some_and(|(start, _)| *start == message_start),
            ),
            selection: self
                .text_selection(row, message_start, false, cx)
                .map(|selection| selection.with_text(text.clone())),
        };
        let Some(work) = link.link.github_work_item.clone() else {
            return pieces.piece(0..text.len());
        };
        let url = link.range.clone();
        let line_start = text[..url.start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = text[url.end..]
            .find('\n')
            .map_or(text.len(), |i| url.end + i);
        // The space after the link is text in React; here it is the chip's gap.
        let space_after = text[url.end..line_end].starts_with([' ', '\t']);
        let line = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .children(words(text, line_start..url.start).map(|range| pieces.piece(range)))
            .child(self.work_item_chip(message_start, &link.link.url, &work, cx))
            .when(space_after, |d| d.child(space()))
            .children(words(text, url.end..line_end).map(|range| pieces.piece(range)));
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .when(line_start > 0, |d| d.child(pieces.piece(0..line_start - 1)))
            .child(line)
            .when(line_end < text.len(), |d| {
                d.child(pieces.piece(line_end + 1..text.len()))
            })
            .into_any_element()
    }
    /// GithubWorkItemPreview's compact chip: "PR #12" or "Issue #7".
    fn work_item_chip(
        &mut self,
        message_start: usize,
        url: &str,
        work: &GithubWorkItemLink,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let anchor = self
            .work_item_cards
            .anchors
            .entry(message_start)
            .or_default()
            .clone();
        let pr = work.kind == "pr";
        let ink = rgb(if pr { VIOLET } else { EMERALD });
        let title = peek_item(&key(work))
            .map(|item| item.title.trim().to_owned())
            .filter(|title| !title.is_empty());
        let label = format!(
            "Open {} #{} in {}{}",
            if pr { "pull request" } else { "issue" },
            work.number,
            work.repo,
            title.map(|t| format!(": {t}")).unwrap_or_default()
        );
        let open_url = url.to_owned();
        let hovered = work.clone();
        div()
            .id(("work-item-chip", message_start))
            .role(Role::Link)
            .aria_label(label)
            .relative()
            .mx(px(1.))
            .pb(px(1.))
            .flex()
            .flex_shrink_0()
            .items_center()
            .cursor_pointer()
            .text_size(px(12.))
            .line_height(px(16.))
            .child(anchor.probe())
            .child(
                div()
                    .h(px(18.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .rounded(px(6.))
                    .bg(alpha(ink, 0.1))
                    .text_color(alpha(ink, 0.9))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(
                        icon(if pr { "git-pull-request" } else { "circle-dot" }, 12.)
                            .text_color(alpha(ink, 0.9)),
                    )
                    .child(format!(
                        "{} #{}",
                        if pr { "PR" } else { "Issue" },
                        work.number
                    )),
            )
            .on_hover(cx.listener(move |this, hovered_now: &bool, _, cx| {
                if *hovered_now {
                    this.show_work_item_card(message_start, hovered.clone(), cx);
                } else {
                    this.hide_work_item_card_after_delay(cx);
                }
            }))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.hide_work_item_card(cx);
                cx.open_url(&open_url);
            }))
            .into_any_element()
    }
    fn show_work_item_card(
        &mut self,
        message_start: usize,
        work: GithubWorkItemLink,
        cx: &mut Context<Self>,
    ) {
        let cards = &mut self.work_item_cards;
        cards.closing = None;
        if cards
            .open
            .as_ref()
            .is_some_and(|(at, _)| *at == message_start)
        {
            return;
        }
        cards.opening = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOVER_OPEN_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                this.work_item_cards.opening = None;
                this.load_work_item(&work, cx);
                this.work_item_cards.open = Some((message_start, work));
                cx.notify();
            });
        }));
    }
    fn hide_work_item_card_after_delay(&mut self, cx: &mut Context<Self>) {
        let cards = &mut self.work_item_cards;
        cards.opening = None;
        if cards.open.is_none() {
            return;
        }
        cards.closing = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOVER_CLOSE_DELAY).await;
            let _ = this.update(cx, |this, cx| this.hide_work_item_card(cx));
        }));
    }
    fn hide_work_item_card(&mut self, cx: &mut Context<Self>) {
        let cards = &mut self.work_item_cards;
        cards.opening = None;
        cards.closing = None;
        if cards.open.take().is_some() {
            cx.notify();
        }
    }
    fn load_work_item(&mut self, work: &GithubWorkItemLink, cx: &mut Context<Self>) {
        let key = key(work);
        if self.work_item_cards.loads.contains_key(&key) {
            return;
        }
        let item = peek_item(&key);
        let details = peek_details(&key);
        if item.is_some() && details.is_some() {
            self.work_item_cards.loads.insert(key, Load::Ready);
            return;
        }
        self.work_item_cards
            .loads
            .insert(key.clone(), Load::Loading);
        let cwd = self.file_link_base.clone().unwrap_or_else(|| ".".into());
        let (repo, kind, number) = (work.repo.clone(), work.kind, work.number as i64);
        let executor = cx.background_executor().clone();
        let fetch_item = item.is_none().then(|| {
            let (cwd, repo) = (cwd.clone(), repo.clone());
            executor.spawn(async move {
                monocode_core::inbox::github_work_item(Path::new(&cwd), &repo, kind, number)
            })
        });
        let fetch_details = details.is_none().then(|| {
            executor.spawn(async move {
                monocode_core::inbox::github_work_item_details(Path::new(&cwd), &repo, kind, number)
            })
        });
        cx.spawn(async move |this, cx| {
            let item = match fetch_item {
                Some(task) => task.await.ok().inspect(|item| {
                    if let Ok(mut items) = ITEMS.lock() {
                        items.insert(key.clone(), item.clone());
                    }
                }),
                None => item,
            };
            let details = match fetch_details {
                Some(task) => task.await.ok().inspect(|details| {
                    if let Ok(mut all) = DETAILS.lock() {
                        all.insert(key.clone(), details.clone());
                    }
                }),
                None => details,
            };
            let load = if item.is_some() || details.is_some() {
                Load::Ready
            } else {
                Load::Unavailable
            };
            let _ = this.update(cx, |this, cx| {
                this.work_item_cards.loads.insert(key, load);
                cx.notify();
            });
        })
        .detach();
    }
    /// The open chip's card, above it (GithubWorkItemCard).
    pub(super) fn work_item_card(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (message_start, work) = self.work_item_cards.open.clone()?;
        let anchor = self.work_item_cards.anchors.get(&message_start)?.clone();
        let key = key(&work);
        let item = peek_item(&key);
        let details = peek_details(&key);
        let load = self
            .work_item_cards
            .loads
            .get(&key)
            .copied()
            .unwrap_or(Load::Loading);
        let pr = work.kind == "pr";
        let (status_icon, status_label, status_ink) = status(pr, item.as_ref());
        let summary = plain_text_summary(details.as_ref().map_or("", |d| d.body.as_str()));
        let updated = item
            .as_ref()
            .map(|item| crate::inbox::relative_time(&item.updated_at))
            .unwrap_or_default();
        let author = details
            .as_ref()
            .map(|d| (d.author.clone(), d.author_avatar_url.clone()))
            .filter(|(name, _)| !name.is_empty());
        let branches = details
            .as_ref()
            .filter(|d| pr && !d.base_ref_name.is_empty() && !d.head_ref_name.is_empty())
            .map(|d| format!("{} ← {}", d.base_ref_name, d.head_ref_name));
        let labels = item.as_ref().map_or(&[][..], |i| &i.labels[..]);
        let assignees = item.as_ref().map_or(&[][..], |i| &i.assignees[..]);
        let summary_text = |text: String| {
            div()
                .mt(px(6.))
                .text_size(px(11.))
                .line_height(px(11. * 1.45))
                .text_color(content(0.55))
                .line_clamp(3)
                .text_ellipsis()
                .child(text)
        };
        let headline = |text: String| {
            div()
                .text_size(px(13.))
                .line_height(px(13. * 1.35))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(content(1.))
                .child(text)
        };
        let body = match (&item, load) {
            (Some(item), _) => div()
                .child(headline(item.title.clone()).mt(px(8.)).line_clamp(2).text_ellipsis())
                .when(!summary.is_empty(), |d| d.child(summary_text(summary.clone())))
                .into_any_element(),
            (None, Load::Loading) => skeleton(),
            (None, _) => div()
                .mt(px(8.))
                .child(headline(format!(
                    "{} #{}",
                    if pr { "Pull request" } else { "Issue" },
                    work.number
                )))
                .child(if summary.is_empty() {
                    div()
                        .mt_1()
                        .text_size(px(11.))
                        .line_height(px(11. * 1.625))
                        .text_color(content(0.5))
                        .child(
                            "Details aren't available here, but the link can still be opened on GitHub.",
                        )
                } else {
                    summary_text(summary.clone())
                })
                .into_any_element(),
        };
        let card = div()
            .id("github-work-item-card")
            .role(Role::Tooltip)
            .min_w_0()
            .flex()
            .flex_col()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.work_item_cards.closing = None;
                } else {
                    this.hide_work_item_card_after_delay(cx);
                }
            }))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(11.))
                    .child(icon(status_icon, 16.).text_color(status_ink))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(content(0.65))
                            .child(work.repo.clone()),
                    )
                    .child(div().flex_shrink_0().text_color(content(0.35)).child("·"))
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(crate::fonts::mono())
                            .text_color(content(0.5))
                            .child(format!("#{}", work.number)),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(content(0.07))
                            .px_2()
                            .py(px(2.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(status_ink)
                            .child(status_label),
                    ),
            )
            .child(body)
            .when(author.is_some() || !updated.is_empty(), |d| {
                d.child(
                    div()
                        .mt(px(10.))
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(10.))
                        .text_color(content(0.45))
                        .when_some(author.clone(), |d, (name, avatar)| {
                            d.child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(crate::inbox::avatar_image(
                                        "github", &name, &avatar, 16., window, cx,
                                    ))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(content(0.55))
                                            .child(name),
                                    ),
                            )
                        })
                        .when(author.is_some() && !updated.is_empty(), |d| d.child("·"))
                        .when(!updated.is_empty(), |d| {
                            d.child(div().flex_shrink_0().child(format!("Updated {updated}")))
                        }),
                )
            })
            .when_some(branches, |d, branches| {
                d.child(
                    div()
                        .mt_2()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .rounded(px(6.))
                        .bg(content(0.045))
                        .px_2()
                        .py(px(6.))
                        .text_size(px(10.))
                        .text_color(content(0.5))
                        .child(icon("git-compare", 14.).text_color(content(0.5)))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_family(crate::fonts::mono())
                                .child(branches),
                        ),
                )
            })
            .when(!labels.is_empty() || !assignees.is_empty(), |d| {
                d.child(
                    div()
                        .mt(px(10.))
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .flex_wrap()
                                .gap_1()
                                .children(labels.iter().take(3).map(label_chip))
                                .when(labels.len() > 3, |d| {
                                    d.child(
                                        div()
                                            .px_1()
                                            .text_size(px(9.))
                                            .text_color(content(0.35))
                                            .child(format!("+{}", labels.len() - 3)),
                                    )
                                }),
                        )
                        .when(!assignees.is_empty(), |d| {
                            let names = assignees
                                .iter()
                                .map(|p| p.login.as_str())
                                .collect::<Vec<_>>()
                                .join(", ");
                            let mut stack = div()
                                .id("work-item-assignees")
                                .aria_label(format!("Assigned to {names}"))
                                .flex_shrink_0()
                                .flex();
                            for (ix, person) in assignees.iter().take(3).enumerate() {
                                stack = stack.child(
                                    div()
                                        .when(ix > 0, |d| d.ml(px(-4.)))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(crate::theme::pane_fill())
                                        .child(crate::inbox::avatar_image(
                                            "github",
                                            &person.login,
                                            &person.avatar_url,
                                            16.,
                                            window,
                                            cx,
                                        )),
                                );
                            }
                            d.child(stack)
                        }),
                )
            })
            .child(
                div()
                    .mt_3()
                    .pt_2()
                    .border_t_1()
                    .border_color(content(0.07))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(10.))
                    .text_color(content(0.35))
                    .child(icon("external-link", 12.).text_color(content(0.35)))
                    .child("Click the chip to open on GitHub"),
            );
        let dismiss = cx.weak_entity();
        Some(
            popover("github-work-item-popover", Anchor::Slot(anchor))
                .side(Side::Top)
                .align(Align::Start)
                .gap(6.)
                .width(CARD_WIDTH)
                .padding(14.)
                .allow_outside_press()
                .on_dismiss(move |_, cx| {
                    let _ = dismiss.update(cx, |this, cx| this.hide_work_item_card(cx));
                })
                .child(card)
                .into_any_element(),
        )
    }
}

/// A chunk's shared search highlights and selection, cut per piece.
struct Pieces {
    text: SharedString,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    selection: Option<crate::transcript::selection::TextSelection>,
}
impl Pieces {
    /// `range` of the chunk, selectable and search-highlighted in place.
    fn piece(&self, range: Range<usize>) -> AnyElement {
        let piece = &self.text[range.clone()];
        // A word's trailing whitespace is the gap after it, as rich_inline's.
        let word = piece.trim_end_matches([' ', '\t']);
        let gap = word.len() < piece.len();
        let end = range.start + word.len();
        let highlights = self
            .highlights
            .iter()
            .filter_map(|(r, style)| {
                let (start, stop) = (r.start.max(range.start), r.end.min(end));
                (start < stop).then(|| (start - range.start..stop - range.start, *style))
            })
            .collect::<Vec<_>>();
        let word: SharedString = word.to_owned().into();
        let styled = StyledText::new(word.clone()).with_highlights(highlights);
        let layout = styled.layout().clone();
        let child = styled.into_any_element();
        let child = match &self.selection {
            Some(selection) => selection.offset(range.start).wrap(word, layout, child),
            None => child,
        };
        div()
            .flex()
            .child(child)
            .when(gap, |d| d.child(space()))
            .into_any_element()
    }
}

/// One space at the bubble's 14px text.
fn space() -> gpui::Div {
    div().w(px(14. * 0.25)).h(px(1.))
}

/// Byte ranges of `range`'s words, each with the spaces that follow it.
fn words(text: &str, range: Range<usize>) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut at = range.start;
    text[range.clone()]
        .split_inclusive([' ', '\t'])
        .map(move |part| {
            let start = at;
            at += part.len();
            start..at
        })
        .filter(|r| !text[r.clone()].trim().is_empty())
}

fn alpha(color: gpui::Rgba, a: f32) -> Hsla {
    let mut color: Hsla = color.into();
    color.a = a;
    color
}

/// workItemStatus: draft before merged before closed, as the React card.
fn status(pr: bool, item: Option<&Item>) -> (&'static str, &'static str, Hsla) {
    let open_icon = if pr { "git-pull-request" } else { "circle-dot" };
    match item {
        None => (
            open_icon,
            if pr { "Pull request" } else { "Issue" },
            content(0.5),
        ),
        Some(item) if item.draft => ("git-pull-request-draft", "Draft", content(0.5)),
        Some(item) if item.state == "merged" => ("git-merge", "Merged", alpha(rgb(VIOLET), 0.9)),
        Some(item) if item.state == "closed" => (
            if pr {
                "git-pull-request-closed"
            } else {
                "circle-x"
            },
            "Closed",
            alpha(rgb(ROSE), 0.9),
        ),
        Some(_) => (open_icon, "Open", alpha(rgb(EMERALD), 0.9)),
    }
}

fn label_chip(label: &monocode_core::inbox::Label) -> gpui::Div {
    let hex = label.color.trim();
    let dot = (hex.len() == 6)
        .then(|| u32::from_str_radix(hex, 16).ok())
        .flatten()
        .map_or_else(|| content(0.55), |c| rgb(c).into());
    div()
        .max_w(px(112.))
        .flex()
        .items_center()
        .gap_1()
        .rounded_full()
        .border_1()
        .border_color(content(0.1))
        .bg(content(0.035))
        .px(px(6.))
        .py(px(2.))
        .text_size(px(9.))
        .text_color(content(0.55))
        .child(div().size(px(6.)).flex_shrink_0().rounded_full().bg(dot))
        .child(div().min_w_0().truncate().child(label.name.clone()))
}

fn skeleton() -> AnyElement {
    let bar = |width: gpui::DefiniteLength, height: f32, alpha: f32| {
        div()
            .h(px(height))
            .w(width)
            .rounded(px(4.))
            .bg(content(alpha))
    };
    div()
        .id("work-item-loading")
        .aria_label("Loading GitHub details")
        .mt(px(10.))
        .flex()
        .flex_col()
        .gap_2()
        .child(bar(gpui::relative(0.8), 12., 0.1))
        .child(bar(gpui::relative(1.), 8., 0.07))
        .child(bar(gpui::relative(2. / 3.), 8., 0.07))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Bounds, TestAppContext, WindowBounds, WindowOptions, point, size};

    #[gpui::test]
    fn a_pasted_pull_request_becomes_an_unclamped_chip_with_a_hover_card(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(900.), px(600.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| TranscriptView::new()),
            )
            .unwrap()
        });
        let work = GithubWorkItemLink {
            kind: "pr",
            repo: "acme/chip-test".into(),
            number: 1142,
        };
        // Seed the app cache so the card never shells out to `gh`.
        ITEMS.lock().unwrap().insert(
            key(&work),
            Item {
                title: "Fix long bubble widths".into(),
                state: "merged".into(),
                ..Default::default()
            },
        );
        DETAILS
            .lock()
            .unwrap()
            .insert(key(&work), Details::default());
        let tail = (1..=12)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        window
            .update(cx, |view, _, cx| {
                let mut source = monocode_core::transcript::Transcript::default();
                source.begin("user");
                source.append(&format!(
                    "hey lets review this pr\n\nhttps://github.com/acme/chip-test/pull/1142 its for the native one\n{tail}"
                ));
                view.patch(0, source.rows, cx);
                view.visible = true;
            })
            .unwrap();
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                let (chunk, link) = view.messages[0].user_link.clone().unwrap();
                assert_eq!(chunk, 0);
                assert_eq!(link.link.github_work_item, Some(work.clone()));
                // React shows a message with a link preview in full.
                assert!(view.clamped_prompts.is_empty());
                // The chip sits on the third line, after the blank one, below
                // the transcript's top inset and the bubble's padding.
                let chip = view.work_item_cards.anchors[&0].get().unwrap();
                let top = view.list.bounds_for_item(0).unwrap().top();
                assert!(chip.top() - top >= px(40.), "{chip:?} {top:?}");
                assert!(chip.top() - top < px(100.), "{chip:?} {top:?}");
                // Every chunk is shown: two rows for this message.
                assert_eq!(view.rows.len(), 2);
                view.show_work_item_card(0, work.clone(), cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert!(view.work_item_cards.open.is_none())
            })
            .unwrap();
        cx.executor().advance_clock(HOVER_OPEN_DELAY);
        cx.run_until_parked();
        window
            .update(cx, |view, window, cx| {
                assert_eq!(view.work_item_cards.loads[&key(&work)], Load::Ready);
                assert!(view.work_item_card(window, cx).is_some());
                let item = peek_item(&key(&work));
                assert_eq!(status(true, item.as_ref()).1, "Merged");
                view.hide_work_item_card_after_delay(cx);
            })
            .unwrap();
        cx.executor().advance_clock(HOVER_CLOSE_DELAY);
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert!(view.work_item_cards.open.is_none())
            })
            .unwrap();
    }
}
