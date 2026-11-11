//! Undo and redo for every editable field. A run of typing, or of deleting
//! with Backspace/Delete, is one step, as native text views group it; any
//! other edit is a step of its own.
use super::*;
use std::time::{Duration, Instant};

const MAX_STEPS: usize = 100;
const MAX_HISTORY_BYTES: usize = 8 * 1024 * 1024;
/// A pause this long inside a typing run starts a new step.
const PAUSE: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum EditKind {
    Typing,
    Deleting,
    Other,
}

struct Snapshot {
    text: SharedString,
    selection: Range<usize>,
}

/// The step being extended: what it does and where its caret ended.
struct Run {
    kind: EditKind,
    caret: usize,
    at: Instant,
}

#[derive(Default)]
pub(super) struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    run: Option<Run>,
}

impl History {
    /// Called before `range` is edited, with the caret the edit leaves.
    fn record(
        &mut self,
        kind: EditKind,
        text: &SharedString,
        selection: Range<usize>,
        range: &Range<usize>,
        caret: usize,
        now: Instant,
    ) {
        let continues = kind != EditKind::Other
            && self.run.as_ref().is_some_and(|run| {
                run.kind == kind
                    && now.saturating_duration_since(run.at) < PAUSE
                    && (range.start == run.caret || range.end == run.caret)
            });
        if !continues {
            self.undo.push(Snapshot {
                text: text.clone(),
                selection,
            });
            let mut bytes = 0;
            let keep = self
                .undo
                .iter()
                .rev()
                .take(MAX_STEPS)
                .take_while(|s| {
                    bytes += s.text.len();
                    bytes <= MAX_HISTORY_BYTES
                })
                .count()
                .max(1);
            let discard = self.undo.len() - keep;
            self.undo.drain(..discard);
            self.redo.clear();
        }
        self.run = Some(Run {
            kind,
            caret,
            at: now,
        });
    }
    fn step(
        &mut self,
        redo: bool,
        text: SharedString,
        selection: Range<usize>,
    ) -> Option<Snapshot> {
        let (source, target) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        let snapshot = source.pop()?;
        target.push(Snapshot { text, selection });
        self.run = None;
        Some(snapshot)
    }
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
    /// The next edit starts a new step.
    pub(super) fn break_run(&mut self) {
        self.run = None;
    }
}

