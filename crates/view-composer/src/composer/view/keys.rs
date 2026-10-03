//! The textarea's `onKeyDown`, `pickSkill`, `pickMention`, the mode
//! toggles, and last-turn recall from Composer.tsx.
//!
//! Keys reach the prompt as actions. The composer's root element captures
//! them on the way down and stops propagation when it handles one, so the
//! prompt's default (a newline, a caret move) only runs otherwise. An open
//! IME composition owns its keys, like `isImeComposition`.

use gpui::{Context, KeyDownEvent, Window};

use super::super::host::{FolderTarget, SessionFolder};
use super::super::model::commands::{
    self, SpaceKeyInput, consume_btw_command, consume_btw_prefix, is_compact_command,
    is_mcp_command, is_session_folder_command, runs_session_folder_command_on_space,
};
use super::super::model::mcp::{McpConnection, new_mcp_tag, tagged_mcp_servers};
use super::super::model::mentions::{ProjectFile, mention_label, replace_mention_token};
use super::super::model::mode_commands::Mode;
use super::super::model::skills::{Skill, replace_slash_token};
use super::super::prompt_input::{Enter, Escape, MoveDown, MoveUp, Tab};
use super::{Composer, ComposerEvent};

/// What a captured key did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyOutcome {
    /// The composer handled it; the prompt's default must not run.
    Handled,
    /// Let the prompt's default run.
    Default,
}

impl Composer {
    fn keys_blocked(&self, cx: &gpui::App) -> bool {
        self.props.disabled || self.creating_skill || self.prompt.read(cx).is_composing()
    }

