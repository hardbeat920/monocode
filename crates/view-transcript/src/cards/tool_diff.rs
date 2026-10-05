//! Port of src/features/sessions/ui/ToolDiffPreview.tsx and the `popover`
//! variant of src/features/files/ui/FilePreview.tsx: a quiet file chip that
//! reveals the tool's own edit, independent of git.
//!
//! [`ToolDiffPopover`] is the card itself: what the edit was, the file, its
//! counts, and the changed lines in a scrolling box. [`ToolDiffPreview`] is
//! the whole control: a trigger that opens the card after a 300ms hover or
//! on focus, keeps it open while the pointer crosses the gap, and closes it
//! 180ms after the pointer leaves both.

use std::rc::Rc;
use std::sync::LazyLock;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, EventEmitter, FocusHandle, Focusable,
    FontWeight, HighlightStyle, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, StyledText, Subscription, Task, Window, deferred, div, px,
};
use monocode_core::block::{ToolPreview, ToolPreviewLine, ToolPreviewLineKind};
use monocode_core::paths::display_path;
use monocode_core::reducer::MAX_PREVIEW_LINES;
use monocode_core::transcript::ToolCallState;
use monocode_core::transcript::paths::resolve_workspace_path;
use monocode_ui::styled::{UiStyled as _, format_integer, glass_backdrop};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, file_type_icon, icon, u};
use regex::Regex;

use super::style;
use super::util::BoundsMap;

/// The open delay after the pointer enters the chip.
pub const OPEN_DELAY: Duration = Duration::from_millis(300);
/// The close delay after the pointer leaves the chip and the card.
pub const CLOSE_DELAY: Duration = Duration::from_millis(180);

/// The card's heading for a call in `status`.
pub fn preview_description(preview: &ToolPreview, status: ToolCallState) -> &'static str {
    match status {
        ToolCallState::Pending => "Proposed changes",
        ToolCallState::Rejected => "Attempted changes \u{b7} tool did not complete",
        ToolCallState::Accepted if preview.content_only == Some(true) => {
            "Written content \u{b7} previous contents unavailable"
        }
        ToolCallState::Accepted => "Change preview",
    }
}

/// The trigger's accessible name.
pub fn trigger_label(preview: &ToolPreview, label: &str, opens: bool) -> String {
    if opens {
        format!("Open {label}")
    } else {
        format!(
            "Preview {}: {label}",
            if preview.content_only == Some(true) {
                "written content"
            } else {
                "changes"
            }
        )
    }
}

const KEYWORDS: &[&str] = &[
    "import",
    "export",
    "from",
    "type",
    "interface",
    "const",
    "let",
    "var",
    "function",
    "return",
    "if",
    "else",
    "for",
    "while",
    "switch",
    "case",
    "class",
    "struct",
    "enum",
    "extends",
    "implements",
    "new",
    "async",
    "await",
    "try",
    "catch",
    "throw",
    "true",
    "false",
    "null",
    "undefined",
    "this",
    "in",
    "of",
    "as",
    "is",
    "void",
    "public",
    "private",
    "protected",
    "static",
    "default",
    "package",
    "def",
    "func",
];

static WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Za-z_][A-Za-z0-9_]*\b").expect("word"));

/// What a run of preview text is, for coloring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Plain,
    Comment,
    Keyword,
    Type,
}

/// `highlight` in FilePreview.tsx as runs: a comment line is one run;
/// otherwise keywords and capitalized names stand out from plain text.
pub fn tokens(text: &str) -> Vec<(std::ops::Range<usize>, Token)> {
    let trimmed = text.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with('#') {
        return vec![(0..text.len(), Token::Comment)];
    }
    let mut runs = Vec::new();
    let mut at = 0;
    for word in WORD.find_iter(text) {
        let token = word.as_str();
        let kind = if KEYWORDS.contains(&token) {
            Token::Keyword
        } else if token.starts_with(|c: char| c.is_ascii_uppercase()) {
            Token::Type
        } else {
            continue;
        };
        if word.start() > at {
            runs.push((at..word.start(), Token::Plain));
        }
        runs.push((word.range(), kind));
        at = word.end();
    }
    if at < text.len() {
        runs.push((at..text.len(), Token::Plain));
    }
    runs
}

