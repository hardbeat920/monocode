//! A composer `!command`: its line, live output and exit, above the message
//! that will carry it to the agent.
use super::*;
use monocode_core::shell_run::{ShellRun, State};

/// Collapsed output shows this many lines.
const LINES: usize = 12;

impl TranscriptView {
    pub(super) fn shell_row(
        &self,
        run: &ShellRun,
        message_start: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let open = self.expanded.contains(&message_start);
        let output = run.output.trim_end();
        let lines = output.lines().count();
        let shown = if open || lines <= LINES {
            output.to_owned()
        } else {
            let skip = lines - LINES;
            output.lines().skip(skip).collect::<Vec<_>>().join("\n")
        };
        let running = run.state == State::Running;
        let (status, status_color) = match (run.state, run.exit) {
            (State::Running, _) => (String::new(), content(0.5)),
            (State::Cancelled, _) => ("Stopped".into(), content(0.5)),
            (State::Completed, _) => (duration(run.duration_ms), content(0.45)),
            (State::Failed, Some(code)) => (format!("Exit {code}"), rgb(0xf87171).into()),
            (State::Failed, None) => ("Failed".into(), rgb(0xf87171).into()),
        };
        let id = run.id.clone();
        let copy = format!("$ {}\n{output}", run.command);
        div()
            .w_full()
            .min_w_0()
            .py_2()
            .child(
                div()
                    .id(("shell-run", message_start))
                    .role(Role::Group)
                    .aria_label(format!("Shell command: {}", run.command))
                    .w_full()
                    .min_w_0()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(content(0.08))
                    .bg(content(0.03))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(32.))
                            .pl(px(10.))
                            .pr(px(4.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(if running {
                                div()
                                    .size(px(14.))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(crate::session_mark::spinner(
                                        SharedString::from(format!("shell-loader:{}", run.id)),
                                        crate::pulse::Rate::Half,
                                    ))
                                    .into_any_element()
                            } else {
                                icon("terminal", 14.)
                                    .text_color(crate::input::Input::shell_color())
                                    .into_any_element()
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(crate::fonts::mono())
                                    .text_size(px(12.5))
                                    .text_color(content(0.9))
                                    .child(run.command.clone()),
                            )
                            .when(!status.is_empty(), |d| {
                                d.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(11.))
                                        .text_color(status_color)
                                        .child(status),
                                )
                            })
                            .child(if running {
                                icon_button("shell-stop", "Stop command", "square")
                                    .on_click(move |_, _, _| {
                                        monocode_core::shell_run::cancel(&id);
                                    })
                                    .into_any_element()
                            } else {
                                icon_button(
                                    "shell-copy",
                                    "Copy command and output",
                                    "response-copy",
                                )
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                })
                                .into_any_element()
                            }),
                    )
                    .when(!shown.is_empty(), |d| {
                        d.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .px(px(10.))
                                .pt(px(2.))
                                .pb(px(8.))
                                .border_t_1()
                                .border_color(content(0.06))
                                .font_family(crate::fonts::mono())
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(content(0.7))
                                .child(shown),
                        )
                    })
                    .when(lines > LINES, |d| {
                        d.child(
                            div()
                                .id(("shell-expand", message_start))
                                .role(Role::Button)
                                .aria_expanded(open)
                                .px(px(10.))
                                .pb(px(8.))
                                .text_size(px(11.))
                                .text_color(content(0.5))
                                .hover(|s| s.text_color(content(0.9)))
                                .cursor_pointer()
                                .child(if open {
                                    "Show less".to_owned()
                                } else {
                                    format!("Show all {lines} lines")
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle(message_start, cx);
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}

fn duration(ms: Option<u64>) -> String {
    match ms {
        Some(ms) if ms >= 60_000 => format!("{}m {}s", ms / 60_000, ms / 1000 % 60),
        Some(ms) if ms >= 1000 => format!("{:.1}s", ms as f64 / 1000.),
        Some(ms) => format!("{ms}ms"),
        None => String::new(),
    }
}