impl Input {
    /// Remember the buffer before an edit of `range` that leaves the caret at
    /// `caret`. An IME composition records once, when it starts.
    pub(super) fn record_undo(
        &mut self,
        kind: EditKind,
        range: &Range<usize>,
        caret: usize,
        cx: &App,
    ) {
        if self.secret || self.read_only {
            return;
        }
        if self.marked.is_some() {
            self.history.break_run();
            return;
        }
        let now = cx.background_executor().now();
        self.history.record(
            kind,
            &self.content,
            self.selection.clone(),
            range,
            caret,
            now,
        );
    }
    pub(super) fn history_available(&self, redo: bool) -> bool {
        !self.read_only
            && !self.secret
            && self.marked.is_none()
            && !(if redo {
                &self.history.redo
            } else {
                &self.history.undo
            })
            .is_empty()
    }
    /// Replace the whole buffer as one undoable step, for text the app
    /// writes into a field the person is editing.
    pub(crate) fn replace_all(&mut self, text: String, cx: &mut Context<Self>) {
        if self.content.as_ref() == text {
            return;
        }
        let range = 0..self.content.len();
        self.record_undo(EditKind::Other, &range, text.len(), cx);
        self.load_text(text, cx);
    }
    pub(super) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.step_history(false, cx);
    }
    pub(super) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.step_history(true, cx);
    }
    fn step_history(&mut self, redo: bool, cx: &mut Context<Self>) {
        if self.read_only || self.secret {
            cx.propagate();
            return;
        }
        if self.marked.is_some() {
            return;
        }
        let Some(snapshot) = self
            .history
            .step(redo, self.content.clone(), self.selection.clone())
        else {
            return;
        };
        let previous_command = self
            .inline_commands
            .then(|| Command::leading(&self.content))
            .flatten();
        self.manual_scroll = false;
        self.reveal_pending = self.reveal_in_parent;
        self.clear_folds();
        self.content = snapshot.text;
        self.selection = self.clip_range(snapshot.selection);
        self.reversed = false;
        self.code_changed();
        self.reveal_code();
        self.command_removed(previous_command, cx);
        cx.emit(Changed);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn modifier() -> &'static str {
        if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        }
    }

    fn field(cx: &mut TestAppContext) -> gpui::WindowHandle<Input> {
        cx.update(bind);
        let window = cx.add_window(|_, cx| Input::new(cx));
        window
            .update(cx, |input, window, cx| window.focus(&input.focus, cx))
            .unwrap();
        cx.refresh().unwrap();
        window
    }

    fn content(window: gpui::WindowHandle<Input>, cx: &mut TestAppContext) -> String {
        window
            .update(cx, |input, _, _| input.content.to_string())
            .unwrap()
    }

    #[gpui::test]
    fn composer_undoes_typing_and_deleting_runs_as_whole_steps(cx: &mut TestAppContext) {
        let window = field(cx);
        let m = modifier();
        cx.simulate_keystrokes(window.into(), "h e l l o");
        cx.simulate_keystrokes(window.into(), "backspace backspace");
        assert_eq!(content(window, cx), "hel");
        cx.simulate_keystrokes(window.into(), &format!("{m}-z"));
        assert_eq!(content(window, cx), "hello");
        cx.simulate_keystrokes(window.into(), &format!("{m}-z"));
        assert_eq!(content(window, cx), "");
        cx.simulate_keystrokes(window.into(), &format!("{m}-shift-z {m}-shift-z"));
        assert_eq!(content(window, cx), "hel");
        // A new edit after undo drops what could be redone.
        cx.simulate_keystrokes(window.into(), &format!("{m}-z x"));
        assert_eq!(content(window, cx), "hellox");
        cx.simulate_keystrokes(window.into(), &format!("{m}-shift-z"));
        assert_eq!(content(window, cx), "hellox");
    }

    #[gpui::test]
    fn pauses_moves_and_bulk_edits_start_new_steps(cx: &mut TestAppContext) {
        let window = field(cx);
        let m = modifier();
        let word = if cfg!(target_os = "macos") {
            "alt"
        } else {
            "ctrl"
        };
        cx.simulate_keystrokes(window.into(), "a b");
        cx.executor().advance_clock(PAUSE);
        cx.simulate_keystrokes(window.into(), "c d");
        // Typing somewhere else is a new step even without a pause.
        cx.simulate_keystrokes(window.into(), "left left x");
        assert_eq!(content(window, cx), "abxcd");
        cx.simulate_keystrokes(window.into(), "end");
        cx.simulate_keystrokes(window.into(), &format!("{word}-backspace"));
        assert_eq!(content(window, cx), "");
        cx.simulate_keystrokes(window.into(), &format!("{m}-z"));
        assert_eq!(content(window, cx), "abxcd");
        cx.simulate_keystrokes(window.into(), &format!("{m}-z"));
        assert_eq!(content(window, cx), "abcd");
        cx.simulate_keystrokes(window.into(), &format!("{m}-z"));
        assert_eq!(content(window, cx), "ab");
        cx.simulate_keystrokes(window.into(), &format!("{m}-z"));
        assert_eq!(content(window, cx), "");
    }

    #[gpui::test]
    fn paste_cut_and_selection_delete_are_single_steps(cx: &mut TestAppContext) {
        let window = field(cx);
        let m = modifier();
        cx.write_to_clipboard(ClipboardItem::new_string("pasted text".into()));
        cx.simulate_keystrokes(window.into(), &format!("a {m}-v b"));
        assert_eq!(content(window, cx), "apasted textb");
        cx.simulate_keystrokes(window.into(), &format!("{m}-a backspace"));
        assert_eq!(content(window, cx), "");
        cx.simulate_keystrokes(window.into(), &format!("{m}-z"));
        assert_eq!(content(window, cx), "apasted textb");
        cx.simulate_keystrokes(window.into(), &format!("{m}-z"));
        assert_eq!(content(window, cx), "apasted text");
        cx.simulate_keystrokes(window.into(), &format!("{m}-z"));
        assert_eq!(content(window, cx), "a");
        cx.simulate_keystrokes(window.into(), &format!("{m}-a {m}-x"));
        assert_eq!(content(window, cx), "");
        cx.simulate_keystrokes(window.into(), &format!("{m}-z"));
        assert_eq!(content(window, cx), "a");
    }

    #[gpui::test]
    fn app_writes_are_undoable_but_loads_and_sends_reset(cx: &mut TestAppContext) {
        let window = field(cx);
        let m = modifier();
        cx.simulate_keystrokes(window.into(), "h i");
        window
            .update(cx, |input, _, cx| {
                input.replace_all("from the phone".into(), cx)
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), &format!("{m}-z"));
        assert_eq!(content(window, cx), "hi");
        window
            .update(cx, |input, _, cx| {
                assert!(input.take(cx).is_some());
                assert!(!input.history_available(false));
                input.set_text("another note".into(), cx);
                assert!(!input.history_available(false));
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), &format!("{m}-z"));
        assert_eq!(content(window, cx), "another note");
    }

    #[gpui::test]
    fn composition_is_one_step_and_secrets_keep_nothing(cx: &mut TestAppContext) {
        let window = field(cx);
        window
            .update(cx, |input, window, cx| {
                input.replace_text_in_range(None, "a", window, cx);
                input.replace_and_mark_text_in_range(None, "k", None, window, cx);
                input.replace_and_mark_text_in_range(None, "か", None, window, cx);
                input.replace_text_in_range(None, "漢", window, cx);
                assert_eq!(input.content.as_ref(), "a漢");
                input.undo(&Undo, window, cx);
                assert_eq!(input.content.as_ref(), "a");
                input.set_text(String::new(), cx);
                input.set_secret(true);
                input.replace_text_in_range(None, "pw", window, cx);
                assert!(!input.history_available(false));
            })
            .unwrap();
    }
}
