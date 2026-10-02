//! The markdown view entity that the transcript and notes embed.
//!
//! Port of the `AgentMarkdown` component in
//! `src/features/sessions/ui/AgentMarkdown.tsx`, with the paced reveal and
//! word fade from `wordFade.tsx`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, Context, DispatchPhase, FocusHandle, Focusable, HitboxBehavior, InteractiveElement,
    IntoElement, KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Render, SharedString, Styled, Task, WeakEntity, Window, actions, canvas, div,
};

use crate::fade::{FadeGate, Pacer, RevealTimeline};
use crate::highlight;
use crate::parse::{Document, IncrementalParser};
use crate::prepare::{PreparedBlock, prepare_block};
use crate::render::{CodeState, Frame, ImageResolver, LayoutCache, Registry, render_blocks};
use crate::selection::{self, ElementKey, Point, Selection};
use crate::style::MarkdownStyle;

actions!(markdown, [Copy, SelectAll]);

/// Key context of [`MarkdownView`].
pub const KEY_CONTEXT: &str = "MarkdownView";

/// Lines of code the UI thread may highlight per frame; more goes to a
/// background job.
const HIGHLIGHT_LINES_PER_FRAME: usize = 48;

/// How long the copy button shows its check mark.
const COPIED_FEEDBACK: Duration = Duration::from_millis(1500);

/// Bind copy and select-all for focused markdown views.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-c", Copy, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-c", Copy, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-a", SelectAll, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-a", SelectAll, Some(KEY_CONTEXT)),
    ]);
}

/// A click on a link in rendered markdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkClick {
    /// The destination as written, such as `https://…`, `src/main.rs:12`, or
    /// `#heading`.
    pub url: SharedString,
}

/// One painted text element (see [`MarkdownView::rendered_text`]).
#[derive(Clone)]
pub struct RenderedText {
    /// The text as laid out. Inline code has a no-break space on each side.
    pub text: SharedString,
    pub bounds: gpui::Bounds<gpui::Pixels>,
    /// The laid-out text, for mapping byte offsets to window positions.
    pub layout: gpui::TextLayout,
}

/// Handles link clicks. Without one, web and mail links open in the system
/// browser and others do nothing.
pub type LinkHandler = Rc<dyn Fn(&LinkClick, &mut Window, &mut App)>;

/// Renders one markdown message. Feed it text with [`Self::set_text`] or
/// [`Self::push_str`]; mark it streaming while the agent writes so new words
/// are paced and fade in.
pub struct MarkdownView {
    style: Rc<MarkdownStyle>,
    reasoning: bool,
    received: String,
    streaming: bool,
    pacer: Pacer,
    gate: FadeGate,
    timeline: RevealTimeline,
    parser: IncrementalParser,
    /// Bytes of `received` that are parsed and shown.
    shown: usize,
    /// Whether `document` is the mended display tree.
    showing_display_tree: bool,
    document: Document,
    prepared: Vec<Rc<PreparedBlock>>,
    code: CodeState,
    registry: Rc<RefCell<Registry>>,
    layout: Rc<RefCell<LayoutCache>>,
    selection: Selection,
    hovered_link: Option<(ElementKey, usize)>,
    pressed_link: Option<(ElementKey, usize)>,
    focus_handle: FocusHandle,
    on_link: Option<LinkHandler>,
    images: Option<ImageResolver>,
    reduced_motion: bool,
    loading_syntaxes: bool,
    copied_task: Option<Task<()>>,
}

