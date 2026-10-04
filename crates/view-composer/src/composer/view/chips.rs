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
use monocode_ui::styled::{UiStyled as _, glass_backdrop};
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

pub(crate) struct AttachmentPreview {
    pub(crate) id: String,
    source: gpui::ImageSource,
    name: String,
    focus: gpui::FocusHandle,
    previous_focus: Option<gpui::FocusHandle>,
}

pub(crate) struct AttachmentImage {
    file: Attachment,
    source: Option<gpui::ImageSource>,
    _load: gpui::Task<()>,
}

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
        &mut self,
        file: &Attachment,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let preview = self.attachment_image(file, cx);
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
            let name = file.name.clone();
            let attachment_id = file.id.clone();
            let full_source = source.clone();
            chip.child(
                div()
                    .id(SharedString::from(format!("attachment-open-{}", file.id)))
                    .debug_selector(|| "composer-attachment-image".into())
                    .size(u(36.))
                    .flex_none()
                    .rounded(u(theme.radius.lg))
                    .overflow_hidden()
                    .cursor_pointer()
                    .tooltip(monocode_ui::widgets::tooltip(format!(
                        "Open {name} full screen"
                    )))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        let focus = cx.focus_handle();
                        let previous_focus = window.focused(cx);
                        focus.focus(window, cx);
                        this.attachment_preview = Some(AttachmentPreview {
                            id: attachment_id.clone(),
                            source: full_source.clone(),
                            name: name.clone(),
                            focus,
                            previous_focus,
                        });
                        cx.notify();
                    }))
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

    fn attachment_image(
        &mut self,
        file: &Attachment,
        cx: &mut Context<Self>,
    ) -> Option<gpui::ImageSource> {
        let preview_url = file.preview_url.as_deref().filter(|url| !url.is_empty());
        let avif = file.kind == AttachmentKind::Image
            && (preview_url.is_some_and(|url| url.starts_with("data:image/avif;base64,"))
                || (file.mime_type == "image/avif" && preview_url.is_none()));
        if !avif {
            return attachment_image(file);
        }
        if let Some(preview) = self.attachment_images.get(&file.id)
            && preview.file == *file
        {
            return preview.source.clone();
        }
        let loading = file.clone();
        let id = file.id.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { load_avif_attachment(&loading) })
                .await;
            this.update(cx, |this, cx| {
                if let Some(preview) = this.attachment_images.get_mut(&id) {
                    preview.source = result.ok().map(gpui::ImageSource::Image);
                    cx.notify();
                }
            })
            .ok();
        });
        self.attachment_images.insert(
            file.id.clone(),
            AttachmentImage {
                file: file.clone(),
                source: None,
                _load: task,
            },
        );
        None
    }

    pub(crate) fn close_attachment_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(preview) = self.attachment_preview.take() {
            if let Some(focus) = preview.previous_focus {
                focus.focus(window, cx);
            }
            cx.notify();
        }
    }

    pub(crate) fn render_attachment_preview(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let preview = self.attachment_preview.as_ref()?;
        let theme = Theme::of(cx).clone();
        let viewport = window.viewport_size();
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position_mode(gpui::AnchoredPositionMode::Window)
                    .position(gpui::point(gpui::px(0.), gpui::px(0.)))
                    .child(
                        div()
                            .id("composer-image-lightbox")
                            .debug_selector(|| "composer-image-lightbox".into())
                            .track_focus(&preview.focus)
                            .relative()
                            .w(viewport.width)
                            .h(viewport.height)
                            .flex()
                            .items_center()
                            .justify_center()
                            .p(u(24.))
                            .child(glass_backdrop(0., 4., gpui::black().opacity(0.85)))
                            .on_key_down(cx.listener(
                                |this, event: &gpui::KeyDownEvent, window, cx| {
                                    if event.keystroke.key == "escape" {
                                        cx.stop_propagation();
                                        this.close_attachment_preview(window, cx);
                                    }
                                },
                            ))
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.close_attachment_preview(window, cx);
                                }),
                            )
                            .child(
                                div()
                                    .debug_selector(|| "composer-image-lightbox-image".into())
                                    .max_w_full()
                                    .max_h_full()
                                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .child(
                                        img(preview.source.clone())
                                            .max_w_full()
                                            .max_h_full()
                                            .object_fit(gpui::ObjectFit::Contain)
                                            .shadow_2xl(),
                                    ),
                            )
                            .child(
                                div()
                                    .id("composer-image-lightbox-close")
                                    .absolute()
                                    .right(u(16.))
                                    .top(u(16.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .size(u(36.))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(gpui::white().opacity(0.15))
                                    .bg(gpui::black().opacity(0.45))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(gpui::black().opacity(0.65)))
                                    .tooltip(monocode_ui::widgets::tooltip("Close image preview"))
                                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.close_attachment_preview(window, cx)
                                    }))
                                    .child(
                                        icon(IconName::X)
                                            .size(u(16.))
                                            .text_color(gpui::white().opacity(0.8)),
                                    ),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .size_0()
                                    .overflow_hidden()
                                    .child(preview.name.clone()),
                            ),
                    ),
            )
            .with_priority(theme.layer.dialog)
            .into_any_element(),
        )
    }
}

