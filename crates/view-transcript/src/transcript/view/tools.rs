//! Tool rows: `ActivityToolRow`, `ToolCall`, `ToolCallSummary`,
//! `MonoCodeCallRow`, and `ApprovalControls` from AgentTranscript.tsx, the
//! edit card from src/features/files/ui/FilePreview.tsx, and the hover diff
//! from ToolDiffPreview.tsx.

use std::sync::LazyLock;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, FontWeight, HighlightStyle, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledText, Window, div, px,
};
use monocode_core::block::{ToolPreview, ToolPreviewKind, ToolPreviewLine, ToolPreviewLineKind};
use monocode_core::js;
use monocode_core::paths::display_path;
use monocode_core::reducer::{
    MAX_PREVIEW_LINES, is_edit_tool, is_read_tool, is_search_tool, stub_file_preview,
};
use monocode_core::transcript::activity::{
    ToolCallState, is_incomplete_tool, needs_approval, resolve_tool_call_display, tool_call_label,
    tool_call_state,
};
use monocode_core::transcript::monocode_call::monocode_tool_call;
use monocode_core::transcript::paths::resolve_workspace_path;
use monocode_core::{Block, transcript::BlockRef};
use monocode_ui::styled::{UiStyled as _, format_integer};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, file_type_icon, folder_type_icon, icon, u};
use regex::Regex;

use super::parts::{chevron, monocode_mark, pending_ring};
use super::style::{TextSizes as _, palette};
use super::{ApprovalDecision, TranscriptEvent, TranscriptView, eid};

fn tool_kind(block: &Block) -> Option<&str> {
    block.tool.as_ref().and_then(|tool| tool.kind.as_deref())
}

fn tool_preview(block: &Block) -> Option<&ToolPreview> {
    block.tool.as_ref().and_then(|tool| tool.preview.as_ref())
}

fn tool_detail(block: &Block) -> Option<String> {
    block
        .tool
        .as_ref()
        .and_then(|tool| tool.detail.as_deref())
        .map(js::trim)
        .filter(|detail| !detail.is_empty())
        .map(str::to_string)
}

/// `ToolCallStatusIcon`: failure stays marked; running and success get none.
fn status_icon(state: ToolCallState, theme: &Theme) -> Option<AnyElement> {
    (state == ToolCallState::Rejected).then(|| {
        icon(IconName::X)
            .size(u(14.))
            .text_color(theme.colors.danger)
            .into_any_element()
    })
}

