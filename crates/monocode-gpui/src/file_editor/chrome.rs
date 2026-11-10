use super::FileEditor;
use crate::{
    file_icons::file_type_icon,
    right_panel::{stroke, tabular},
    theme::*,
};
use gpui::{AnyElement, App, Context, Role, div, prelude::*, px};
use std::path::{Component, Path};

const HEADER_HEIGHT: f32 = 30.;
const CONTROL_HEIGHT: f32 = 20.;
const CONTROL_INSET: f32 = (HEADER_HEIGHT - 1. - CONTROL_HEIGHT) / 2.;

impl FileEditor {
    pub(super) fn path_header(&self, controls: Option<AnyElement>, cx: &App) -> AnyElement {
        let path = Path::new(&self.path);
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        let git = self.git.read(cx);
        let root = [git.root.as_str(), self.cwd.as_str()]
            .into_iter()
            .filter(|root| !root.is_empty() && *root != "~")
            .map(Path::new)
            .filter(|root| path.strip_prefix(root).is_ok())
            .max_by_key(|root| root.as_os_str().len());
        let relative = root
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path);
        let folders = relative
            .parent()
            .into_iter()
            .flat_map(Path::components)
            .filter_map(|component| match component {
                Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" › ");
        let folders = if folders.is_empty() {
            root.and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            folders
        };
        div()
            .id("file-breadcrumbs")
            .debug_selector(|| "file-breadcrumbs".into())
            .role(Role::Navigation)
            .aria_label("File path")
            .aria_description(self.path.clone())
            .h(px(HEADER_HEIGHT))
            .flex_shrink_0()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .pl_3()
            .pr(px(CONTROL_INSET))
            .border_b_1()
            .border_color(stroke())
            .text_size(px(11.))
            .line_height(px(16.))
            .child(file_type_icon(&name, false, false, 16.))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(!folders.is_empty(), |d| {
                        d.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(content(0.45))
                                .child(folders),
                        )
                        .child(icon("chevron-right", 12.).text_color(content(0.3)))
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex_shrink(0.1)
                            .truncate()
                            .text_color(content(0.85))
                            .child(name.into_owned()),
                    ),
            )
            .children(controls)
            .into_any_element()
    }

    pub(super) fn word_wrap_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let wrap = self.input.read(cx).word_wrap();
        div()
            .id("file-word-wrap")
            .debug_selector(|| "file-word-wrap".into())
            .role(Role::Button)
            .aria_label("Word wrap")
            .aria_description("Toggle word wrap (Alt+Z)")
            .aria_toggled(if wrap {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
            .cursor_pointer()
            .flex_shrink_0()
            .h(px(CONTROL_HEIGHT))
            .px_2()
            .flex()
            .items_center()
            .rounded_md()
            .text_size(px(11.))
            .line_height(px(14.))
            .text_color(content(if wrap { 1. } else { 0.45 }))
            .when(wrap, |d| d.bg(content(0.15)))
            .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
            .child("Wrap")
            .on_click(cx.listener(|this, _, _, cx| this.toggle_word_wrap(cx)))
            .into_any_element()
    }

    pub(super) fn view_mode_control(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("file-mode")
            .role(Role::TabList)
            .aria_label("File view")
            .h(px(CONTROL_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .rounded_md()
            .bg(content(0.1))
            .p(px(2.))
            .children(
                [("Preview", true), ("Source", false)]
                    .into_iter()
                    .map(|(label, preview)| {
                        div()
                            .id(label)
                            .role(Role::Tab)
                            .aria_label(label)
                            .aria_selected(self.preview == preview)
                            .cursor_pointer()
                            .h_full()
                            .flex()
                            .items_center()
                            .px_2()
                            .rounded(px(4.))
                            .text_size(px(11.))
                            .line_height(px(14.))
                            .text_color(content(if self.preview == preview { 1. } else { 0.45 }))
                            .when(self.preview == preview, |d| d.bg(content(0.15)))
                            .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                            .child(label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.preview != preview {
                                    this.toggle_preview(window, cx);
                                }
                            }))
                    }),
            )
            .into_any_element()
    }

    pub(super) fn status_footer(&self, cx: &App) -> AnyElement {
        let input = self.input.read(cx);
        let (line, column) = input.cursor_location();
        let lines = input.editor_line_count();
        let document = self.document.as_ref();
        let ending = match document {
            Some(document) if document.crlf => "CRLF",
            Some(document) if document.cr => "CR",
            _ => "LF",
        };
        let encoding = if document.is_some_and(|document| document.bom) {
            "UTF-8 BOM"
        } else {
            "UTF-8"
        };
        let position = format!("Ln {line}, Col {column}");
        let details = format!(
            "{lines} {} · {encoding} · {ending}",
            if lines == 1 { "line" } else { "lines" }
        );
        div()
            .id("file-editor-status")
            .debug_selector(|| "file-editor-status".into())
            .role(Role::Status)
            .aria_label(format!("{position} · {details}"))
            .h(px(24.))
            .flex_shrink_0()
            .min_w_0()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .border_t_1()
            .border_color(stroke())
            .font_features(tabular())
            .text_size(px(10.))
            .line_height(px(16.))
            .text_color(content(0.45))
            .child(div().flex_shrink_0().child(position))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_right()
                    .child(details),
            )
            .into_any_element()
    }
}
