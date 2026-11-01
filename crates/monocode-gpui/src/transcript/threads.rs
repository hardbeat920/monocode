use super::*;
use monocode_core::tool::threads::ThreadRun;

impl gpui::EventEmitter<crate::conversation::OpenThread> for TranscriptView {}
impl gpui::EventEmitter<crate::conversation::CloseThreads> for TranscriptView {}

/// Another thread, as the messages it sent here name it.
#[derive(Clone, PartialEq)]
pub(crate) struct Sender {
    pub(crate) title: SharedString,
    pub(crate) provider: String,
}

impl TranscriptView {
    /// The thread that sent the prompt at `start`, when another thread did.
    pub(super) fn prompt_sender(&self, start: usize) -> Option<i64> {
        // Receipts are keyed by the first row after the prompt block.
        let turn = (start..self.source.len())
            .find(|&at| !self.source[at].is_prompt())
            .unwrap_or(self.source.len());
        self.turns
            .get(&turn)?
            .sender
            .filter(|id| self.senders.contains_key(id))
    }
    /// The line above another thread's message: its harness, its title,
    /// which opens it, and whether it started this thread.
    pub(super) fn sender_label(
        &self,
        start: usize,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let id = self.prompt_sender(start)?;
        let sender = self.senders.get(&id)?;
        let opened = !self.source[..start]
            .iter()
            .any(|row| row.first && row.role == "user");
        let title = sender.title.clone();
        Some(
            div()
                .w_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .max_w(px(COLUMN))
                        .px_4()
                        .pt_2()
                        .flex()
                        .when(self.chat_layout, |d| d.justify_end())
                        .child(
                            div()
                                .debug_selector(move || format!("thread-sender-{start}"))
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(11.))
                                .line_height(px(16.))
                                .text_color(content(0.45))
                                .child(provider_icon(&sender.provider, 14.).flex_shrink_0())
                                .child(
                                    div()
                                        .id(("thread-sender", start))
                                        .role(gpui::Role::Button)
                                        .aria_label(format!("Open {title}"))
                                        .min_w_0()
                                        .truncate()
                                        .cursor_pointer()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(content(0.8))
                                        .hover(|s| s.text_color(content(1.)))
                                        .child(title)
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(crate::conversation::OpenThread(id))
                                        })),
                                )
                                .child(div().flex_shrink_0().child(if opened {
                                    if self.subagent {
                                        "· started this subagent"
                                    } else {
                                        "· started this thread"
                                    }
                                } else {
                                    "· message"
                                })),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// How a child thread reads in the spawn tree.
#[derive(Clone, Copy, PartialEq)]
enum Tone {
    Running,
    Attention,
    Failed,
    Done,
}
impl Tone {
    fn color(self) -> gpui::Hsla {
        gpui::rgb(match self {
            Tone::Running => 0x7dd3fc,
            Tone::Attention => 0xfbbf24,
            Tone::Failed => 0xf87171,
            Tone::Done => 0x6ee7b7,
        })
        .into()
    }
    /// Only settled children can be removed from the tree.
    fn finished(self) -> bool {
        matches!(self, Tone::Failed | Tone::Done)
    }
}
struct Child {
    id: Option<i64>,
    title: String,
    harness: String,
    model: String,
    label: &'static str,
    tone: Tone,
}