impl TranscriptView {
    /// `ActivityToolRow`: a call in the work trail. A failed call opens onto
    /// its error.
    pub(super) fn render_activity_tool_row(
        &mut self,
        key: &str,
        block: &BlockRef,
        live: bool,
        bare: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(call) = monocode_tool_call(block) {
            return self.render_monocode_call(key, block, &call, cx);
        }
        let cwd = self.cwd();
        let label = tool_call_label(block, cwd.as_deref());
        let state = tool_call_state(block);
        let pending = needs_approval(block);
        let error_detail = (!pending && state == ToolCallState::Rejected)
            .then(|| tool_detail(block))
            .flatten();
        let theme = Theme::of(cx).clone();
        let summary = self.render_tool_summary(
            key,
            &block.id,
            &label,
            tool_preview(block),
            bare,
            state == ToolCallState::Rejected,
            state,
            cx,
        );
        let icon = (!bare).then(|| {
            activity_tool_icon(eid(key, &format!("spin:{}", block.id)), state, live, &theme)
        });
        let mut column = div().flex().flex_col().min_w_0();
        if let Some(detail) = error_detail {
            let toggle = format!("error:{}", block.id);
            let open = self.toggled(&toggle, false);
            let toggle_key = toggle.clone();
            column =
                column.child(
                    div()
                        .id(eid(key, &toggle))
                        .flex()
                        .items_center()
                        .gap(u(6.))
                        .py(u(4.))
                        .min_w_0()
                        .cursor_pointer()
                        .tooltip(tooltip(format!(
                            "{} error details for {label}",
                            if open { "Hide" } else { "Show" }
                        )))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle(toggle_key.clone(), false, cx)
                        }))
                        .children(icon)
                        .child(div().flex().flex_1().min_w_0().child(summary))
                        .children(status_icon(state, &theme))
                        .child(chevron(
                            open,
                            gpui::Hsla {
                                a: 0.6,
                                ..theme.colors.danger
                            },
                        )),
                );
            if open {
                column = column.child(error_text(&detail, &theme, !bare));
            }
        } else {
            column = column.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .py(u(4.))
                    .min_w_0()
                    .children(icon)
                    .child(summary)
                    .when(!pending, |el| el.children(status_icon(state, &theme))),
            );
        }
        if pending {
            column = column.child(self.render_approval_controls(key, block, cx));
        }
        column.into_any_element()
    }

    /// `ToolCall`: a call outside the trail, such as an edit awaiting approval.
    pub(super) fn render_tool_call(
        &mut self,
        key: &str,
        block: &BlockRef,
        embedded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let frame = |el: gpui::Div| {
            if embedded {
                el.py(u(2.))
            } else {
                el.px(u(16.)).py(u(4.))
            }
        };
        if let Some(call) = monocode_tool_call(block) {
            let row = self.render_monocode_call(key, block, &call, cx);
            return frame(div()).child(row).into_any_element();
        }
        let cwd = self.cwd();
        let preview = tool_preview(block);
        let label = tool_call_label(block, cwd.as_deref());
        let detail = tool_detail(block);
        let state = tool_call_state(block);
        let theme = Theme::of(cx).clone();
        let title = if block.text.is_empty() {
            block.tool.as_ref().and_then(|tool| tool.title.as_deref())
        } else {
            Some(block.text.as_str())
        };
        let edit = is_edit_tool(tool_kind(block), title, preview);
        let compact = is_read_tool(tool_kind(block), Some(&label), preview)
            || is_search_tool(tool_kind(block), Some(&label), preview);
        let expandable = !compact && detail.as_deref().is_some_and(|detail| detail != label);

        if edit {
            let body = if needs_approval(block) {
                let preview = preview
                    .cloned()
                    .unwrap_or_else(|| stub_file_preview(tool_kind(block), Some(&label)));
                self.render_file_preview(key, &block.id, &preview, state, false, cx)
            } else {
                let summary = self.render_tool_summary(
                    key,
                    &block.id,
                    &label,
                    preview,
                    false,
                    state == ToolCallState::Rejected,
                    state,
                    cx,
                );
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .py(u(4.))
                    .min_w_0()
                    .children(tool_call_icon(state, &theme))
                    .child(summary)
                    .into_any_element()
            };
            let approval = self.render_approval_controls(key, block, cx);
            return frame(div()).child(body).child(approval).into_any_element();
        }

        if is_incomplete_tool(block, &label, state) {
            return div().into_any_element();
        }

        let state_label = match state {
            ToolCallState::Accepted => "Accepted",
            ToolCallState::Rejected => "Rejected",
            ToolCallState::Pending => "Pending",
        };
        let summary = self.render_tool_summary(
            key,
            &block.id,
            &label,
            preview,
            false,
            state == ToolCallState::Rejected,
            ToolCallState::Accepted,
            cx,
        );
        let mut column = frame(div());
        if expandable {
            let toggle = format!("tool:{}", block.id);
            let open = self.toggled(&toggle, false);
            let toggle_key = toggle.clone();
            column =
                column.child(
                    div()
                        .id(eid(key, &toggle))
                        .flex()
                        .items_center()
                        .gap(u(8.))
                        .w_full()
                        .min_w_0()
                        .py(u(6.))
                        .rounded(u(8.))
                        .cursor_pointer()
                        .tooltip(tooltip(format!("{state_label} tool call: {label}")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle(toggle_key.clone(), false, cx)
                        }))
                        .children(tool_call_icon(state, &theme))
                        .child(summary)
                        .child(chevron(open, theme.content(0.35))),
                );
            if open {
                column = column.child(
                    div()
                        .mt(u(6.))
                        .px(u(10.))
                        .min_w_0()
                        .font_family(theme.fonts.mono.clone())
                        .text_px_l5(12.)
                        .text_color(theme.content(0.55))
                        .child(detail.unwrap_or_default()),
                );
            }
        } else {
            column = column.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .w_full()
                    .min_w_0()
                    .children(tool_call_icon(state, &theme))
                    .child(summary),
            );
        }
        let approval = self.render_approval_controls(key, block, cx);
        column.child(approval).into_any_element()
    }

    /// `ToolCallSummary`: the action word and the file it touched, as a chip
    /// that opens the file (or its diff for an edit).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_tool_summary(
        &mut self,
        key: &str,
        block_id: &str,
        label: &str,
        preview: Option<&ToolPreview>,
        chip: bool,
        failed: bool,
        status: ToolCallState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cwd = self.cwd();
        let display = resolve_tool_call_display(label, preview, cwd.as_deref());
        let theme = Theme::of(cx).clone();
        let (Some(action), Some(target)) = (display.action.clone(), display.target.clone()) else {
            let tone = if failed {
                theme.colors.danger
            } else if chip {
                theme.content(0.65)
            } else {
                theme.content(0.8)
            };
            return div()
                .id(eid(key, &format!("label:{block_id}")))
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(theme.fonts.mono.clone())
                .text_px_l5(13.)
                .text_color(tone)
                .tooltip(tooltip(label.to_string()))
                .child(label.to_string())
                .into_any_element();
        };
        let opens_diff = action == "Edit" || action == "Write";
        let file_path = display.file_path.clone();
        let can_preview = preview.is_some_and(|preview| {
            preview.kind == ToolPreviewKind::Write
                && display.preview_matches_file
                && (preview.content_only == Some(true)
                    || preview.lines.as_ref().is_some_and(|lines| {
                        lines
                            .iter()
                            .any(|line| line.kind != ToolPreviewLineKind::Context)
                    }))
        });
        let action_tone = if failed {
            theme.colors.danger
        } else {
            theme.content(0.5)
        };
        let target_tone = if failed {
            theme.colors.danger
        } else if chip {
            theme.content(0.7)
        } else {
            theme.content(0.85)
        };
        let action_el = div()
            .flex_none()
            .font_family(theme.fonts.sans.clone())
            .text_sm_ui()
            .text_color(action_tone)
            .child(action.clone());
        let row = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .font_family(theme.fonts.mono.clone())
            .text_px_l5(13.)
            .child(action_el);
        if !display.is_file {
            return row
                .child(
                    div()
                        .id(eid(key, &format!("target:{block_id}")))
                        .flex_1()
                        .min_w_0()
                        .pl(u(4.))
                        .truncate()
                        .text_color(target_tone)
                        .tooltip(tooltip(target.clone()))
                        .child(target),
                )
                .into_any_element();
        }
        let file_icon = if action == "List" {
            folder_type_icon(display.file_name.clone(), false, false).into_any_element()
        } else {
            file_type_icon(display.file_name.clone()).into_any_element()
        };
        let link = theme.colors.link;
        let mut target_el = div()
            .id(eid(key, &format!("target:{block_id}")))
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(4.))
            .rounded(u(4.))
            .px(u(4.))
            .text_color(target_tone)
            .child(file_icon)
            .child(div().min_w_0().truncate().child(target.clone()));
        target_el = if chip {
            target_el.max_w_full().bg(theme.content(0.06))
        } else {
            target_el.flex_1()
        };
        let interactive = file_path.is_some();
        if interactive {
            let path = file_path.clone().unwrap_or_default();
            let hover_bg = theme.content(if chip { 0.10 } else { 0.06 });
            // A plain file link underlines; a chip or a diff preview lights up.
            let underline = !chip && !can_preview;
            target_el = target_el
                .py(u(2.))
                .my(u(-2.))
                .cursor_pointer()
                .hover(move |s| {
                    let s = s.text_color(link);
                    if underline {
                        s.underline()
                    } else {
                        s.bg(hover_bg)
                    }
                })
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.stop_propagation();
                    if opens_diff {
                        cx.emit(TranscriptEvent::OpenDiff { path: path.clone() });
                    } else {
                        cx.emit(TranscriptEvent::OpenFile {
                            path: path.clone(),
                            line: None,
                        });
                    }
                }));
        }
        target_el = if can_preview {
            let preview = preview.cloned().expect("previewed write");
            let label = target.clone();
            let theme = theme.clone();
            target_el
                .hoverable_tooltip(move |_, cx| {
                    let preview = preview.clone();
                    let label = label.clone();
                    let theme = theme.clone();
                    cx.new(|_| DiffPopover {
                        preview,
                        label,
                        status,
                        cwd: None,
                        theme,
                    })
                    .into()
                })
                .tooltip_show_delay(Duration::from_millis(300))
        } else {
            target_el.tooltip(tooltip(target))
        };
        row.child(target_el).into_any_element()
    }

    /// `MonoCodeCallRow`: an app CLI call reads like the other rows, with
    /// the action instead of the binary path.
    fn render_monocode_call(
        &mut self,
        key: &str,
        block: &BlockRef,
        call: &monocode_core::transcript::monocode_call::MonoCodeToolCall,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let state = tool_call_state(block);
        let output = tool_detail(block).or_else(|| {
            tool_preview(block)
                .and_then(|preview| preview.output.as_deref())
                .map(js::trim)
                .filter(|output| !output.is_empty())
                .map(str::to_string)
        });
        let has_error = state == ToolCallState::Rejected && output.is_some();
        let pending_approval = needs_approval(block);
        let command = format!("monocode app {}", call.action);
        let verb = if pending_approval {
            "Run"
        } else if state == ToolCallState::Pending {
            "Running"
        } else {
            "Ran"
        };
        let rejected = state == ToolCallState::Rejected;
        let toggle = format!("error:{}", block.id);
        let open = self.toggled(&toggle, false);
        let summary = div()
            .id(eid(key, &format!("monocode:{}", block.id)))
            .flex()
            .items_center()
            .gap(u(6.))
            .py(u(4.))
            .w_full()
            .min_w_0()
            .child(
                div()
                    .flex_none()
                    .font_family(theme.fonts.sans.clone())
                    .text_sm_ui()
                    .text_color(if rejected {
                        theme.colors.danger
                    } else {
                        theme.content(0.5)
                    })
                    .child(verb),
            )
            .child(
                div()
                    .id(eid(key, &format!("monocode-chip:{}", block.id)))
                    .flex()
                    .min_w_0()
                    .max_w_full()
                    .items_center()
                    .gap(u(4.))
                    .rounded(u(4.))
                    .bg(theme.content(0.06))
                    .px(u(4.))
                    .font_family(theme.fonts.mono.clone())
                    .text_px_l5(13.)
                    .text_color(if rejected {
                        theme.colors.danger
                    } else {
                        theme.content(0.7)
                    })
                    .tooltip(tooltip(command.clone()))
                    .child(monocode_mark(14.))
                    .child(div().min_w_0().truncate().child(command)),
            )
            .children(status_icon(state, &theme))
            .when(has_error, |el| {
                el.child(chevron(
                    open,
                    Hsla {
                        a: 0.6,
                        ..theme.colors.danger
                    },
                ))
            });
        let summary = if has_error {
            let toggle_key = toggle.clone();
            summary
                .cursor_pointer()
                .tooltip(tooltip(format!(
                    "{} error details for MonoCode: {}",
                    if open { "Hide" } else { "Show" },
                    call.label
                )))
                .on_click(
                    cx.listener(move |this, _, _, cx| this.toggle(toggle_key.clone(), false, cx)),
                )
        } else {
            summary
        };
        let approval = self.render_approval_controls(key, block, cx);
        div()
            .min_w_0()
            .child(summary)
            .when(open && has_error, |el| {
                el.child(error_text(output.as_deref().unwrap_or(""), &theme, true))
            })
            .when(pending_approval, |el| {
                el.child(
                    div()
                        .id(eid(key, &format!("monocode-command:{}", block.id)))
                        .max_h(u(128.))
                        .overflow_y_scroll()
                        .py(u(4.))
                        .pl(u(20.))
                        .min_w_0()
                        .font_family(theme.fonts.mono.clone())
                        .text_px_l5(12.)
                        .text_color(theme.content(0.7))
                        .child(call.command.clone()),
                )
            })
            .child(approval)
            .into_any_element()
    }

    /// `ApprovalControls`: Allow and Deny under a call waiting on the user.
    pub(super) fn render_approval_controls(
        &mut self,
        key: &str,
        block: &Block,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(approval) = block
            .approval
            .as_ref()
            .filter(|approval| approval.decided.is_none())
        else {
            return div().into_any_element();
        };
        if !self.config.approvals {
            return div().into_any_element();
        }
        let request_id = approval.request_id;
        let theme = Theme::of(cx).clone();
        let button = |label: &'static str, fill: Hsla, hover: Hsla, ink: Hsla| {
            div()
                .id(eid(key, &format!("{label}:{}", block.id)))
                .rounded(u(6.))
                .px(u(10.))
                .py(u(2.))
                .text_px(11.)
                .line_height(u(16.))
                .font_family(theme.fonts.sans.clone())
                .bg(fill)
                .text_color(ink)
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(label)
        };
        div()
            .mt(u(6.))
            .flex()
            .gap(u(8.))
            .child(
                button(
                    "Allow",
                    theme.colors.content,
                    theme.content(0.8),
                    theme.colors.background_base,
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(TranscriptEvent::Approval {
                        request_id,
                        decision: ApprovalDecision::Allow,
                    })
                })),
            )
            .child(
                button(
                    "Deny",
                    theme.content(0.1),
                    theme.content(0.2),
                    theme.content(0.7),
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(TranscriptEvent::Approval {
                        request_id,
                        decision: ApprovalDecision::Deny,
                    })
                })),
            )
            .into_any_element()
    }

    /// `FilePreview` as a card: the file, its counts, and the first changed lines.
    pub(super) fn render_file_preview(
        &mut self,
        key: &str,
        block_id: &str,
        preview: &ToolPreview,
        status: ToolCallState,
        popover: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cwd = self.cwd();
        let theme = Theme::of(cx).clone();
        let path = preview.path.clone().filter(|path| !path.is_empty());
        let file_path = path.as_ref().map(|path| {
            resolve_workspace_path(path, cwd.as_deref()).unwrap_or_else(|| path.clone())
        });
        let on_open = file_path.map(|file| {
            cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                cx.emit(TranscriptEvent::OpenDiff { path: file.clone() });
            })
        });
        file_preview(
            eid(key, &format!("file:{block_id}")),
            preview,
            status,
            cwd.as_deref(),
            popover,
            &theme,
            on_open,
        )
        .into_any_element()
    }
}

