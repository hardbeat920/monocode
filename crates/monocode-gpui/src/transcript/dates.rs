//! AgentTranscript's DaySeparator and opensNewStretch, in the reader's local time.
//! Like a messaging app, a badge marks each new day; times stay on messages.
use super::*;
use chrono::{DateTime, Datelike, Local, NaiveDate};

pub(super) fn full_stamp(at: u64) -> String {
    i64::try_from(at)
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .map(|date| date.with_timezone(&Local).format("%c").to_string())
        .unwrap_or_default()
}

fn local_day(at: u64) -> Option<NaiveDate> {
    DateTime::from_timestamp_millis(i64::try_from(at).ok()?)
        .map(|date| date.with_timezone(&Local).date_naive())
}

pub(crate) fn opens_stretch(at: Option<u64>, previous: Option<u64>) -> bool {
    let Some(at) = at.filter(|at| local_day(*at).is_some()) else {
        return false;
    };
    previous.is_none_or(|previous| local_day(at) != local_day(previous))
}

fn day_label(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        "Today".into()
    } else if Some(date) == today.pred_opt() {
        "Yesterday".into()
    } else if date.year() == today.year() {
        date.format("%a, %b %-d").to_string()
    } else {
        date.format("%a, %b %-d, %Y").to_string()
    }
}

fn separator(at: u64) -> Option<gpui::AnyElement> {
    let day = day_label(local_day(at)?, Local::now().date_naive());
    Some(
        div()
            .id(SharedString::from(format!("mono-day-separator-{at}")))
            .debug_selector(|| "mono-day-separator".into())
            .role(Role::Label)
            .aria_label(day.clone())
            .w_full()
            .flex()
            .justify_center()
            .px_4()
            .pt_6()
            .pb_3()
            .text_size(px(12.))
            .line_height(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded_full()
                    .bg(content(0.07))
                    .px_3()
                    .py_1()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(content(0.75))
                    .child(day),
            )
            .into_any_element(),
    )
}

/// When the message before `start` was sent: the previous prompt, or the
/// conversation's first row.
pub(crate) fn previous_send(source: &[Row], start: usize) -> Option<u64> {
    source[..start]
        .iter()
        .enumerate()
        .rfind(|(ix, row)| row.first && (row.is_prompt() || *ix == 0))
        .and_then(|(_, row)| row.sent_at)
}

/// A reply posted without a prompt, such as a habit report, on a later day
/// than the message before it. It continues the turn but opens its own day.
pub(super) fn opens_later_day(source: &[Row], start: usize) -> bool {
    let row = &source[start];
    let Some(day) = row.sent_at.and_then(local_day) else {
        return false;
    };
    row.role == "assistant"
        && row.first
        && source[..start]
            .iter()
            .rev()
            .filter(|r| r.first)
            .find_map(|r| r.sent_at)
            .and_then(local_day)
            .is_some_and(|previous| previous != day)
}

pub(crate) fn mono_opens_stretch(source: &[Row], start: usize) -> bool {
    opens_stretch(source[start].sent_at, previous_send(source, start))
}

impl TranscriptView {
    fn previous_send(&self, start: usize) -> Option<u64> {
        previous_send(&self.source, start)
    }

    pub(super) fn mono_opens_stretch(&self, start: usize) -> bool {
        mono_opens_stretch(&self.source, start)
    }

    /// A user opens its stretch above its bubble. A standalone/internal run
    /// opens it above the Mono's header, since its prompt is hidden.
    fn mono_stamp(&self, start: usize) -> Option<u64> {
        if !self.mono || self.document {
            return None;
        }
        let row = self.source.get(start)?;
        let stamp = if row.role == "user" {
            start
        } else if row.is_prompt() {
            return None;
        } else {
            let turn = self.source[..start]
                .iter()
                .rposition(|r| r.is_prompt())
                .map_or(0, |ix| ix + 1);
            if start != turn {
                return opens_later_day(&self.source, start)
                    .then_some(row.sent_at)
                    .flatten();
            }
            let prompt = self.prompt_start(turn);
            if self.source[prompt..turn].iter().any(|r| r.role == "user") {
                return None;
            }
            prompt
        };
        self.mono_opens_stretch(stamp)
            .then_some(self.source[stamp].sent_at)
            .flatten()
    }

