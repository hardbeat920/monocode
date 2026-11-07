// The Focused layout's ⌥Tab switcher: hold Option, tap Tab to step through
// every open session across every project, most recent first, and let go to
// switch. A tab split into several sessions offers each of its panes, and a
// project with nothing open still gets a card that opens it.
use super::*;
use crate::tab_tile::TileSize;
use std::{collections::HashMap, sync::Arc};

actions!(
    tab_switcher,
    [
        StepSwitcher,
        StepSwitcherBack,
        CommitSwitcher,
        DismissSwitcher
    ]
);

const CARD_WIDTH: f32 = 176.;
/// The preview's width inside a card's padding and border.
const PAGE_WIDTH: f32 = CARD_WIDTH - 18.;
/// Thumbnails are stored at twice the preview's width, for Retina.
const THUMB_WIDTH: u32 = (PAGE_WIDTH * 2.) as u32;
/// A visible chat is re-captured at most this often while it works.
const THUMB_POLL: std::time::Duration = std::time::Duration::from_secs(60);
const BADGE_TILE: TileSize = TileSize {
    w: 20.,
    h: 17.,
    radius: 5.,
};
pub(super) fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("alt-tab", StepSwitcher, Some("Workspace")),
        KeyBinding::new("alt-shift-tab", StepSwitcherBack, Some("Workspace")),
        KeyBinding::new("alt-tab", StepSwitcher, Some("TabSwitcher")),
        KeyBinding::new("alt-shift-tab", StepSwitcherBack, Some("TabSwitcher")),
        KeyBinding::new("right", StepSwitcher, Some("TabSwitcher")),
        KeyBinding::new("left", StepSwitcherBack, Some("TabSwitcher")),
        KeyBinding::new("alt-right", StepSwitcher, Some("TabSwitcher")),
        KeyBinding::new("alt-left", StepSwitcherBack, Some("TabSwitcher")),
        KeyBinding::new("enter", CommitSwitcher, Some("TabSwitcher")),
        KeyBinding::new("escape", DismissSwitcher, Some("TabSwitcher")),
    ]);
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Entry {
    Session(i64),
    Project(String),
}

/// Last captured picture of each open chat, for the switcher's cards.
#[derive(Default)]
pub(super) struct Thumbs {
    pub(super) images: HashMap<i64, Arc<gpui::RenderImage>>,
    taken: HashMap<i64, std::time::Instant>,
    poll: Option<gpui::Task<()>>,
}

pub(super) struct Open {
    entries: Vec<Entry>,
    index: usize,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    _deactivate: Subscription,
}

/// Window pixels (RGBA) to a card-sized BGRA sprite.
fn shrink(frame: image::RgbaImage) -> Arc<gpui::RenderImage> {
    let (w, h) = frame.dimensions();
    let width = THUMB_WIDTH.min(w.max(1));
    let height = ((h as f64 * width as f64 / w.max(1) as f64).round() as u32).max(1);
    let mut small =
        image::imageops::resize(&frame, width, height, image::imageops::FilterType::Triangle);
    for px in small.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    Arc::new(gpui::RenderImage::new(vec![image::Frame::new(small)]))
}

/// Open sessions by recency, then the rest in tab order.
fn by_recency(open: &[i64], recent: &[i64]) -> Vec<i64> {
    let mut ids: Vec<i64> = recent
        .iter()
        .copied()
        .filter(|id| open.contains(id))
        .collect();
    ids.extend(open.iter().copied().filter(|id| !recent.contains(id)));
    ids
}

/// Moves `id` to the front of the recency list.
pub(super) fn touch(recent: &mut Vec<i64>, id: i64) {
    recent.retain(|entry| *entry != id);
    recent.insert(0, id);
    recent.truncate(64);
}

fn step(index: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (index as isize + delta).rem_euclid(len as isize) as usize
}

impl Shell {
    /// A session can vanish under the open switcher (a mobile delete).
    fn switcher_session_live(&self, id: i64) -> bool {
        self.session_summaries.contains_key(&id) && !self.deleting.contains(&id)
    }

    /// Drops cards whose session went away, keeping the highlight in range.
    fn prune_switcher(&mut self) {
        let live: Vec<bool> = match &self.tab_switcher {
            Some(open) => open
                .entries
                .iter()
                .map(|entry| match entry {
                    Entry::Session(id) => self.switcher_session_live(*id),
                    Entry::Project(_) => true,
                })
                .collect(),
            None => return,
        };
        let Some(open) = self.tab_switcher.as_mut() else {
            return;
        };
        let mut keep = live.into_iter();
        open.entries.retain(|_| keep.next().unwrap_or(false));
        open.index = open.index.min(open.entries.len().saturating_sub(1));
    }

