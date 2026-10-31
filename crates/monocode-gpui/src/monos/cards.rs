use super::*;
use gpui::{Div, ElementId, Stateful};
use serde_json::{Value, json};
pub(crate) struct CardAction(pub Value);
impl EventEmitter<CardAction> for crate::transcript::TranscriptView {}
/// The chat's one-line cards: a 12px-round outline that brightens on hover.
fn line_card(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .flex()
        .max_w(px(384.))
        .min_w_0()
        .items_center()
        .gap_2p5()
        .rounded_xl()
        .border_1()
        .border_color(content(0.1))
        .line_height(px(20.))
        .hover(|s| s.border_color(content(0.2)).bg(content(0.05)))
}
/// Suggested replies, one line card each. Picking one sends it; once any
/// reply follows, the picked one is checked and the rest step back.
fn choices(
    value: &Value,
    chosen: Option<&str>,
    cx: &mut Context<crate::transcript::TranscriptView>,
) -> AnyElement {
    let options: Vec<String> = value["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    let picked = chosen.and_then(|reply| options.iter().position(|o| o.trim() == reply.trim()));
    let answered = chosen.is_some();
    // Each card hugs its own label (items_start stops the stretch) and
    // wraps once it reaches 80% of the transcript's width.
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap_1p5()
        .children(options.into_iter().enumerate().map(|(ix, label)| {
            let checked = picked == Some(ix);
            let mark = if checked {
                div()
                    .size(px(16.))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(rgb(0x34d399))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("check", 11.).text_color(rgb(0x062e22)))
            } else {
                div()
                    .size(px(16.))
                    .flex_shrink_0()
                    .rounded_full()
                    .border(px(1.5))
                    .border_color(content(0.35))
                    .when(!answered, |d| {
                        d.group_hover("mono-choice", |s| s.border_color(content(0.7)))
                    })
            };
            let answer = label.clone();
            div()
                .id(("mono-choice", ix))
                .debug_selector(move || format!("mono-choice-{ix}"))
                .group("mono-choice")
                .role(Role::Button)
                .aria_label(label.clone())
                .aria_toggled(if checked {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
                .flex()
                .max_w(gpui::relative(0.8))
                .min_w_0()
                .items_center()
                .gap_2p5()
                .rounded_xl()
                .border_1()
                .border_color(content(if checked { 0.2 } else { 0.1 }))
                .line_height(px(20.))
                .py(px(6.))
                .pl(px(10.))
                .pr_3()
                .when(answered && !checked, |d| d.opacity(0.45))
                .when(!answered, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.border_color(content(0.2)).bg(content(0.05)))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(CardAction(json!({"type":"choice","text":answer})))
                        }))
                })
                .child(mark)
                .child(
                    div()
                        .min_w_0()
                        .text_size(px(13.))
                        .text_color(content(1.))
                        .child(label),
                )
        }))
        .into_any_element()
}
/// ArtifactCard: the document's icon and title, opening it in the reader.
fn artifact(value: &Value, cx: &mut Context<crate::transcript::TranscriptView>) -> AnyElement {
    let html = value["kind"] == "html";
    let label = if html { "visual" } else { "document" };
    let title = value["title"].as_str().unwrap_or("Document").to_string();
    let card = value.clone();
    div()
        .flex()
        .child(
            line_card(SharedString::from(format!(
                "mono-artifact-{}",
                value["id"].as_str().unwrap_or_default()
            )))
            .aria_label(format!("Open {label} {title}"))
            .debug_selector(|| "mono-artifact-card".into())
            .min_h(px(38.))
            .py_2()
            .pl_3()
            .pr(px(14.))
            .child(icon(if html { "chart-square" } else { "file" }, 16.).text_color(content(0.55)))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .text_color(content(1.))
                    .child(title),
            )
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(CardAction(card.clone())))),
        )
        .into_any_element()
}
/// LaunchedSessionCard: its status, title and the provider and project it
/// runs in. Without a known state it shows the plain chat mark.
pub(crate) fn launch(
    id: i64,
    view: Option<&crate::transcript::LaunchView>,
    cx: &mut Context<crate::transcript::TranscriptView>,
) -> AnyElement {
    let title = view.map_or("Agent session".to_string(), |v| v.title.clone());
    let (label, glyph, color) = match view.map(|v| v.state) {
        Some("working") => ("Working", "loader-03", content(0.55)),
        Some("input") => ("Needs input", "clock", rgb(0xfbbf24).into()),
        Some("draft") => ("Draft", "circle-dashed-hi", content(0.45)),
        Some("failed") => ("Failed", "alert-circle", rgb(0xf87171).into()),
        Some("stopped") => ("Stopped", "circle-x", content(0.45)),
        Some("unavailable") => ("Unavailable", "chatting", content(0.45)),
        Some(_) => ("Done", "check-circle", {
            let mut c: gpui::Hsla = rgb(0x34d399).into();
            c.a = 0.9;
            c
        }),
        None => ("", "chatting", content(0.45)),
    };
    // A working session's loader turns on the shared clock, at 15fps since
    // each step repaints the transcript it sits in.
    let mark = if glyph == "loader-03" {
        crate::pulse::looping(
            gpui::SharedString::from(format!("launch-loader-{id}")),
            std::time::Duration::from_secs(1),
            crate::pulse::Rate::Half,
            move |turn| {
                icon(glyph, 16.)
                    .text_color(color)
                    .with_transformation(gpui::Transformation::rotate(gpui::percentage(turn)))
            },
        )
        .into_any_element()
    } else {
        icon(glyph, 16.).text_color(color).into_any_element()
    };
    let project = view
        .map(|v| {
            std::path::Path::new(&v.cwd)
                .file_name()
                .map_or(v.cwd.clone(), |n| n.to_string_lossy().to_string())
        })
        .unwrap_or_default();
    let harness = view.map(|v| v.harness.clone()).unwrap_or_default();
    div()
        .flex()
        .child(
            line_card(("mono-launched-session", id as usize))
                .aria_label(if label.is_empty() {
                    format!("Open session {title}")
                } else {
                    format!("Open session {title}, {}", label.to_lowercase())
                })
                .debug_selector(move || format!("mono-launch-card-{id}"))
                .py(px(6.))
                .pl_3()
                .pr(px(6.))
                .child(mark)
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.))
                        .text_color(content(1.))
                        .child(title),
                )
                .when(view.is_some(), |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .max_w(px(160.))
                            .min_w_0()
                            .items_center()
                            .gap_1()
                            .rounded_full()
                            .border_1()
                            .border_color(content(0.1))
                            .bg(content(0.05))
                            .px_2()
                            .py(px(2.))
                            .text_size(px(12.))
                            .text_color(content(0.65))
                            .child(provider_icon(&harness, 14.))
                            .child(div().min_w_0().truncate().child(project)),
                    )
                })
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(CardAction(json!({"type":"session","sessionId":id})))
                })),
        )
        .into_any_element()
}
/// `chosen` is the reply sent after the card, if any, which answers it.
pub(crate) fn render(
    value: &Value,
    chosen: Option<&str>,
    cx: &mut Context<crate::transcript::TranscriptView>,
) -> AnyElement {
    let kind = value["type"].as_str().unwrap_or_default();
    match kind {
        "document" => return artifact(value, cx),
        "choices" => return choices(value, chosen, cx),
        "session" => {
            let id = value["sessionId"].as_i64().unwrap_or_default();
            let view = value["state"]
                .as_str()
                .map(|state| crate::transcript::LaunchView {
                    title: value["title"].as_str().unwrap_or("Agent session").into(),
                    state: match state {
                        "working" => "working",
                        "input" => "input",
                        "draft" => "draft",
                        "failed" => "failed",
                        "stopped" => "stopped",
                        _ => "done",
                    },
                    harness: value["harness"].as_str().unwrap_or_default().into(),
                    cwd: value["cwd"].as_str().unwrap_or_default().into(),
                });
            return launch(id, view.as_ref(), cx);
        }
        _ => {}
    }
    let title = value["title"]
        .as_str()
        .or_else(|| value["name"].as_str())
        .unwrap_or(match kind {
            "pr" => "Pull request",
            "session" => "Session",
            "habit" => "Suggested habit",
            _ => "",
        })
        .to_string();
    let card = value.clone();
    let glyph = match kind {
        "pr" => "git-pull-request",
        "session" => "message-square",
        "habit" => "clock",
        _ => "file",
    };
    div()
        .rounded_lg()
        .border_1()
        .border_color(content(0.12))
        .bg(content(0.035))
        .p_3()
        .max_w(px(480.))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(icon(glyph, 16.))
                .child(title),
        )
        .when_some(
            value["note"].as_str().or_else(|| value["summary"].as_str()),
            |d, n| {
                d.child(
                    div()
                        .text_size(px(12.))
                        .text_color(content(0.55))
                        .child(n.to_string()),
                )
            },
        )
        .when(kind == "session", |d| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(content(0.5))
                    .child(value["state"].as_str().unwrap_or("Saved").to_string()),
            )
        })
        .when(kind == "pr", |d| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(content(0.5))
                    .child(format!(
                        "{} #{} · {}",
                        value["repo"].as_str().unwrap_or_default(),
                        value["number"],
                        value["state"].as_str().unwrap_or("GitHub")
                    )),
            )
        })
        .when(kind == "habit", |d| {
            d.child(
                div().text_size(px(12.)).text_color(content(0.5)).child(
                    serde_json::from_value::<habits::Schedule>(value["schedule"].clone())
                        .map(|s| s.label())
                        .unwrap_or_default(),
                ),
            )
        })
        .child(
            panels::button(
                "mono-card-open",
                if kind == "habit" && value["started"] != true {
                    "Start habit"
                } else {
                    "Open"
                },
            )
            .bg(content(0.08))
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(CardAction(card.clone())))),
        )
        .into_any_element()
}
/// launchedSessionState: where a session the Mono started stands.
fn launched_state(session: &Conversation, cx: &App) -> &'static str {
    if session.needs_approval() {
        "input"
    } else if session.busy {
        "working"
    } else if !session.summary.has_messages {
        "draft"
    } else {
        match session.external_outcome(cx) {
            (false, status) if status.starts_with("Stopped") => "stopped",
            (false, _) => "failed",
            _ => "done",
        }
    }
}
impl Monos {
    pub(super) fn refresh_cards(&mut self, cx: &mut Context<Self>) {
        let Some(mono) = self.current().cloned() else {
            return;
        };
        let Some(chat) = self.chat() else {
            return;
        };
        let now = monocode_core::time::now_ms();
        // Saved summaries keep cards current without opening their providers.
        let summaries = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).mono_session_views(cx))
            .unwrap_or_default();
        // LaunchedSessionCard: each launch's live state, for the chat's cards.
        let launches = mono
            .launches
            .iter()
            .map(|launch| {
                let session = self
                    .owner
                    .upgrade()
                    .and_then(|o| o.read(cx).mono_session_entity(launch.session_id));
                let view = match session {
                    Some(session) => {
                        let session = session.read(cx);
                        crate::transcript::LaunchView {
                            title: session.summary.title.clone(),
                            state: launched_state(session, cx),
                            harness: session.summary.provider.clone(),
                            cwd: session.summary.cwd.clone(),
                        }
                    }
                    None => {
                        let summary = summaries.iter().find(|s| s.id == launch.session_id);
                        crate::transcript::LaunchView {
                            title: summary
                                .map(|s| s.title.clone())
                                .unwrap_or_else(|| launch.title.clone()),
                            state: summary.map_or("unavailable", |s| {
                                if s.needs_input {
                                    "input"
                                } else if s.busy {
                                    "working"
                                } else if !s.has_messages {
                                    "draft"
                                } else {
                                    "done"
                                }
                            }),
                            harness: launch.harness.clone(),
                            cwd: summary
                                .map(|s| s.cwd.clone())
                                .unwrap_or_else(|| launch.cwd.clone()),
                        }
                    }
                };
                (launch.session_id, view)
            })
            .collect();
        let transcript = chat.read(cx).transcript.clone();
        transcript.update(cx, |t, cx| t.set_launches(launches, cx));
        // Only mutable cards need a snapshot; never clone the conversation's
        // prose or tool output while refreshing a few card states.
        let cards = chat
            .read(cx)
            .mono_rows(cx)
            .iter()
            .enumerate()
            .filter(|(_, r)| r.role == "mono-card")
            .map(|(index, row)| (index, row.text.clone()))
            .collect::<Vec<_>>();
        for (index, text) in cards {
            let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            let original = value.clone();
            let Some(id) = value["id"].as_str().map(str::to_owned) else {
                continue;
            };
            if value["type"] == "session" {
                if let Some(session) = value["sessionId"].as_i64().and_then(|id| {
                    self.owner
                        .upgrade()
                        .and_then(|o| o.read(cx).mono_session_entity(id))
                }) {
                    let session = session.read(cx);
                    value["title"] = json!(session.summary.title);
                    value["state"] = json!(launched_state(session, cx));
                    value["harness"] = json!(session.summary.provider);
                    value["cwd"] = json!(session.summary.cwd);
                }
            } else if value["type"] == "habit" {
                value["started"] = json!(self.habits.iter().any(|h| h.id == format!("card-{id}")));
            } else if value["type"] == "document" {
                if let Some(document) = mono
                    .documents
                    .iter()
                    .find(|d| Some(d.id.as_str()) == value["documentId"].as_str())
                {
                    value["title"] = json!(document.title);
                    value["kind"] = json!(document.kind);
                    value["summary"] = json!(document.summary);
                }
            } else if value["type"] == "pr" {
                if self
                    .card_refresh
                    .get(&id)
                    .is_some_and(|at| now.saturating_sub(*at) < 60000)
                {
                    continue;
                }
                self.card_refresh.insert(id.clone(), now);
                let cwd = mono.projects.first().cloned().unwrap_or_else(|| ".".into());
                let repo = value["repo"].as_str().map(str::to_owned);
                let number = value["number"].as_u64().unwrap_or(0);
                let view = chat.clone();
                let work = cx
                    .background_executor()
                    .spawn(async move { mono::cards::pull_request(&cwd, repo.as_deref(), number) });
                cx.spawn(async move |_, cx| {
                    if let Ok(metadata) = work.await {
                        for key in ["title", "state", "url", "isDraft", "additions", "deletions"] {
                            value[key] = metadata[key].clone();
                        }
                        let _ = view.update(cx, |v, cx| v.mono_patch_card(index, &id, value, cx));
                    }
                })
                .detach();
                continue;
            }
            if value != original {
                let _ = chat.update(cx, |v, cx| v.mono_patch_card(index, &id, value, cx));
            }
        }
    }

    pub(super) fn hold_card(&mut self, session: i64, mono: &str, value: Value) {
        if let Some(run) = self.running.get_mut(&session) {
            run.cards.push(value);
        } else if let Some(mono) = self.roster.get_mut(mono) {
            mono.pending_reports.push(format!("mono-card:{value}"));
        }
    }

    pub(super) fn card_action(&mut self, mono: &str, value: &Value, cx: &mut Context<Self>) {
        if value["type"] != "choice" {
            self.active = Some(mono.into());
            if let Some(handle) = self.parent_window {
                let app: &mut App = cx;
                app.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, _| window.activate_window());
                });
            }
        }
        match value["type"].as_str().unwrap_or_default() {
            "choice" => {
                if let Some(text) = value["text"].as_str() {
                    self.enqueue(mono, text.into(), Vec::new(), false, cx);
                }
            }
            "session" => {
                if let Some(id) = value["sessionId"].as_i64() {
                    self.route = Some(Route::Session(id));
                    self.show_overview(cx);
                    self.overview_tab = OverviewTab::Sessions;
                    cx.notify();
                }
            }
            "document" => {
                if let Some(id) = value["documentId"].as_str() {
                    self.open_document(id, cx);
                    self.route = Some(Route::Document(id.into()));
                    self.show_overview(cx);
                    self.overview_tab = OverviewTab::Documents;
                    cx.notify();
                }
            }
            "habit" => {
                if value["started"] == true {
                    self.show_overview(cx);
                    self.overview_tab = OverviewTab::Habits;
                    self.route = Some(Route::Habit(format!(
                        "card-{}",
                        value["id"].as_str().unwrap_or_default()
                    )));
                    cx.notify();
                    return;
                }
                let result = serde_json::from_value::<habits::Schedule>(value["schedule"].clone())
                    .map_err(|e| e.to_string())
                    .and_then(|schedule| {
                        Habit::new(
                            value["name"].as_str().unwrap_or_default().into(),
                            value["instructions"].as_str().unwrap_or_default().into(),
                            schedule,
                            monocode_core::time::now_ms(),
                        )
                    });
                match result {
                    Ok(mut habit) => {
                        habit.id = format!("card-{}", value["id"].as_str().unwrap_or_default());
                        let root = self.root.clone();
                        let id = mono.to_string();
                        self.file_work(
                            id.clone(),
                            move || {
                                habits::update(&root, &id, |all| {
                                    if !all.iter().any(|h| h.id == habit.id) {
                                        all.push(habit.clone());
                                    }
                                    Ok(())
                                })
                            },
                            cx,
                        );
                        self.show_overview(cx);
                        self.overview_tab = OverviewTab::Habits;
                        self.route = None;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            "pr" => {
                if let Some(url) = value["url"]
                    .as_str()
                    .filter(|s| s.starts_with("https://github.com/"))
                {
                    crate::inbox::open_link_later(url, cx);
                } else if let (Some(repo), Some(number)) =
                    (value["repo"].as_str(), value["number"].as_u64())
                {
                    crate::inbox::open_link_later(
                        &format!("https://github.com/{repo}/pull/{number}"),
                        cx,
                    );
                }
            }
            _ => (),
        }
        cx.notify();
    }
}