    pub(super) fn mono_separator(&self, start: usize) -> Option<gpui::AnyElement> {
        self.mono_stamp(start).and_then(separator)
    }

    pub(super) fn pending_separator(&self, index: usize) -> Option<gpui::AnyElement> {
        let at = self.pending.get(index)?.sent_at;
        let previous = if index == 0 {
            self.previous_send(self.source.len())
        } else {
            self.pending[index - 1].sent_at
        };
        opens_stretch(at, previous)
            .then_some(at)
            .flatten()
            .and_then(separator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(day: u32, hour: u32, minute: u32) -> u64 {
        Local
            .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .unwrap()
            .timestamp_millis() as u64
    }

    #[gpui::test]
    fn habit_reports_on_later_days_get_their_own_day_inside_one_turn(
        cx: &mut gpui::TestAppContext,
    ) {
        let view = cx.new(|_| {
            let mut view = TranscriptView::new();
            view.mono_mode("cat", 0xe86b5f, "MonoCat");
            view
        });
        view.update(cx, |view, cx| {
            let row = |role: &str, text: &str, sent_at| Row {
                role: role.into(),
                text: text.into(),
                first: true,
                sent_at,
            };
            view.patch(
                0,
                vec![
                    row("user", "Check performance", Some(at(16, 7, 39))),
                    row("assistant", "Undated reply", None),
                    row("assistant", "Habit report one", Some(at(17, 9, 0))),
                    row("assistant", "Same-day follow-up", Some(at(17, 9, 1))),
                    row("assistant", "Habit report two", Some(at(19, 9, 0))),
                    row("assistant", "Habit report three", Some(at(21, 9, 1))),
                ],
                cx,
            );
            assert_eq!(view.mono_stamp(0), Some(at(16, 7, 39)));
            assert_eq!(
                view.mono_stamp(1),
                None,
                "the reply shares its prompt's day"
            );
            assert_eq!(view.mono_stamp(2), Some(at(17, 9, 0)));
            assert_eq!(view.mono_stamp(3), None, "same-day runs stack");
            assert_eq!(view.mono_stamp(4), Some(at(19, 9, 0)));
            assert_eq!(view.mono_stamp(5), Some(at(21, 9, 1)));
            // The Mono signs each later day again, under that day's badge.
            let headers: Vec<_> = view
                .rows
                .iter()
                .filter(|r| matches!(r.body, Body::WorkLine { .. }))
                .map(|r| (view.messages[r.message].start, r.first))
                .collect();
            assert_eq!(headers, [(1, true), (2, true), (4, true), (5, true)]);
        });
    }

    #[test]
    fn stretches_match_react_and_cross_midnight_in_local_time() {
        assert!(!opens_stretch(None, None));
        assert!(opens_stretch(Some(at(5, 9, 0)), None));
        assert!(!opens_stretch(Some(at(5, 10, 0)), Some(at(5, 9, 0))));
        assert!(
            !opens_stretch(Some(at(5, 22, 30)), Some(at(5, 7, 54))),
            "a long pause within one day is not a new day"
        );
        assert!(opens_stretch(Some(at(6, 0, 1)), Some(at(5, 23, 59))));
        let today = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert_eq!(day_label(today, today), "Today");
        assert_eq!(day_label(today.pred_opt().unwrap(), today), "Yesterday");
        assert_eq!(
            day_label(NaiveDate::from_ymd_opt(2025, 12, 30).unwrap(), today),
            "Tue, Dec 30, 2025"
        );
        assert_eq!(
            day_label(NaiveDate::from_ymd_opt(2026, 2, 3).unwrap(), today),
            "Tue, Feb 3"
        );
    }

    #[gpui::test]
    fn mono_dates_survive_streaming_and_split_reactions_and_completion_runs(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let view = cx.new(|_| {
            let mut view = TranscriptView::new();
            view.mono_mode("cat", 0xe86b5f, "MonoCat");
            view
        });
        view.update(cx, |view, cx| {
            let row = |role: &str, text: &str, sent_at| Row {
                role: role.into(),
                text: text.into(),
                first: true,
                sent_at,
            };
            view.patch(
                0,
                vec![
                    row("user", "Yesterday", Some(at(4, 23, 55))),
                    row("assistant", "<reaction>👍</reaction>", Some(at(4, 23, 56))),
                    row("user", "Today", Some(at(5, 0, 5))),
                    row("assistant", "Done", Some(at(5, 0, 6))),
                    row("mono-system", "Finished", Some(at(5, 0, 10))),
                    row("assistant", "Follow-up", Some(at(5, 0, 11))),
                    row("mono-system", "A later result", Some(at(5, 4, 0))),
                    row("assistant", "Later reply", Some(at(5, 4, 1))),
                ],
                cx,
            );
            assert_eq!(view.mono_stamp(0), Some(at(4, 23, 55)));
            assert_eq!(view.mono_stamp(2), Some(at(5, 0, 5)));
            assert_eq!(view.mono_stamp(3), None, "no duplicate before the reply");
            assert_eq!(
                view.mono_stamp(5),
                None,
                "nearby completion continues the same stretch"
            );
            assert_eq!(
                view.mono_stamp(7),
                None,
                "a later run on the same day has no badge of its own"
            );
            assert!(
                !view.stacks_on_next(0),
                "a new day breaks reaction stacking"
            );
            assert_eq!(view.mono_group_start(5), 3);
            assert_eq!(
                view.mono_group_start(7),
                3,
                "a same-day run continues the turn, however long the break"
            );
            view.streaming(true, cx);
            view.patch(8, vec![row("user", "Still today", Some(at(5, 4, 10)))], cx);
            assert_eq!(view.mono_stamp(8), None);
            view.patch(9, vec![row("assistant", "Reply", Some(at(5, 4, 11)))], cx);
            view.streaming(false, cx);
            assert_eq!(view.mono_stamp(8), None);
            assert_eq!(view.mono_stamp(0), Some(at(4, 23, 55)));
            view.set_pending(
                vec![PendingMessage {
                    id: "tomorrow".into(),
                    text: "Tomorrow".into(),
                    error: None,
                    sent_at: Some(at(6, 0, 5)),
                    attachments: Vec::new(),
                }],
                cx,
            );
            assert!(view.pending_separator(0).is_some());
            view.streaming(true, cx);
            view.patch(10, vec![row("user", "Tomorrow", Some(at(6, 0, 5)))], cx);
            view.set_pending(Vec::new(), cx);
            assert_eq!(
                view.mono_stamp(10),
                Some(at(6, 0, 5)),
                "delivery keeps the queued message's date"
            );
            view.mono = false;
            assert_eq!(
                view.mono_stamp(0),
                None,
                "normal sessions keep their layout"
            );
        });
    }

    #[gpui::test]
    fn mono_renders_date_pills_and_removes_the_session_outline(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::new(
                        gpui::point(px(0.), px(0.)),
                        size(px(1100.), px(700.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|cx| {
                        let mut view = TranscriptView::new();
                        let now = monocode_core::time::now_ms();
                        view.patch(
                            0,
                            vec![
                                Row {
                                    role: "user".into(),
                                    text: "Yesterday's message".into(),
                                    first: true,
                                    sent_at: Some(now - 86_400_000),
                                },
                                Row {
                                    role: "assistant".into(),
                                    text: "First reply".into(),
                                    first: true,
                                    sent_at: None,
                                },
                                Row {
                                    role: "user".into(),
                                    text: "Today's message".into(),
                                    first: true,
                                    sent_at: Some(now),
                                },
                                Row {
                                    role: "assistant".into(),
                                    text: "Second reply".into(),
                                    first: true,
                                    sent_at: None,
                                },
                            ],
                            cx,
                        );
                        view
                    })
                },
            )
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(visual.debug_bounds("prompt-outline").is_some());
        assert!(visual.debug_bounds("mono-day-separator").is_none());
        window
            .update(visual, |view, _, cx| {
                view.mono_mode("cat", 0xe86b5f, "MonoCat");
                view.rebuild(0, cx);
            })
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("prompt-outline").is_none());
        assert!(visual.debug_bounds("mono-day-separator").is_some());
    }
}