impl TranscriptView {
    fn thread_run_at(&self, ix: usize) -> Option<(&ToolCall, ThreadRun)> {
        let Body::Tool { call, .. } = &self.rows.get(ix)?.body else {
            return None;
        };
        match call.panel()? {
            Panel::Thread(run) => Some((call, run)),
            _ => None,
        }
    }
    fn remeasure_threads(&mut self, ids: &HashSet<i64>) {
        let rows: Vec<usize> = (0..self.rows.len())
            .filter(|&ix| {
                self.thread_run_at(ix)
                    .is_some_and(|(_, run)| run.id.is_some_and(|id| ids.contains(&id)))
            })
            .collect();
        for ix in rows {
            self.list.remeasure_items(ix..ix + 1);
        }
    }
    /// The visible children of the spawn tree around row `ix`: adjacent child
    /// rows form one tree under the lead, minus those the user removed.
    fn thread_tree(&self, ix: usize) -> Vec<(usize, Child)> {
        let mut from = ix;
        while from > 0 && self.thread_run_at(from - 1).is_some() {
            from -= 1;
        }
        let mut to = ix;
        while self.thread_run_at(to + 1).is_some() {
            to += 1;
        }
        (from..=to)
            .filter_map(|at| {
                let (call, run) = self.thread_run_at(at)?;
                let child = child(self, call, &run);
                (at == ix || !child.id.is_some_and(|id| self.thread_hidden(id)))
                    .then_some((at, child))
            })
            .collect()
    }
    /// One child's details open at a time, so a long tree never expands whole.
    pub(crate) fn toggle_thread(&mut self, id: i64, start: usize, cx: &mut Context<Self>) {
        let _ = start;
        let opening = !self.thread_expanded.contains(&id);
        let mut touched: HashSet<i64> = std::mem::take(&mut self.thread_expanded);
        touched.insert(id);
        if opening {
            self.thread_expanded.insert(id);
        }
        self.list.pause_following_tail();
        self.remeasure_threads(&touched);
        cx.notify();
    }
    /// Removed here, or closed by the lead or from this tree.
    pub(crate) fn thread_hidden(&self, id: i64) -> bool {
        self.thread_dismissed.contains(&id)
            || self
                .thread_states
                .get(&id)
                .is_some_and(|state| state["closed"] == true)
    }
    pub(crate) fn dismiss_threads(&mut self, ids: &[i64], cx: &mut Context<Self>) {
        cx.emit(crate::conversation::CloseThreads(ids.to_vec()));
        let ids: HashSet<i64> = ids.iter().copied().collect();
        for id in &ids {
            self.thread_dismissed.insert(*id);
            self.thread_expanded.remove(id);
        }
        self.remeasure_threads(&ids);
        cx.notify();
    }
    pub(crate) fn set_thread_states(
        &mut self,
        states: HashMap<i64, serde_json::Value>,
        cx: &mut Context<Self>,
    ) {
        if self.thread_states != states {
            self.thread_states = states;
            for (ix, row) in self.rows.iter().enumerate() {
                if matches!(&row.body, Body::Tool{call,..} if call.thread_action().is_some()) {
                    self.list.remeasure_items(ix..ix + 1);
                }
            }
            cx.notify();
        }
    }
}

fn child(view: &TranscriptView, call: &ToolCall, run: &ThreadRun) -> Child {
    let state = view.thread_states.get(&run.id.unwrap_or_default());
    let title = state
        .and_then(|v| v["title"].as_str())
        .unwrap_or(&run.title)
        .to_owned();
    let harness = state
        .and_then(|v| v["harness"].as_str())
        .or(run.harness.as_deref())
        .filter(|harness| !harness.is_empty())
        // thread_start defaults to the lead's own harness.
        .unwrap_or(&view.provider)
        .to_owned();
    let model = state
        .and_then(|v| v["model"].as_str())
        .or(run.model.as_deref())
        .unwrap_or_default()
        .to_owned();
    let status =
        state
            .and_then(|v| v["status"].as_str())
            .unwrap_or(if call.state == ToolState::Running {
                "starting"
            } else if call.state == ToolState::Failed {
                "failed"
            } else {
                "idle"
            });
    let (label, tone) = match status {
        "needsInput" => ("Needs input", Tone::Attention),
        "working" => ("Working", Tone::Running),
        "starting" => ("Starting", Tone::Running),
        "failed" => ("Failed", Tone::Failed),
        _ if state.is_some_and(|v| v["success"] == false) => ("Failed", Tone::Failed),
        _ if state.is_some_and(|v| v["success"].is_null()) => ("Saved", Tone::Done),
        _ => ("Idle", Tone::Done),
    };
    Child {
        id: run.id,
        title,
        harness,
        model,
        label,
        tone,
    }
}

