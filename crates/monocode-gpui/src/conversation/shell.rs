//! Composer bash mode. `!command` runs in the thread's folder, then starts a
//! turn that hands its output to the agent, with any lines typed under the
//! command as the question. Commands sent during a turn or another run queue
//! behind it.
use super::*;
use gpui::{App, Hsla};
use monocode_core::shell_run::{self, ShellRun};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const BORDER_FADE: Duration = Duration::from_millis(200);

#[derive(Default)]
pub(super) struct State {
    border: BorderFade,
    /// The id of the run in progress.
    running: Option<String>,
    queue: VecDeque<Queued>,
    next_id: u64,
}
struct Queued {
    id: u64,
    command: String,
    question: String,
    attachments: Vec<std::path::PathBuf>,
}

/// Eases the composer border between its resting ink and bash mode's green,
/// in both directions. A flip mid-fade restarts from wherever it had got to.
#[derive(Default)]
struct BorderFade {
    mix: f32,
    from: f32,
    active: bool,
    started: Option<Instant>,
}
impl BorderFade {
    /// 0 at rest, 1 in bash mode; the flag says the fade is still running.
    fn mix(&mut self, active: bool, now: Instant, reduced: bool) -> (f32, bool) {
        let to = f32::from(active);
        if reduced {
            *self = Self {
                mix: to,
                from: to,
                active,
                started: None,
            };
            return (to, false);
        }
        if active != self.active {
            self.active = active;
            self.from = self.mix;
            self.started = Some(now);
        }
        let Some(started) = self.started else {
            return (self.mix, false);
        };
        let t = (now.saturating_duration_since(started).as_secs_f32() / BORDER_FADE.as_secs_f32())
            .min(1.);
        self.mix = self.from + (to - self.from) * crate::reveal::bezier(t, 0.22, 1., 0.36, 1.);
        if t >= 1. {
            self.started = None;
        }
        (self.mix, t < 1.)
    }
}

