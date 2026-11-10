//! Compact project tool tabs shared by the rounded and expanded layouts.
use crate::{right_panel::RightTab, theme::*};
use gpui::{App, MouseButton, Role, Stateful, Window, div, prelude::*, px};
use std::time::Instant;

const TAB_TEXT_SIZE: f32 = 12.;

pub(crate) struct TabReveal<T = RightTab> {
    open: T,
    closing: T,
    start: Instant,
}

impl<T: Copy + PartialEq> TabReveal<T> {
    pub(crate) fn new(open: T, closing: T) -> Self {
        Self {
            open,
            closing,
            start: Instant::now(),
        }
    }

    pub(crate) fn progress(&self, current: T, window: &mut Window, cx: &App) -> Option<(T, f32)> {
        if self.open != current || cx.reduce_motion() {
            return None;
        }
        // TabButton's duration-200 ease-out on the label's grid track.
        let t = self.start.elapsed().as_secs_f32() / 0.2;
        if t < 1. {
            window.request_animation_frame();
        }
        Some((
            self.closing,
            crate::reveal::bezier(t.clamp(0., 1.), 0., 0., 0.58, 1.),
        ))
    }
}

pub(crate) struct TabLabel {
    text: &'static str,
    count: usize,
    max_width: Option<f32>,
}
impl From<&'static str> for TabLabel {
    fn from(text: &'static str) -> Self {
        Self {
            text,
            count: 0,
            max_width: None,
        }
    }
}
impl From<(&'static str, usize)> for TabLabel {
    fn from((text, count): (&'static str, usize)) -> Self {
        Self {
            text,
            count,
            max_width: None,
        }
    }
}
impl TabLabel {
    /// Space for the label track, including its leading gap and optional badge.
    /// Keep this separate from the selection animation so a narrow label uses
    /// an ellipsis rather than exposing a clipped slice of its full text.
    pub(crate) fn max_width(mut self, width: f32) -> Self {
        self.max_width = Some(width.max(0.));
        self
    }
}

/// An icon whose label unfolds only while selected.
pub(crate) fn tab_button(
    id: &'static str,
    label: impl Into<TabLabel>,
    path: &'static str,
    selected: bool,
    dot: bool,
    open: f32,
    window: &mut Window,
) -> Stateful<gpui::Div> {
    let label = label.into();
    let count_width = if label.count > 0 {
        (label_width(&label.count.to_string(), window) + 8.).max(16.) + 6.
    } else {
        0.
    };
    let natural_width = label_width(label.text, window) + 6. + count_width;
    let width = label
        .max_width
        .map_or(natural_width, |max| max.min(natural_width));
    let open = if width > 0. { open } else { 0. };
    let ink = content(if selected { 1. } else { 0.5 });
    div()
        .h(px(24.))
        .rounded_md()
        .flex()
        .items_center()
        .bg(content(if selected { 0.1 } else { 0. }))
        .opacity(if selected { 1. } else { 0.65 })
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Tab)
        .aria_label(label.text)
        .aria_selected(selected)
        .group(id)
        .cursor_pointer()
        .px(px(6. + 2. * open))
        .flex_shrink_0()
        .text_size(px(TAB_TEXT_SIZE))
        .line_height(px(20.))
        .text_color(ink)
        .hover(|s| {
            s.opacity(1.)
                .bg(if selected {
                    title_tab_fill(true, true)
                } else {
                    content(0.)
                })
                .text_color(content(1.))
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .relative()
                .size(px(14.))
                .flex_shrink_0()
                .child(icon(path, 14.).text_color(ink).when(!selected, |i| {
                    i.group_hover(id, |s| s.text_color(content(1.)))
                }))
                .when(dot, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(-2.))
                            .left(px(10.))
                            .size(px(6.))
                            .rounded_full()
                            .bg(accent_color()),
                    )
                }),
        )
        .when(open > 0., |d| {
            d.child(
                div()
                    .w(px(width * open))
                    .overflow_hidden()
                    .opacity(open)
                    .child(
                        div()
                            .w(px(width))
                            .pl(px(6.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .whitespace_nowrap()
                            .child(
                                div()
                                    .debug_selector(move || format!("{id}-label"))
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(label.text),
                            )
                            .when(label.count > 0, |d| {
                                d.child(
                                    div()
                                        .flex_shrink_0()
                                        .min_w(px(16.))
                                        .h(px(16.))
                                        .px(px(4.))
                                        .rounded(px(4.))
                                        .bg(accent_color())
                                        .text_color(gpui::white())
                                        .text_size(px(10.))
                                        .line_height(px(16.))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .font_features(crate::right_panel::tabular())
                                        .text_center()
                                        .child(label.count.to_string()),
                                )
                            }),
                    ),
            )
        })
}

fn label_width(label: &str, window: &mut Window) -> f32 {
    let mut font = window.text_style().font();
    font.weight = gpui::FontWeight::NORMAL;
    let run = gpui::TextRun {
        len: label.len(),
        font,
        color: gpui::Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(label.to_owned().into(), px(TAB_TEXT_SIZE), &[run], None)
        .width
        .as_f32()
        .ceil()
}
