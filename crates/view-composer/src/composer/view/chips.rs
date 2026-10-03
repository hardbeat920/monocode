//! Ports of src/features/sessions/ui/ChatContextChip.tsx and
//! AttachmentChip.tsx as the composer draws them: removable chips above the
//! prompt, with a hover preview for context items and a thumbnail for
//! images.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use gpui::{
    AnyElement, Context, ImageFormat, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _, Window, div, img,
    prelude::FluentBuilder as _,
};
use monocode_core::Attachment;
use monocode_core::attachment::{AttachmentKind, is_attachment_folder};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::widgets::popover_frame;
use monocode_ui::{IconName, Theme, file_type_icon, folder_type_icon, icon, u};

use super::super::model::chat_context::{
    ChatContextItem, DiffLineChange, chat_context_key, context_excerpt, context_file_name,
    line_range,
};
use super::Composer;
use crate::pickers::anchor::{Side, anchored_popover};

const HOVER_OPEN_DELAY: Duration = Duration::from_millis(220);
const HOVER_CLOSE_DELAY: Duration = Duration::from_millis(100);

/// The chip whose preview is showing, and the timer that will change it.
#[derive(Default)]
pub(crate) struct ChipPreview {
    pub open: Option<String>,
    pub epoch: u64,
}

/// `chipLabel`.
struct ChipLabel {
    action: &'static str,
    full: String,
}

fn chip_label(item: &ChatContextItem) -> ChipLabel {
    match item {
        ChatContextItem::Quote { text } => ChipLabel {
            action: "Quoted text",
            full: context_excerpt(text),
        },
        ChatContextItem::Code {
            path,
            start_line,
            end_line,
        } => ChipLabel {
            action: "Open selected lines",
            full: format!("{path}, lines {}", line_range(*start_line, *end_line)),
        },
        ChatContextItem::Comment {
            path,
            line,
            comment,
            ..
        } => ChipLabel {
            action: "Comment",
            full: format!(
                "{}, {}",
                match line {
                    Some(line) => format!("{path}:{line}"),
                    None => path.clone(),
                },
                context_excerpt(comment)
            ),
        },
    }
}

/// `fileTarget`: where clicking the chip opens.
fn file_target(item: &ChatContextItem) -> Option<(String, i64)> {
    match item {
        ChatContextItem::Code {
            path, start_line, ..
        } => Some((path.clone(), *start_line)),
        ChatContextItem::Comment {
            path,
            line: Some(line),
            change,
            ..
        } if *change != DiffLineChange::Removed => Some((path.clone(), *line)),
        _ => None,
    }
}

fn chip_icon(item: &ChatContextItem, theme: &Theme) -> AnyElement {
    match item {
        ChatContextItem::Code { path, .. } => div()
            .size(u(14.))
            .flex_none()
            .child(file_type_icon(context_file_name(path)).size(14.))
            .into_any_element(),
        ChatContextItem::Quote { .. } => icon(IconName::TextQuote)
            .size(u(14.))
            .text_color(theme.content(0.45))
            .into_any_element(),
        ChatContextItem::Comment { .. } => icon(IconName::MessageSquare)
            .size(u(14.))
            .text_color(theme.content(0.45))
            .into_any_element(),
    }
}

fn line_tag(text: String, theme: &Theme) -> impl IntoElement {
    div()
        .flex_none()
        .font_family(theme.fonts.mono.clone())
        .text_px(10.)
        .tabular()
        .text_color(theme.content(0.45))
        .child(text)
}

fn truncated(text: String, max: f32) -> gpui::Div {
    div().min_w_0().max_w(u(max)).truncate().child(text)
}

/// The chip body (`label.body`).
fn chip_body(item: &ChatContextItem, theme: &Theme) -> Vec<AnyElement> {
    match item {
        ChatContextItem::Quote { text } => {
            vec![truncated(context_excerpt(text), 224.).into_any_element()]
        }
        ChatContextItem::Code {
            path,
            start_line,
            end_line,
        } => vec![
            truncated(context_file_name(path), 176.).into_any_element(),
            line_tag(format!("L{}", line_range(*start_line, *end_line)), theme).into_any_element(),
        ],
        ChatContextItem::Comment {
            path,
            line,
            comment,
            ..
        } => {
            let mut out = vec![
                truncated(context_file_name(path), 128.)
                    .flex_none()
                    .into_any_element(),
            ];
            if let Some(line) = line {
                out.push(line_tag(format!("L{line}"), theme).into_any_element());
            }
            out.push(
                truncated(context_excerpt(comment), 192.)
                    .text_color(theme.content(0.55))
                    .into_any_element(),
            );
            out
        }
    }
}