    pub(crate) fn on_enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.enter(window, cx) == KeyOutcome::Handled {
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_move_up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.arrow(true, window, cx) == KeyOutcome::Handled {
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_move_down(
        &mut self,
        _: &MoveDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.arrow(false, window, cx) == KeyOutcome::Handled {
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_tab(&mut self, _: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab(window, cx) == KeyOutcome::Handled {
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if self.escape(window, cx) == KeyOutcome::Handled {
            cx.stop_propagation();
        }
    }

    /// A space at the end of `/add-to-folder` opens the folder picker.
    pub(crate) fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key != "space" || self.keys_blocked(cx) {
            return;
        }
        let (text, selection) = {
            let prompt = self.prompt.read(cx);
            (prompt.text().to_string(), prompt.selection())
        };
        let modifiers = event.keystroke.modifiers;
        let runs = runs_session_folder_command_on_space(&SpaceKeyInput {
            text: &text,
            selection_start: selection.start,
            selection_end: selection.end,
            alt_key: modifiers.alt,
            ctrl_key: modifiers.control,
            meta_key: modifiers.platform,
        });
        if runs {
            cx.stop_propagation();
            self.open_session_folder_picker(window, cx);
        }
    }

    /// Enter without Shift.
    pub(crate) fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) -> KeyOutcome {
        if self.keys_blocked(cx) {
            return KeyOutcome::Default;
        }
        let value = self.prompt_text(cx);
        if consume_btw_command(&value).matched {
            self.submit(window, cx);
            return KeyOutcome::Handled;
        }
        if self.mention_open() {
            if let Some(file) = self.ranked_files.get(self.mention_active).cloned() {
                self.pick_mention(&file.file, window, cx);
                return KeyOutcome::Handled;
            }
            self.mention = None;
        }
        if is_compact_command(&value) || is_mcp_command(&value) || is_session_folder_command(&value)
        {
            self.submit(window, cx);
            return KeyOutcome::Handled;
        }
        if let Some(slash) = self.slash.clone() {
            if let Some(skill) = self.ranked_skills().get(self.skill_active).cloned() {
                self.pick_skill(&skill, window, cx);
                return KeyOutcome::Handled;
            }
            if slash.query.is_empty() {
                return KeyOutcome::Handled;
            }
            self.slash = None;
        }
        self.submit(window, cx);
        KeyOutcome::Handled
    }

    /// Arrow Up (`up`) or Down.
    pub(crate) fn arrow(
        &mut self,
        up: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> KeyOutcome {
        if self.keys_blocked(cx) {
            return KeyOutcome::Default;
        }
        if self.mention_open() {
            let len = self.ranked_files.len();
            if len > 0 {
                self.mention_active = step(self.mention_active, len, up);
                cx.notify();
            }
            return KeyOutcome::Handled;
        }
        if self.slash.is_some() {
            let len = self.ranked_skills().len();
            if len > 0 {
                self.skill_active = step(self.skill_active, len, up);
                cx.notify();
            }
            return KeyOutcome::Handled;
        }
        let selection = self.prompt.read(cx).selection();
        if up
            && self.props.edit_last_turn_supported
            && self.navigation_empty()
            && selection.start == 0
            && selection.end == 0
        {
            self.recall_last_turn(_window, cx);
            return KeyOutcome::Handled;
        }
        KeyOutcome::Default
    }

    pub(crate) fn tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> KeyOutcome {
        if self.keys_blocked(cx) {
            return KeyOutcome::Default;
        }
        if self.mention_open() {
            if let Some(file) = self.ranked_files.get(self.mention_active).cloned() {
                self.pick_mention(&file.file, window, cx);
            }
            return KeyOutcome::Handled;
        }
        if self.slash.is_some() {
            if let Some(skill) = self.ranked_skills().get(self.skill_active).cloned() {
                self.pick_skill(&skill, window, cx);
            }
            return KeyOutcome::Handled;
        }
        KeyOutcome::Default
    }

    pub(crate) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> KeyOutcome {
        if self.props.disabled || self.prompt.read(cx).is_composing() {
            return KeyOutcome::Default;
        }
        if self.creating_skill {
            self.cancel_create_skill(window, cx);
            return KeyOutcome::Handled;
        }
        if self.mcp_picker_open {
            self.dismiss_mcp_picker(true, window, cx);
            return KeyOutcome::Handled;
        }
        if self.session_folder_open {
            self.session_folder_open = false;
            self.focus(window, cx);
            cx.notify();
            return KeyOutcome::Handled;
        }
        if self.plus_open {
            self.plus_open = false;
            cx.notify();
            return KeyOutcome::Handled;
        }
        if self.mention_open() {
            self.mention = None;
            self.refresh_ranked_files(cx);
            cx.notify();
            return KeyOutcome::Handled;
        }
        if self.slash.is_some() {
            self.slash = None;
            cx.notify();
            return KeyOutcome::Handled;
        }
        KeyOutcome::Default
    }

    /// `pickSkill`.
    pub(crate) fn pick_skill(
        &mut self,
        skill: &Skill,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(token) = self.slash.clone() else {
            self.slash = None;
            self.creating_skill = false;
            cx.notify();
            return;
        };
        let value = self.prompt_text(cx);
        if skill.is_builtin(commands::MCP) {
            let rest = &value[token.end.min(value.len())..];
            let rest = rest.strip_prefix(char::is_whitespace).unwrap_or(rest);
            let next = format!("{}{}", &value[..token.start], rest);
            self.mcp_insert_at = Some(token.start);
            self.set_prompt(next, token.start, cx);
            self.report_draft(cx);
            self.sync_has_value();
            self.slash = None;
            self.open_mcp_picker(window, cx);
            return;
        }
        if skill.is_builtin(commands::SESSION_FOLDER) && self.props.folders_enabled {
            let invocation = commands::SESSION_FOLDER;
            let next = replace_slash_token(&value, &token, invocation);
            let mut cursor = token.start + invocation.len() + 1;
            if next[cursor.min(next.len())..].starts_with(' ') {
                cursor += 1;
            }
            self.set_prompt(next, cursor.min(value.len() + invocation.len() + 2), cx);
            self.sync_has_value();
            self.slash = None;
            self.creating_skill = false;
            self.open_session_folder_picker(window, cx);
            return;
        }
        let next = replace_slash_token(&value, &token, &skill.invocation);
        let mut cursor = token.start + skill.invocation.len() + 1;
        if next[cursor.min(next.len())..].starts_with(' ') {
            cursor += 1;
        }
        let cursor = cursor.min(next.len());
        self.set_prompt(next.clone(), cursor, cx);
        self.sync_has_value();
        self.slash = None;
        self.creating_skill = false;
        if skill.is_builtin(commands::BTW) {
            self.enter_btw_from_prefix(&next, window, cx);
        }
        self.focus(window, cx);
        self.report_draft(cx);
        cx.notify();
    }

    /// `pickMention`.
    pub(crate) fn pick_mention(
        &mut self,
        file: &ProjectFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(token) = self.mention.clone() else {
            self.mention = None;
            cx.notify();
            return;
        };
        let value = self.prompt_text(cx);
        let label = if super::super::model::highlight::is_note_mention_path(&file.path) {
            file.relative.clone()
        } else {
            mention_label(file, &self.mention_index)
        };
        let next = replace_mention_token(&value, &token, &label);
        let mut cursor = token.start + label.len() + 1;
        if next[cursor.min(next.len())..].starts_with(' ') {
            cursor += 1;
        }
        let cursor = cursor.min(next.len());
        self.set_prompt(next, cursor, cx);
        self.sync_has_value();
        self.mention = None;
        self.refresh_ranked_files(cx);
        self.focus(window, cx);
        self.report_draft(cx);
        cx.notify();
    }

    /// `enterBtwFromPrefix`: `/btw ` opens the side conversation as soon as
    /// it is typed, carrying the rest over as the unsent question.
    pub(crate) fn enter_btw_from_prefix(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.props.btw_enabled
            || self.props.inbox_card.is_some()
            || self.props.note_card.is_some()
            || self.props.handoff_card.is_some()
        {
            return false;
        }
        if !self.attachments.is_empty() {
            return false;
        }
        let Some(rest) = consume_btw_prefix(value) else {
            return false;
        };
        let host = self.host.clone();
        if !host.btw(rest, true, window, cx) {
            return false;
        }
        self.draft_revision += 1;
        self.set_prompt(String::new(), 0, cx);
        self.report_draft_text("", cx);
        self.sync_has_value();
        self.slash = None;
        self.mention = None;
        cx.notify();
        true
    }

    // Modes.

    /// `clearLeadingMode`: turning a mode off also drops its leading
    /// command from the text.
    pub(crate) fn clear_leading_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let value = self.prompt_text(cx);
        if super::super::model::mode_commands::leading_mode_command(&value, &self.skill_names())
            .map(|token| token.mode)
            != Some(mode)
        {
            return;
        }
        // `/^\/[a-z]+\s?/`.
        let name_end = 1 + value[1..]
            .bytes()
            .take_while(u8::is_ascii_lowercase)
            .count();
        let end = match value[name_end..].chars().next() {
            Some(c) if c.is_whitespace() => name_end + c.len_utf8(),
            _ => name_end,
        };
        let next = value[end..].to_string();
        self.set_prompt(next, 0, cx);
        self.report_draft(cx);
        self.sync_has_value();
    }

    /// A row in the + menu.
    pub(crate) fn toggle_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.mode_active(mode);
        self.modes = Default::default();
        match mode {
            Mode::Plan => self.modes.plan = !active,
            Mode::Operator => self.modes.operator = !active,
            Mode::Orchestrator => self.modes.orchestration = !active,
            Mode::Draft => self.modes.draft = !active,
            Mode::Btw => {}
        }
        if active {
            self.clear_leading_mode(mode, cx);
        }
        self.plus_open = false;
        self.focus(window, cx);
        cx.notify();
    }

    /// A mode pill's clear button.
    pub(crate) fn clear_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        match mode {
            Mode::Plan => self.modes.plan = false,
            Mode::Operator => self.modes.operator = false,
            Mode::Orchestrator => self.modes.orchestration = false,
            Mode::Draft => self.modes.draft = false,
            Mode::Btw => {}
        }
        self.clear_leading_mode(mode, cx);
        self.focus(window, cx);
        cx.notify();
    }

    // Pickers.

    /// `openSessionFolderPicker`.
    pub(crate) fn open_session_folder_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cwd = self.props.cwd.clone();
        self.session_folders = self.host.session_folders(&cwd, cx);
        self.session_folder_open = true;
        let folders = self.session_folders.clone();
        self.bar.open_folder_picker(&folders, window, cx);
        cx.notify();
    }