    pub(super) fn switcher_available(&self) -> bool {
        !self.workspace.classic_layout && self.settings.is_none()
    }

    fn switcher_entries(&self) -> Vec<Entry> {
        let open: Vec<i64> = self
            .workspace
            .open_sessions()
            .into_iter()
            .filter(|id| {
                *id > 0
                    && self.session_summaries.contains_key(id)
                    && !self.deleting.contains(id)
                    && !self.orchestration_worker(*id)
            })
            .collect();
        let sessions = by_recency(&open, &self.switcher_recent);
        let with_sessions: HashSet<&str> = sessions
            .iter()
            .filter_map(|id| self.session_summaries.get(id))
            .map(|s| s.cwd.as_str())
            .collect();
        let projects: Vec<String> = self
            .workspace
            .visible_project_order()
            .into_iter()
            .filter(|path| !with_sessions.contains(path.as_str()))
            .collect();
        sessions
            .into_iter()
            .map(Entry::Session)
            .chain(projects.into_iter().map(Entry::Project))
            .collect()
    }

    /// Photographs the window while it shows `id`: the frame on screen is
    /// read back once and shrunk off the main thread. Skipped when the last
    /// picture is newer than `fresh`, so switching costs nothing most times.
    pub(super) fn capture_thumb(
        &mut self,
        id: i64,
        fresh: std::time::Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.classic_layout
            || self.app_view_open()
            || self.tab_switcher.is_some()
            || self.settings.is_some()
            || id <= 0
            || !self.session_summaries.contains_key(&id)
            || self.workspace.active != Some(id)
            || self
                .switcher_thumbs
                .taken
                .get(&id)
                .is_some_and(|at| at.elapsed() < fresh)
        {
            return;
        }
        let Ok(frame) = window.render_to_image() else {
            return;
        };
        self.switcher_thumbs
            .taken
            .insert(id, std::time::Instant::now());
        cx.spawn_in(window, async move |this, cx| {
            let image = cx
                .background_executor()
                .spawn(async move { shrink(frame) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let open = this.workspace.open_sessions();
                let thumbs = &mut this.switcher_thumbs;
                if let Some(old) = thumbs.images.insert(id, image) {
                    cx.drop_image(old, Some(window));
                }
                // Closed chats give their pictures back.
                let gone: Vec<i64> = thumbs
                    .images
                    .keys()
                    .copied()
                    .filter(|id| !open.contains(id))
                    .collect();
                for gone in gone {
                    thumbs.taken.remove(&gone);
                    if let Some(old) = thumbs.images.remove(&gone) {
                        cx.drop_image(old, Some(window));
                    }
                }
                if this.tab_switcher.is_some() {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Once a minute, re-photograph the chat on screen if it is working, so a
    /// streaming thread's card stays current without a capture every frame.
    pub(super) fn ensure_thumb_poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.switcher_thumbs.poll.is_some() || self.workspace.classic_layout {
            return;
        }
        self.switcher_thumbs.poll = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(THUMB_POLL).await;
                let Ok(()) = this.update_in(cx, |this, window, cx| {
                    let Some(id) = this.workspace.active else {
                        return;
                    };
                    let working = this.session_summaries.contains_key(&id)
                        && this.session_metadata(id, cx).busy;
                    let missing = !this.switcher_thumbs.images.contains_key(&id);
                    if window.is_window_active() && (working || missing) {
                        this.capture_thumb(id, THUMB_POLL / 2, window, cx);
                    }
                }) else {
                    break;
                };
            }
        }));
    }

    pub(super) fn step_switcher(
        &mut self,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = self.tab_switcher.as_mut() {
            open.index = step(open.index, open.entries.len(), delta);
            cx.notify();
            return;
        }
        if !self.switcher_available() {
            return;
        }
        let entries = self.switcher_entries();
        if entries.len() < 2 {
            return;
        }
        let current = self.workspace.active.map(Entry::Session);
        // Starts one away from where you are, so a quick ⌥Tab flips back.
        let here = entries.iter().position(|e| Some(e) == current.as_ref());
        let index = match (here, delta < 0) {
            (Some(_), true) => entries.len() - 1,
            (Some(here), false) => step(here, entries.len(), 1),
            (None, true) => entries.len() - 1,
            (None, false) => 0,
        };
        if let Some(active) = self.workspace.active {
            self.capture_thumb(active, std::time::Duration::from_secs(5), window, cx);
        }
        self.close_menu(cx);
        self.close_preview(cx);
        // Option let go in another app never reaches this window: close
        // instead of leaving the dimmed switcher up for the return.
        let _deactivate = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.dismiss_switcher(window, cx);
            }
        });
        let focus = cx.focus_handle();
        let restore = window.focused(cx);
        window.focus(&focus, cx);
        self.tab_switcher = Some(Open {
            entries,
            index,
            focus,
            restore,
            _deactivate,
        });
        cx.notify();
        // Option already up: a tap that beat the first frame switches at once.
        if !window.modifiers().alt {
            self.commit_switcher(None, window, cx);
        }
    }

    pub(super) fn commit_switcher(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.tab_switcher.take() else {
            return;
        };
        cx.notify();
        match open.entries.get(index.unwrap_or(open.index)).cloned() {
            Some(Entry::Session(id)) if self.switcher_session_live(id) => self.open(id, window, cx),
            Some(Entry::Session(_)) => self.dismiss_focus(open.restore, window, cx),
            Some(Entry::Project(path)) => self.select_project(path, window, cx),
            None => self.dismiss_focus(open.restore, window, cx),
        }
    }

    pub(super) fn dismiss_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.tab_switcher.take() {
            self.dismiss_focus(open.restore, window, cx);
            cx.notify();
        }
    }

    fn dismiss_focus(
        &mut self,
        restore: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match restore {
            Some(restore) => window.focus(&restore, cx),
            None => self.focus_active(window, cx),
        }
    }

    /// Letting go of Option picks the highlighted card.
    pub(super) fn switcher_modifiers(
        &mut self,
        event: &gpui::ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab_switcher.is_some() && !event.modifiers.alt {
            self.commit_switcher(None, window, cx);
        }
    }

    fn switcher_card(
        &self,
        ix: usize,
        entry: &Entry,
        selected: bool,
        aspect: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (project, title, provider, status) = match entry {
            Entry::Session(id) => {
                let s = self.session_metadata(*id, cx);
                let status = if s.needs_approval() {
                    Some(
                        icon("circle-alert", 12.)
                            .text_color(rgb(0xfbbf24))
                            .into_any_element(),
                    )
                } else if s.busy || s.responding() {
                    // The tab strip's own thinking mark, so it moves in step.
                    self.tab_marks.get(id).map(|mark| {
                        mark.clone()
                            .cached(StyleRefinement::default().size(px(14.)))
                            .into_any_element()
                    })
                } else {
                    None
                };
                let title = s.display_title();
                (
                    s.summary.cwd.clone(),
                    if title.is_empty() {
                        "New session".into()
                    } else {
                        title
                    },
                    Some(s.summary.provider.clone()),
                    status,
                )
            }
            Entry::Project(path) => (path.clone(), self.project_label(path), None, None),
        };
        let project_label = self.project_label(&project);
        let mascot = self.project_mark(&project, 14.);
        let shell = cx.weak_entity();
        let hover = shell.clone();
        let thumb = match entry {
            Entry::Session(id) => self.switcher_thumbs.images.get(id).cloned(),
            Entry::Project(_) => None,
        };
        let page = match thumb {
            Some(image) => gpui::img(image)
                .w(px(PAGE_WIDTH))
                .h(px((PAGE_WIDTH * aspect).round()))
                .object_fit(gpui::ObjectFit::Cover)
                .rounded(px(6.))
                .border_1()
                .border_color(content(0.08))
                .into_any_element(),
            // Not photographed yet: the bare page surface, never a skeleton.
            None => div()
                .w(px(PAGE_WIDTH))
                .h(px((PAGE_WIDTH * aspect).round()))
                .rounded(px(6.))
                .bg(pane_fill())
                .border_1()
                .border_color(content(0.08))
                .into_any_element(),
        };
        // The provider badge rides the preview's corner, half outside it, so
        // the rounded clip never cuts it.
        let preview = div()
            .relative()
            .child(page)
            .children(provider.map(|provider| {
                div().absolute().top(px(-5.)).left(px(-5.)).child(mark_tile(
                    &BADGE_TILE,
                    Face::provider(&provider),
                    provider_glyph(&provider, 10.),
                ))
            }));
        div()
            .id(("tab-switcher-card", ix))
            .debug_selector(move || format!("tab-switcher-card-{ix}"))
            .role(Role::ListBoxOption)
            .aria_label(SharedString::from(format!("{title}, {project_label}")))
            .w(px(CARD_WIDTH))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .rounded(px(10.))
            .border_1()
            .border_color(if selected {
                content(0.22)
            } else {
                gpui::transparent_black()
            })
            .when(selected, |d| d.bg(content(0.08)))
            .cursor_pointer()
            .on_hover(move |hovered, _, cx| {
                if *hovered {
                    let _ = hover.update(cx, |this, cx| {
                        if let Some(open) = this.tab_switcher.as_mut()
                            && open.index != ix
                        {
                            open.index = ix;
                            cx.notify();
                        }
                    });
                }
            })
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(move |_, window, cx| {
                let _ = shell.update(cx, |this, cx| this.commit_switcher(Some(ix), window, cx));
            })
            .child(preview)
            .child(
                div()
                    .px_0p5()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .min_w_0()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(content(0.92))
                            .children(status)
                            .child(div().min_w_0().truncate().child(title)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .min_w_0()
                            .text_size(px(11.))
                            .text_color(content(0.45))
                            .child(div().flex_shrink_0().child(mascot))
                            .child(div().min_w_0().truncate().child(project_label)),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn tab_switcher_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.ensure_thumb_poll(window, cx);
        self.prune_switcher();
        let open = self.tab_switcher.as_ref()?;
        let viewport = window.viewport_size();
        let max_width = viewport.width.as_f32() - 48.;
        // Previews keep the window's shape, so each reads as the page shrunk.
        let aspect = (viewport.height.as_f32() / viewport.width.as_f32().max(1.)).clamp(0.45, 0.9);
        let cards: Vec<AnyElement> = open
            .entries
            .iter()
            .enumerate()
            .map(|(ix, entry)| self.switcher_card(ix, entry, ix == open.index, aspect, cx))
            .collect();
        let scroll = self.tab_switcher_scroll.clone();
        // Keep the highlighted card in view when there are more than fit.
        scroll.scroll_to_item(open.index);
        Some(
            crate::modal::window_overlay(
                window,
                div()
                    .size_full()
                    // Dims the app as the collapsed sidebar's peek does.
                    .bg(black(if is_light() { 0.18 } else { 0.4 }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("tab-switcher")
                            .debug_selector(|| "tab-switcher".into())
                            .track_focus(&open.focus)
                            .key_context("TabSwitcher")
                            .role(Role::ListBox)
                            .aria_label("Switch session")
                            .on_action(cx.listener(|this, _: &StepSwitcher, window, cx| {
                                this.step_switcher(1, window, cx)
                            }))
                            .on_action(cx.listener(|this, _: &StepSwitcherBack, window, cx| {
                                this.step_switcher(-1, window, cx)
                            }))
                            .on_action(cx.listener(|this, _: &CommitSwitcher, window, cx| {
                                this.commit_switcher(None, window, cx)
                            }))
                            .on_action(cx.listener(|this, _: &DismissSwitcher, window, cx| {
                                this.dismiss_switcher(window, cx)
                            }))
                            .on_mouse_down_out(
                                cx.listener(|this, _, window, cx| {
                                    this.dismiss_switcher(window, cx)
                                }),
                            )
                            .max_w(px(max_width))
                            .p_2()
                            .rounded(px(14.))
                            .bg(floating_fill())
                            .border_1()
                            .border_color(content(0.1))
                            .shadow(vec![BoxShadow {
                                color: black(0.35),
                                offset: gpui::point(px(0.), px(12.)),
                                blur_radius: px(40.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            // Padding sits outside the scroller, so a card
                            // scrolled to the edge keeps the same gap on
                            // every side.
                            .child(
                                div()
                                    .id("tab-switcher-strip")
                                    .flex()
                                    .gap_2()
                                    .overflow_x_scroll()
                                    .track_scroll(&scroll)
                                    .children(cards),
                            ),
                    ),
            )
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[test]
    fn recency_leads_and_the_rest_keep_tab_order() {
        assert_eq!(by_recency(&[1, 2, 3, 4], &[3, 9, 1]), vec![3, 1, 2, 4]);
        let mut recent = vec![1, 2, 3];
        touch(&mut recent, 3);
        assert_eq!(recent, vec![3, 1, 2]);
        assert_eq!(step(0, 3, -1), 2);
        assert_eq!(step(2, 3, 1), 0);
    }

    #[gpui::test]
    fn lists_sessions_across_projects_by_recency(cx: &mut TestAppContext) {
        let (window, first, second) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                shell.workspace.classic_layout = false;
                shell.open(first, window, cx);
                shell.open(second, window, cx);
                assert_eq!(
                    shell.switcher_entries(),
                    vec![Entry::Session(second), Entry::Session(first)]
                );
                shell.workspace.classic_layout = true;
                shell.step_switcher(1, window, cx);
                assert!(shell.tab_switcher.is_none());
            })
            .unwrap();
    }
}
