//! Keeps phone notices for when the user is away from the Mac. While they use
//! it, a notice is held here; once the Mac has sat idle (or locked) long
//! enough, the ones still worth hearing go out, or one summary when there are
//! many. The app reports whether the user is at the Mac; the host does the
//! holding.
use super::notify::Notice;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// A held notice older than this is no news, however long the user stayed.
const MAX_AGE: Duration = Duration::from_secs(10 * 60);
/// A phone is sent a few notices one by one; more than this, one summary.
pub(super) const DIGEST_AFTER: usize = 3;

/// Whether the user is at the Mac, as far as phone notices care.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Attendance {
    /// The setting is off: every notice goes straight to the phone.
    #[default]
    Ignore,
    /// Using the Mac: notices wait.
    AtDesk,
    /// Idle long enough, or locked: notices go out, held ones with them.
    Away,
}

/// What the app sees of the user, reported about once a second.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub attendance: Attendance,
    /// Sessions with an approval or a question still open, by phone ID.
    pub waiting: Vec<String>,
    /// The session on screen in front of the user, by phone ID.
    pub viewing: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Held {
    pub category: String,
    pub notice: Notice,
}

impl Held {
    fn session(&self) -> Option<&str> {
        self.notice.data["sessionId"].as_str()
    }

    /// What it is about, so a newer notice replaces an older one: a session's
    /// "finished" supersedes its "needs approval", an Inbox item stands alone.
    fn key(&self) -> String {
        if let Some(session) = self.session() {
            format!("session:{session}")
        } else if let Some(item) = self.notice.data["key"].as_str() {
            format!("item:{item}")
        } else {
            format!(
                "{}:{}:{}",
                self.category, self.notice.title, self.notice.body
            )
        }
    }

    /// Still worth a push, judging by what the user has done since.
    fn current(&self, report: &Report, catalog: &Value) -> bool {
        let Some(id) = self.session() else {
            return true;
        };
        match self.category.as_str() {
            "agentInput" => report.waiting.iter().any(|waiting| waiting == id),
            "agentFinished" => {
                let working = catalog["sessions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|session| session["id"] == id && session["busy"] == true);
                !working && report.viewing.as_deref() != Some(id)
            }
            _ => true,
        }
    }
}

/// One notice standing in for a crowd: the counts, and which to look at first,
/// a session waiting on the user if there is one.
pub(super) fn digest(items: &[&Held]) -> Notice {
    let count = |category: &str| items.iter().filter(|h| h.category == category).count();
    let (input, done) = (count("agentInput"), count("agentFinished"));
    let other = items.len() - input - done;
    let parts: Vec<String> = [
        (input, "waiting on you"),
        (done, "finished"),
        (other, "other"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, what)| format!("{n} {what}"))
    .collect();
    let first = items
        .iter()
        .find(|h| h.category == "agentInput")
        .or(items.first())
        .map(|h| h.notice.title.as_str())
        .filter(|title| !title.is_empty())
        .unwrap_or("Session");
    Notice {
        title: format!("{} updates while you were away", items.len()),
        subtitle: None,
        body: format!("{}. Start with {first}.", parts.join(", ")),
        // It names no session, so a tap just opens the app.
        data: json!({}),
    }
}

#[derive(Default)]
pub(super) struct Presence {
    attendance: Attendance,
    held: Vec<(Instant, Held)>,
}

impl Presence {
    /// Holds `held` if the user is at the Mac. Returns it back when it should
    /// go now.
    pub(super) fn hold(&mut self, held: Held) -> Option<Held> {
        if self.attendance != Attendance::AtDesk {
            return Some(held);
        }
        let key = held.key();
        self.held
            .retain(|(at, other)| at.elapsed() < MAX_AGE && other.key() != key);
        self.held.push((Instant::now(), held));
        None
    }

