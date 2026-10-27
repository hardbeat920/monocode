//! Retain unchanged text-row heights and share probes within each frame.
use super::*;

#[derive(PartialEq)]
struct LayoutStyle {
    text: gpui::TextStyle,
    rem: Pixels,
    scale: f32,
    reduced_motion: bool,
}

#[derive(Default)]
pub(super) struct Measurements {
    style: Option<LayoutStyle>,
    viewport: gpui::Size<Pixels>,
    rows: HashMap<usize, Pixels>,
    /// Bubble widths by message start, with the content they were measured for.
    prompt_widths: HashMap<usize, (u64, Pixels)>,
    retained: HashMap<(usize, usize), RetainedHeight>,
    frame: u64,
    #[cfg(test)]
    pub(super) fresh: usize,
    #[cfg(test)]
    fresh_prompt_widths: usize,
}
struct RetainedHeight {
    height: Pixels,
    row: UiRow,
    after_receipt: bool,
    frame: u64,
}

impl Measurements {
    pub(super) fn rows_changed(&mut self) {
        // Prompt widths stay: a rebuilt message checks its fingerprint.
        self.rows.clear();
    }
    /// Heights depend on the viewport. Prompt widths are natural text widths
    /// and the bubble's `max_w` and flex shrinking handle narrow windows.
    fn viewport_changed(&mut self) {
        self.rows.clear();
        self.retained.clear();
    }
    pub(super) fn clear(&mut self) {
        self.viewport_changed();
        self.prompt_widths.clear();
    }
}

pub(super) fn prompt_fingerprint(rows: &[UiRow], attachments: &[attachments::Attachment]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut state = std::collections::hash_map::DefaultHasher::new();
    for row in rows {
        if let Body::Plain(text) = &row.body {
            text.hash(&mut state);
        }
    }
    attachments.len().hash(&mut state);
    for attachment in attachments {
        attachment.hash_layout(&mut state);
    }
    state.finish()
}

