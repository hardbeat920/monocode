//! What a draft is to its thread rather than to one screen: the page, the
//! values of its `mc.state`, the person's notes, and whether the person or the
//! agent is driving it. The desktop card and the phone each draw the page and
//! report the person's input here; the agent reads it back in its snapshots.
//!
//! Only `mc.state` is shared between pages. Scroll, focus and anything else a
//! page keeps to itself stay on the screen that drew it.
use super::{Anchor, DraftContent, Feedback};
use serde::Serialize;
use serde_json::{json, Map, Value};

/// What the page reports as `mc.state` is capped at this many bytes of JSON.
const MAX_STATE_BYTES: usize = 4000;
const MAX_PENDING: usize = 50;
const MAX_MESSAGES: usize = 20;
const MAX_TRAIL: usize = 40;

/// Which screen did something, so it is not sent back to the one that did it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    Desktop,
    Phone,
}

/// A note's pin, drawn on every screen's page in the order it was pinned.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Pin {
    pub x: f64,
    pub y: f64,
    /// The screen whose page drew this pin when it was placed.
    pub side: Side,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Anchor>,
}

/// What the person and the page sent that the agent has not read.
#[derive(Debug, Default, PartialEq)]
pub struct Outbox {
    pub notes: Vec<Feedback>,
    pub messages: Vec<String>,
    pub actions: Vec<String>,
}

#[derive(Debug)]
pub struct SharedDraft {
    content: DraftContent,
    title: Option<String>,
    /// Bumps whenever the page itself is replaced.
    generation: u64,
    /// Bumps on every change another screen should show.
    revision: u64,
    state: Option<Map<String, Value>>,
    /// Bumps whenever `state` changes. A screen whose page has an older one
    /// sets the page to `state`.
    state_revision: u64,
    pins: Vec<Pin>,
    /// Notes pinned but not sent yet.
    pending: Vec<Feedback>,
    /// Notes the person sent that the agent has not read.
    outbox: Vec<Feedback>,
    /// What the page sent with `mc.send` that the agent has not read.
    messages: Vec<String>,
    /// What the person did in the page by hand since the agent last looked.
    trail: Vec<String>,
    /// The person used the page, on any screen, since the agent last resumed.
    control: bool,
    /// Why the agent's wait should end.
    signal: Option<&'static str>,
    /// The agent is waiting for the person.
    waiting: bool,
}

impl SharedDraft {
    pub fn new(content: DraftContent, title: Option<String>) -> Self {
        Self {
            content,
            title,
            generation: 1,
            revision: 1,
            state: None,
            state_revision: 1,
            pins: Vec::new(),
            pending: Vec::new(),
            outbox: Vec::new(),
            messages: Vec::new(),
            trail: Vec::new(),
            control: false,
            signal: None,
            waiting: false,
        }
    }

    fn bump(&mut self) {
        self.revision += 1;
    }