    /// Takes the app's report: drops what the user has dealt with, and
    /// returns what to send if they are no longer at the Mac.
    pub(super) fn update(&mut self, report: &Report, catalog: &Value) -> Vec<Held> {
        self.attendance = report.attendance;
        self.held
            .retain(|(at, held)| at.elapsed() < MAX_AGE && held.current(report, catalog));
        if self.attendance == Attendance::AtDesk {
            return Vec::new();
        }
        std::mem::take(&mut self.held)
            .into_iter()
            .map(|(_, held)| held)
            .collect()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.held.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(category: &str, session: &str) -> Held {
        Held {
            category: category.into(),
            notice: Notice {
                title: "Fix push".into(),
                subtitle: None,
                body: "Finished".into(),
                data: json!({"sessionId":session}),
            },
        }
    }

    fn at_desk() -> Report {
        Report {
            attendance: Attendance::AtDesk,
            ..Report::default()
        }
    }

    fn away(waiting: &[&str]) -> Report {
        Report {
            attendance: Attendance::Away,
            waiting: waiting.iter().map(|id| id.to_string()).collect(),
            viewing: None,
        }
    }

    fn catalog(sessions: Value) -> Value {
        json!({"projects":[],"monos":[],"sessions":sessions})
    }

    #[test]
    fn nothing_is_held_when_the_setting_is_off_or_the_user_is_away() {
        let mut presence = Presence::default();
        assert!(presence.hold(held("agentFinished", "a")).is_some());
        presence.update(&away(&[]), &catalog(json!([])));
        assert!(presence.hold(held("agentFinished", "a")).is_some());
        assert_eq!(presence.len(), 0);
    }

    #[test]
    fn notices_wait_at_the_desk_and_go_out_when_the_user_leaves() {
        let mut presence = Presence::default();
        presence.update(&at_desk(), &catalog(json!([])));
        assert!(presence.hold(held("agentFinished", "a")).is_none());
        assert!(presence.hold(held("agentInput", "b")).is_none());
        let sent = presence.update(&away(&["b"]), &catalog(json!([{"id":"a","busy":false}])));
        assert_eq!(sent.len(), 2);
        assert_eq!(presence.len(), 0);
    }

    #[test]
    fn a_question_answered_at_the_desk_is_not_sent_later() {
        let mut presence = Presence::default();
        presence.update(&at_desk(), &catalog(json!([])));
        presence.hold(held("agentInput", "b"));
        assert!(presence.update(&away(&[]), &catalog(json!([]))).is_empty());
    }

    #[test]
    fn a_finish_that_started_working_again_is_not_sent() {
        let mut presence = Presence::default();
        presence.update(&at_desk(), &catalog(json!([])));
        presence.hold(held("agentFinished", "a"));
        let again = catalog(json!([{"id":"a","busy":true}]));
        assert!(presence.update(&away(&[]), &again).is_empty());
    }

    #[test]
    fn a_session_has_one_held_notice_and_the_newest_wins() {
        let mut presence = Presence::default();
        presence.update(&at_desk(), &catalog(json!([])));
        presence.hold(held("agentInput", "a"));
        presence.hold(held("agentFinished", "a"));
        assert_eq!(presence.len(), 1);
        let sent = presence.update(&away(&[]), &catalog(json!([{"id":"a","busy":false}])));
        assert_eq!(sent[0].category, "agentFinished");
    }

    #[test]
    fn a_finish_seen_on_the_desktop_is_dropped_but_a_question_stays() {
        let mut presence = Presence::default();
        presence.update(&at_desk(), &catalog(json!([])));
        presence.hold(held("agentFinished", "a"));
        presence.hold(held("agentInput", "b"));
        presence.update(
            &Report {
                attendance: Attendance::AtDesk,
                waiting: vec!["b".into()],
                viewing: Some("a".into()),
            },
            &catalog(json!([])),
        );
        assert_eq!(presence.len(), 1);
    }

    #[test]
    fn notices_without_a_session_are_held_and_sent_as_they_are() {
        let mut presence = Presence::default();
        presence.update(&at_desk(), &catalog(json!([])));
        let mono = Held {
            category: "monoMessages".into(),
            notice: Notice {
                title: "Scout".into(),
                subtitle: None,
                body: "Found it".into(),
                data: json!({"monoId":"m"}),
            },
        };
        assert!(presence.hold(mono.clone()).is_none());
        assert_eq!(presence.update(&away(&[]), &catalog(json!([]))), [mono]);
    }

    #[test]
    fn going_back_to_the_desk_keeps_what_is_held() {
        let mut presence = Presence::default();
        presence.update(&at_desk(), &catalog(json!([])));
        presence.hold(held("agentFinished", "a"));
        assert!(presence.update(&at_desk(), &catalog(json!([]))).is_empty());
        assert_eq!(presence.len(), 1);
    }

    #[test]
    fn a_crowd_becomes_one_summary_that_leads_with_what_waits_on_the_user() {
        let items = [
            held("agentFinished", "a"),
            held("agentFinished", "b"),
            held("agentInput", "c"),
            held("agentFinished", "d"),
        ];
        let refs: Vec<&Held> = items.iter().collect();
        let digest = digest(&refs);
        assert_eq!(digest.title, "4 updates while you were away");
        assert_eq!(
            digest.body,
            "1 waiting on you, 3 finished. Start with Fix push."
        );
    }
}