/// Shapes a user bubble's text as render lays it out.
pub(super) fn shape_prompt(
    text: SharedString,
    wrap_width: Option<Pixels>,
    window: &Window,
) -> impl Iterator<Item = gpui::WrappedLine> {
    // The transcript inherits the shell's UI font; the bubble sets it too.
    let run = gpui::TextRun {
        len: text.len(),
        font: gpui::font(crate::fonts::UI),
        color: Default::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_text(text, PROMPT_TEXT_SIZE, &[run], wrap_width, None)
        .unwrap_or_default()
        .into_iter()
}

fn prompt_bubble_width(widths: impl Iterator<Item = Pixels>) -> Pixels {
    // Match StyledText's intrinsic measurement. Without rounding up, adding
    // then subtracting the padding can leave less room than the widest line.
    // The minimum also leaves room for the disclosure on a blank last chunk.
    // Text at least as wide as the cap renders exactly as the cap does, so
    // stop shaping there: pasted logs and code usually reach it at once.
    let cap = PROMPT_BUBBLE_MAX - PROMPT_PAD_X;
    let mut widest = PROMPT_DISCLOSURE_MIN;
    for width in widths {
        widest = widest.max(width);
        if widest >= cap {
            return PROMPT_BUBBLE_MAX;
        }
    }
    (widest.ceil() + PROMPT_PAD_X).min(PROMPT_BUBBLE_MAX)
}

impl TranscriptView {
    /// Virtualized chunks share one bubble width, even when only a narrow
    /// continuation is on screen. Single-row prompts keep their natural layout.
    pub(super) fn split_prompt_width(&mut self, ix: usize, window: &Window) -> Option<Pixels> {
        let row = &self.rows[ix];
        let message = &self.messages[row.message];
        if !self.chat_layout || message.role != "user" || (row.first && row.last) {
            return None;
        }
        let fingerprint = message.prompt_fingerprint;
        if let Some((cached, width)) = self.measurements.prompt_widths.get(&message.start)
            && *cached == fingerprint
        {
            return Some(*width);
        }
        let width = if message.attachments.is_empty() {
            prompt_bubble_width(
                self.rows[message.display_start..]
                    .iter()
                    .take_while(|other| other.message == row.message)
                    .filter_map(|other| match &other.body {
                        Body::Plain(text) => Some(text.clone()),
                        _ => None,
                    })
                    .flat_map(|text| shape_prompt(text, None, window))
                    .map(|line| line.width()),
            )
        } else {
            // Attachment chips wrap inside the same column as every text chunk.
            PROMPT_BUBBLE_MAX
        };
        if self.measurements.prompt_widths.len() >= 256 {
            self.measurements.prompt_widths.clear();
        }
        #[cfg(test)]
        {
            self.measurements.fresh_prompt_widths += 1;
        }
        self.measurements
            .prompt_widths
            .insert(message.start, (fingerprint, width));
        Some(width)
    }

    pub(super) fn begin_measurements(&mut self, window: &Window, cx: &Context<Self>) {
        let style = LayoutStyle {
            text: window.text_style(),
            rem: window.rem_size(),
            scale: window.scale_factor(),
            reduced_motion: cx.reduce_motion(),
        };
        if self.measurements.style.as_ref() != Some(&style) {
            // GPUI retains measured item heights independently of text styling.
            self.list.remeasure_items(0..self.list.item_count());
            self.phase_layouts.clear();
            self.measurements.style = Some(style);
            self.measurements.clear();
        }
        if self.measurements.viewport != self.viewport.size {
            self.measurements.viewport_changed();
            self.measurements.viewport = self.viewport.size;
        }
        self.measurements.rows.clear();
        let motions = &self.motions;
        let previous = self.measurements.frame;
        self.measurements.frame = previous.wrapping_add(1);
        self.measurements
            .retained
            .retain(|key, entry| entry.frame == previous && !motions.contains_key(key));
    }

    pub(super) fn measure_row_height(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Pixels {
        // The viewport probe can discover a resize after render's follow pass.
        if self.measurements.viewport != self.viewport.size {
            self.measurements.viewport_changed();
            self.measurements.viewport = self.viewport.size;
        }
        if let Some(height) = self.measurements.rows.get(&ix) {
            return *height;
        }
        let key = self.rows[ix].key;
        let stable = stable_height(&self.rows[ix], &self.messages, &self.motions);
        let after_receipt = after_receipt(&self.rows, &self.messages, ix);
        if stable
            && let Some(retained) = self.measurements.retained.get_mut(&key)
            && same_layout(&retained.row, &self.rows[ix])
            && retained.after_receipt == after_receipt
        {
            retained.frame = self.measurements.frame;
            self.measurements.rows.insert(ix, retained.height);
            return retained.height;
        }
        let mut row = self.row(ix, window, cx);
        let height = row
            .layout_as_root(
                size(
                    AvailableSpace::Definite(self.viewport.size.width),
                    AvailableSpace::MinContent,
                ),
                window,
                cx,
            )
            .height;
        #[cfg(test)]
        {
            self.measurements.fresh += 1;
        }
        self.measurements.rows.insert(ix, height);
        if stable {
            if self.measurements.retained.len() >= 256
                && !self.measurements.retained.contains_key(&key)
            {
                // This is a viewport/follow working set, never a history cache.
                self.measurements.retained.clear();
            }
            self.measurements.retained.insert(
                key,
                RetainedHeight {
                    height,
                    row: self.rows[ix].clone(),
                    after_receipt,
                    frame: self.measurements.frame,
                },
            );
        }
        height
    }
}

fn stable_height(
    row: &UiRow,
    messages: &[Message],
    motions: &HashMap<(usize, usize), Motion>,
) -> bool {
    // Independent child views, attachments and disclosures can resize without
    // a source patch. Retain only text geometry that rebuild invalidates.
    matches!(row.body, Body::Markdown(_))
        && !row.in_phase()
        && !motions.contains_key(&row.key)
        && messages[row.message].attachments.is_empty()
}

pub(super) fn after_receipt(rows: &[UiRow], messages: &[Message], ix: usize) -> bool {
    messages
        .get(rows[ix].message)
        .is_some_and(|message| message.under_line)
        || (ix > 0 && matches!(rows[ix - 1].body, Body::WorkLine { .. }))
}

pub(super) fn same_layout(a: &UiRow, b: &UiRow) -> bool {
    a.key == b.key
        && a.first == b.first
        && a.last == b.last
        && a.work == b.work
        && a.work_tail == b.work_tail
        && a.thought_phase == b.thought_phase
        && match (&a.body, &b.body) {
            (Body::Markdown(a), Body::Markdown(b)) => Arc::ptr_eq(a, b) || a == b,
            _ => false,
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Entity, Render, TestAppContext, WindowBounds, WindowOptions, point};
    use std::{cell::RefCell, rc::Rc, time::Duration};

    type Probe = Box<dyn FnOnce(&mut TranscriptView, &mut Window, &mut Context<TranscriptView>)>;

    struct Fixture {
        transcript: Entity<TranscriptView>,
        probe: Option<Probe>,
    }
    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let transcript = self.transcript.clone();
            let probe = self.probe.take();
            div()
                .relative()
                .size_full()
                .child(self.transcript.clone())
                .child(
                    canvas(
                        move |_, window, cx| {
                            if let Some(probe) = probe {
                                transcript.update(cx, |view, cx| probe(view, window, cx));
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
        }
    }

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }

    fn chat(cx: &mut TestAppContext) -> gpui::WindowHandle<Fixture> {
        fixture(cx, false)
    }

    fn fixture(cx: &mut TestAppContext, mono: bool) -> gpui::WindowHandle<Fixture> {
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(900.), px(600.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    let transcript = cx.new(|cx| {
                        let mut view = TranscriptView::new();
                        if mono {
                            view.mono_mode("cat", 0xe86b5f, "MonoCat");
                        }
                        view.visible = false;
                        view.streaming = true;
                        view.smooth_reveal = false;
                        view.patch(
                            0,
                            vec![row("user", "Question"), row("assistant", "A short answer.")],
                            cx,
                        );
                        view.visible = true;
                        view
                    });
                    cx.new(|_| Fixture {
                        transcript,
                        probe: None,
                    })
                },
            )
            .unwrap()
        });
        render(window, cx);
        render(window, cx);
        window
    }

    fn render(window: gpui::WindowHandle<Fixture>, cx: &mut TestAppContext) {
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
    }

    // GPUI layout must run in a real draw phase, rather than an update handler.
    fn probe<R: 'static>(
        window: gpui::WindowHandle<Fixture>,
        cx: &mut TestAppContext,
        inspect: impl FnOnce(&mut TranscriptView, &mut Window, &mut Context<TranscriptView>) -> R
        + 'static,
    ) -> R {
        let result = Rc::new(RefCell::new(None));
        let output = result.clone();
        window
            .update(cx, |fixture, _, cx| {
                fixture.probe = Some(Box::new(move |view, window, cx| {
                    *output.borrow_mut() = Some(inspect(view, window, cx));
                }));
                cx.notify();
            })
            .unwrap();
        render(window, cx);

        result.borrow_mut().take().expect("the layout probe ran")
    }

    fn answer(view: &TranscriptView) -> usize {
        view.rows
            .iter()
            .position(|row| matches!(row.body, Body::Markdown(_)))
            .unwrap()
    }

    #[test]
    fn prompt_padding_does_not_shorten_fractional_text_widths() {
        // These advances straddle an f32 precision boundary after padding is
        // added. The old calculation makes the available text width smaller.
        for base in [104_f32, 232., 488.] {
            let text = px(f32::from_bits(base.to_bits() + 1));
            assert!((text + px(24.)) - px(24.) < text);
            let bubble = prompt_bubble_width([px(40.), text, px(60.)].into_iter());
            assert!(bubble - px(24.) >= text, "the widest line must still fit");
        }
    }

    #[test]
    fn prompt_width_stops_measuring_at_the_bubble_cap() {
        let past_cap = std::iter::from_fn(|| -> Option<Pixels> { panic!("measured past the cap") });
        let lines = [px(100.), PROMPT_BUBBLE_MAX - PROMPT_PAD_X].into_iter();
        assert_eq!(
            prompt_bubble_width(lines.chain(past_cap)),
            PROMPT_BUBBLE_MAX
        );
        assert_eq!(
            prompt_bubble_width([px(551.5)].into_iter()),
            PROMPT_BUBBLE_MAX
        );
        assert_eq!(prompt_bubble_width([px(400.2)].into_iter()), px(425.));
        assert_eq!(
            prompt_bubble_width([px(10.)].into_iter()),
            PROMPT_DISCLOSURE_MIN + PROMPT_PAD_X
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn shared_prompt_width_keeps_real_font_lines_unwrapped() {
        let system = gpui::WindowTextSystem::new(Arc::new(gpui::TextSystem::new(Arc::new(
            gpui_apple::AppleTextSystem::new(),
        ))));
        for family in [".AppleSystemUIFont", "Helvetica", "Times New Roman"] {
            let (mut fitted, mut capped) = (0, 0);
            for count in 1..40 {
                let text: SharedString = format!("{} final word", "iW ".repeat(count)).into();
                let run = gpui::TextRun {
                    len: text.len(),
                    font: gpui::font(family),
                    color: Default::default(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let natural = system
                    .shape_text(
                        text.clone(),
                        PROMPT_TEXT_SIZE,
                        std::slice::from_ref(&run),
                        None,
                        None,
                    )
                    .unwrap();
                let widest = natural
                    .iter()
                    .map(|line| line.width())
                    .fold(px(0.), Pixels::max);
                let width = prompt_bubble_width(natural.iter().map(|line| line.width()));
                if widest >= PROMPT_BUBBLE_MAX - PROMPT_PAD_X {
                    // A capped bubble wraps its widest line, as render does.
                    assert_eq!(width, PROMPT_BUBBLE_MAX, "{family}, {count}");
                    capped += 1;
                    continue;
                }
                let wrapped = system
                    .shape_text(
                        text,
                        PROMPT_TEXT_SIZE,
                        &[run],
                        Some(width - PROMPT_PAD_X),
                        None,
                    )
                    .unwrap();
                assert!(
                    wrapped.iter().all(|line| line.wrap_boundaries.is_empty()),
                    "the widest line must not acquire another line: {family}, {count}"
                );
                fitted += 1;
            }
            assert!(fitted > 0 && capped > 0, "both paths ran for {family}");
        }
    }

    #[gpui::test]
    fn prompt_widths_survive_streaming_suffix_updates_and_refresh_after_edits(
        cx: &mut TestAppContext,
    ) {
        let window = chat(cx);
        probe(window, cx, |view, window, cx| {
            view.visible = false;
            let mut transcript = monocode_core::transcript::Transcript::default();
            transcript.begin("user");
            transcript.append(&"An earlier prompt\n".repeat(13));
            transcript.begin("assistant");
            transcript.append("An earlier answer");
            let prompt = transcript.begin("user");
            transcript.append(&format!(
                "A wide prompt line for measurement\n{}",
                "x\n".repeat(512)
            ));
            let assistant = transcript.begin("assistant");
            transcript.append("Answer");
            view.expanded_prompts.insert(0);
            view.expanded_prompts.insert(prompt);
            view.patch(0, transcript.rows, cx);
            view.begin_measurements(window, cx);
            let prefix_width = view.split_prompt_width(0, window).unwrap();
            let tail = view
                .rows
                .iter()
                .position(|row| view.messages[row.message].start == prompt && row.last)
                .unwrap();
            let width = view.split_prompt_width(tail, window).unwrap();
            let fresh = view.measurements.fresh_prompt_widths;
            for i in 0..10 {
                // More than one draw may separate streaming source patches.
                for _ in 0..3 {
                    view.begin_measurements(window, cx);
                }
                view.patch(
                    assistant,
                    vec![row("assistant", &format!("Answer {i}"))],
                    cx,
                );
                assert_eq!(view.split_prompt_width(tail, window), Some(width));
                assert_eq!(
                    view.measurements.fresh_prompt_widths, fresh,
                    "an unchanged, expanded prompt must not be measured again"
                );
            }
            // A continuation edit must invalidate the opening message's key,
            // even though the supplied source row is not its first row.
            let mut edited = view.source[prompt + 1].clone();
            assert!(!edited.first);
            edited.text = "A longer replacement line ".repeat(20);
            view.patch(prompt + 1, vec![edited], cx);
            assert!(view.split_prompt_width(tail, window).unwrap() > width);
            assert_eq!(view.split_prompt_width(0, window), Some(prefix_width));
            assert_eq!(view.measurements.fresh_prompt_widths, fresh + 1);

            window.set_rem_size(window.rem_size() + px(1.));
            view.begin_measurements(window, cx);
            assert!(
                view.measurements.prompt_widths.is_empty(),
                "style changes must still invalidate all prompt widths"
            );
            view.split_prompt_width(tail, window).unwrap();
            assert_eq!(view.measurements.fresh_prompt_widths, fresh + 2);
        });
    }

    fn prompt_tail(view: &TranscriptView, prompt: usize) -> usize {
        view.rows
            .iter()
            .position(|row| view.messages[row.message].start == prompt && row.last)
            .unwrap()
    }

    #[gpui::test]
    fn mono_prompt_widths_survive_streaming_replies_and_refresh_with_attachments(
        cx: &mut TestAppContext,
    ) {
        let window = fixture(cx, true);
        probe(window, cx, |view, window, cx| {
            assert!(view.mono && view.chat_layout);
            view.visible = false;
            let mut transcript = monocode_core::transcript::Transcript::default();
            let prompt = transcript.begin("user");
            transcript.append(&format!(
                "A wide prompt line for measurement\n{}",
                "x\n".repeat(64)
            ));
            let assistant = transcript.begin("assistant");
            transcript.append("Answer");
            view.expanded_prompts.insert(prompt);
            view.patch(0, transcript.rows, cx);
            view.begin_measurements(window, cx);
            let width = view
                .split_prompt_width(prompt_tail(view, prompt), window)
                .unwrap();
            let fresh = view.measurements.fresh_prompt_widths;
            for i in 0..10 {
                for _ in 0..3 {
                    view.begin_measurements(window, cx);
                }
                // Mono rebuilds the whole group, from the prompt that opens it.
                view.patch(
                    assistant,
                    vec![row("assistant", &format!("Answer {i}"))],
                    cx,
                );
                let tail = prompt_tail(view, prompt);
                assert_eq!(view.split_prompt_width(tail, window), Some(width));
                assert_eq!(
                    view.measurements.fresh_prompt_widths, fresh,
                    "a streaming reply must not measure its prompt again"
                );
            }
            // The manifest after a prompt changes its attachments, not its text.
            let manifest = Row {
                role: monocode_core::transcript::attachments::ROLE.into(),
                text: monocode_core::transcript::attachments::from_paths(&[
                    std::path::PathBuf::from("/tmp/notes.txt"),
                ])
                .to_string(),
                first: true,
                sent_at: None,
            };
            view.patch(assistant, vec![manifest, row("assistant", "Answer")], cx);
            assert!(
                !view.messages[view.rows[prompt_tail(view, prompt)].message]
                    .attachments
                    .is_empty()
            );
            let tail = prompt_tail(view, prompt);
            assert_eq!(
                view.split_prompt_width(tail, window),
                Some(PROMPT_BUBBLE_MAX)
            );
            assert_eq!(view.measurements.fresh_prompt_widths, fresh + 1);
        });
    }

    #[gpui::test]
    fn prompt_widths_survive_viewport_resizes(cx: &mut TestAppContext) {
        let window = chat(cx);
        let (prompt, width, viewport, fresh) = probe(window, cx, |view, window, cx| {
            let mut transcript = monocode_core::transcript::Transcript::default();
            let prompt = transcript.begin("user");
            transcript.append(&format!("A wide prompt line\n{}", "x\n".repeat(24)));
            transcript.begin("assistant");
            transcript.append("Answer");
            view.expanded_prompts.insert(prompt);
            view.patch(0, transcript.rows, cx);
            view.begin_measurements(window, cx);
            let width = view
                .split_prompt_width(prompt_tail(view, prompt), window)
                .unwrap();
            (
                prompt,
                width,
                view.viewport.size,
                view.measurements.fresh_prompt_widths,
            )
        });
        for width in [600., 400., 900.] {
            cx.simulate_window_resize(window.into(), size(px(width), px(600.)));
            render(window, cx);
            render(window, cx);
        }
        cx.simulate_window_resize(window.into(), size(px(500.), px(600.)));
        render(window, cx);
        probe(window, cx, move |view, window, cx| {
            view.begin_measurements(window, cx);
            assert_ne!(view.viewport.size, viewport, "the viewport resized");
            assert_eq!(view.measurements.viewport, view.viewport.size);
            let tail = prompt_tail(view, prompt);
            assert_eq!(view.split_prompt_width(tail, window), Some(width));
            assert_eq!(
                view.measurements.fresh_prompt_widths, fresh,
                "a resize must not measure prompt widths again"
            );
        });
    }

    #[gpui::test]
    fn retained_row_measurements_skip_unchanged_layout_and_match_fresh_heights(
        cx: &mut TestAppContext,
    ) {
        let window = chat(cx);
        probe(window, cx, |view, window, cx| {
            let ix = answer(view);
            let scroll = view.list.logical_scroll_top();
            view.begin_measurements(window, cx);
            let before = view.measurements.fresh;
            let retained = view.measure_row_height(ix, window, cx);
            assert_eq!(view.measurements.fresh, before);
            view.measurements.clear();
            let fresh = view.measure_row_height(ix, window, cx);
            assert!((retained - fresh).abs() < px(0.01));
            assert_eq!(view.measurements.fresh, before + 1);
            assert_eq!(view.list.logical_scroll_top().item_ix, scroll.item_ix);
            assert_eq!(
                view.list.logical_scroll_top().offset_in_item,
                scroll.offset_in_item
            );
            view.begin_measurements(window, cx);
            view.update_follow(window, cx);
            let after_follow = view.measurements.fresh;
            let first_footer = view.footer_height(window, cx);
            assert_eq!(view.footer_height(window, cx), first_footer);
            assert_eq!(view.measurements.fresh, after_follow);
        });
    }

    #[gpui::test]
    fn retained_row_measurements_refresh_after_text_resize_and_font_changes(
        cx: &mut TestAppContext,
    ) {
        let window = chat(cx);
        let short = probe(window, cx, |view, window, cx| {
            let short = view.measure_row_height(answer(view), window, cx);
            view.patch(
                1,
                vec![row("assistant", &"More words in the answer. ".repeat(40))],
                cx,
            );
            view.motions.clear();
            let ix = answer(view);
            view.begin_measurements(window, cx);
            let before = view.measurements.fresh;
            let grown = view.measure_row_height(ix, window, cx);
            assert!(grown > short);
            assert_eq!(view.measurements.fresh, before + 1);
            short
        });
        let wide = probe(window, cx, |view, window, cx| {
            view.measure_row_height(answer(view), window, cx)
        });
        cx.simulate_window_resize(window.into(), size(px(400.), px(600.)));
        render(window, cx);
        render(window, cx);
        probe(window, cx, move |view, window, cx| {
            let ix = answer(view);
            let narrow = view.measure_row_height(ix, window, cx);
            assert!(narrow > wide);
            assert!(wide > short);
            window.set_rem_size(px(22.));
            view.begin_measurements(window, cx);
            let before = view.measurements.fresh;
            let _ = view.measure_row_height(ix, window, cx);
            assert_eq!(view.measurements.fresh, before + 1);
        });
    }

    #[gpui::test]
    fn retained_row_measurements_keep_height_animation_live(cx: &mut TestAppContext) {
        let window = chat(cx);
        probe(window, cx, |view, window, cx| {
            let ix = answer(view);
            let full = view.measure_row_height(ix, window, cx);
            let now = Instant::now();
            view.motions
                .insert(view.rows[ix].key, Motion::new(true, false, now));
            view.frame_now = now + Duration::from_millis(30);
            view.begin_measurements(window, cx);
            let opening = view.measure_row_height(ix, window, cx);
            view.frame_now = now + Duration::from_millis(150);
            view.begin_measurements(window, cx);
            let later = view.measure_row_height(ix, window, cx);
            assert!(opening < later);
            assert!(later < full);
        });
    }

    #[gpui::test]
    fn retained_row_measurements_refresh_when_closing_rows_leave(cx: &mut TestAppContext) {
        let window = chat(cx);
        probe(window, cx, |view, window, cx| {
            view.patch(
                1,
                vec![row(
                    "assistant",
                    &format!("Short.\n\n{}", "A wrapped second paragraph. ".repeat(25)),
                )],
                cx,
            );
            view.motions.clear();
            view.begin_measurements(window, cx);
            let indices: Vec<_> = view
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| matches!(row.body, Body::Markdown(_)))
                .map(|(ix, _)| ix)
                .collect();
            assert_eq!(indices.len(), 2);
            let short = view.measure_row_height(indices[0], window, cx);
            let long = view.measure_row_height(indices[1], window, cx);
            assert!(long > short);
            view.motions.insert(
                view.rows[indices[0]].key,
                Motion::new(false, false, Instant::now() - Duration::from_secs(2)),
            );
            view.animate_frame(cx);
            view.begin_measurements(window, cx);
            let before = view.measurements.fresh;
            assert_eq!(view.measure_row_height(indices[0], window, cx), long);
            assert_eq!(view.measurements.fresh, before + 1);
        });
    }

    #[gpui::test]
    fn streaming_tail_changes_reuse_prefix_rows_and_measurements(cx: &mut TestAppContext) {
        let window = chat(cx);
        probe(window, cx, |view, window, cx| {
            let mut text =
                "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n\nTail".to_owned();
            view.patch(1, vec![row("assistant", &text)], cx);
            view.motions.clear();
            view.begin_measurements(window, cx);
            let first = answer(view);
            let key = view.rows[first].key;
            let Body::Markdown(original) = view.rows[first].body.clone() else {
                unreachable!()
            };
            let height = view.measure_row_height(first, window, cx);
            text.push_str(" grows while the prefix stays unchanged.");
            view.patch(1, vec![row("assistant", &text)], cx);
            view.motions.clear();
            view.begin_measurements(window, cx);
            let first = view.rows.iter().position(|row| row.key == key).unwrap();
            let Body::Markdown(current) = &view.rows[first].body else {
                unreachable!()
            };
            assert!(Arc::ptr_eq(&original, current));
            let before = view.measurements.fresh;
            assert_eq!(view.measure_row_height(first, window, cx), height);
            assert_eq!(
                view.measurements.fresh, before,
                "the stable prefix must not be laid out again"
            );
            assert!(
                view.reveals.is_empty(),
                "unanimated history must not retain reveal text or word timings"
            );
        });
    }
}