    pub fn content(&self) -> &DraftContent {
        &self.content
    }
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn state(&self) -> Option<&Map<String, Value>> {
        self.state.as_ref()
    }
    pub fn state_revision(&self) -> u64 {
        self.state_revision
    }
    pub fn pins(&self) -> &[Pin] {
        &self.pins
    }
    pub fn pending(&self) -> &[Feedback] {
        &self.pending
    }
    pub fn control(&self) -> bool {
        self.control
    }
    pub fn signal(&self) -> Option<&'static str> {
        self.signal
    }
    pub fn waiting(&self) -> bool {
        self.waiting
    }

    /// The agent sent a new page: its state and pins go with the old one.
    pub fn replace(&mut self, content: DraftContent) {
        self.content = content;
        self.generation += 1;
        self.state = None;
        self.state_revision += 1;
        self.pins.clear();
        self.control = false;
        self.bump();
    }

    /// A page reported its `mc.state` after the person or the agent changed
    /// it there. Returns whether the shared values changed.
    pub fn report_state(&mut self, values: Map<String, Value>) -> bool {
        if self.state.as_ref() == Some(&values) || too_big(&values) {
            return false;
        }
        self.state = Some(values);
        self.state_revision += 1;
        self.bump();
        true
    }

    /// The person changed some of the page's state on another screen. Only
    /// keys the page has are taken, as `mc.state` itself would.
    pub fn patch_state(&mut self, values: &Map<String, Value>) -> Result<bool, String> {
        let Some(state) = &self.state else {
            return Err("This draft has no state to change".into());
        };
        let unknown: Vec<&str> = values
            .keys()
            .filter(|key| !state.contains_key(*key))
            .map(String::as_str)
            .collect();
        if !unknown.is_empty() {
            return Err(format!("Unknown state {}", unknown.join(", ")));
        }
        let mut next = state.clone();
        next.extend(values.iter().map(|(k, v)| (k.clone(), v.clone())));
        if too_big(&next) || self.state.as_ref() == Some(&next) {
            return Ok(false);
        }
        self.state = Some(next);
        self.state_revision += 1;
        self.bump();
        Ok(true)
    }

    /// The person pinned notes on one screen's page.
    pub fn pin(&mut self, notes: Vec<Feedback>, side: Side) {
        if notes.is_empty() {
            return;
        }
        let room = MAX_PENDING.saturating_sub(self.pending.len());
        for mut note in notes.into_iter().take(room) {
            self.pins.push(Pin {
                x: note.x,
                y: note.y,
                side,
                at: note.at.take().and_then(Anchor::checked),
            });
            self.pending.push(note);
        }
        self.bump();
    }

    /// Hands the pinned notes to the agent. Returns whether there were any.
    pub fn send(&mut self) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        self.outbox.append(&mut self.pending);
        self.signal = Some("feedback");
        self.bump();
        true
    }

    /// The page sent lines with `mc.send`; they go to the agent at once.
    pub fn tell(&mut self, messages: Vec<String>) -> bool {
        if messages.is_empty() {
            return false;
        }
        self.messages.extend(messages);
        self.messages.truncate(MAX_MESSAGES);
        self.signal = Some("message");
        true
    }

    /// The person clicked or typed in the page, on any screen.
    pub fn touch(&mut self, actions: Vec<String>) {
        self.trail
            .extend(actions.into_iter().filter(|action| !action.is_empty()));
        self.trail.truncate(MAX_TRAIL);
        if !self.control {
            self.control = true;
            self.bump();
        }
    }

    /// The person is done trying it.
    pub fn hand_back(&mut self) {
        self.signal = Some("handed back");
        self.control = false;
        self.bump();
    }

    /// The agent takes the draft back.
    pub fn resume(&mut self) {
        self.signal = None;
        if self.control {
            self.control = false;
            self.bump();
        }
    }

    pub fn set_waiting(&mut self, waiting: bool) {
        if self.waiting != waiting {
            self.waiting = waiting;
            self.bump();
        }
    }

    /// The notes, page messages and hand-done actions the agent has not seen,
    /// handed over.
    pub fn take_outbox(&mut self) -> Outbox {
        self.signal = None;
        Outbox {
            notes: std::mem::take(&mut self.outbox),
            messages: std::mem::take(&mut self.messages),
            actions: std::mem::take(&mut self.trail),
        }
    }

    /// What a phone shows of the draft besides its page.
    pub fn summary(&self) -> Value {
        json!({
            "rev": self.revision,
            "generation": self.generation,
            "kind": match self.content {
                DraftContent::Html(_) => "html",
                DraftContent::Url(_) => "url",
            },
            "state": self.state,
            "stateRev": self.state_revision,
            "pins": self.pins,
            "pending": self.pending,
            "control": self.control,
            "waiting": self.waiting,
        })
    }
}