/// `highlight`: comments at 45% ink, keywords teal, type names amber, the
/// rest at 80%. Context lines fade to 70%.
///
/// GPUI blends a highlight's color over the text color, so a translucent
/// highlight cannot dim text. The plain ink comes back as the color to set
/// on the text's parent, and only the tokens are highlights.
pub fn highlight_line(text: &str, dimmed: bool, theme: &Theme) -> (Hsla, StyledText) {
    let fade = |color: Hsla| {
        if dimmed {
            Hsla {
                a: color.a * 0.7,
                ..color
            }
        } else {
            color
        }
    };
    let runs = tokens(text);
    let base = if runs
        .first()
        .is_some_and(|(_, token)| *token == Token::Comment)
    {
        fade(theme.content(0.45))
    } else {
        fade(theme.content(0.8))
    };
    let highlights = runs
        .into_iter()
        .filter_map(|(range, token)| {
            let color = match token {
                Token::Plain | Token::Comment => return None,
                Token::Keyword => style::teal_300(),
                Token::Type => style::amber_200_90(),
            };
            Some((
                range,
                HighlightStyle {
                    color: Some(fade(color)),
                    ..Default::default()
                },
            ))
        })
        .collect::<Vec<_>>();
    (
        base,
        StyledText::new(SharedString::from(text.to_string())).with_highlights(highlights),
    )
}

/// `PreviewLine` in the scrolling popover: no truncation, a gutter with the
/// line number and the +/− mark.
fn preview_line(line: &ToolPreviewLine, theme: &Theme) -> AnyElement {
    let (tint, bar, mark, mark_color) = match line.kind {
        ToolPreviewLineKind::Add => (
            Some(style::teal_800_20()),
            style::teal_400(),
            "+",
            style::teal_400(),
        ),
        ToolPreviewLineKind::Del => (
            Some(style::rose_800_20()),
            style::rose_400(),
            "\u{2212}",
            style::rose_400(),
        ),
        ToolPreviewLineKind::Context => (
            None,
            gpui::transparent_black(),
            " ",
            gpui::transparent_black(),
        ),
    };
    let mono = theme.fonts.mono.clone();
    let (ink, text) = highlight_line(&line.text, line.kind == ToolPreviewLineKind::Context, theme);
    div()
        .relative()
        .flex()
        .items_center()
        .min_w_full()
        .when_some(tint, |el, tint| el.bg(tint))
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(u(2.))
                .bg(bar),
        )
        .child(
            div()
                .w(u(28.))
                .flex_none()
                .pr(u(4.))
                .flex()
                .justify_end()
                .font_family(mono.clone())
                .text_px(10.)
                .text_color(theme.content(0.35))
                .child(
                    line.number
                        .map(|number| number.to_string())
                        .unwrap_or_default(),
                ),
        )
        .child(
            div()
                .w(u(12.))
                .flex_none()
                .flex()
                .justify_center()
                .font_family(mono.clone())
                .text_px(10.)
                .font_weight(FontWeight::BOLD)
                .text_color(mark_color)
                .child(mark),
        )
        .child(
            div()
                .flex_none()
                .pr(u(8.))
                .whitespace_nowrap()
                .font_family(mono)
                .text_px(11.)
                .line_height(u(18.))
                .text_color(ink)
                .child(text),
        )
        .into_any_element()
}

type OpenFile = Rc<dyn Fn(&str, &mut Window, &mut App)>;
type Close = Rc<dyn Fn(&mut Window, &mut App)>;

/// The card: a heading with a close button over `FilePreview variant="popover"`.
pub struct ToolDiffPopover {
    preview: ToolPreview,
    label: String,
    status: ToolCallState,
    cwd: Option<String>,
    scroll: ScrollHandle,
    focus: FocusHandle,
    on_open_file: Option<OpenFile>,
    on_close: Option<Close>,
}