impl MarkdownView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let style = MarkdownStyle::default();
        let fade = Duration::from_secs_f32(style.word_fade_ms / 1000.);
        Self {
            style: Rc::new(style),
            reasoning: false,
            received: String::new(),
            streaming: false,
            pacer: Pacer::new(0, false),
            gate: FadeGate::default(),
            timeline: RevealTimeline::new(0, fade),
            parser: IncrementalParser::new(),
            shown: 0,
            showing_display_tree: false,
            document: Document::default(),
            prepared: Vec::new(),
            code: CodeState::default(),
            registry: Rc::new(RefCell::new(Registry::default())),
            layout: Rc::new(RefCell::new(LayoutCache::default())),
            selection: Selection::default(),
            hovered_link: None,
            pressed_link: None,
            focus_handle: cx.focus_handle(),
            on_link: None,
            images: None,
            reduced_motion: false,
            loading_syntaxes: false,
            copied_task: None,
        }
    }

    /// A view showing `text` at once, without pacing or fades.
    pub fn with_text(text: impl Into<String>, cx: &mut Context<Self>) -> Self {
        let mut view = Self::new(cx);
        view.received = text.into();
        view.pacer = Pacer::new(view.received.len(), false);
        view.timeline.reset(view.received.len());
        view
    }

    pub fn style(&self) -> &MarkdownStyle {
        &self.style
    }

    pub fn set_style(&mut self, style: MarkdownStyle, cx: &mut Context<Self>) {
        if *self.style == style {
            return;
        }
        self.timeline
            .set_fade(Duration::from_secs_f32(style.word_fade_ms / 1000.));
        self.style = Rc::new(style);
        self.prepared.clear();
        self.code.clear();
        cx.notify();
    }

    /// Use the dimmer reasoning text colors (`.agent-reasoning`).
    pub fn set_reasoning(&mut self, reasoning: bool, cx: &mut Context<Self>) {
        if self.reasoning != reasoning {
            self.reasoning = reasoning;
            self.prepared.clear();
            cx.notify();
        }
    }

    /// The full text received so far, including any part not yet revealed.
    pub fn text(&self) -> &str {
        &self.received
    }

    /// Replace the text. Text that extends the current text streams in as an
    /// append; anything else replaces the message.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if text == self.received {
            return;
        }
        if !text.starts_with(self.received.as_str()) {
            self.selection.clear();
            self.hovered_link = None;
        }
        self.received.clear();
        self.received.push_str(text);
        cx.notify();
    }

    /// Append streamed text.
    pub fn push_str(&mut self, delta: &str, cx: &mut Context<Self>) {
        if delta.is_empty() {
            return;
        }
        self.received.push_str(delta);
        cx.notify();
    }

    /// Whether the agent is still writing this message. Streaming text is
    /// paced word by word and fades in.
    pub fn set_streaming(&mut self, streaming: bool, cx: &mut Context<Self>) {
        if self.streaming != streaming {
            self.streaming = streaming;
            cx.notify();
        }
    }

    pub fn is_streaming(&self) -> bool {
        self.streaming
    }

    /// Whether the reveal or a fade is still running.
    pub fn is_animating(&self) -> bool {
        self.pacer.is_revealing(self.received.len()) || self.timeline.is_fading(Instant::now())
    }

    /// Turn off fades for this view, on top of [`App::reduce_motion`].
    pub fn set_reduced_motion(&mut self, reduced: bool, cx: &mut Context<Self>) {
        if self.reduced_motion != reduced {
            self.reduced_motion = reduced;
            cx.notify();
        }
    }

    /// Handle link clicks.
    pub fn on_link_click(&mut self, handler: impl Fn(&LinkClick, &mut Window, &mut App) + 'static) {
        self.on_link = Some(Rc::new(handler));
    }

    /// Decide how image URLs load. Return `None` to show the alt text.
    pub fn set_image_resolver(
        &mut self,
        resolver: impl Fn(&str) -> Option<gpui::ImageSource> + 'static,
        cx: &mut Context<Self>,
    ) {
        self.images = Some(Rc::new(resolver));
        cx.notify();
    }

    /// The selected text, as copy would write it.
    pub fn selected_text(&self) -> Option<String> {
        self.selection.text(&self.registry.borrow().elements)
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selection.clear() {
            cx.notify();
        }
    }

    /// The document as currently shown.
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// The text elements laid out in the last frame, in document order, with
    /// their window bounds. Blocks far outside the visible area are skipped. Hosts can use it to hit-test or scroll to text;
    /// tests use it to aim pointer events.
    pub fn rendered_text(&self) -> Vec<RenderedText> {
        let registry = self.registry.borrow();
        registry
            .elements
            .iter()
            .zip(&registry.layouts)
            .filter_map(|(element, layout)| {
                let layout = layout.as_ref()?;
                Some(RenderedText {
                    text: element.text.clone(),
                    bounds: layout.bounds(),
                    layout: layout.clone(),
                })
            })
            .collect()
    }

    pub(crate) fn mark_copied(&mut self, key: ElementKey, cx: &mut Context<Self>) {
        self.code.copied = Some(key);
        self.copied_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FEEDBACK).await;
            this.update(cx, |this, cx| {
                if this.code.copied == Some(key) {
                    this.code.copied = None;
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        match self.selected_text() {
            Some(text) => cx.write_to_clipboard(gpui::ClipboardItem::new_string(text)),
            None => cx.propagate(),
        }
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selection.select_all();
        cx.notify();
    }

    /// Advance the reveal and reparse what it shows.
    fn sync_document(&mut self, now: Instant) {
        let shown = self.pacer.advance(&self.received, self.streaming, now);
        let settled = !self.streaming && shown == self.received.len();
        let source_changed = self.parser.source() != &self.received[..shown];
        if source_changed {
            self.parser.set_text(&self.received[..shown]);
        }
        if source_changed || self.showing_display_tree == settled {
            // While text streams, show the mended tree; once it settles, the
            // tree as written.
            self.document = if settled {
                self.parser.tree().clone()
            } else {
                self.parser.display_tree()
            };
            self.showing_display_tree = !settled;
        }
        self.shown = shown;
    }

    fn sync_prepared(&mut self) {
        let blocks = &self.document.blocks;
        self.prepared.truncate(blocks.len());
        for (ix, top) in blocks.iter().enumerate() {
            let fresh = self
                .prepared
                .get(ix)
                .is_none_or(|prepared| !Arc::ptr_eq(&prepared.source, top));
            if fresh {
                let prepared = Rc::new(prepare_block(top, ix, &self.style, self.reasoning));
                if ix < self.prepared.len() {
                    self.prepared[ix] = prepared;
                } else {
                    self.prepared.push(prepared);
                }
            }
        }
    }

    fn load_syntaxes(&mut self, cx: &mut Context<Self>) {
        if self.loading_syntaxes {
            return;
        }
        self.loading_syntaxes = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async {
                    highlight::syntaxes_blocking();
                })
                .await;
            this.update(cx, |this, cx| {
                this.loading_syntaxes = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Focusable for MarkdownView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MarkdownView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let reduced = self.reduced_motion || cx.reduce_motion();
        self.sync_document(now);

        let fade = Duration::from_secs_f32(self.style.word_fade_ms / 1000.);
        let active = self.streaming || self.pacer.is_revealing(self.received.len());
        let fading = !reduced && self.gate.fading(active, now, fade);
        if fading {
            self.timeline.record(self.shown, now);
            self.timeline.prune(now);
        } else {
            self.timeline.reset(self.shown);
        }

        let parsed_at = Instant::now();
        self.sync_prepared();
        let prepared_at = Instant::now();
        self.registry
            .borrow_mut()
            .set_selection(self.selection.clone());

        self.code.budget = HIGHLIGHT_LINES_PER_FRAME;
        self.code.needs_syntaxes = false;
        let style = self.style.clone();
        let blocks = {
            let mut frame = Frame {
                style: &style,
                now,
                timeline: (fading && self.timeline.is_fading(now)).then_some(&self.timeline),
                registry: &self.registry,
                hovered_link: self.hovered_link,
                code: &mut self.code,
                view: cx.weak_entity(),
                images: self.images.as_ref(),
                text_system: window.text_system().clone(),
            };
            render_blocks(&self.prepared, &self.layout, &mut frame)
        };
        self.code.retain_blocks(self.prepared.len());
        for job in std::mem::take(&mut self.code.jobs) {
            let job = job.into_send();
            cx.spawn(async move |this, cx| {
                let job = cx
                    .background_executor()
                    .spawn(async move { job.run() })
                    .await
                    .into_inner();
                this.update(cx, |this, cx| {
                    this.code.finish(job);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        if trace_enabled() {
            eprintln!(
                "monocode-markdown: parse {:?}, prepare {:?}, build {:?}, {} blocks",
                parsed_at - now,
                prepared_at - parsed_at,
                prepared_at.elapsed(),
                self.prepared.len()
            );
        }
        if self.code.needs_syntaxes {
            self.load_syntaxes(cx);
        }

        if self.pacer.needs_frame() || fading {
            window.request_animation_frame();
        }

        let mouse = mouse_layer(
            self.registry.clone(),
            self.layout.clone(),
            cx.weak_entity(),
            self.focus_handle.clone(),
        );

        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::select_all))
            .relative()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .font_family(style.font_family.clone())
            .text_size(style.text_size)
            .line_height(style.line_height)
            .text_color(style.body_text)
            .child(mouse)
            .children(blocks)
    }
}

/// The first child of the view. Its prepaint clears the frame registry before
/// the text elements register, and its paint installs the mouse listeners.
/// Painting first means the listeners run after every child's, so a button
/// that stops propagation keeps the click.
fn mouse_layer(
    registry: Rc<RefCell<Registry>>,
    layout: Rc<RefCell<LayoutCache>>,
    view: WeakEntity<MarkdownView>,
    focus: FocusHandle,
) -> impl IntoElement {
    let prepaint_registry = registry.clone();
    canvas(
        move |bounds, window, _| {
            prepaint_registry.borrow_mut().clear();
            // Record where the view is and which part of it shows, for the
            // next frame's choice of blocks to lay out.
            let mask = window.content_mask().bounds;
            let top = mask.top().max(bounds.top());
            let bottom = mask.bottom().min(bounds.bottom()).max(top);
            let mut layout = layout.borrow_mut();
            layout.view = Some(bounds);
            layout.visible = Some(top - bounds.top()..bottom - bounds.top());
            window.insert_hitbox(bounds, HitboxBehavior::Normal)
        },
        move |bounds, hitbox, window, _| {
            // A press anywhere else clears this message's selection.
            {
                let view = view.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && !bounds.contains(&event.position) {
                        view.update(cx, |this, cx| {
                            this.pressed_link = None;
                            if this.selection.clear() {
                                cx.notify();
                            }
                        })
                        .ok();
                    }
                });
            }
            {
                let view = view.clone();
                let registry = registry.clone();
                let hitbox = hitbox.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble
                        || event.button != MouseButton::Left
                        || !hitbox.is_hovered(window)
                    {
                        return;
                    }
                    let registry = registry.borrow();
                    let Some((ix, offset)) = registry.hit(event.position) else {
                        return;
                    };
                    let element = registry.elements[ix].clone();
                    let link = registry
                        .link_at(event.position)
                        .map(|(key, link_ix, _)| (key, link_ix));
                    drop(registry);
                    focus.focus(window, cx);
                    view.update(cx, |this, cx| {
                        match event.click_count {
                            0 | 1 => {
                                this.selection.begin(Point {
                                    key: element.key,
                                    offset,
                                });
                                this.pressed_link = link;
                            }
                            2 => {
                                let range = selection::word_range(&element.text, offset);
                                this.selection.begin_range(element.key, range);
                                this.pressed_link = None;
                            }
                            _ => {
                                let range = if element.line_mode {
                                    selection::line_range(&element.text, offset)
                                } else {
                                    0..element.text.len()
                                };
                                this.selection.begin_range(element.key, range);
                                this.pressed_link = None;
                            }
                        }
                        cx.notify();
                    })
                    .ok();
                });
            }
            {
                let view = view.clone();
                let registry = registry.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let Some(this) = view.upgrade() else {
                        return;
                    };
                    let dragging = this.read(cx).selection.is_dragging();
                    let registry = registry.borrow();
                    if dragging && event.pressed_button == Some(MouseButton::Left) {
                        let Some((ix, offset)) = registry.hit(event.position) else {
                            return;
                        };
                        let point = Point {
                            key: registry.elements[ix].key,
                            offset,
                        };
                        let order = registry.keys();
                        drop(registry);
                        this.update(cx, |this, cx| {
                            if this.selection.drag_to(point, &order) {
                                cx.notify();
                            }
                        });
                        return;
                    }
                    let hovered = bounds
                        .contains(&event.position)
                        .then(|| registry.link_at(event.position))
                        .flatten()
                        .map(|(key, link_ix, _)| (key, link_ix));
                    drop(registry);
                    if this.read(cx).hovered_link != hovered {
                        this.update(cx, |this, cx| {
                            this.hovered_link = hovered;
                            cx.notify();
                        });
                    }
                });
            }
            {
                let view = view.clone();
                let registry = registry.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                        return;
                    }
                    let Some(this) = view.upgrade() else {
                        return;
                    };
                    if !this.read(cx).selection.is_dragging() {
                        return;
                    }
                    let released_on = registry
                        .borrow()
                        .link_at(event.position)
                        .map(|(key, ix, url)| ((key, ix), url));
                    let (click, handler) = this.update(cx, |this, cx| {
                        let has_selection = this.selection.end();
                        let pressed = this.pressed_link.take();
                        cx.notify();
                        let click = match (pressed, released_on) {
                            (Some(pressed), Some((released, url)))
                                if !has_selection && pressed == released =>
                            {
                                Some(LinkClick {
                                    url: url.to_string().into(),
                                })
                            }
                            _ => None,
                        };
                        (click, this.on_link.clone())
                    });
                    if let Some(click) = click {
                        match handler {
                            Some(handler) => handler(&click, window, cx),
                            None => open_web_link(&click.url, cx),
                        }
                    }
                });
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// `MONOCODE_MARKDOWN_TRACE=1` prints per-frame timings of the view's own
/// work (parse, prepare, element building) to stderr.
fn trace_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("MONOCODE_MARKDOWN_TRACE").is_some())
}

/// Default link behavior: web and mail links open in the system browser
/// (AgentMarkdown opens `https?:` links with the opener plugin).
fn open_web_link(url: &str, cx: &mut App) {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
    {
        cx.open_url(url);
    }
}