fn lead_header(
    view: &TranscriptView,
    tree: &[(usize, Child)],
    start: usize,
    cx: &mut Context<TranscriptView>,
) -> gpui::AnyElement {
    let count = |f: fn(Tone) -> bool| tree.iter().filter(|(_, c)| f(c.tone)).count();
    let working = count(|t| matches!(t, Tone::Running | Tone::Attention));
    let failed = count(|t| t == Tone::Failed);
    let mut summary = vec![format!(
        "{} agent{}",
        tree.len(),
        if tree.len() == 1 { "" } else { "s" }
    )];
    if working > 0 {
        summary.push(format!("{working} working"));
    }
    if failed > 0 {
        summary.push(format!("{failed} failed"));
    }
    let finished: Vec<i64> = tree
        .iter()
        .filter(|(_, c)| c.tone.finished())
        .filter_map(|(_, c)| c.id)
        .collect();
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_1p5()
        .py_1()
        .child(provider_icon(&view.provider, 16.))
        .child(
            div()
                .text_size(px(14.))
                .text_color(content(0.8))
                .child("Lead"),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(11.))
                .text_color(content(0.45))
                .child(summary.join(" · ")),
        )
        .when(!finished.is_empty(), |d| {
            d.child(
                div()
                    .id(("thread-clear", start))
                    .role(gpui::Role::Button)
                    .aria_label("Clear finished agents")
                    .px_2()
                    .py_0p5()
                    .rounded(px(6.))
                    .text_size(px(11.))
                    .text_color(content(0.55))
                    .cursor_pointer()
                    .hover(|s| s.bg(content(0.08)).text_color(content(0.85)))
                    .child("Clear finished")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.dismiss_threads(&finished, cx);
                    })),
            )
        })
        .into_any_element()
}