/// `ChatContextPreview`.
fn chat_context_preview(item: &ChatContextItem, openable: bool, theme: &Theme) -> AnyElement {
    let header = |title: String| {
        div()
            .flex()
            .items_center()
            .gap(u(6.))
            .min_w_0()
            .text_px(11.)
            .text_color(theme.content(0.50))
            .child(chip_icon(item, theme))
            .child(div().min_w_0().truncate().child(title))
    };
    match item {
        ChatContextItem::Quote { text } => div()
            .child(header("Quoted text".into()))
            .child(
                div()
                    .mt(u(8.))
                    .border_l_2()
                    .border_color(theme.content(0.15))
                    .pl(u(10.))
                    .text_px(12.)
                    .line_height(u(20.))
                    .text_color(theme.content(0.80))
                    .child(text.clone()),
            )
            .into_any_element(),
        ChatContextItem::Code {
            path,
            start_line,
            end_line,
        } => {
            let lines = line_range(*start_line, *end_line);
            let title = if end_line > start_line {
                format!("Lines {lines}")
            } else {
                format!("Line {lines}")
            };
            div()
                .child(header(title))
                .child(
                    div()
                        .mt(u(6.))
                        .font_family(theme.fonts.mono.clone())
                        .text_px(11.)
                        .line_height(u(16.))
                        .text_color(theme.content(0.55))
                        .child(path.clone()),
                )
                .when(openable, |el| {
                    el.child(
                        div()
                            .mt(u(8.))
                            .text_px(11.)
                            .text_color(theme.content(0.40))
                            .child("Click to open"),
                    )
                })
                .into_any_element()
        }
        ChatContextItem::Comment {
            path,
            line,
            change,
            code,
            comment,
        } => {
            let (marker, bg, fg) = match change {
                DiffLineChange::Added => (
                    "+",
                    gpui::Hsla {
                        a: 0.15,
                        ..theme.colors.success
                    },
                    theme.colors.success,
                ),
                DiffLineChange::Removed => (
                    "-",
                    gpui::Hsla {
                        a: 0.15,
                        ..theme.colors.danger
                    },
                    theme.colors.danger,
                ),
                DiffLineChange::Unchanged => (" ", theme.content(0.06), theme.content(0.70)),
            };
            let title = match line {
                Some(line) => format!("{path}:{line}"),
                None => path.clone(),
            };
            div()
                .child(header(title))
                .child(
                    div()
                        .mt(u(8.))
                        .rounded(u(theme.radius.md))
                        .px(u(8.))
                        .py(u(4.))
                        .bg(bg)
                        .text_color(fg)
                        .font_family(theme.fonts.mono.clone())
                        .text_px(11.)
                        .line_height(u(16.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(format!("{marker} {code}")),
                )
                .child(
                    div()
                        .mt(u(8.))
                        .text_px(12.)
                        .line_height(u(20.))
                        .text_color(theme.content(0.85))
                        .child(comment.clone()),
                )
                .into_any_element()
        }
    }
}

impl Composer {
    /// Opens or closes a chip preview after the React hover delays.
    pub(crate) fn hover_chip(
        &mut self,
        key: String,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.chip_preview.epoch += 1;
        let epoch = self.chip_preview.epoch;
        let delay = if hovered {
            HOVER_OPEN_DELAY
        } else {
            HOVER_CLOSE_DELAY
        };
        if hovered && self.chip_preview.open.as_deref() == Some(key.as_str()) {
            return;
        }
        let task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| {
                if this.chip_preview.epoch != epoch {
                    return;
                }
                this.chip_preview.open = hovered.then_some(key);
                cx.notify();
            })
            .ok();
        });
        self._tasks.push(task);
    }

    /// `ChatContextChip` with a remove button.
    pub(crate) fn render_context_chip(
        &self,
        item: &ChatContextItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let key = chat_context_key(item);
        let label = chip_label(item);
        let target = file_target(item);
        let openable = target.is_some();
        let open = self.chip_preview.open.as_deref() == Some(key.as_str());
        let id = SharedString::from(format!("context-chip-{key}"));
        let hover_key = key.clone();
        let remove_key = key.clone();
        let click_key = key.clone();
        let button = div()
            .id(id.clone())
            .flex()
            .h_full()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .rounded(u(theme.radius.md))
            .pl(u(6.))
            .pr(u(4.))
            .when(openable, |el| {
                let ink = theme.colors.content;
                el.hover(move |style| style.text_color(ink))
            })
            .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                this.hover_chip(hover_key.clone(), *hovered, window, cx);
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                if let Some((path, line)) = target.clone() {
                    this.chip_preview.open = None;
                    cx.emit(super::ComposerEvent::OpenFile {
                        path,
                        line: Some(line),
                    });
                } else {
                    this.chip_preview.open = Some(click_key.clone());
                }
                let _ = window;
                cx.notify();
            }))
            .child(chip_icon(item, &theme))
            .children(chip_body(item, &theme));
        let remove_hover = theme.content(0.15);
        let remove_ink = theme.colors.content;
        let remove = div()
            .id(SharedString::from(format!("context-chip-remove-{key}")))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(16.))
            .rounded_full()
            .text_color(theme.content(0.40))
            .hover(move |style| style.bg(remove_hover).text_color(remove_ink))
            .tooltip(monocode_ui::widgets::tooltip(format!(
                "Remove {}",
                label.full
            )))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.chip_preview.open = None;
                this.remove_context_item(&remove_key, window, cx);
            }))
            .child(
                icon(IconName::X)
                    .size(u(12.))
                    .text_color(theme.content(0.40)),
            );
        let mut chip = div()
            .relative()
            .flex()
            .flex_none()
            .h(u(24.))
            .min_w_0()
            .max_w_full()
            .items_center()
            .rounded(u(theme.radius.md))
            .bg(theme.content(0.10))
            .pr(u(2.))
            .text_px(11.)
            .line_height(gpui::relative(1.))
            .text_color(theme.content(0.80))
            .child(button)
            .child(remove);
        if open {
            let width = if matches!(item, ChatContextItem::Code { .. }) {
                280.
            } else {
                360.
            };
            let preview = popover_frame(SharedString::from(format!("context-preview-{key}")))
                .width(width)
                .max_height(320.)
                .animate(self.props.animate)
                .child(
                    div()
                        .p(u(12.))
                        .text_color(theme.colors.content)
                        .child(chat_context_preview(item, openable, &theme)),
                );
            chip = chip.child(anchored_popover(
                Side::Top,
                6.,
                theme.layer.popover,
                window,
                preview,
            ));
        }
        let _ = label.action;
        chip.into_any_element()
    }

    /// `AttachmentChip` with a remove button.
    pub(crate) fn render_attachment_chip(
        &self,
        file: &Attachment,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let preview = attachment_image(file);
        let id = file.id.clone();
        let remove_hover = theme.content(0.15);
        let remove_ink = theme.colors.content;
        let image = file.kind == AttachmentKind::Image && preview.is_some();
        let remove = div()
            .id(SharedString::from(format!("attachment-remove-{id}")))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_full()
            .hover(move |style| style.bg(remove_hover).text_color(remove_ink))
            .tooltip(monocode_ui::widgets::tooltip("Remove"))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.remove_attachment(&id, window, cx);
            }));
        let remove = if image {
            remove
                .absolute()
                .top(u(-4.))
                .right(u(-4.))
                .size(u(20.))
                .bg(theme.content(0.20))
                .shadow_sm()
                .child(
                    icon(IconName::X)
                        .size(u(12.))
                        .text_color(theme.content(0.70)),
                )
        } else {
            remove.size(u(16.)).child(
                icon(IconName::X)
                    .size(u(12.))
                    .text_color(theme.content(0.40)),
            )
        };
        let chip = div()
            .relative()
            .flex()
            .flex_none()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .rounded(u(theme.radius.md));
        let chip = if let Some(source) = preview.filter(|_| image) {
            chip.child(
                div()
                    .size(u(36.))
                    .flex_none()
                    .rounded(u(theme.radius.lg))
                    .overflow_hidden()
                    .child(
                        img(source)
                            .size_full()
                            .rounded(u(theme.radius.lg))
                            .object_fit(gpui::ObjectFit::Cover),
                    ),
            )
        } else {
            let glyph = if is_attachment_folder(file) {
                folder_type_icon(file.name.clone(), false, false).size(16.)
            } else {
                file_type_icon(file.name.clone()).size(16.)
            };
            chip.bg(theme.content(0.10))
                .py(u(2.))
                .pl(u(4.))
                .pr(u(4.))
                .child(
                    div()
                        .size(u(20.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .child(glyph),
                )
                .child(
                    div()
                        .min_w_0()
                        .max_w(u(140.))
                        .truncate()
                        .text_px(11.)
                        .line_height(gpui::relative(1.))
                        .text_color(theme.content(0.80))
                        .child(file.name.clone()),
                )
        };
        chip.child(remove).into_any_element()
    }
}

/// `attachmentPreviewSrc` as an image source: inline bytes, else the file.
fn attachment_image(file: &Attachment) -> Option<gpui::ImageSource> {
    if let Some(data) = file.data.as_deref().filter(|data| !data.is_empty())
        && file.kind == AttachmentKind::Image
        && let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data)
    {
        let format = ImageFormat::from_mime_type(&file.mime_type).unwrap_or(ImageFormat::Png);
        return Some(gpui::ImageSource::Image(Arc::new(gpui::Image::from_bytes(
            format, bytes,
        ))));
    }
    if let Some(url) = file.preview_url.as_deref().filter(|url| !url.is_empty()) {
        return Some(gpui::ImageSource::from(SharedString::from(url.to_string())));
    }
    file.path
        .as_deref()
        .filter(|path| !path.is_empty() && file.kind == AttachmentKind::Image)
        .map(|path| gpui::ImageSource::from(std::path::PathBuf::from(path)))
}
