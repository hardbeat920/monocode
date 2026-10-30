//! Compact interaction state survives an inactive transcript cache eviction.
use super::*;

use monocode_core::store::{HistoryPage, PAGE_TURNS};

/// Reads the page of turns before a store cursor, or with `true` all of them.
/// Called on the background executor.
pub(crate) type HistoryPager = Arc<dyn Fn(i64, bool) -> Result<HistoryPage, String> + Send + Sync>;

/// Keep source row indices intact for persistence, tools and live patches.
/// Only the displayed window needs Markdown and presentation objects.
pub(crate) struct PreparedHistory {
    rows: Vec<Row>,
    parsers: HashMap<usize, MarkdownParser>,
}
impl PreparedHistory {
    /// Run on the history loader, before handing its result to the UI.
    pub(crate) fn new(rows: Vec<Row>, mono: bool) -> Self {
        let start = if mono {
            page_start(&rows, rows.len())
        } else {
            0
        };
        let parsers = prepare_parsers(&rows[start..], start, mono);
        Self { rows, parsers }
    }
}

pub(super) fn page_start(rows: &[Row], before: usize) -> usize {
    rows[..before.min(rows.len())]
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, row)| row.first && row.is_prompt())
        .nth(PAGE_TURNS - 1)
        .map_or(0, |(at, _)| at)
}

fn prepare_parsers(rows: &[Row], offset: usize, mono: bool) -> HashMap<usize, MarkdownParser> {
    let mut parsers = HashMap::new();
    let mut at = 0;
    while at < rows.len() {
        let end = (at + 1..rows.len())
            .find(|&ix| rows[ix].first)
            .unwrap_or(rows.len());
        if rows[at].role == "assistant" || (!mono && rows[at].role == "reasoning") {
            let text: String = rows[at..end].iter().map(|row| row.text.as_str()).collect();
            let mut parser = MarkdownParser::new();
            parser.set_text(&text);
            parsers.insert(offset + at, parser);
        }
        at = end;
    }
    parsers
}

pub(super) struct HistoryPosition {
    key: Option<(usize, usize)>,
    offset: ListOffset,
    following: bool,
}
impl TranscriptView {
    pub(crate) fn install_prepared_history(
        &mut self,
        history: PreparedHistory,
        cx: &mut Context<Self>,
    ) {
        self.prepared_parsers = history.parsers;
        self.patch(0, history.rows, cx);
    }

    /// Where turns above the loaded rows come from. A paged Mono loads only
    /// its newest turns; scrolling up reads the rest a page at a time.
    pub(crate) fn set_history_pager(&mut self, before: Option<i64>, pager: Option<HistoryPager>) {
        self.history_before = before;
        self.history_pager = pager;
        self.history_failed = false;
    }
    /// Saved turn receipts, keyed by store row, for the window's turns.
    pub(crate) fn set_turns(&mut self, turns: HashMap<usize, monocode_core::agent::TurnInfo>) {
        let base = self.base;
        self.turns.retain(|&turn, _| turn <= base);
        self.turns
            .extend(turns.into_iter().map(|(turn, info)| (turn + base, info)));
    }
    /// The window's turn receipts, keyed by store row, as settings save them.
    pub(crate) fn window_turns(&self) -> HashMap<usize, monocode_core::agent::TurnInfo> {
        self.turns
            .iter()
            .filter(|(turn, _)| **turn > self.base)
            .map(|(turn, info)| (turn - self.base, info.clone()))
            .collect()
    }
    /// Reveal a saved search hit. Its row counts from the first turn, so read
    /// every older turn first; the loaded rows then number from it too.
    pub(crate) fn reveal_saved_search(&mut self, row: usize, query: &str, cx: &mut Context<Self>) {
        if self.history_before.is_some() {
            self.history_search = Some((row, query.into()));
            self.list.pause_following_tail();
            if !self.history_loading {
                self.load_older_page(true, cx);
            }
            return;
        }
        self.reveal_search(row, query, cx);
    }