/// `ActivityToolIcon`: a dashed ring while running, a dash once done.
fn activity_tool_icon(
    id: gpui::ElementId,
    state: ToolCallState,
    live: bool,
    theme: &Theme,
) -> AnyElement {
    if state == ToolCallState::Pending {
        return pending_ring(id, theme.content(0.4), live);
    }
    icon(IconName::Minus)
        .size(u(14.))
        .text_color(theme.content(0.5))
        .into_any_element()
}

/// `ToolCallIcon`.
fn tool_call_icon(state: ToolCallState, theme: &Theme) -> Option<AnyElement> {
    match state {
        ToolCallState::Rejected => Some(
            icon(IconName::X)
                .size(u(14.))
                .text_color(theme.colors.danger)
                .into_any_element(),
        ),
        ToolCallState::Pending => Some(
            icon(IconName::CircleDashed)
                .size(u(14.))
                .text_color(theme.content(0.4))
                .into_any_element(),
        ),
        ToolCallState::Accepted => None,
    }
}

/// Error output under a failed call.
fn error_text(text: &str, theme: &Theme, indent: bool) -> impl IntoElement {
    div()
        .min_w_0()
        .py(u(4.))
        .when(indent, |el| el.pl(u(20.)))
        .font_family(theme.fonts.mono.clone())
        .text_px_l5(12.)
        .text_color(Hsla {
            a: 0.8,
            ..theme.colors.danger
        })
        .child(text.to_string())
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

/// `highlight` in FilePreview.tsx: comments dim, keywords teal, types amber.
fn highlight_line(text: &str, dimmed: bool, theme: &Theme) -> StyledText {
    let base = theme.content(if dimmed { 0.8 * 0.7 } else { 0.8 });
    let trimmed = text.trim_start();
    let shown = SharedString::from(text.to_string());
    if trimmed.starts_with("//") || trimmed.starts_with('#') {
        let ink = theme.content(if dimmed { 0.45 * 0.7 } else { 0.45 });
        return StyledText::new(shown).with_highlights(vec![(
            0..text.len(),
            HighlightStyle {
                color: Some(ink),
                ..Default::default()
            },
        )]);
    }
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
    let mut runs = vec![(
        0..text.len(),
        HighlightStyle {
            color: Some(base),
            ..Default::default()
        },
    )];
    for word in WORD.find_iter(text) {
        let token = word.as_str();
        let color = if KEYWORDS.contains(&token) {
            Some(fade(palette::teal_300()))
        } else if token.starts_with(|c: char| c.is_ascii_uppercase()) {
            Some(fade(palette::amber_200_90()))
        } else {
            None
        };
        if let Some(color) = color {
            runs.push((
                word.range(),
                HighlightStyle {
                    color: Some(color),
                    ..Default::default()
                },
            ));
        }
    }
    // Later highlights win where they overlap, so split the base run.
    StyledText::new(shown).with_highlights(split_runs(runs))
}

/// Flatten overlapping highlight ranges (base first, tokens after) into
/// sorted, disjoint runs.
fn split_runs(
    runs: Vec<(std::ops::Range<usize>, HighlightStyle)>,
) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    let Some((base_range, base)) = runs.first().cloned() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut at = base_range.start;
    for (range, style) in runs.into_iter().skip(1) {
        if range.start > at {
            out.push((at..range.start, base));
        }
        out.push((range.clone(), style));
        at = range.end;
    }
    if at < base_range.end {
        out.push((at..base_range.end, base));
    }
    out
}