impl ToolDiffPopover {
    pub fn new(
        preview: ToolPreview,
        label: impl Into<String>,
        status: ToolCallState,
        cwd: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            preview,
            label: label.into(),
            status,
            cwd,
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            on_open_file: None,
            on_close: None,
        }
    }

    /// The file name opens the file. Gets the resolved path.
    pub fn on_open_file(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_open_file = Some(Rc::new(handler));
        self
    }

    /// Shows the close button.
    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }

    pub fn description(&self) -> &'static str {
        preview_description(&self.preview, self.status)
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// The lines the card shows (`.slice(0, MAX_PREVIEW_LINES)`).
    pub fn lines(&self) -> Vec<&ToolPreviewLine> {
        self.preview
            .lines
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .take(MAX_PREVIEW_LINES)
            .collect()
    }

    /// The path the file name opens, resolved against the workspace.
    pub fn file_path(&self) -> Option<String> {
        let path = self
            .preview
            .path
            .as_deref()
            .filter(|path| !path.is_empty())?;
        Some(resolve_workspace_path(path, self.cwd.as_deref()).unwrap_or_else(|| path.to_string()))
    }

    /// Open the file as the name button does.
    pub fn open_file(&self, window: &mut Window, cx: &mut App) {
        if let (Some(path), Some(open)) = (self.file_path(), self.on_open_file.clone()) {
            open(&path, window, cx);
        }
    }
}

impl Focusable for ToolDiffPopover {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ToolDiffPopover {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let preview = &self.preview;
        let path = preview.path.as_deref().filter(|path| !path.is_empty());
        let file_name = preview
            .file_name
            .clone()
            .filter(|name| !name.is_empty())
            .or_else(|| {
                path.and_then(|path| {
                    path.split(['/', '\\'])
                        .rfind(|part| !part.is_empty())
                        .map(str::to_string)
                })
            });
        let lines = self.lines();
        let show_diff = preview.content_only == Some(true)
            || lines.iter().any(|line| {
                matches!(
                    line.kind,
                    ToolPreviewLineKind::Add | ToolPreviewLineKind::Del
                )
            });
        let added = preview.additions.unwrap_or(0);
        let deleted = preview.deletions.unwrap_or(0);
        let label = match path {
            Some(path) => display_path(path, self.cwd.as_deref()),
            None => file_name
                .clone()
                .or_else(|| preview.title.clone())
                .unwrap_or_else(|| "File".into()),
        };
        let link = style::sky_300();
        let opens = self.file_path().is_some() && self.on_open_file.is_some();
        let name = div()
            .id("tool-diff-file")
            .flex_1()
            .min_w_0()
            .truncate()
            .font_family(theme.fonts.mono.clone())
            .text_px(12.)
            .line_height(u(16.))
            .medium()
            .text_color(theme.content(0.85))
            .when_some(path.map(str::to_string), |el, path| {
                el.tooltip(tooltip(path))
            })
            .when(opens, |el| {
                el.cursor_pointer()
                    .hover(move |s| s.text_color(link).underline())
                    .on_click(cx.listener(|this, _, window, cx| this.open_file(window, cx)))
            })
            .child(label);
        let counts: AnyElement = if added > 0 || deleted > 0 {
            div()
                .flex()
                .flex_none()
                .gap(u(4.))
                .font_family(theme.fonts.sans.clone())
                .text_px(11.)
                .semibold()
                .tabular()
                .when(added > 0, |el| {
                    el.child(
                        div()
                            .text_color(theme.colors.success)
                            .child(format!("+{}", format_integer(added))),
                    )
                })
                .when(deleted > 0, |el| {
                    el.child(
                        div()
                            .text_color(theme.colors.danger)
                            .child(format!("-{}", format_integer(deleted))),
                    )
                })
                .into_any_element()
        } else {
            match self.status {
                ToolCallState::Rejected => icon(IconName::X)
                    .size(u(14.))
                    .text_color(theme.colors.danger)
                    .into_any_element(),
                ToolCallState::Pending => icon(IconName::CircleDashed)
                    .size(u(14.))
                    .text_color(theme.content(0.4))
                    .into_any_element(),
                ToolCallState::Accepted => div().into_any_element(),
            }
        };
        let close = self.on_close.clone().map(|close| {
            div()
                .id("tool-diff-close")
                .flex_none()
                .rounded(u(4.))
                .p(u(2.))
                .cursor_pointer()
                .hover(|s| s.bg(theme.content(0.08)))
                .tooltip(tooltip("Close preview"))
                .on_click(move |_, window, cx| close(window, cx))
                .child(
                    icon(IconName::X)
                        .size(u(12.))
                        .text_color(theme.content(0.5)),
                )
        });
        let header = div()
            .flex()
            .items_center()
            .gap(u(8.))
            .border_b(px(1.))
            .border_color(theme.colors.stroke)
            .px(u(10.))
            .py(u(6.))
            .font_family(theme.fonts.sans.clone())
            .text_px(11.)
            .text_color(theme.content(0.5))
            .child(div().flex_1().min_w_0().child(self.description()))
            .children(close);
        let file_row = div()
            .flex()
            .items_center()
            .gap(u(8.))
            .px(u(10.))
            .py(u(8.))
            .child(file_type_icon(file_name.unwrap_or_else(|| "file".into())))
            .child(name)
            .child(counts);
        let mut body = div()
            .id("tool-diff-lines")
            .max_h(u(180.))
            .overflow_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col();
        if preview.content_only == Some(true) && lines.is_empty() {
            body = body.child(
                div()
                    .px(u(12.))
                    .py(u(8.))
                    .font_family(theme.fonts.mono.clone())
                    .text_px(12.)
                    .line_height(u(16.))
                    .text_color(theme.content(0.5))
                    .child("Empty file"),
            );
        }
        for line in &lines {
            body = body.child(preview_line(line, &theme));
        }
        div()
            .id("tool-diff-popover")
            .track_focus(&self.focus)
            .relative()
            .w(u(460.))
            .max_h(u(280.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(theme.colors.popover_border)
            .shadow_xl()
            // `Popover`'s glass: a blurred backdrop under a tint.
            .child(glass_backdrop(
                theme.radius.xl,
                24.,
                theme.colors.popover_backdrop,
            ))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape"
                    && let Some(close) = this.on_close.clone()
                {
                    cx.stop_propagation();
                    close(window, cx);
                }
            }))
            .child(header)
            .child(div().min_w_0().child(file_row).when(show_diff, |el| {
                el.child(div().h(px(1.)).bg(theme.content(0.1))).child(body)
            }))
    }
}