    pub(super) fn load_older_history(&mut self, cx: &mut Context<Self>) {
        if self.history_loading {
            return;
        }
        if self.history_start == 0 {
            self.load_older_page(false, cx);
            return;
        }
        self.history_loading = true;
        self.list.pause_following_tail();
        let previous = self.history_start;
        let before = self
            .history_search
            .as_ref()
            .map_or(previous, |(row, _)| row.saturating_add(1).min(previous));
        let start = page_start(&self.source, before);
        // Snapshot only this page. A render/scroll callback must never copy
        // the long-lived conversation or parse its Markdown synchronously.
        let rows = self.source[start..previous].to_vec();
        let epoch = self.highlight_epoch;
        let mono = self.mono;
        let work = cx
            .background_executor()
            .spawn(async move { prepare_parsers(&rows, start, mono) });
        cx.spawn(async move |this, cx| {
            let parsers = work.await;
            let _ = this.update(cx, |this, cx| {
                if epoch != this.highlight_epoch || this.history_start != previous {
                    return;
                }
                this.history_loading = false;
                this.history_start = start;
                this.prepared_parsers.extend(parsers);
                this.rebuild(start, cx);
                if let Some((row, query)) = this.history_search.take() {
                    this.reveal_search(row, &query, cx);
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Read the turns above the loaded rows from the store, off the main thread.
    fn load_older_page(&mut self, all: bool, cx: &mut Context<Self>) {
        let (Some(before), Some(pager)) = (self.history_before, self.history_pager.clone()) else {
            return;
        };
        self.history_loading = true;
        self.list.pause_following_tail();
        let epoch = self.highlight_epoch;
        let mono = self.mono;
        let work = cx.background_executor().spawn(async move {
            let page = pager(before, all)?;
            let parsers = prepare_parsers(&page.rows, 0, mono);
            Ok::<_, String>((page, parsers))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if epoch != this.highlight_epoch || this.history_before != Some(before) {
                    return;
                }
                this.history_loading = false;
                // A failed read keeps the cursor; the next scroll retries.
                this.history_failed = result.is_err();
                if let Ok((page, parsers)) = result {
                    // A pending saved hit counts from the first turn, not from
                    // the loaded rows, so it must not move with them.
                    let search = this.history_search.take();
                    this.prepend_history(page, parsers, cx);
                    if let Some((row, query)) = search {
                        this.reveal_saved_search(row, &query, cx);
                    }
                }
            });
        })
        .detach();
    }
    /// Put an older page above the loaded rows. Every view row index moves
    /// down by the page; store rows keep their numbers through `base`.
    fn prepend_history(
        &mut self,
        page: HistoryPage,
        parsers: HashMap<usize, MarkdownParser>,
        cx: &mut Context<Self>,
    ) {
        self.history_before = page.before;
        let k = page.rows.len();
        if k == 0 {
            return;
        }
        self.shift_rows(&|row| Some(row + k));
        for message in &mut self.messages {
            message.start += k;
            message.turn += k;
        }
        for row in &mut self.rows {
            if row.key.0 < SAVED_DRAFT_START {
                row.key.0 += k;
            }
        }
        self.base += k;
        self.source_bytes = page.rows.iter().fold(self.source_bytes, |bytes, row| {
            bytes.saturating_add(row.text.capacity().saturating_add(row.role.capacity()))
        });
        let mut source = page.rows;
        source.append(&mut self.source);
        self.source = source;
        for (i, row) in self.source[..k].iter().enumerate() {
            if row.role == "tool"
                && row.first
                && serde_json::from_str::<ToolCall>(&row.text)
                    .is_ok_and(|call| matches!(call.panel(), Some(Panel::Agent(_))))
            {
                self.agent_panel_rows.insert(i);
            }
        }
        for (turn, info) in page.turns {
            self.turns.entry(turn).or_insert(info);
        }
        self.prepared_parsers.extend(parsers);
        self.history_start = 0;
        self.rebuild(0, cx);
    }
    /// Renumber every source-indexed piece of state; `None` drops it.
    fn shift_rows(&mut self, f: &dyn Fn(usize) -> Option<usize>) {
        fn map<V>(map: &mut HashMap<usize, V>, f: &dyn Fn(usize) -> Option<usize>) {
            *map = std::mem::take(map)
                .into_iter()
                .filter_map(|(row, value)| Some((f(row)?, value)))
                .collect();
        }
        fn set(set: &mut HashSet<usize>, f: &dyn Fn(usize) -> Option<usize>) {
            *set = std::mem::take(set).into_iter().filter_map(f).collect();
        }
        fn pair<T>(value: &mut Option<(usize, T)>, f: &dyn Fn(usize) -> Option<usize>) {
            *value = value.take().and_then(|(row, rest)| Some((f(row)?, rest)));
        }
        // Sentinels, such as the saved draft's anchor, are not rows.
        let row = |at: usize| {
            if at >= SAVED_DRAFT_START {
                Some(at)
            } else {
                f(at)
            }
        };
        self.agent_panel_rows = std::mem::take(&mut self.agent_panel_rows)
            .into_iter()
            .filter_map(f)
            .collect();
        map(&mut self.visuals, f);
        map(&mut self.prepared_parsers, f);
        map(&mut self.entrances, f);
        map(&mut self.reactions, f);
        map(&mut self.note_feedback, f);
        map(&mut self.phases, f);
        map(&mut self.phase_scrolls, f);
        map(&mut self.work_folds, f);
        map(&mut self.reveals, f);
        map(&mut self.step_queue, f);
        map(&mut self.turns, f);
        map(&mut self.elapsed, f);
        map(&mut self.fold_targets, f);
        for target in self.fold_targets.values_mut() {
            *target = target.and_then(f);
        }
        set(&mut self.note_pending, f);
        set(&mut self.expanded, f);
        set(&mut self.expanded_prompts, f);
        set(&mut self.clamped_prompts, f);
        set(&mut self.windowed_phases, f);
        set(&mut self.seen_steps, f);
        self.action_anchors = std::mem::take(&mut self.action_anchors)
            .into_iter()
            .filter_map(|((at, name), slot)| Some(((f(at)?, name), slot)))
            .collect();
        self.motions = std::mem::take(&mut self.motions)
            .into_iter()
            .filter_map(|((at, part), motion)| Some(((f(at)?, part), motion)))
            .collect();
        pair(&mut self.history_search, f);
        pair(&mut self.search_match, f);
        pair(&mut self.copied_response, f);
        pair(&mut self.hovered_action, f);
        pair(&mut self.target_action, f);
        pair(&mut self.prompt_rise, f);
        self.copied = self
            .copied
            .and_then(|at| f(at as usize))
            .map(|at| at as u64);
        self.phase_toggle = self.phase_toggle.and_then(f);
        self.anchor_turn = self.anchor_turn.and_then(row);
        self.introduced_prompt = self.introduced_prompt.and_then(f);
        self.live_start = f(self.live_start).unwrap_or(0);
        if let Some(burst) = &mut self.burst {
            match f(burst.prompt) {
                Some(prompt) => burst.prompt = prompt,
                None => self.burst = None,
            }
        }
        self.mono_groups.get_mut().clear();
        self.hover_rows.clear();
        self.hovered_turn = None;
        self.hovered_prompt = None;
        self.hovered_prompt_actions = None;
        self.phase_layouts.clear();
        self.selection.clear();
        self.outline = Default::default();
    }
    /// Forget older pages; the next history numbers rows from the window again.
    fn drop_older_history(&mut self) {
        let base = std::mem::take(&mut self.base);
        self.history_before = None;
        if base > 0 {
            self.shift_rows(&|row| row.checked_sub(base));
        }
    }

    pub(crate) fn replace_history(&mut self, rows: Vec<Row>, cx: &mut Context<Self>) {
        let len = rows.len();
        self.drop_older_history();
        self.selection.clear();
        self.history_position = None;
        self.history_loading = false;
        self.history_requested = false;
        self.history_search = None;
        self.prepared_parsers.clear();
        self.mono_groups.get_mut().clear();
        self.active_provider_turn_id = None;
        self.search_match = None;
        self.anchor_turn = None;
        self.prompt_rise = None;
        self.prompt_rise_pending = false;
        self.introduced_prompt = None;
        self.turns.retain(|&start, _| start < len);
        self.elapsed.retain(|&start, _| start < len);
        self.expanded.retain(|&start| start < len);
        self.phases.retain(|&start, _| start < len);
        self.phase_layouts.clear();
        self.attachment_images.clear();
        self.visuals.clear();
        self.reveals.clear();
        self.markdown_cache = Default::default();
        self.measurements = Default::default();
        self.motions.clear();
        self.entrances.clear();
        self.hover_rows.clear();
        self.hovered_turn = None;
        self.hovered_prompt = None;
        self.hovered_prompt_actions = None;
        self.outline = Default::default();
        self.reflow = Default::default();
        self.follow = Default::default();
        self.highlight_epoch = self.highlight_epoch.wrapping_add(1);
        self.highlights.clear();
        self.highlighting.clear();
        self.highlight_queue.clear();
        self.highlight_active = false;
        self.highlight_order.clear();
        self.highlight_bytes = 0;
        self.restore_history(rows, cx);
    }
    /// A conservative payload estimate, including the source, parsed markdown
    /// and presentation copies. The inactive cache also has a session count cap.
    pub(crate) fn history_weight(&self) -> usize {
        self.source_bytes
            .saturating_mul(4)
            .saturating_add(
                self.source
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Row>()),
            )
            .saturating_add(self.highlight_bytes)
            .saturating_add(self.markdown_cache.retained_bytes())
            // Charge each retained interpreter's cap to the inactive cache.
            .saturating_add(
                self.visuals
                    .len()
                    .saturating_mul(monocode_core::visual::MEMORY_LIMIT_BYTES),
            )
    }
    pub(crate) fn suspend_history(&mut self, cx: &mut Context<Self>) -> bool {
        if self.visible
            || self.streaming
            || self.history_position.is_some()
            || !self.pending.is_empty()
            || self.saved_draft.is_some()
            || !self.note_pending.is_empty()
            || self.target_action.is_some()
        {
            return false;
        }
        let offset = self.list.logical_scroll_top();
        let base = self.base;
        self.drop_older_history();
        self.selection.clear();
        self.history_position = Some(HistoryPosition {
            key: self
                .rows
                .get(offset.item_ix)
                .and_then(|row| Some((row.key.0.checked_sub(base)?, row.key.1))),
            offset,
            following: self.list.is_following_tail(),
        });
        self.source = Vec::new();
        self.agent_panel_rows.clear();
        self.source_bytes = 0;
        self.history_start = 0;
        self.history_loading = false;
        self.history_requested = false;
        self.history_search = None;
        self.prepared_parsers = HashMap::new();
        self.mono_groups = Default::default();
        self.attachment_images = HashMap::new();
        self.visuals = HashMap::new();
        self.messages = Vec::new();
        self.rows = Vec::new();
        self.reveals = HashMap::new();
        self.markdown_cache = Default::default();
        self.measurements = Default::default();
        self.phase_layouts = HashMap::new();
        self.motions = HashMap::new();
        self.entrances = HashMap::new();
        self.taken = Vec::new();
        self.hover_rows = HashMap::new();
        self.hovered_turn = None;
        self.hovered_prompt = None;
        self.hovered_prompt_actions = None;
        self.action_anchors = HashMap::new();
        self.outline = Default::default();
        self.reflow = Default::default();
        self.follow = Default::default();
        // A running syntax task owns its input until completion; its old epoch
        // cannot repopulate this cache after suspension or after a quick reopen.
        self.highlight_epoch = self.highlight_epoch.wrapping_add(1);
        self.highlights = HashMap::new();
        self.highlighting = HashSet::new();
        self.highlight_queue = VecDeque::new();
        self.highlight_active = false;
        self.highlight_order = VecDeque::new();
        self.highlight_bytes = 0;
        self.list.reset(1);
        cx.notify();
        true
    }
    pub(crate) fn restore_history(&mut self, rows: Vec<Row>, cx: &mut Context<Self>) {
        self.restore_prepared_history(PreparedHistory::new(rows, self.mono), cx);
    }
    pub(crate) fn restore_prepared_history(
        &mut self,
        history: PreparedHistory,
        cx: &mut Context<Self>,
    ) {
        let position = self.history_position.take();
        self.drop_older_history();
        // Reopening also handles a source that became empty in another window.
        self.source = Vec::new();
        self.agent_panel_rows.clear();
        self.source_bytes = 0;
        self.messages = Vec::new();
        self.rows = Vec::new();
        self.history_start = 0;
        self.history_loading = false;
        self.history_requested = false;
        self.mono_groups.get_mut().clear();
        self.list.reset(1);
        self.install_prepared_history(history, cx);
        if let Some(position) = position {
            if position.following {
                self.list.set_follow_mode(FollowMode::Tail);
                self.list.scroll_to_end();
            } else {
                self.list.pause_following_tail();
                let item_ix = position
                    .key
                    .and_then(|key| self.rows.iter().position(|row| row.key == key))
                    .unwrap_or(position.offset.item_ix.min(self.rows.len()));
                self.list.scroll_to(ListOffset {
                    item_ix,
                    offset_in_item: position.offset.offset_in_item,
                });
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn long_mono(turns: usize) -> Vec<Row> {
        (0..turns).flat_map(|turn| [
            Row { role: "user".into(), text: format!("Question {turn}"), first: true, sent_at: Some(turn as u64) },
            Row { role: "assistant".into(), text: format!("Answer **{turn}**.\n\nA second paragraph with [a link](https://example.com)."), first: true, sent_at: None },
        ]).collect()
    }

    #[gpui::test]
    fn long_mono_initial_display_and_live_patches_keep_history_unmaterialized(
        cx: &mut TestAppContext,
    ) {
        let source = long_mono(5_000);
        let prepared = PreparedHistory::new(source, true);
        assert_eq!(prepared.parsers.len(), PAGE_TURNS);
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.mono_mode("mono", 0xffffff, "Mono");
            view.install_prepared_history(prepared, cx);
            assert_eq!(view.source.len(), 10_000);
            assert_eq!(view.messages.len(), PAGE_TURNS * 2);
            assert!(
                view.rows.len() < 100,
                "initial layout must depend on the recent page"
            );
            assert_eq!(view.messages[0].start, 9_980);
            assert_eq!(
                view.source[0].text, "Question 0",
                "the archived source remains available"
            );
            view.streaming = true;
            let mut next = long_mono(1);
            next[0].text = "New question".into();
            next[1].text = "New answer".into();
            view.patch(10_000, next, cx);
            assert_eq!(view.source.len(), 10_002);
            assert!(view.messages.len() <= (PAGE_TURNS + 1) * 2);
            assert_eq!(view.messages.last().unwrap().start, 10_001);
            assert_eq!(view.source[0].text, "Question 0");
        });
    }

    #[gpui::test]
    fn internal_mono_reports_also_keep_the_initial_display_bounded(cx: &mut TestAppContext) {
        let mut rows = long_mono(1_000);
        for row in &mut rows {
            if row.is_prompt() {
                row.role = "mono-system".into();
            }
        }
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.mono_mode("mono", 0xffffff, "Mono");
            view.install_prepared_history(PreparedHistory::new(rows, true), cx);
            assert_eq!(view.messages.len(), PAGE_TURNS * 2);
            assert_eq!(view.messages[0].start, 1_980);
            assert!(view.rows.len() < 100);
            assert!(
                view.rows
                    .iter()
                    .any(|row| matches!(row.body, Body::WorkLine { .. }))
            );
            assert_eq!(view.source.len(), 2_000);
        });
    }

    #[gpui::test]
    fn mono_group_cache_follows_changed_prompt_boundaries(cx: &mut TestAppContext) {
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.mono_mode("mono", 0xffffff, "Mono");
            let mut rows = long_mono(2);
            rows[2].role = "mono-system".into();
            view.patch(0, rows, cx);
            assert_eq!(view.mono_group_start(3), 1);
            let mut prompt = view.source[2].clone();
            prompt.role = "user".into();
            view.patch(2, vec![prompt], cx);
            assert_eq!(view.mono_group_start(3), 3);
        });
    }