pub(super) fn render(
    view: &mut TranscriptView,
    call: &ToolCall,
    run: &ThreadRun,
    start: usize,
    cx: &mut Context<TranscriptView>,
) -> gpui::AnyElement {
    let id = run.id.unwrap_or_default();
    if run.id.is_some_and(|id| view.thread_hidden(id)) {
        return div()
            .id(("live-thread", start))
            .h(px(0.))
            .into_any_element();
    }
    let Child {
        title,
        harness,
        model,
        label,
        tone,
        ..
    } = child(view, call, run);
    let tree = view
        .rows
        .iter()
        .position(|row| row.key.0 == start)
        .map(|ix| view.thread_tree(ix))
        .unwrap_or_default();
    let place = tree
        .iter()
        .position(|(_, c)| c.id == run.id && run.id.is_some())
        .unwrap_or(0);
    let (first, last) = (place == 0, place + 1 >= tree.len());
    let header = first.then(|| lead_header(view, &tree, start, cx));
    let remove_title = title.clone();
    let hover_group = SharedString::from(format!("thread-row-{start}"));
    let state = view.thread_states.get(&id);
    let open = view.thread_expanded.contains(&id);
    let mut exchange = Vec::new();
    if open {
        let mut last_reply = String::new();
        for row in &view.source {
            if row.role != "tool" {
                continue;
            }
            let Ok(tool) = serde_json::from_str::<ToolCall>(&row.text) else {
                continue;
            };
            match tool.thread_action() {
                Some("start" | "send") => {
                    if let Some(Panel::Thread(thread)) = tool.panel()
                        && thread.id == run.id
                        && !thread.prompt.is_empty()
                    {
                        exchange.push(("Lead", thread.prompt));
                        last_reply.clear();
                    }
                }
                Some("wait") => {
                    let result = tool.thread_result();
                    if result["threadId"].as_i64() == run.id
                        && result["status"] == "idle"
                        && let Some(reply) = result["reply"]
                            .as_str()
                            .filter(|s| !s.is_empty() && *s != last_reply)
                    {
                        exchange.push(("Child", reply.to_owned()));
                        last_reply = reply.to_owned();
                    }
                }
                _ => (),
            }
        }
        if let Some(reply) = state
            .and_then(|v| v["reply"].as_str())
            .filter(|s| !s.is_empty() && *s != last_reply)
        {
            exchange.push(("Child", reply.to_owned()));
        } else if call.state == ToolState::Failed {
            exchange.push(("Error", call.output.clone()));
        }
    }
    let line = content(0.1);
    div()
        .id(("live-thread", start))
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .children(header)
        .child(
            div()
                .relative()
                .pl(px(22.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .top(px(-4.))
                        .w(px(12.))
                        .h(px(20.))
                        .border_l_1()
                        .border_b_1()
                        .border_color(line)
                        .rounded_bl(px(7.)),
                )
                .when(!last, |d| {
                    d.child(
                        div()
                            .absolute()
                            .left(px(8.))
                            .top(px(16.))
                            .bottom_0()
                            .border_l_1()
                            .border_color(line),
                    )
                })
                .child(
                    div()
                        .group(hover_group.clone())
                        .flex()
                        .items_center()
                        .gap_2()
                        .py_1()
                        .child(
                            div()
                                .id(("thread-toggle", start))
                                .role(gpui::Role::Button)
                                .aria_label(format!(
                                    "{} {title}'s exchange",
                                    if open { "Hide" } else { "Show" }
                                ))
                                .aria_expanded(open)
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_1p5()
                                .py_1()
                                .rounded(px(6.))
                                .cursor_pointer()
                                .hover(|s| s.bg(content(0.06)))
                                .child(provider_icon(&harness, 16.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(14.))
                                        .text_color(content(0.8))
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(content(0.45))
                                        .child(model),
                                )
                                .child(
                                    div()
                                        .px_1p5()
                                        .rounded_full()
                                        .border_1()
                                        .border_color(tone.color().opacity(0.25))
                                        .bg(tone.color().opacity(0.1))
                                        .text_size(px(11.))
                                        .text_color(tone.color())
                                        .child(label),
                                )
                                .child(
                                    icon(
                                        if open {
                                            "chevron-down"
                                        } else {
                                            "chevron-right"
                                        },
                                        12.,
                                    )
                                    .text_color(content(0.4)),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_thread(id, start, cx);
                                })),
                        )
                        .when(tone.finished() && run.id.is_some(), |d| {
                            d.child(
                                div()
                                    .id(("thread-remove", start))
                                    .role(gpui::Role::Button)
                                    .aria_label(format!("Remove {remove_title}"))
                                    .p_1()
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .opacity(0.)
                                    .group_hover(hover_group.clone(), |s| s.opacity(1.))
                                    .hover(|s| s.bg(content(0.08)))
                                    .child(icon("close", 12.).text_color(content(0.55)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.dismiss_threads(&[id], cx);
                                    })),
                            )
                        })
                        .when(run.id.is_some(), |d| {
                            d.child(
                                div()
                                    .id(("thread-open", start))
                                    .role(gpui::Role::Button)
                                    .aria_label("Open child thread")
                                    .p_1()
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(content(0.08)))
                                    .child(icon("arrow-up-right", 14.).text_color(content(0.5)))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(crate::conversation::OpenThread(id))
                                    })),
                            )
                        }),
                )
                .when(open, |d| {
                    d.child(
                        div()
                            .pl(px(20.))
                            .border_l_1()
                            .border_color(content(0.1))
                            .children(exchange.into_iter().map(|(speaker, text)| {
                                div()
                                    .py_1()
                                    .text_size(px(12.))
                                    .line_height(px(18.))
                                    .child(div().text_color(content(0.45)).child(speaker))
                                    .child(div().text_color(content(0.8)).child(text))
                            })),
                    )
                }),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }

    #[gpui::test]
    fn only_prompts_another_thread_sent_name_their_sender(cx: &mut TestAppContext) {
        let lead = 7;
        let view = cx.new(|cx| {
            let mut view = TranscriptView::new();
            view.senders = [(
                lead,
                Sender {
                    title: "Refactor provider lifecycle".into(),
                    provider: "claude".into(),
                },
            )]
            .into();
            view.turns = [(
                1,
                monocode_core::agent::TurnInfo {
                    sender: Some(lead),
                    ..Default::default()
                },
            )]
            .into();
            view.restore_history(
                vec![
                    row("user", "Audit the parked paths"),
                    row("assistant", "Three paths."),
                    row("user", "Check restart too"),
                    row("assistant", "Safe."),
                ],
                cx,
            );
            view
        });
        view.update(cx, |view, cx| {
            assert_eq!(view.prompt_sender(0), Some(lead));
            assert!(view.sender_label(0, cx).is_some());
            assert_eq!(view.prompt_sender(2), None, "the user's own message");
            assert!(view.sender_label(2, cx).is_none());

            // A live turn the lead opened names it; a steer the user adds does not.
            view.live_sender = Some(lead);
            view.streaming(true, cx);
            view.patch(
                4,
                vec![row("user", "Draft path 2"), row("assistant", "")],
                cx,
            );
            assert_eq!(view.prompt_sender(4), Some(lead));
            view.patch(6, vec![row("user", "Keep it small")], cx);
            assert_eq!(view.prompt_sender(6), None);

            // A sender this thread can't name is not shown.
            view.senders.clear();
            assert_eq!(view.prompt_sender(0), None);
        });
    }
}