fn too_big(values: &Map<String, Value>) -> bool {
    serde_json::to_string(values).map_or(true, |text| text.len() >= MAX_STATE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> SharedDraft {
        SharedDraft::new(
            DraftContent::Html("<p>x</p>".into()),
            Some("Pricing".into()),
        )
    }
    fn values(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }
    fn note(text: &str) -> Feedback {
        Feedback {
            note: text.into(),
            tag: "button".into(),
            name: "Go".into(),
            x: 10.,
            y: 20.,
            at: None,
        }
    }

    #[test]
    fn a_page_report_is_the_shared_state_and_the_phone_patches_its_keys() {
        let mut draft = draft();
        assert!(draft.patch_state(&values(json!({"radius": 4}))).is_err());
        let first = draft.state_revision();
        assert!(draft.report_state(values(json!({"radius": 8, "glow": false}))));
        assert!(draft.state_revision() > first);
        // The same values again, such as a page's echo of a patch, change nothing.
        let revision = draft.revision();
        assert!(!draft.report_state(values(json!({"radius": 8, "glow": false}))));
        assert_eq!(draft.revision(), revision);

        assert!(draft.patch_state(&values(json!({"radius": 12}))).unwrap());
        assert_eq!(
            draft.state(),
            Some(&values(json!({"radius": 12, "glow": false})))
        );
        let error = draft.patch_state(&values(json!({"size": 1}))).unwrap_err();
        assert!(error.contains("size"), "{error}");
        assert!(!draft.patch_state(&values(json!({"radius": 12}))).unwrap());
    }

    #[test]
    fn oversized_state_is_ignored_like_the_page_ignores_it() {
        let mut draft = draft();
        assert!(!draft.report_state(values(json!({"text": "x".repeat(MAX_STATE_BYTES)}))));
        assert!(draft.state().is_none());
    }

    #[test]
    fn a_new_page_drops_the_old_pages_state_pins_and_control() {
        let mut draft = draft();
        draft.report_state(values(json!({"radius": 8})));
        draft.pin(vec![note("Bigger")], Side::Phone);
        draft.touch(vec!["clicked \"Go\"".into()]);
        draft.replace(DraftContent::Html("<p>y</p>".into()));
        assert_eq!(draft.generation(), 2);
        assert!(draft.state().is_none());
        assert!(draft.pins().is_empty());
        assert!(!draft.control());
        // Notes written on the old page still reach the agent.
        assert_eq!(draft.pending().len(), 1);
    }

    #[test]
    fn notes_from_either_screen_wait_together_then_go_to_the_agent_once() {
        let mut draft = draft();
        assert!(!draft.send());
        draft.pin(vec![note("Bigger")], Side::Desktop);
        draft.pin(vec![note("Bluer")], Side::Phone);
        assert_eq!(draft.pins().len(), 2);
        assert_eq!(draft.pins()[1].side, Side::Phone);
        assert!(draft.send());
        assert_eq!(draft.signal(), Some("feedback"));
        assert!(draft.pending().is_empty());
        let outbox = draft.take_outbox();
        assert_eq!(outbox.notes.len(), 2);
        assert_eq!(draft.signal(), None);
        assert!(draft.take_outbox().notes.is_empty());
    }

    #[test]
    fn the_person_has_control_from_any_screen_until_they_hand_it_back() {
        let mut draft = draft();
        draft.touch(vec!["clicked \"Go\"".into(), String::new()]);
        assert!(draft.control());
        draft.hand_back();
        assert!(!draft.control());
        assert_eq!(draft.signal(), Some("handed back"));
        let outbox = draft.take_outbox();
        assert_eq!(outbox.actions, vec!["clicked \"Go\"".to_string()]);
        draft.touch(Vec::new());
        draft.resume();
        assert!(!draft.control());
        assert_eq!(draft.signal(), None);
    }

    #[test]
    fn a_page_message_ends_the_agents_wait() {
        let mut draft = draft();
        assert!(!draft.tell(Vec::new()));
        assert!(draft.tell(vec!["Use this one".into()]));
        assert_eq!(draft.signal(), Some("message"));
        assert_eq!(
            draft.take_outbox().messages,
            vec!["Use this one".to_string()]
        );
    }

    #[test]
    fn a_pin_keeps_the_element_it_was_placed_on_and_the_agent_never_sees_it() {
        let mut draft = draft();
        let anchored = |path: &str, ax: f64| Feedback {
            at: Some(Anchor {
                path: path.into(),
                ax,
                ay: 0.5,
            }),
            ..note("Bigger")
        };
        draft.pin(
            vec![
                anchored("0.2.1", 0.25),
                anchored("0.x", 0.5),
                anchored("1", 1.5),
            ],
            Side::Phone,
        );
        let pins: Vec<_> = draft.pins().iter().map(|pin| pin.at.clone()).collect();
        assert_eq!(
            pins,
            [
                Some(Anchor {
                    path: "0.2.1".into(),
                    ax: 0.25,
                    ay: 0.5
                }),
                None,
                None
            ]
        );
        assert!(draft.pending().iter().all(|note| note.at.is_none()));
        assert_eq!(draft.summary()["pins"][0]["at"]["path"], "0.2.1");
        assert!(draft.summary()["pins"][1].get("at").is_none());
    }

    #[test]
    fn the_phone_summary_carries_what_it_draws_besides_the_page() {
        let mut draft = draft();
        draft.report_state(values(json!({"radius": 8})));
        draft.pin(vec![note("Bigger")], Side::Desktop);
        let summary = draft.summary();
        assert_eq!(summary["kind"], "html");
        assert_eq!(summary["state"]["radius"], 8);
        assert_eq!(summary["pins"][0]["x"], 10.);
        assert_eq!(summary["pins"][0]["side"], "desktop");
        assert_eq!(summary["pending"][0]["note"], "Bigger");
        assert_eq!(summary["rev"], draft.revision());
    }
}