    #[gpui::test]
    fn older_mono_pages_preserve_the_reading_anchor_and_search_reaches_archived_rows(
        cx: &mut TestAppContext,
    ) {
        let view = cx.new(|_| TranscriptView::new());
        let (key, offset) = view.update(cx, |view, cx| {
            view.mono_mode("mono", 0xffffff, "Mono");
            view.install_prepared_history(PreparedHistory::new(long_mono(35), true), cx);
            view.list.pause_following_tail();
            let offset = ListOffset {
                item_ix: 4,
                offset_in_item: px(7.),
            };
            view.list.scroll_to(offset);
            let key = view.rows[4].key;
            view.load_older_history(cx);
            view.load_older_history(cx);
            (key, offset.offset_in_item)
        });
        cx.run_until_parked();
        view.update(cx, |view, cx| {
            assert!(!view.history_loading);
            assert_eq!(
                view.messages[0].start, 30,
                "one request adds ten earlier turns"
            );
            let restored = view.list.logical_scroll_top();
            assert_eq!(view.rows[restored.item_ix].key, key);
            assert_eq!(restored.offset_in_item, offset);
            view.reveal_search(1, "Answer", cx);
        });
        cx.run_until_parked();
        view.update(cx, |view, _| {
            assert_eq!(view.search_match, Some((1, "Answer".into())));
            assert!(view.messages.iter().any(|message| message.start == 1));
            assert_eq!(view.source.len(), 70);
        });
    }