fn load_avif_attachment(file: &Attachment) -> Result<Arc<gpui::Image>, String> {
    let inline = file
        .preview_url
        .as_deref()
        .and_then(|url| url.strip_prefix("data:image/avif;base64,"))
        .or_else(|| file.data.as_deref().filter(|data| !data.is_empty()));
    let bytes = match inline {
        Some(data) => base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|error| error.to_string())?,
        None => std::fs::read(file.path.as_deref().ok_or("image has no bytes or path")?)
            .map_err(|error| error.to_string())?,
    };
    let rgba = monocode_editor::image_view::decode_avif_rgba(&bytes)?;
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(Arc::new(gpui::Image::from_bytes(
        ImageFormat::Png,
        png.into_inner(),
    )))
}

/// `attachmentPreviewSrc` as an image source: inline bytes, else the file.
fn attachment_image(file: &Attachment) -> Option<gpui::ImageSource> {
    if let Some(url) = file.preview_url.as_deref().filter(|url| !url.is_empty()) {
        if let Some(data) = url.strip_prefix("data:")
            && let Some((header, data)) = data.split_once(',')
            && let Some(mime) = header.strip_suffix(";base64")
            && let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data)
            && let Some(format) = ImageFormat::from_mime_type(mime)
        {
            return Some(gpui::ImageSource::Image(Arc::new(gpui::Image::from_bytes(
                format, bytes,
            ))));
        }
        return Some(gpui::ImageSource::from(SharedString::from(url.to_string())));
    }
    if let Some(data) = file.data.as_deref().filter(|data| !data.is_empty())
        && file.kind == AttachmentKind::Image
        && let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data)
    {
        let format = ImageFormat::from_mime_type(&file.mime_type).unwrap_or(ImageFormat::Png);
        return Some(gpui::ImageSource::Image(Arc::new(gpui::Image::from_bytes(
            format, bytes,
        ))));
    }
    file.path
        .as_deref()
        .filter(|path| !path.is_empty() && file.kind == AttachmentKind::Image)
        .map(|path| gpui::ImageSource::from(std::path::PathBuf::from(path)))
}

#[cfg(test)]
mod attachment_image_tests {
    use super::*;

    #[test]
    fn avif_attachment_preview_preserves_dimensions_and_color() {
        let file = Attachment {
            kind: AttachmentKind::Image,
            mime_type: "image/avif".into(),
            path: Some("/missing/on-this-desktop/image.avif".into()),
            data: Some(
                base64::engine::general_purpose::STANDARD.encode(include_bytes!(
                    "../../../../editor/tests/fixtures/red-blue.avif"
                )),
            ),
            ..Attachment::default()
        };
        let preview = load_avif_attachment(&file).unwrap();
        let pixels = image::load_from_memory(&preview.bytes).unwrap().to_rgba8();
        assert_eq!(pixels.dimensions(), (32, 16));
        let left = pixels.get_pixel(4, 8);
        let right = pixels.get_pixel(28, 8);
        assert!(left[0] > left[2]);
        assert!(right[2] > right[0]);
    }
}