    /// SessionFolderPicker's `onPick`.
    pub(crate) fn pick_session_folder(
        &mut self,
        target: FolderTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session_folder_open = false;
        self.session_folder_selected = true;
        let host = self.host.clone();
        host.place_in_folder(target, window, cx);
        let value = self.prompt_text(cx);
        let trimmed = value.trim_start();
        if trimmed.eq_ignore_ascii_case("/add-to-folder") {
            let next = format!("{value} ");
            let end = next.len();
            self.set_prompt(next, end, cx);
            self.sync_has_value();
        }
        self.focus(window, cx);
        cx.notify();
    }

    pub(crate) fn dismiss_session_folder_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session_folder_open = false;
        self.focus(window, cx);
        cx.notify();
    }

    /// `openMcpPicker`.
    pub(crate) fn open_mcp_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.props.execution_cwd.clone();
        self.mcp_servers = self.host.mcp_servers(&cwd, self.props.harness, cx);
        self.mcp_picker_open = true;
        let (servers, harness) = (self.mcp_servers.clone(), self.props.harness);
        self.bar.open_mcp_picker(servers, harness, window, cx);
        cx.notify();
    }

    /// McpServerPicker's `onPick`: tag the server inline, once.
    pub(crate) fn pick_mcp_server(
        &mut self,
        server: &McpConnection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous = self
            .selected_mcp
            .iter()
            .find(|tag| {
                tag.server.provider == server.provider
                    && tag.server.name == server.name
                    && tag.server.scope == server.scope
                    && tag.server.config_path == server.config_path
            })
            .cloned();
        let tag = previous
            .clone()
            .unwrap_or_else(|| new_mcp_tag(server, &self.selected_mcp));
        if previous.is_none() {
            self.selected_mcp.push(tag.clone());
            self.save_mcp_tags(cx);
        }
        let value = self.prompt_text(cx);
        if previous.is_none() || tagged_mcp_servers(&value, std::slice::from_ref(&tag)).is_empty() {
            let caret = self.prompt.read(cx).selection_start();
            let mut at = self.mcp_insert_at.unwrap_or(caret).min(value.len());
            while !value.is_char_boundary(at) {
                at -= 1;
            }
            let before = &value[..at];
            let after = &value[at..];
            let leading = if !before.is_empty() && !before.ends_with(char::is_whitespace) {
                " "
            } else {
                ""
            };
            let trailing = if after.starts_with(char::is_whitespace) && !after.is_empty() {
                ""
            } else {
                " "
            };
            let insertion = format!("{leading}{}{trailing}", tag.token);
            let next = format!("{before}{insertion}{after}");
            self.draft_revision += 1;
            self.set_prompt(next, at + insertion.len(), cx);
            self.sync_has_value();
            self.mention = None;
            self.report_draft(cx);
        }
        self.mcp_insert_at = None;
        self.mcp_picker_open = false;
        self.bar.close_mcp_picker(cx);
        self.prompt
            .update(cx, |prompt, cx| prompt.invalidate_decorations(cx));
        self.focus(window, cx);
        cx.notify();
    }

    pub(crate) fn dismiss_mcp_picker(
        &mut self,
        refocus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mcp_insert_at = None;
        self.mcp_picker_open = false;
        self.bar.close_mcp_picker(cx);
        if refocus {
            self.focus(window, cx);
        }
        cx.notify();
    }

    /// The MCP picker's Manage row.
    pub(crate) fn manage_mcp(&mut self, cx: &mut Context<Self>) {
        self.mcp_insert_at = None;
        self.mcp_picker_open = false;
        self.bar.close_mcp_picker(cx);
        cx.emit(ComposerEvent::OpenMcpSettings);
        cx.notify();
    }

    /// SkillPicker's create row.
    pub(crate) fn start_create_skill(&mut self, cx: &mut Context<Self>) {
        self.creating_skill = true;
        self.create_error = None;
        cx.notify();
    }

    pub(crate) fn cancel_create_skill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.creating_skill = false;
        self.create_error = None;
        self.sync_tokens(cx);
        self.focus(window, cx);
        cx.notify();
    }

    /// SkillPicker's `onCreate`.
    pub(crate) fn create_skill(
        &mut self,
        name: String,
        scope: super::super::host::NewSkillScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_busy = true;
        self.create_error = None;
        let cwd = self.props.execution_cwd.clone();
        let task = self.host.create_skill(&cwd, &name, scope, cx);
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.create_busy = false;
                match result {
                    Ok(path) => {
                        if let Some(token) = this.slash.clone() {
                            let value = this.prompt_text(cx);
                            let rest = &value[token.end.min(value.len())..];
                            let rest = rest.strip_prefix(char::is_whitespace).unwrap_or(rest);
                            let next = format!("{}{}", &value[..token.start], rest);
                            this.set_prompt(next, token.start, cx);
                            this.sync_has_value();
                        }
                        this.creating_skill = false;
                        this.slash = None;
                        this.create_error = None;
                        let context = this.skill_context();
                        this.host.reload_skills(&context, true, cx);
                        cx.emit(ComposerEvent::OpenFile { path, line: None });
                        this.focus(window, cx);
                    }
                    Err(error) => this.create_error = Some(error),
                }
                cx.notify();
            })
            .ok();
        });
        self._tasks.push(task);
        cx.notify();
    }

    // Last-turn recall.

    /// `recallLastTurn`: Up in an empty composer edits the last turn; again
    /// leaves edit mode.
    pub fn recall_last_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.props.edit_last_turn_supported {
            return;
        }
        let Some(recall) = self.props.last_turn_recall.clone() else {
            return;
        };
        if self.resend_edited {
            self.exit_edit_mode(window, cx);
            return;
        }
        let borrowed = recall
            .attachments
            .iter()
            .map(|file| file.id.clone())
            .collect();
        self.restore_draft(&recall.text, recall.attachments, Some(borrowed), window, cx);
        self.set_resend_edited(true, cx);
        cx.notify();
    }

    /// `exitEditMode`: Cancel edit.
    pub fn exit_edit_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft_revision += 1;
        self.paste_generation += 1;
        self.set_prompt(String::new(), 0, cx);
        self.context_items.clear();
        self.report_draft_text("", cx);
        for file in std::mem::take(&mut self.attachments) {
            if !self.borrowed_attachment_ids.remove(&file.id) {
                self.host.revoke_attachment(&file, cx);
            }
        }
        self.set_resend_edited(false, cx);
        self.plus_open = false;
        self.slash = None;
        self.mention = None;
        self.mcp_picker_open = false;
        self.selected_mcp.clear();
        self.save_mcp_tags(cx);
        self.sync_has_value();
        self.focus(window, cx);
        cx.notify();
    }
}

/// `(index ± 1) mod len`.
fn step(index: usize, len: usize, up: bool) -> usize {
    if up {
        (index + len - 1) % len
    } else {
        (index + 1) % len
    }
}

/// Session folder rows for the picker.
pub(crate) fn folder_rows(
    folders: &[SessionFolder],
) -> Vec<crate::pickers::session_folder_picker::SessionFolderRow> {
    folders
        .iter()
        .map(
            |folder| crate::pickers::session_folder_picker::SessionFolderRow {
                id: folder.id.clone().into(),
                name: folder.name.clone().into(),
                session_count: folder.session_count,
            },
        )
        .collect()
}