    #[gpui::test]
    fn paged_mono_reads_older_turns_above_a_window_whose_rows_keep_their_numbers(
        cx: &mut TestAppContext,
    ) {
        use monocode_core::agent::TurnInfo;
        let full = long_mono(35);
        // A store whose window starts at row 50 and whose cursors are rows.
        let pager: HistoryPager = {
            let full = full.clone();
            Arc::new(move |before, all| {
                let before = before as usize;
                let start = if all || before <= 20 { 0 } else { before - 20 };
                let turns = HashMap::from([(
                    1,
                    TurnInfo {
                        model: format!("Model {}", start / 2),
                        ..Default::default()
                    },
                )]);
                Ok(HistoryPage {
                    rows: full[start..before].to_vec(),
                    turns,
                    before: (start > 0).then_some(start as i64),
                })
            })
        };
        let view = cx.new(|_| TranscriptView::new());
        let key = view.update(cx, |view, cx| {
            view.mono_mode("mono", 0xffffff, "Mono");
            view.install_prepared_history(PreparedHistory::new(full[50..].to_vec(), true), cx);
            view.set_history_pager(Some(50), Some(pager));
            view.set_turns(HashMap::from([(
                1,
                TurnInfo {
                    model: "Window".into(),
                    ..Default::default()
                },
            )]));
            assert!(!view.history_complete());
            view.list.pause_following_tail();
            view.list.scroll_to(ListOffset {
                item_ix: 2,
                offset_in_item: px(5.),
            });
            let key = view.rows[2].key;
            view.load_older_history(cx);
            key
        });
        cx.run_until_parked();
        view.update(cx, |view, cx| {
            assert!(!view.history_loading);
            assert_eq!(view.base, 20);
            assert_eq!(view.source_rows(), &full[50..]);
            assert_eq!(view.history_rows(), &full[30..]);
            assert_eq!(view.messages[0].start, 0, "the page is displayed");
            assert_eq!(view.turns[&1].model, "Model 15");
            assert_eq!(view.turns[&21].model, "Window");
            assert_eq!(view.window_turns()[&1].model, "Window");
            assert_eq!(view.window_turns().len(), 1);
            let restored = view.list.logical_scroll_top();
            assert_eq!(view.rows[restored.item_ix].key, (key.0 + 20, key.1));
            assert_eq!(restored.offset_in_item, px(5.));

            // Live patches stay in the store's numbering.
            view.patch(20, long_mono(1)[..1].to_vec(), cx);
            assert_eq!(view.source_rows().len(), 21);
            assert_eq!(view.history_rows()[40].text, "Question 0");

            view.reveal_saved_search(1, "Answer", cx);
        });
        cx.run_until_parked();
        view.update(cx, |view, cx| {
            assert!(view.history_complete());
            assert_eq!(view.base, 50);
            assert_eq!(view.history_rows()[..70], full[..]);
            assert_eq!(view.search_match, Some((1, "Answer".into())));
            assert_eq!(view.turns[&1].model, "Model 0");
            assert_eq!(view.turns[&31].model, "Model 15");
            assert_eq!(view.turns[&51].model, "Window");

            // Reopening numbers rows from the window again.
            view.restore_history(full[50..].to_vec(), cx);
            assert_eq!(view.base, 0);
            assert_eq!(view.history_rows(), &full[50..]);
            assert_eq!(view.turns.len(), 1);
            assert_eq!(view.turns[&1].model, "Window");
        });
    }