/// `PreviewLine`.
fn preview_line(line: &ToolPreviewLine, theme: &Theme) -> impl IntoElement {
    let (tint, bar, mark, mark_color) = match line.kind {
        ToolPreviewLineKind::Add => (
            Some(palette::teal_800_20()),
            palette::teal_400(),
            "+",
            palette::teal_400(),
        ),
        ToolPreviewLineKind::Del => (
            Some(palette::rose_800_20()),
            palette::rose_400(),
            "\u{2212}",
            palette::rose_400(),
        ),
        ToolPreviewLineKind::Context => (
            None,
            gpui::transparent_black(),
            " ",
            gpui::transparent_black(),
        ),
    };
    div()
        .relative()
        .flex()
        .items_center()
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
                .font_family(theme.fonts.mono.clone())
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
                .font_family(theme.fonts.mono.clone())
                .text_px(10.)
                .font_weight(FontWeight::BOLD)
                .text_color(mark_color)
                .child(mark),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pr(u(8.))
                .truncate()
                .font_family(theme.fonts.mono.clone())
                .text_px(11.)
                .line_height(u(18.))
                .child(highlight_line(
                    &line.text,
                    line.kind == ToolPreviewLineKind::Context,
                    theme,
                )),
        )
}

/// `FilePreview`: `card` is the bordered transcript card, `popover` the bare
/// body the hover preview shows.
#[allow(clippy::too_many_arguments)]
fn file_preview(
    id: gpui::ElementId,
    preview: &ToolPreview,
    status: ToolCallState,
    cwd: Option<&str>,
    popover: bool,
    theme: &Theme,
    on_open: Option<impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static>,
) -> impl IntoElement {
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
    let lines: Vec<&ToolPreviewLine> = preview
        .lines
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .take(MAX_PREVIEW_LINES)
        .collect();
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
        Some(path) => display_path(path, cwd),
        None => file_name
            .clone()
            .or_else(|| preview.title.clone())
            .unwrap_or_else(|| "File".into()),
    };
    let link = theme.colors.link;
    let mut name = div()
        .id(id)
        .flex_1()
        .min_w_0()
        .truncate()
        .font_family(theme.fonts.mono.clone())
        .text_px(12.)
        .line_height(u(16.))
        .medium()
        .text_color(theme.content(0.85))
        .child(label);
    if let Some(on_open) = on_open {
        name = name
            .cursor_pointer()
            .hover(move |s| s.text_color(link).underline())
            .on_click(on_open);
    }
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
        match status {
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
    let card = div()
        .min_w_0()
        .when(!popover, |el| {
            el.overflow_hidden()
                .rounded(u(10.))
                .border_1()
                .border_color(theme.content(0.1))
                .bg(theme.content(0.06))
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap(u(8.))
                .px(u(10.))
                .py(u(8.))
                .child(file_type_icon(file_name.unwrap_or_else(|| "file".into())))
                .child(name)
                .child(counts),
        );
    if !show_diff {
        return card;
    }
    let mut body = div().flex().flex_col().min_w_0();
    if preview.content_only == Some(true) && lines.is_empty() {
        body = body.child(
            div()
                .px(u(12.))
                .py(u(8.))
                .font_family(theme.fonts.mono.clone())
                .text_xs_ui()
                .text_color(theme.content(0.5))
                .child("Empty file"),
        );
    }
    for line in lines {
        body = body.child(preview_line(line, theme));
    }
    card.child(div().h(px(1.)).bg(theme.content(0.1)))
        .child(body)
}

/// The hover preview of a write row's own edit (`ToolDiffPreview`).
struct DiffPopover {
    preview: ToolPreview,
    label: String,
    status: ToolCallState,
    cwd: Option<String>,
    theme: Theme,
}

impl Render for DiffPopover {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let description = match self.status {
            ToolCallState::Pending => "Proposed changes",
            ToolCallState::Rejected => "Attempted changes \u{b7} tool did not complete",
            ToolCallState::Accepted if self.preview.content_only == Some(true) => {
                "Written content \u{b7} previous contents unavailable"
            }
            ToolCallState::Accepted => "Change preview",
        };
        let _ = &self.label;
        div()
            .w(u(460.))
            .max_h(u(280.))
            .overflow_hidden()
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(theme.colors.popover_border)
            .bg(theme.colors.background_base)
            .shadow_lg()
            .child(
                div()
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
                    .child(description),
            )
            .child(file_preview(
                "diff-popover".into(),
                &self.preview,
                self.status,
                self.cwd.as_deref(),
                true,
                theme,
                None::<fn(&gpui::ClickEvent, &mut Window, &mut App)>,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_token_runs_out_of_the_base_run() {
        let base = HighlightStyle::default();
        let token = HighlightStyle {
            color: Some(gpui::red()),
            ..Default::default()
        };
        let runs = split_runs(vec![(0..10, base), (2..4, token), (6..7, token)]);
        let ranges: Vec<_> = runs.iter().map(|(range, _)| range.clone()).collect();
        assert_eq!(ranges, [0..2, 2..4, 4..6, 6..7, 7..10]);
    }
}