impl Conversation {
    /// A run is going or waiting; messages queue behind it so they carry its output.
    pub(super) fn shell_busy(&self) -> bool {
        self.shell.running.is_some() || !self.shell.queue.is_empty()
    }
    /// The composer border between `rest` and bash mode's green, and whether
    /// it is still moving and needs another frame.
    pub(super) fn shell_border(&mut self, rest: Hsla, active: bool, cx: &App) -> (Hsla, bool) {
        let (mix, moving) = self
            .shell
            .border
            .mix(active, Instant::now(), cx.reduce_motion());
        let (from, to) = (
            rest.to_rgb(),
            input::Input::shell_color().opacity(0.5).to_rgb(),
        );
        let lerp = |a: f32, b: f32| a + (b - a) * mix;
        let color = gpui::Rgba {
            r: lerp(from.r, to.r),
            g: lerp(from.g, to.g),
            b: lerp(from.b, to.b),
            a: lerp(from.a, to.a),
        };
        (color.into(), moving)
    }
    /// 0 at rest, 1 once the composer has fully settled into bash mode.
    pub(super) fn shell_progress(&self) -> f32 {
        self.shell.border.mix
    }
    /// The toolbar's stand-in for the model and access pills while a `!`
    /// command is being typed: what mode this is, and where it will run.
    pub(super) fn shell_chip(&self, progress: f32) -> impl IntoElement {
        let ink = input::Input::shell_color();
        let cwd = self
            .settings
            .worktree_cwd
            .as_deref()
            .unwrap_or(&self.summary.cwd);
        let folder = std::path::Path::new(cwd).file_name().map_or_else(
            || cwd.to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        div()
            .debug_selector(|| "composer-shell-chip".into())
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap_2()
            .opacity(progress)
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(26.))
                    .px(px(8.))
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(ink.opacity(0.14))
                    .text_color(ink)
                    .child(icon("terminal", 14.).text_color(ink))
                    .child(div().text_size(px(11.)).child("Bash")),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(content(0.5))
                    .child(format!("Runs in {folder} \u{b7} Enter to run")),
            )
    }
    pub(super) fn shell_running(&self) -> bool {
        self.shell.running.is_some()
    }
    pub(super) fn cancel_shell(&mut self) {
        if let Some(id) = &self.shell.running {
            shell_run::cancel(id);
        }
    }
    /// Only local, non-Mono threads run commands; elsewhere `!` is plain text.
    pub(super) fn shell_available(&self) -> bool {
        !self.is_remote() && self.settings.mono.is_none()
    }

    pub(super) fn submit_shell(&mut self, cx: &mut Context<Self>) {
        if !self.shell_available() {
            self.send(cx);
            return;
        }
        if self.needs_history_restore() {
            self.defer_history_action(|this, cx| this.submit_shell(cx), cx);
            return;
        }
        if !self.loaded || self.input.read(cx).is_composing() {
            return;
        }
        let Some((command, question)) = self
            .input
            .read(cx)
            .shell_command()
            .map(|(command, question)| (command.to_owned(), question.to_owned()))
        else {
            return;
        };
        if self.settings.worktree_removed {
            self.status = "Select a branch or worktree to continue this conversation.".into();
            cx.notify();
            return;
        }
        if self.settings.new_worktree && self.settings.worktree_cwd.is_none() {
            self.status = "Send a message first so this thread can create its worktree.".into();
            cx.notify();
            return;
        }
        self.input.update(cx, |input, cx| input.take(cx));
        self.shell.next_id += 1;
        self.shell.queue.push_back(Queued {
            id: self.shell.next_id,
            command,
            question,
            attachments: std::mem::take(&mut self.attachments),
        });
        self.pump_shell(cx);
        cx.notify();
    }

    /// Starts the next queued command once no turn or run is going, then
    /// lets queued messages go.
    pub(super) fn pump_shell(&mut self, cx: &mut Context<Self>) {
        if self.shell.running.is_some()
            || self.busy
            || self.stopping
            || self.preparing_worktree
            || !self.loaded
            || !self.history_ready()
        {
            return;
        }
        match self.shell.queue.pop_front() {
            Some(next) => self.run_shell(next, cx),
            None => self.pump_follow_ups(cx),
        }
    }

    /// The turn that hands a finished run's output to the agent.
    fn share_shell(
        &mut self,
        question: String,
        attachments: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let message = self.message(question, attachments, None, cx);
        self.launch_message(message, cx);
        if !self.busy {
            self.pump_shell(cx);
        }
    }

    fn run_shell(&mut self, queued: Queued, cx: &mut Context<Self>) {
        let Queued {
            command,
            question,
            attachments,
            ..
        } = queued;
        let mut share = Some((question, attachments));
        let cwd = self
            .settings
            .worktree_cwd
            .clone()
            .unwrap_or_else(|| self.summary.cwd.clone());
        let mut run = ShellRun::new(&command, &cwd);
        let row = run.row();
        let sent_at = row.sent_at;
        let at = self.transcript.read(cx).source_rows().len();
        self.shell.running = Some(run.id.clone());
        if !self.summary.has_messages {
            self.composer_motion.capture(self.composer_anchor.get());
        }
        self.summary.has_messages = true;
        self.summary.updated_at = monocode_core::time::now_ms();
        self.transcript
            .update(cx, |view, cx| view.patch(at, vec![row], cx));
        let (tx, rx) = async_channel::unbounded::<ShellRun>();
        let (store, session) = (self.store.clone(), self.summary.id);
        let row = move |run: &ShellRun| {
            let mut row = run.row();
            row.sent_at = sent_at;
            row
        };
        cx.background_executor()
            .spawn(async move {
                let save = |run: &ShellRun| {
                    locked(&store).and_then(|mut store| store.write_rows(session, at, &[row(run)]))
                };
                let saved = save(&run);
                if saved.is_ok() {
                    shell_run::execute(&mut run, |run| {
                        if run.state == shell_run::State::Running {
                            let _ = tx.send_blocking(run.clone());
                        }
                    });
                } else {
                    run.state = shell_run::State::Failed;
                }
                // The next turn reads the store, so the result lands there
                // before the conversation lets queued messages go.
                if let Err(error) = saved.and_then(|_| save(&run)) {
                    run.output = format!("{}\nCouldn't save this run: {error}", run.output);
                    run.state = shell_run::State::Failed;
                }
                let _ = tx.send_blocking(run);
            })
            .detach();
        cx.spawn(async move |this, cx| {
            while let Ok(run) = rx.recv().await {
                let done = run.state != shell_run::State::Running;
                let mut row = run.row();
                row.sent_at = sent_at;
                let updated = this.update(cx, |this, cx| {
                    this.transcript
                        .update(cx, |view, cx| view.patch(at, vec![row], cx));
                    if done {
                        this.shell.running = None;
                        this.summary.updated_at = monocode_core::time::now_ms();
                        // A stopped command stays in the transcript and goes
                        // with the next message instead.
                        match share.take() {
                            Some((question, attachments))
                                if run.state != shell_run::State::Cancelled =>
                            {
                                this.share_shell(question, attachments, cx)
                            }
                            _ => this.pump_shell(cx),
                        }
                        cx.emit(SessionChanged);
                    }
                    cx.notify();
                });
                if done || updated.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.emit(SessionChanged);
    }

    /// Commands waiting for the turn or the run ahead of them.
    pub(super) fn shell_queue(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.shell.queue.is_empty() {
            return None;
        }
        let card = composer_shelf::card("shell-queue-card", self.classic_layout).children(
            self.shell.queue.iter().enumerate().map(|(index, queued)| {
                let id = queued.id;
                div()
                    .id(("queued-shell", id as usize))
                    .role(Role::Group)
                    .aria_label(format!("Queued command: {}", queued.command))
                    .min_h(px(32.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .when(index > 0, |d| d.border_t_1().border_color(content(0.05)))
                    .child(icon("terminal", 14.).text_color(input::Input::shell_color()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(crate::fonts::mono())
                            .text_size(px(12.))
                            .text_color(content(0.75))
                            .child(queued.command.clone()),
                    )
                    .child(
                        div()
                            .id(("queued-shell-remove", id as usize))
                            .role(Role::Button)
                            .aria_label("Remove queued command")
                            .size(px(24.))
                            .flex_shrink_0()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(content(0.5))
                            .hover(|s| s.bg(content(0.08)).text_color(content(0.9)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(icon("close", 14.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.shell.queue.retain(|queued| queued.id != id);
                                this.pump_shell(cx);
                                cx.notify();
                            })),
                    )
            }),
        );
        Some(card.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, WindowHandle};
    use monocode_core::store::Store;
    use std::sync::Mutex;

    #[test]
    fn the_border_fades_into_bash_mode_and_back_and_snaps_when_motion_is_reduced() {
        let t0 = Instant::now();
        let mut fade = BorderFade::default();
        assert_eq!(fade.mix(false, t0, false), (0., false));
        let (start, moving) = fade.mix(true, t0, false);
        assert!(moving && start == 0.);
        let (halfway, moving) = fade.mix(true, t0 + BORDER_FADE / 2, false);
        assert!(moving && halfway > 0. && halfway < 1.);
        let (leaving, moving) = fade.mix(false, t0 + BORDER_FADE / 2, false);
        assert!(
            moving && (leaving - halfway).abs() < 1e-6,
            "a flip resumes from where it was"
        );
        assert_eq!(fade.mix(false, t0 + BORDER_FADE * 2, false), (0., false));
        assert_eq!(fade.mix(true, t0 + BORDER_FADE * 2, true), (1., false));
    }

    fn conversation(cx: &mut TestAppContext) -> WindowHandle<Conversation> {
        let cwd = std::env::temp_dir().canonicalize().unwrap();
        let store = Store::open(Path::new(":memory:")).unwrap();
        // A provider that fails after saving the turn, so nothing real launches.
        let mut summary = store.create("codex", cwd.to_str().unwrap()).unwrap();
        summary.provider = "fixture".into();
        cx.update(|cx| {
            input::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                    view.loaded = true;
                    view.input
                        .update(cx, |input, cx| input.set_shell_commands(true, cx));
                    view
                })
            })
            .unwrap()
        })
    }
    fn runs(view: &Conversation, cx: &gpui::App) -> Vec<ShellRun> {
        view.transcript
            .read(cx)
            .source_rows()
            .iter()
            .filter(|row| row.role == shell_run::ROLE)
            .filter_map(|row| ShellRun::parse(&row.text))
            .collect()
    }

    #[gpui::test]
    fn a_bang_draft_swaps_the_model_pills_for_a_bash_chip(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(crate::settings::Preferences::default());
            cx.set_reduce_motion(true);
        });
        let window = conversation(cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |view, _, cx| {
                view.loaded = true;
                view.input
                    .update(cx, |input, cx| input.set_text("echo hi".into(), cx));
                cx.notify();
            })
            .unwrap();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("composer-model").is_some());
        assert!(visual.debug_bounds("composer-shell-chip").is_none());
        window
            .update(visual, |view, _, cx| {
                view.input
                    .update(cx, |input, cx| input.set_text("!echo hi".into(), cx));
            })
            .unwrap();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("composer-shell-chip").is_some());
        assert!(visual.debug_bounds("composer-model").is_none());
    }

    /// The rows after the shell row: the turn that shared it.
    fn after_run(view: &Conversation, cx: &gpui::App) -> Vec<monocode_core::transcript::Row> {
        let rows = view.transcript.read(cx).source_rows();
        let at = rows
            .iter()
            .rposition(|row| row.role == shell_run::ROLE)
            .unwrap();
        rows[at + 1..].to_vec()
    }

    #[gpui::test]
    fn a_bang_command_runs_then_starts_a_turn_with_its_question(cx: &mut TestAppContext) {
        for (draft, question) in [
            ("!echo shell-ok", ""),
            ("!echo shell-ok\nWhy is that?", "Why is that?"),
        ] {
            let window = conversation(cx);
            window
                .update(cx, |view, _, cx| {
                    view.input
                        .update(cx, |input, cx| input.set_text(draft.into(), cx));
                    assert!(view.input.read(cx).shell_mode());
                    view.submit_shell(cx);
                    assert!(!view.busy, "the command runs before the turn");
                    assert!(view.shell_running());
                    assert!(view.summary.has_messages);
                    assert!(view.input.read(cx).content.is_empty());
                    let runs = runs(view, cx);
                    assert_eq!(runs.len(), 1);
                    assert_eq!(runs[0].command, "echo shell-ok");
                    assert_eq!(runs[0].state, shell_run::State::Running);
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |view, _, cx| {
                    assert!(!view.shell_running());
                    let runs = runs(view, cx);
                    assert_eq!(runs[0].state, shell_run::State::Completed);
                    assert_eq!(runs[0].output.trim(), "shell-ok");
                    let turn = after_run(view, cx);
                    assert_eq!(turn[0].role, "user", "the run starts a turn right away");
                    assert_eq!(turn[0].text, question);
                    let rows = view.transcript.read(cx).source_rows();
                    assert_eq!(
                        rows,
                        locked(&view.store)
                            .unwrap()
                            .load(view.summary.id)
                            .unwrap()
                            .rows
                    );
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn a_stopped_command_starts_no_turn(cx: &mut TestAppContext) {
        let window = conversation(cx);
        window
            .update(cx, |view, _, cx| {
                view.input
                    .update(cx, |input, cx| input.set_text("!sleep 5".into(), cx));
                view.submit_shell(cx);
                view.cancel_shell();
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                assert!(!view.busy && !view.shell_busy());
                assert_eq!(runs(view, cx)[0].state, shell_run::State::Cancelled);
                assert!(after_run(view, cx).is_empty());
            })
            .unwrap();
    }

    #[gpui::test]
    fn escape_stops_a_running_command(cx: &mut TestAppContext) {
        let window = conversation(cx);
        window
            .update(cx, |view, _, cx| {
                view.input
                    .update(cx, |input, cx| input.set_text("!sleep 30".into(), cx));
                view.submit_shell(cx);
                assert!(view.shell_running() && !view.busy);
                view.stop_if_busy(cx);
            })
            .unwrap();
        let started = Instant::now();
        cx.run_until_parked();
        assert!(started.elapsed() < Duration::from_secs(10));
        window
            .update(cx, |view, _, cx| {
                assert!(!view.shell_busy());
                assert_eq!(runs(view, cx)[0].state, shell_run::State::Cancelled);
                assert!(after_run(view, cx).is_empty());
            })
            .unwrap();
    }

    #[gpui::test]
    fn programmatic_and_escaped_bangs_are_plain_messages(cx: &mut TestAppContext) {
        for (text, sent) in [
            ("!rm -rf nothing", "!rm -rf nothing"),
            ("\\!important", "!important"),
        ] {
            let window = conversation(cx);
            window
                .update(cx, |view, _, cx| {
                    // Automations and quick submit fill the composer and call `send`.
                    view.input
                        .update(cx, |input, cx| input.set_text(text.into(), cx));
                    view.send(cx);
                    assert!(view.busy);
                    assert!(!view.shell_busy());
                    let rows = view.transcript.read(cx).source_rows();
                    assert!(rows.iter().all(|row| row.role != shell_run::ROLE));
                    assert_eq!(rows[0].text, sent);
                })
                .unwrap();
            cx.run_until_parked();
        }
    }

    #[gpui::test]
    fn commands_sent_during_a_turn_run_after_it_and_messages_wait_for_them(
        cx: &mut TestAppContext,
    ) {
        let window = conversation(cx);
        window
            .update(cx, |view, _, cx| {
                view.input
                    .update(cx, |input, cx| input.set_text("Start".into(), cx));
                view.send(cx);
                assert!(view.busy);
                view.input
                    .update(cx, |input, cx| input.set_text("!echo queued".into(), cx));
                view.submit_shell(cx);
                assert!(!view.shell_running(), "the turn runs first");
                assert!(view.shell_busy());
                assert!(runs(view, cx).is_empty());
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                assert!(!view.busy);
                assert!(!view.shell_busy());
                assert_eq!(runs(view, cx)[0].output.trim(), "queued");
                assert_eq!(after_run(view, cx)[0].role, "user");
                // A message sent while a command runs waits behind its turn.
                view.input
                    .update(cx, |input, cx| input.set_text("!sleep 0.2".into(), cx));
                view.submit_shell(cx);
                assert!(view.shell_running());
                view.input
                    .update(cx, |input, cx| input.set_text("What happened?".into(), cx));
                view.send(cx);
                assert!(!view.busy);
                assert_eq!(view.follow_ups.state.messages[0].text, "What happened?");
                let shared = view
                    .transcript
                    .read(cx)
                    .source_rows()
                    .iter()
                    .filter(|row| row.role == "user")
                    .count();
                assert_eq!(shared, 2);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                assert_eq!(runs(view, cx).len(), 2);
                let turn = after_run(view, cx);
                assert_eq!(turn[0].role, "user", "the second run shared its own turn");
                assert!(turn.iter().all(|row| row.text != "What happened?"));
            })
            .unwrap();
    }

    #[gpui::test]
    fn shared_store_keeps_the_run_before_the_turn_it_starts(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!(
            "monocode-shell-shared-{}-{}",
            std::process::id(),
            monocode_core::time::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = Store::open_monocode(&dir.join("sessions.db")).unwrap();
        let mut summary = store.create("codex", dir.to_str().unwrap()).unwrap();
        let mut earlier = monocode_core::transcript::Transcript::default();
        earlier.begin("user");
        earlier.append("Earlier question");
        earlier.begin("assistant");
        earlier.append(&"A long earlier answer line\n".repeat(40));
        store.write_rows(summary.id, 0, &earlier.rows).unwrap();
        let loaded = store.load(summary.id).unwrap().rows;
        summary.provider = "fixture".into();
        summary.has_messages = true;
        let store = Arc::new(Mutex::new(store));
        let window = cx.update(|cx| {
            input::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, store.clone(), cx);
                    view.loaded = true;
                    view.input
                        .update(cx, |input, cx| input.set_shell_commands(true, cx));
                    view.transcript
                        .update(cx, |transcript, cx| transcript.patch(0, loaded, cx));
                    view
                })
            })
            .unwrap()
        });
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                view.input.update(cx, |input, cx| {
                    input.set_text("!echo from-shell".into(), cx)
                });
                view.submit_shell(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                let saved = locked(&view.store)
                    .unwrap()
                    .load(view.summary.id)
                    .unwrap()
                    .rows;
                assert_eq!(view.transcript.read(cx).source_rows(), saved.as_slice());
                let user = saved.iter().rposition(|row| row.role == "user").unwrap();
                assert_eq!(saved[user].text, "");
                assert_eq!(saved[user - 1].role, shell_run::ROLE, "{saved:#?}");
                let run = ShellRun::parse(&saved[user - 1].text).unwrap();
                assert_eq!(run.output.trim(), "from-shell");
            })
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