    #[gpui::test]
    fn replacing_history_discards_an_in_flight_mono_page(cx: &mut TestAppContext) {
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.mono_mode("mono", 0xffffff, "Mono");
            view.install_prepared_history(PreparedHistory::new(long_mono(25), true), cx);
            view.load_older_history(cx);
            view.replace_history(long_mono(1), cx);
        });
        cx.run_until_parked();
        view.update(cx, |view, _| {
            assert!(!view.history_loading);
            assert_eq!(view.source.len(), 2);
            assert_eq!(view.messages.len(), 2);
            assert_eq!(view.history_start, 0);
        });
        view.update(cx, |view, cx| view.replace_history(Vec::new(), cx));
        view.update(cx, |view, _| {
            assert!(view.source.is_empty() && view.messages.is_empty());
            assert_eq!(view.history_start, 0);
        });
    }

    fn rows() -> Vec<Row> {
        vec![
            Row {
                role: "user".into(),
                text: "Find the needle".into(),
                first: true,
                sent_at: Some(7),
            },
            Row {
                role: "assistant".into(),
                text: "needle answer\n\nsecond paragraph".into(),
                first: true,
                sent_at: Some(8),
            },
        ]
    }

    #[gpui::test]
    fn suspended_history_releases_payload_and_restores_search_folds_and_scroll(
        cx: &mut TestAppContext,
    ) {
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.visible(false, cx);
            let original = rows();
            view.patch(0, original.clone(), cx);
            view.reveal_search(1, "needle", cx);
            view.phases.insert(1, true);
            let offset = view.list.logical_scroll_top();
            let key = view.rows.get(offset.item_ix).unwrap().key;
            assert!(view.history_weight() > 0);
            let epoch = view.highlight_epoch;
            assert!(view.suspend_history(cx));
            assert!(view.source.is_empty() && view.rows.is_empty() && view.messages.is_empty());
            assert_eq!(view.history_weight(), 0);
            assert_ne!(view.highlight_epoch, epoch);
            assert!(!view.suspend_history(cx));
            view.restore_history(original.clone(), cx);
            assert_eq!(view.source.len(), original.len());
            assert_eq!(view.source[0].sent_at, Some(7));
            assert_eq!(view.search_match, Some((1, "needle".into())));
            assert!(view.expanded.contains(&1));
            assert_eq!(view.phases.get(&1), Some(&true));
            assert!(!view.list.is_following_tail());
            let restored = view.list.logical_scroll_top();
            assert_eq!(view.rows[restored.item_ix].key, key);
            assert_eq!(restored.offset_in_item, offset.offset_in_item);
        });
    }

    #[gpui::test]
    fn visible_running_and_unsaved_action_histories_are_pinned(cx: &mut TestAppContext) {
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.patch(0, rows(), cx);
            assert!(!view.suspend_history(cx));
            view.visible(false, cx);
            view.streaming = true;
            assert!(!view.suspend_history(cx));
            view.streaming = false;
            view.note_pending.insert(1);
            assert!(!view.suspend_history(cx));
            view.note_pending.clear();
            assert!(view.suspend_history(cx));
            view.restore_history(Vec::new(), cx);
            assert!(view.source.is_empty());
            assert!(view.history_position.is_none());
        });
    }
}
