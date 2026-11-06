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
/// Lines a card's preview keeps from the end of its thread.
const PREVIEW_LINES: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq)]
enum LineKind {
    User,
    Agent,
    Activity,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PreviewLine {
    kind: LineKind,
    text: String,
}

/// The last messages of a thread: rows that continue a message (`first`
/// false) join the one before, reasoning and tool calls show as one-line
/// activity, and markup is flattened to a line of plain text.
fn preview_lines(rows: &[monocode_core::transcript::Row]) -> Vec<PreviewLine> {
    let mut lines: Vec<PreviewLine> = Vec::new();
    let start = rows.len().saturating_sub(PREVIEW_LINES * 8);
    let mut open = false;
    for row in &rows[start..] {
        if !row.first && open {
            if let Some(last) = lines.last_mut() {
                last.text.push_str(&row.text);
            }
            continue;
        }
        let kind = match row.role.as_str() {
            "user" => LineKind::User,
            "assistant" => LineKind::Agent,
            "tool" | "reasoning" => LineKind::Activity,
            _ => {
                open = false;
                continue;
            }
        };
        open = true;
        lines.push(PreviewLine {
            kind,
            text: row.text.clone(),
        });
    }
    let mut lines: Vec<PreviewLine> = lines
        .into_iter()
        .filter_map(|mut line| {
            let flat = line
                .text
                .split_whitespace()
                .map(|word| word.trim_matches(|c| matches!(c, '*' | '`' | '#' | '_')))
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if flat.is_empty() {
                return None;
            }
            line.text = match line.kind {
                LineKind::Activity => flat.chars().take(60).collect(),
                _ => flat.chars().take(220).collect(),
            };
            Some(line)
        })
        .collect();
    // Back-to-back tool calls fold to the latest one.
    lines.dedup_by(|b, a| {
        a.kind == LineKind::Activity && b.kind == LineKind::Activity && {
            std::mem::swap(a, b);
            true
        }
    });
    let keep = lines.len().saturating_sub(PREVIEW_LINES);
    lines.split_off(keep)
}

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
    /// Previews of chats that are not loaded, read once when the switcher opens.
    stored: HashMap<i64, Vec<PreviewLine>>,
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
        // Chats that were never opened this run are read from the store once.
        let stored: HashMap<i64, Vec<PreviewLine>> = match locked(&self.store) {
            Ok(store) => entries
                .iter()
                .filter_map(|entry| match entry {
                    Entry::Session(id) if !self.sessions.contains_key(id) => Some(*id),
                    _ => None,
                })
                .take(16)
                .filter_map(|id| Some((id, preview_lines(&store.load(id).ok()?.rows))))
                .collect(),
            Err(_) => HashMap::new(),
        };
        let focus = cx.focus_handle();
        let restore = window.focused(cx);
        window.focus(&focus, cx);
        self.tab_switcher = Some(Open {
            entries,
            index,
            focus,
            restore,
            stored,
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
            Some(Entry::Session(id)) => self.open(id, window, cx),
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

    /// The thread's last few lines, oldest first: the live transcript when the
    /// chat is loaded, otherwise the snapshot taken when the switcher opened.
    fn switcher_preview(&self, id: i64, cx: &App) -> Vec<PreviewLine> {
        match self.sessions.get(&id) {
            Some(view) => preview_lines(view.read(cx).transcript.read(cx).source_rows()),
            None => self
                .tab_switcher
                .as_ref()
                .and_then(|open| open.stored.get(&id).cloned())
                .unwrap_or_default(),
        }
    }

    /// The card's picture: the chat page drawn small, with the same parts in
    /// the same places — title bar, centred transcript column and composer —
    /// rather than a zoomed-in crop of the text.
    fn mini_page(
        &self,
        entry: &Entry,
        project_label: &str,
        lines: Vec<PreviewLine>,
        aspect: f32,
    ) -> AnyElement {
        const W: f32 = PAGE_WIDTH;
        const TEXT: f32 = 5.;
        const LEADING: f32 = 7.;
        let height = (W * aspect).round();
        let column = W * 0.68;
        let composer = |grow: bool| {
            div()
                .w(px(column))
                .h(px(if grow { 16. } else { 12. }))
                .flex_shrink_0()
                .rounded(px(4.))
                .bg(content(0.07))
                .border_1()
                .border_color(content(0.1))
                .px(px(4.))
                .flex()
                .items_center()
                .child(
                    div()
                        .w(px(column * 0.35))
                        .h(px(2.))
                        .rounded(px(1.))
                        .bg(content(0.2)),
                )
        };
        let title_bar = div()
            .h(px(9.))
            .flex_shrink_0()
            .px(px(4.))
            .flex()
            .items_center()
            .gap(px(3.))
            .border_b_1()
            .border_color(content(0.06))
            .child(
                div()
                    .w(px(W * 0.22))
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(content(0.22)),
            );
        let body = if lines.is_empty() {
            let heading = match entry {
                Entry::Project(_) => format!("Open {project_label}"),
                Entry::Session(_) => format!("What should we work on in {project_label}?"),
            };
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .child(
                    div()
                        .w(px(column))
                        .text_size(px(6.5))
                        .line_height(px(8.))
                        .text_color(content(0.85))
                        .truncate()
                        .child(heading),
                )
                .child(composer(true))
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .pb(px(4.))
                .child(
                    div()
                        .w(px(column))
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .justify_end()
                        .gap(px(3.))
                        .children(lines.into_iter().map(|line| {
                            match line.kind {
                                LineKind::User => div()
                                    .ml_auto()
                                    .max_w(px(column * 0.75))
                                    .rounded(px(2.5))
                                    .bg(user_bubble_fill())
                                    .px(px(2.5))
                                    .py(px(1.))
                                    .text_size(px(TEXT))
                                    .line_height(px(LEADING))
                                    .text_color(content(0.8))
                                    .line_clamp(2)
                                    .child(line.text)
                                    .into_any_element(),
                                LineKind::Agent => div()
                                    .text_size(px(TEXT))
                                    .line_height(px(LEADING))
                                    .text_color(content(0.7))
                                    .line_clamp(4)
                                    .child(line.text)
                                    .into_any_element(),
                                LineKind::Activity => div()
                                    .text_size(px(TEXT))
                                    .line_height(px(LEADING))
                                    .text_color(content(0.38))
                                    .truncate()
                                    .child(line.text)
                                    .into_any_element(),
                            }
                        })),
                )
                .child(div().h(px(4.)).flex_shrink_0())
                .child(composer(false))
        };
        div()
            .w(px(W))
            .h(px(height))
            .flex_shrink_0()
            .overflow_hidden()
            .rounded(px(6.))
            .bg(pane_fill())
            .border_1()
            .border_color(content(0.08))
            .flex()
            .flex_col()
            .child(title_bar)
            .child(body)
            .into_any_element()
    }

    fn switcher_card(
        &self,
        ix: usize,
        entry: &Entry,
        selected: bool,
        aspect: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (project, title, provider, status, lines) = match entry {
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
                    self.switcher_preview(*id, cx),
                )
            }
            Entry::Project(path) => (
                path.clone(),
                self.project_label(path),
                None,
                None,
                Vec::new(),
            ),
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
            None => self.mini_page(entry, &project_label, lines, aspect),
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
    fn preview_joins_streamed_rows_and_folds_tool_runs() {
        use monocode_core::transcript::Row;
        let row = |role: &str, text: &str, first: bool| Row {
            role: role.into(),
            text: text.into(),
            first,
            sent_at: None,
        };
        let lines = preview_lines(&[
            row("user", "Fix the **build**", true),
            row("tool", "cargo check", true),
            row("tool", "cargo test", true),
            row("assistant", "All ", true),
            row("assistant", "green.", false),
        ]);
        assert_eq!(
            lines,
            vec![
                PreviewLine {
                    kind: LineKind::User,
                    text: "Fix the build".into()
                },
                PreviewLine {
                    kind: LineKind::Activity,
                    text: "cargo test".into()
                },
                PreviewLine {
                    kind: LineKind::Agent,
                    text: "All green.".into()
                },
            ]
        );
    }

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