/// What the reader did with the chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolDiffEvent {
    /// `onOpen`: the chip was clicked.
    Open,
    /// `onOpenFile(path)` from the card.
    OpenFile { path: String },
}

type Trigger = Box<dyn Fn(&mut Window, &mut App) -> AnyElement>;

/// `<ToolDiffPreview preview label status cwd onOpen onOpenFile>{children}`.
pub struct ToolDiffPreview {
    popover: gpui::Entity<ToolDiffPopover>,
    label: String,
    preview: ToolPreview,
    opens: bool,
    open: bool,
    trigger_hovered: bool,
    surface_hovered: bool,
    trigger_focused: bool,
    surface_focused: bool,
    /// Focus going back to the chip after Escape must not reopen the card.
    refocusing: bool,
    trigger_focus: FocusHandle,
    surface_focus: FocusHandle,
    trigger: Trigger,
    bounds: BoundsMap,
    timer: Option<Task<()>>,
    _focus: Vec<Subscription>,
}

impl EventEmitter<ToolDiffEvent> for ToolDiffPreview {}

impl ToolDiffPreview {
    /// `trigger` draws the chip's content (the React children). `opens`:
    /// a click on the chip reports [`ToolDiffEvent::Open`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        preview: ToolPreview,
        label: impl Into<String>,
        status: ToolCallState,
        cwd: Option<String>,
        opens: bool,
        trigger: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let label = label.into();
        let weak = cx.entity().downgrade();
        let close_weak = weak.clone();
        let popover = cx.new(|cx| {
            ToolDiffPopover::new(preview.clone(), label.clone(), status, cwd, cx)
                .on_open_file(move |path, _, cx| {
                    let path = path.to_string();
                    weak.update(cx, |this, cx| {
                        this.dismiss(false, None, cx);
                        cx.emit(ToolDiffEvent::OpenFile { path });
                    })
                    .ok();
                })
                .on_close(move |window, cx| {
                    close_weak
                        .update(cx, |this, cx| this.dismiss(true, Some(window), cx))
                        .ok();
                })
        });
        let trigger_focus = cx.focus_handle();
        let surface_focus = popover.read(cx).focus.clone();
        let subscriptions = vec![
            cx.on_focus(&trigger_focus, window, |this, _, cx| {
                this.trigger_focused = true;
                if std::mem::take(&mut this.refocusing) {
                    return;
                }
                this.show(cx);
            }),
            cx.on_blur(&trigger_focus, window, |this, _, cx| {
                this.trigger_focused = false;
                this.schedule_close(cx);
            }),
            cx.on_focus_in(&surface_focus, window, |this, _, cx| {
                this.surface_focused = true;
                this.show(cx);
            }),
            cx.on_focus_out(&surface_focus, window, |this, _, _, cx| {
                this.surface_focused = false;
                this.schedule_close(cx);
            }),
        ];
        Self {
            popover,
            label,
            preview,
            opens,
            open: false,
            trigger_hovered: false,
            surface_hovered: false,
            trigger_focused: false,
            surface_focused: false,
            refocusing: false,
            trigger_focus,
            surface_focus,
            trigger: Box::new(trigger),
            bounds: BoundsMap::default(),
            timer: None,
            _focus: subscriptions,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn popover(&self) -> &gpui::Entity<ToolDiffPopover> {
        &self.popover
    }

    /// The trigger's accessible name.
    pub fn trigger_label(&self) -> String {
        trigger_label(&self.preview, &self.label, self.opens)
    }

    /// `show`.
    pub fn show(&mut self, cx: &mut Context<Self>) {
        self.timer = None;
        self.open = true;
        cx.notify();
    }

    /// `dismiss`: close now. With `restore_focus`, focus inside the card
    /// goes back to the chip first, so it does not reopen the card.
    pub fn dismiss(
        &mut self,
        restore_focus: bool,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.timer = None;
        if restore_focus
            && let Some(window) = window
            && self.surface_focus.contains_focused(window, cx)
        {
            self.refocusing = true;
            self.trigger_focus.focus(window, cx);
        }
        self.trigger_hovered = false;
        self.surface_hovered = false;
        self.open = false;
        cx.notify();
    }

    /// The pointer entered or left the chip.
    pub fn hover_trigger(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.trigger_hovered = hovered;
        if hovered {
            self.timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(OPEN_DELAY).await;
                this.update(cx, |this, cx| {
                    this.timer = None;
                    this.open = true;
                    cx.notify();
                })
                .ok();
            }));
        } else {
            self.schedule_close(cx);
        }
    }

    /// The pointer entered or left the card.
    pub fn hover_surface(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.surface_hovered = hovered;
        if hovered {
            self.timer = None;
        } else {
            self.schedule_close(cx);
        }
    }

    /// `scheduleClose`: close once the pointer and focus have both left.
    pub fn schedule_close(&mut self, cx: &mut Context<Self>) {
        self.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CLOSE_DELAY).await;
            this.update(cx, |this, cx| {
                this.timer = None;
                if !this.trigger_hovered
                    && !this.surface_hovered
                    && !this.trigger_focused
                    && !this.surface_focused
                {
                    this.open = false;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// A click on the chip: close the card and open the file or diff.
    pub fn click(&mut self, cx: &mut Context<Self>) {
        self.dismiss(false, None, cx);
        if self.opens {
            cx.emit(ToolDiffEvent::Open);
        }
    }
}

impl Focusable for ToolDiffPreview {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.trigger_focus.clone()
    }
}

impl Render for ToolDiffPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let focused = self.trigger_focus.is_focused(window);
        let content = (self.trigger)(window, cx);
        let trigger = div()
            .id("tool-diff-trigger")
            .relative()
            .track_focus(&self.trigger_focus)
            .cursor_pointer()
            .when(focused, |el| {
                el.border_1().border_color(style::sky_ring()).rounded(u(4.))
            })
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.hover_trigger(*hovered, cx)))
            // A click does not focus the chip, as in WebKit, so it does not
            // open the card on its way to opening the file.
            .capture_any_mouse_down(|_, window, _| window.prevent_default())
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.click(cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if this.open
                    && (key == "down" || (key == "tab" && !event.keystroke.modifiers.shift))
                {
                    cx.stop_propagation();
                    this.surface_focus.clone().focus(window, cx);
                }
            }))
            .child(self.bounds.track("trigger"))
            .child(content);
        let surface = (self.open)
            .then(|| self.bounds.get("trigger"))
            .flatten()
            .map(|anchor| {
                let layer = theme.layer.popover;
                deferred(
                    gpui::anchored()
                        .position(gpui::point(anchor.left(), anchor.bottom() + px(6.)))
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            div()
                                .id("tool-diff-surface")
                                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                    this.hover_surface(*hovered, cx)
                                }))
                                .on_mouse_down_out(
                                    cx.listener(|this, _, _, cx| this.dismiss(false, None, cx)),
                                )
                                .child(self.popover.clone()),
                        ),
                )
                .with_priority(layer)
            });
        div()
            .child(trigger)
            .when_some(surface, |el, surface| el.child(surface))
    }
}

/// The element id of a chip in a list of rows.
pub fn trigger_id(key: &str) -> ElementId {
    ElementId::Name(format!("tool-diff:{key}").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::block::ToolPreviewKind;

    pub fn edit_preview() -> ToolPreview {
        let mut preview = ToolPreview::new(ToolPreviewKind::Write);
        preview.path = Some("/Users/me/Documents/notes.md".into());
        preview.file_name = Some("notes.md".into());
        preview.additions = Some(1);
        preview.deletions = Some(1);
        preview.lines = Some(vec![
            ToolPreviewLine {
                kind: ToolPreviewLineKind::Del,
                text: "  before".into(),
                number: Some(1),
                extra: Default::default(),
            },
            ToolPreviewLine {
                kind: ToolPreviewLineKind::Add,
                text: "  after".into(),
                number: Some(1),
                extra: Default::default(),
            },
            ToolPreviewLine {
                kind: ToolPreviewLineKind::Context,
                text: "keep".into(),
                number: Some(2),
                extra: Default::default(),
            },
        ]);
        preview
    }

    #[test]
    fn labels_write_previews_accurately_including_failed_tools() {
        let mut written = ToolPreview::new(ToolPreviewKind::Write);
        written.content_only = Some(true);
        assert!(
            preview_description(&written, ToolCallState::Accepted).starts_with("Written content")
        );
        assert_eq!(
            preview_description(&written, ToolCallState::Pending),
            "Proposed changes"
        );
        assert!(
            preview_description(&written, ToolCallState::Rejected).starts_with("Attempted changes")
        );
        assert_eq!(
            preview_description(&edit_preview(), ToolCallState::Accepted),
            "Change preview"
        );
    }

    #[test]
    fn names_the_chip_by_what_a_click_does() {
        assert_eq!(
            trigger_label(&edit_preview(), "notes.md", true),
            "Open notes.md"
        );
        assert_eq!(
            trigger_label(&edit_preview(), "notes.md", false),
            "Preview changes: notes.md"
        );
        let mut written = ToolPreview::new(ToolPreviewKind::Write);
        written.content_only = Some(true);
        assert_eq!(
            trigger_label(&written, "a.md", false),
            "Preview written content: a.md"
        );
    }

    #[test]
    fn colors_keywords_types_and_comments() {
        assert_eq!(
            tokens("const Foo = bar"),
            [
                (0..5, Token::Keyword),
                (5..6, Token::Plain),
                (6..9, Token::Type),
                (9..15, Token::Plain),
            ]
        );
        assert_eq!(tokens("  // note"), [(0..9, Token::Comment)]);
        assert_eq!(tokens("plain"), [(0..5, Token::Plain)]);
    }
}
