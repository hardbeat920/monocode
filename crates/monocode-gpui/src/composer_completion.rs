//! The slash/@ layer lives beside Input, so typing never rebuilds the transcript
//! or shell. Its popover keeps native text/IME focus in the composer.
use crate::{
    conversation::git_actions::GitAction,
    input,
    popover::{Anchor, AnchorSlot, Side, popover},
    theme::*,
};
use gpui::{
    App, Context, Entity, EventEmitter, Focusable, KeyBinding, MouseButton, Role, SharedString,
    Subscription, Window, actions, div, prelude::*, px,
};
use monocode_core::{
    composer::{self, Token, Trigger},
    files::ProjectFile,
    skills::DiscoveredSkill,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

actions!(completion, [Accept, Dismiss]);
pub(crate) fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", Accept, Some("Completion")),
        KeyBinding::new("escape", Dismiss, Some("Completion")),
    ]);
}
pub(crate) struct ModePicked(pub &'static str);
#[derive(Clone)]
struct Choice {
    label: String,
    detail: String,
    insertion: String,
    builtin: Option<&'static str>,
    mode: Option<&'static str>,
    file: bool,
    server: Option<monocode_core::mcp::McpConnection>,
}
impl Choice {
    fn icon(&self) -> (&'static str, gpui::Hsla) {
        if let Some(command) = self.builtin.and_then(input::Command::from_name) {
            return (command.icon(), command.color());
        }
        let (path, dark, light) = match self.builtin {
            Some("btw") => ("command-btw", 0x99f6e4, 0x0f766e),
            Some("compact") => ("command-compact", 0xc4b5fd, 0x6d28d9),
            Some("stage") => (GitAction::Stage.icon(), 0xbbf7d0, 0x15803d),
            Some("commit") => (GitAction::Commit.icon(), 0xfed7aa, 0xc2410c),
            Some("visualize") => return ("command-visualize", accent_color()),
            _ => {
                return (
                    if self.insertion.starts_with("@note/") {
                        "note"
                    } else {
                        "sparkles"
                    },
                    accent_color(),
                );
            }
        };
        let dark_theme = content(1.).l > 0.5;
        let mut color: gpui::Hsla = gpui::rgb(if dark_theme { dark } else { light }).into();
        if dark_theme {
            color.a = 0.9;
        }
        (path, color)
    }
}
pub(crate) struct Completion {
    input: Entity<input::Input>,
    store: Option<crate::runtime::Database>,
    notes: Vec<monocode_core::notes::Note>,
    cwd: String,
    token: Option<Token>,
    dismissed: Option<Token>,
    skills: Vec<DiscoveredSkill>,
    provider: monocode_core::provider::Kind,
    native_commands: Vec<monocode_core::provider::NativeCommand>,
    native_loading: bool,
    native_revision: usize,
    mcp: Vec<monocode_core::mcp::McpConnection>,
    selected_mcp: Vec<(String, monocode_core::mcp::McpConnection)>,
    mcp_open: bool,
    disabled_paths: Vec<String>,
    known: std::collections::HashSet<String>,
    files: Vec<ProjectFile>,
    /// `@name` → `@relative/path` for file names unique in the project; the
    /// picker writes the short label and sending expands it back.
    short_mentions: std::collections::HashMap<String, String>,
    choices: Vec<Choice>,
    active: usize,
    scroll: gpui::ScrollHandle,
    loading: bool,
    loaded: Option<Instant>,
    error: Option<String>,
    generation: usize,
    cancel: Arc<AtomicBool>,
    anchor: AnchorSlot,
    composer_anchor: AnchorSlot,
    container_anchor: AnchorSlot,
    inline_field: bool,
    _observe: Subscription,
}
impl EventEmitter<ModePicked> for Completion {}
/// Composer.tsx's `buildMentionIndex`: a name is a label only when it is
/// unique and needs no quoting.
fn short_mentions(files: &[ProjectFile]) -> std::collections::HashMap<String, String> {
    let mut counts = std::collections::HashMap::<&str, usize>::new();
    for file in files {
        *counts.entry(file.name.as_str()).or_default() += 1;
    }
    files
        .iter()
        .filter(|file| {
            file.name != file.relative
                && counts[file.name.as_str()] == 1
                && !file.name.chars().any(char::is_whitespace)
        })
        .map(|file| {
            (
                composer::mention_text(&file.name),
                composer::mention_text(&file.relative),
            )
        })
        .collect()
}
impl Drop for Completion {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Completion {
    pub(crate) fn new(input: Entity<input::Input>, cwd: String, cx: &mut Context<Self>) -> Self {
        input.update(cx, |input, cx| input.enable_inline_commands(cx));
        let observe = cx.observe(&input, |this, _, cx| this.sync(cx));
        Self {
            input,
            store: None,
            notes: Vec::new(),
            cwd,
            token: None,
            dismissed: None,
            skills: Vec::new(),
            provider: monocode_core::provider::Kind::Codex,
            native_commands: Vec::new(),
            native_loading: false,
            native_revision: 0,
            mcp: Vec::new(),
            selected_mcp: Vec::new(),
            mcp_open: false,
            disabled_paths: Vec::new(),
            known: [
                "/plan",
                "/operator",
                "/orchestrator",
                "/compact",
                "/mcp",
                "/btw",
                "/stage",
                "/commit",
                "/visualize",
            ]
            .map(str::to_owned)
            .into_iter()
            .collect(),
            files: Vec::new(),
            short_mentions: Default::default(),
            choices: Vec::new(),
            active: 0,
            scroll: gpui::ScrollHandle::new(),
            loading: false,
            loaded: None,
            error: None,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            anchor: AnchorSlot::default(),
            composer_anchor: AnchorSlot::default(),
            container_anchor: AnchorSlot::default(),
            inline_field: false,
            _observe: observe,
        }
    }
    pub(crate) fn with_store(mut self, store: crate::runtime::Database) -> Self {
        self.store = Some(store);
        self
    }
    /// Growing document fields anchor suggestions beside the visible caret.
    pub(crate) fn inline_field(mut self) -> Self {
        self.inline_field = true;
        self
    }
    /// Keep composer suggestions inside the conversation, below its tabs.
    pub(crate) fn container_probe(&self) -> impl IntoElement {
        self.container_anchor.layout_probe()
    }
    pub(crate) fn composer_probe(&self) -> impl IntoElement {
        // Include the composer's 1px border instead of measuring its padded
        // text area, so the menu's edges align with the whole input box.
        div()
            .absolute()
            .inset(px(-1.))
            .child(self.composer_anchor.layout_probe())
    }
    pub(crate) fn set_cwd(&mut self, cwd: &str, cx: &mut Context<Self>) {
        if self.cwd == cwd {
            return;
        }
        self.cancel.store(true, Ordering::Relaxed);
        self.generation += 1;
        self.cwd = cwd.into();
        self.skills.clear();
        self.native_commands.clear();
        self.native_loading = false;
        self.native_revision = self.native_revision.wrapping_add(1);
        self.mcp.clear();
        self.selected_mcp.clear();
        self.mcp_open = false;
        self.files.clear();
        self.short_mentions.clear();
        self.notes.clear();
        self.known = [
            "/plan",
            "/operator",
            "/orchestrator",
            "/compact",
            "/mcp",
            "/btw",
            "/stage",
            "/commit",
            "/visualize",
        ]
        .map(str::to_owned)
        .into_iter()
        .collect();
        self.loaded = None;
        self.loading = false;
        self.token = None;
        self.dismissed = None;
        self.sync(cx);
    }
    pub(crate) fn set_provider(&mut self, provider: &str, cx: &mut Context<Self>) {
        let provider = monocode_core::provider::Kind::from_id(provider);
        if self.provider != provider {
            for command in &self.native_commands {
                self.known.remove(&format!("/{}", command.invocation));
            }
            for (token, _) in &self.selected_mcp {
                self.known.remove(token);
            }
            self.provider = provider;
            self.native_commands.clear();
            self.native_loading = false;
            self.native_revision = self.native_revision.wrapping_add(1);
            self.selected_mcp.clear();
            if self.token.is_some() {
                self.load_native(cx);
            }
            self.highlight(cx);
            self.rank();
            cx.notify();
        }
    }
    pub(crate) fn set_native_commands(
        &mut self,
        commands: Vec<monocode_core::provider::NativeCommand>,
        cx: &mut Context<Self>,
    ) {
        self.native_revision = self.native_revision.wrapping_add(1);
        for command in &self.native_commands {
            self.known.remove(&format!("/{}", command.invocation));
        }
        self.native_commands = commands
            .into_iter()
            .filter(|command| command.source == self.provider)
            .collect();
        self.known.extend(
            self.native_commands
                .iter()
                .map(|command| format!("/{}", command.invocation)),
        );
        self.highlight(cx);
        self.rank();
        cx.notify();
    }
    /// A skill or provider command already named `/{name}`, which wins over
    /// MonoCode's own command of the same name.
    pub(crate) fn has_user_command(&self, name: &str) -> bool {
        self.skills
            .iter()
            .map(|skill| skill.name.as_str())
            .chain(self.native_commands.iter().map(|c| c.invocation.as_str()))
            .any(|command| command.trim_start_matches('/').eq_ignore_ascii_case(name))
    }
    pub(crate) fn open_mcp(&mut self, cx: &mut Context<Self>) {
        let content = &self.input.read(cx).content;
        self.token = composer::token_at(content, content.trim_end().len());
        self.dismissed = None;
        self.mcp_open = true;
        if !self.loading {
            self.load(cx);
        }
        self.rank();
        cx.notify();
    }
    pub(crate) fn prepare_prompt(&self, text: &str) -> String {
        if self.cwd.starts_with("remote://") {
            return text.to_owned();
        }
        let text = self.expand_mentions(text);
        let text = monocode_core::provider::native_prompt(self.provider, &text);
        let text = self
            .store
            .as_ref()
            .and_then(|store| crate::runtime::locked(store).ok())
            .and_then(|store| store.apply_notes_to_turn(&text).ok())
            .unwrap_or(text);
        let servers = self
            .selected_mcp
            .iter()
            .filter(|(token, _)| {
                text.match_indices(token.as_str()).any(|(start, _)| {
                    let line_start = text[..start].rfind('\n').map_or(0, |index| index + 1);
                    if text[line_start..start].trim_start().starts_with('>') {
                        return false;
                    }
                    let end = start + token.len();
                    let before = text[..start]
                        .chars()
                        .next_back()
                        .is_none_or(|c| c.is_whitespace() || "([{ ".contains(c));
                    let after = text[end..]
                        .chars()
                        .next()
                        .is_none_or(|c| c.is_whitespace() || ")]}.!?;,".contains(c));
                    before && after
                })
            })
            .map(|(_, server)| {
                format!(
                    "{} ({})",
                    serde_json::to_string(&server.name).unwrap_or_default(),
                    server.provider
                )
            })
            .collect::<Vec<_>>();
        if servers.is_empty() {
            text
        } else {
            format!(
                "MCP context: Use the configured server{} {} when relevant to this request.\n\n{text}",
                if servers.len() == 1 { "" } else { "s" },
                servers.join(", ")
            )
        }
    }
    /// The bare file name when no other project file shares it.
    fn mention_label(&self, file: &ProjectFile) -> String {
        let full = composer::mention_text(&file.relative);
        let short = composer::mention_text(&file.name);
        if self.short_mentions.get(&short) == Some(&full) {
            short
        } else {
            full
        }
    }
    /// Short `@name` labels back to the project-relative paths the prompt
    /// resolves.
    pub(crate) fn expand_mentions(&self, text: &str) -> String {
        if self.short_mentions.is_empty() {
            return text.to_owned();
        }
        let labels = self.short_mentions.keys().cloned().collect();
        let mut text = text.to_owned();
        for range in composer::highlight_ranges(&text, &labels).into_iter().rev() {
            let full = self.short_mentions[&text[range.clone()]].clone();
            text.replace_range(range, &full);
        }
        text
    }
    pub(crate) fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.mcp_open = false;
        self.dismissed = self.token.take();
        cx.notify();
    }
    fn sync(&mut self, cx: &mut Context<Self>) {
        if self.cwd.starts_with("remote://") {
            self.choices.clear();
            self.token = None;
            self.input.update(cx, |input, cx| {
                input.highlight_composer_tokens(Vec::new(), cx)
            });
            return;
        }
        let disabled = cx
            .try_global::<crate::settings::Preferences>()
            .map(|p| p.disabled_skill_paths.clone())
            .unwrap_or_default();
        if self.disabled_paths != disabled {
            self.disabled_paths = disabled;
            self.loaded = None;
            self.token = None;
        }
        self.highlight(cx);
        let token = self.input.read(cx).completion_token();
        if token.is_some() && token == self.dismissed {
            return;
        }
        // An empty caret token closes the picker and clears an earlier Escape
        // dismissal, so deleting and retyping the same command can reopen it.
        self.dismissed = None;
        let close_mcp = self.mcp_open && token.as_ref().is_none_or(|token| token.query != "mcp");
        if close_mcp {
            self.mcp_open = false;
        }
        if token == self.token {
            if close_mcp {
                cx.notify();
            }
            return;
        }
        self.token = token;
        self.active = 0;
        if self.token.is_some()
            && !self.loading
            && self
                .loaded
                .is_none_or(|at| at.elapsed() > Duration::from_secs(30))
        {
            self.load(cx);
        }
        self.rank();
        cx.notify();
    }
    fn load_native(&mut self, cx: &mut Context<Self>) {
        if self.native_loading
            || !matches!(
                self.provider,
                monocode_core::provider::Kind::Pi | monocode_core::provider::Kind::Omp
            )
        {
            return;
        }
        self.native_loading = true;
        let native_revision = self.native_revision;
        let provider = self.provider;
        let cwd = self.cwd.clone();
        let generation = self.generation;
        let cancel = self.cancel.clone();
        let work = cx.background_executor().spawn(async move {
            if cancel.load(Ordering::Relaxed) {
                return Err("Stopped".into());
            }
            let Some(binary) = monocode_core::provider::resolve(provider) else {
                return Ok(Vec::new());
            };
            monocode_core::provider::discover_commands(
                monocode_core::provider::Request {
                    kind: provider,
                    binary,
                    cwd: cwd.into(),
                    prompt: String::new(),
                    provider_id: None,
                    model: None,
                    settings: Default::default(),
                },
                cancel,
            )
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation || this.provider != provider {
                    return;
                }
                this.native_loading = false;
                if this.native_revision != native_revision {
                    return;
                }
                match result {
                    Ok(commands) => this.set_native_commands(commands, cx),
                    Err(error) => {
                        this.error = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.load_native(cx);
        let cancel = self.cancel.clone();
        let cwd = self.cwd.clone();
        let generation = self.generation;
        let disabled = self.disabled_paths.clone();
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            let notes = store
                .and_then(|s| {
                    crate::runtime::locked(&s)
                        .ok()
                        .and_then(|s| s.list_notes().ok())
                })
                .unwrap_or_default();
            let skills =
                monocode_core::skills::list_skills(cwd.clone(), Some(disabled)).unwrap_or_default();
            let files =
                monocode_core::files::list_project_files_sync_cancellable(&cwd, Some(&cancel));
            let mcp = monocode_core::mcp::mcp_discover(cwd).unwrap_or_default();
            (skills, files, notes, mcp)
        });
        cx.spawn(async move |this, cx| {
            let (skills, files, notes, mcp) = work.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.generation {
                    return;
                }
                this.loading = false;
                this.loaded = Some(Instant::now());
                this.skills = skills;
                this.mcp = mcp;
                this.notes = notes;
                match files {
                    Ok(files) => this.files = files,
                    Err(error) => this.error = Some(error),
                }
                this.short_mentions = short_mentions(&this.files);
                this.known = [
                    "/plan",
                    "/operator",
                    "/orchestrator",
                    "/compact",
                    "/mcp",
                    "/btw",
                    "/stage",
                    "/commit",
                    "/visualize",
                ]
                .map(str::to_owned)
                .into_iter()
                .chain(
                    this.native_commands
                        .iter()
                        .map(|c| format!("/{}", c.invocation)),
                )
                .chain(this.selected_mcp.iter().map(|(token, _)| token.clone()))
                .chain(this.skills.iter().map(|s| format!("/{}", s.name)))
                .chain(
                    this.files
                        .iter()
                        .map(|f| composer::mention_text(&f.relative)),
                )
                .chain(this.short_mentions.keys().cloned())
                .chain(this.notes.iter().map(|n| format!("@note/{}", n.slug)))
                .collect();
                this.highlight(cx);
                this.rank();
                cx.notify();
            });
        })
        .detach();
    }
    fn highlight(&self, cx: &mut Context<Self>) {
        let ranges = composer::highlight_ranges(&self.input.read(cx).content, &self.known);
        self.input
            .update(cx, |input, cx| input.highlight_composer_tokens(ranges, cx));
    }
    fn rank(&mut self) {
        self.choices.clear();
        let Some(token) = &self.token else {
            return;
        };
        if self.mcp_open {
            let mut servers = self.mcp.clone();
            servers.sort_by_key(|server| {
                (
                    server.provider != self.provider.id() || !server.enabled,
                    server.name.to_lowercase(),
                    server.provider.clone(),
                )
            });
            self.choices = servers
                .into_iter()
                .map(|server| Choice {
                    label: server.name.clone(),
                    detail: format!(
                        "{} · {} · {}",
                        server.provider,
                        server.scope,
                        if server.provider != self.provider.id() {
                            "Different provider"
                        } else if !server.enabled {
                            "Disabled in provider configuration"
                        } else {
                            "Configured for this provider"
                        }
                    ),
                    insertion: String::new(),
                    builtin: None,
                    mode: None,
                    file: false,
                    server: Some(server),
                })
                .collect();
        } else if token.trigger == Trigger::Mention {
            self.choices = composer::rank_files(&self.files, &token.query, 60)
                .into_iter()
                .map(|file| Choice {
                    label: file.name.clone(),
                    detail: file.relative.clone(),
                    insertion: self.mention_label(file),
                    builtin: None,
                    mode: None,
                    file: true,
                    server: None,
                })
                .collect();
            let query = token
                .query
                .strip_prefix("note/")
                .unwrap_or(&token.query)
                .trim_end_matches('/');
            let mut notes: Vec<_> = self
                .notes
                .iter()
                .filter_map(|note| {
                    let title = composer::score(query, &note.title).map(|score| score + 400);
                    let slug = composer::score(query, &note.slug);
                    let tag = note
                        .tags
                        .iter()
                        .filter_map(|tag| composer::score(query.trim_start_matches('#'), tag))
                        .max()
                        .map(|score| score + 200);
                    title.or(slug).or(tag).map(|score| (score, note))
                })
                .collect();
            notes.sort_by(|(a_score, a), (b_score, b)| {
                b_score.cmp(a_score).then(b.updated_at.cmp(&a.updated_at))
            });
            if token.query.starts_with("note/") {
                self.choices.clear();
            }
            self.choices
                .extend(notes.into_iter().take(8).map(|(_, note)| Choice {
                    label: note.title.clone(),
                    detail: format!("note/{}", note.slug),
                    insertion: format!("@note/{}", note.slug),
                    builtin: None,
                    mode: None,
                    file: false,
                    server: None,
                }));
        } else {
            let mut choices = Vec::new();
            for (order, (id, label, detail)) in [
                (
                    "draft",
                    "Draft",
                    "Save this message without starting the agent",
                ),
                (
                    "orchestrator",
                    "Orchestrator",
                    "Plan and coordinate agent work",
                ),
                (
                    "operator",
                    "Operator",
                    "Give this thread access to MonoCode",
                ),
                ("plan", "Plan mode", "Review a plan before building"),
                (
                    "btw",
                    "Side question",
                    "Ask a read-only side question about the current turn",
                ),
                (
                    "compact",
                    "Compact context",
                    "Summarize older conversation context to free space",
                ),
                (
                    "add-to-folder",
                    "Add to folder",
                    "Place this session in an existing or new sidebar folder",
                ),
                ("mcp", "MCP server", "Find an MCP server for this message"),
                ("stage", "Stage", "Stage only this session's changes"),
                (
                    "commit",
                    "Commit",
                    "Stage and commit this session's changes",
                ),
                (
                    "visualize",
                    "Visualize",
                    "Create an inline visual of this topic or a specific request",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                if matches!(id, "stage" | "commit") && self.has_user_command(id) {
                    continue;
                }
                if id == "btw"
                    && matches!(
                        self.provider,
                        monocode_core::provider::Kind::Fx
                            | monocode_core::provider::Kind::Hermes
                            | monocode_core::provider::Kind::Antigravity
                    )
                {
                    continue;
                }
                if let Some(score) = composer::score(&token.query, &format!("{id} {label}")) {
                    // Only the bare slash menu uses the fixed command order.
                    let priority = if token.query.is_empty() {
                        100 - order as i64
                    } else {
                        100
                    };
                    choices.push((
                        score + priority,
                        Choice {
                            label: label.into(),
                            detail: detail.into(),
                            insertion: format!("/{id}"),
                            builtin: Some(id),
                            mode: (!matches!(
                                id,
                                "compact" | "btw" | "stage" | "commit" | "visualize"
                            ))
                            .then_some(id),
                            file: false,
                            server: None,
                        },
                    ));
                }
            }
            for command in &self.native_commands {
                if let Some(score) = composer::score(
                    &token.query,
                    &format!(
                        "{} {} {}",
                        command.invocation,
                        command.description,
                        command.aliases.join(" ")
                    ),
                ) {
                    choices.push((
                        score,
                        Choice {
                            label: command.invocation.trim_start_matches('/').into(),
                            detail: {
                                let hint = command.input_hint.clone().unwrap_or_else(|| {
                                    command
                                        .subcommands
                                        .iter()
                                        .map(|subcommand| {
                                            subcommand.usage.as_deref().unwrap_or(&subcommand.name)
                                        })
                                        .collect::<Vec<_>>()
                                        .join(" · ")
                                });
                                [command.description.clone(), hint]
                                    .into_iter()
                                    .filter(|part| !part.is_empty())
                                    .collect::<Vec<_>>()
                                    .join(" · ")
                            },
                            insertion: format!("/{}", command.invocation),
                            builtin: None,
                            mode: None,
                            file: false,
                            server: None,
                        },
                    ));
                }
            }
            for skill in &self.skills {
                if matches!(
                    skill.name.as_str(),
                    "plan"
                        | "compact"
                        | "mcp"
                        | "operator"
                        | "orchestrator"
                        | "add-to-folder"
                        | "draft"
                        | "btw"
                        | "mono"
                        | "monocode"
                ) {
                    continue;
                }
                if let Some(score) = composer::score(
                    &token.query,
                    &format!("{} {}", skill.name, skill.description),
                ) {
                    choices.push((
                        score,
                        Choice {
                            label: skill.name.trim_start_matches('/').into(),
                            detail: if skill.description.is_empty() {
                                format!("{} · {}", skill.scope, skill.source)
                            } else {
                                skill.description.clone()
                            },
                            insertion: format!("/{}", skill.name),
                            builtin: None,
                            mode: None,
                            file: false,
                            server: None,
                        },
                    ));
                }
            }
            choices.sort_by(|(a, x), (b, y)| b.cmp(a).then_with(|| x.label.cmp(&y.label)));
            let limit = if self.native_commands.is_empty() {
                60
            } else {
                usize::MAX
            };
            self.choices = choices.into_iter().take(limit).map(|(_, c)| c).collect();
        }
        self.active = self.active.min(self.choices.len().saturating_sub(1));
        self.scroll.scroll_to_item(self.active);
    }
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if !self.choices.is_empty() {
            self.active =
                (self.active as isize + delta).rem_euclid(self.choices.len() as isize) as usize;
            self.scroll.scroll_to_item(self.active);
            cx.notify();
        }
    }
    fn pick(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(choice) = self.choices.get(ix).cloned() else {
            return;
        };
        let Some(token) = self.token.take() else {
            return;
        };
        // Validate against the live caret; an asynchronously loaded catalog must
        // never replace a stale range after a move or IME composition.
        let live = self.input.read(cx);
        let current_token = live.completion_token().or_else(|| {
            self.mcp_open
                .then(|| composer::token_at(&live.content, live.content.trim_end().len()))
                .flatten()
        });
        if current_token.as_ref() != Some(&token) {
            self.mcp_open = false;
            return;
        }
        if choice.mode == Some("mcp") {
            self.token = Some(token);
            self.mcp_open = true;
            if !self.loading {
                self.load(cx);
            }
            self.rank();
            cx.notify();
            return;
        }
        let insertion = if let Some(server) = &choice.server {
            let base = format!("@mcp/{}", server.name);
            let mut token = base.clone();
            if self.selected_mcp.iter().any(|(known, _)| known == &token) {
                token = format!("@mcp/{}/{}", server.provider, server.name);
            }
            if self.selected_mcp.iter().any(|(known, _)| known == &token) {
                token = format!("@mcp/{}/{}/{}", server.provider, server.scope, server.name);
            }
            let disambiguated = token.clone();
            let mut suffix = 2;
            while self.selected_mcp.iter().any(|(known, _)| known == &token) {
                token = format!("{disambiguated}-{suffix}");
                suffix += 1;
            }
            self.selected_mcp.push((token.clone(), server.clone()));
            self.known.insert(token.clone());
            format!("{token} ")
        } else {
            match choice.mode {
                Some(mode) => format!("/{mode} "),
                None => format!("{} ", choice.insertion),
            }
        };
        self.mcp_open = false;
        let mut range = token.range;
        if self.input.read(cx).content[range.end..].starts_with(' ') {
            range.end += 1;
        }
        self.input.update(cx, |input, cx| {
            input.replace_completion(range, &insertion, window, cx)
        });
        self.token = None;
        if let Some(mode) = choice.mode {
            cx.emit(ModePicked(mode));
        }
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }
}
impl Render for Completion {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.token.is_some() && self.input.focus_handle(cx).is_focused(window);
        let mut root = div()
            .debug_selector(|| "composer-text-field".into())
            .relative()
            .w_full()
            .child(self.anchor.layout_probe())
            .key_context(if open { "Completion" } else { "" })
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                if this.token.is_some() && !this.input.read(cx).is_composing() {
                    this.step(-1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                if this.token.is_some() && !this.input.read(cx).is_composing() {
                    this.step(1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &input::Submit, window, cx| {
                if this.token.is_some() && !this.input.read(cx).is_composing() {
                    if !this.choices.is_empty() {
                        this.pick(this.active, window, cx);
                    } else {
                        this.dismiss(cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|this, _: &Accept, window, cx| {
                this.pick(this.active, window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &Dismiss, _, cx| {
                this.dismiss(cx);
                cx.stop_propagation();
            }))
            .child(self.input.clone());
        if open {
            let rows: Vec<_> = self
                .choices
                .iter()
                .enumerate()
                .map(|(ix, choice)| {
                    let (icon_path, icon_color) = choice.icon();
                    div()
                        .id(("completion-row", ix))
                        .role(Role::ListItem)
                        .aria_label(choice.label.clone())
                        .px_3()
                        .py_1p5()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .gap_2()
                        .bg(if ix == self.active {
                            content(0.1)
                        } else {
                            content(0.)
                        })
                        .hover(|s| s.bg(content(0.08)))
                        .child(
                            div()
                                .size(px(30.))
                                .flex_shrink_0()
                                .rounded(px(6.))
                                .bg(content(0.05))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(if choice.file {
                                    crate::file_icons::file_type_icon(
                                        &choice.label,
                                        false,
                                        false,
                                        16.,
                                    )
                                } else {
                                    icon(icon_path, 16.)
                                        .text_color(icon_color)
                                        .into_any_element()
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .truncate()
                                        .child(choice.label.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(content(0.45))
                                        .truncate()
                                        .child(choice.detail.clone()),
                                ),
                        )
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, window, cx| this.pick(ix, window, cx)))
                        .into_any_element()
                })
                .collect();
            let weak = cx.weak_entity();
            let title = if self.mcp_open {
                "MCP SERVERS"
            } else if self
                .token
                .as_ref()
                .is_some_and(|t| t.trigger == Trigger::Mention)
            {
                "FILES & NOTES"
            } else {
                "SKILLS & COMMANDS"
            };
            let anchor = if self.composer_anchor.get().is_some() {
                self.composer_anchor.clone()
            } else {
                self.anchor.clone()
            };
            let bounds = anchor.get().unwrap_or_default();
            let width = bounds
                .size
                .width
                .as_f32()
                .min((window.viewport_size().width.as_f32() - 16.).max(0.));
            let gap = 6.;
            // Measure from the conversation's top, rather than the window's,
            // so a centered composer's menu cannot cover the session tabs.
            let top = self.container_anchor.get().map_or(0., |b| b.top().as_f32());
            let max_height = (bounds.top().as_f32() - top - gap - 8.).clamp(0., 400.);
            let (anchor, side, width, max_height) = if self.inline_field {
                let caret = self.input.read(cx).cursor_anchor().unwrap_or(bounds);
                let above = (caret.top().as_f32() - gap - 8.).max(0.);
                let below =
                    (window.viewport_size().height.as_f32() - caret.bottom().as_f32() - gap - 8.)
                        .max(0.);
                let (y, side, room) = if below >= above {
                    (caret.bottom(), Side::Bottom, below)
                } else {
                    (caret.top(), Side::Top, above)
                };
                (
                    Anchor::Point(gpui::point(caret.left(), y)),
                    side,
                    width.min(400.),
                    room.min(400.),
                )
            } else {
                (Anchor::Slot(anchor), Side::Top, width, max_height)
            };
            root = root.child(
                popover("composer-completions", anchor)
                    .side(side)
                    .gap(gap)
                    .width(width)
                    .max_height(max_height)
                    .on_dismiss(move |_, cx| {
                        let _ = weak.update(cx, |this, cx| this.dismiss(cx));
                    })
                    .child(
                        div()
                            .id("composer-completion-list")
                            .debug_selector(|| "composer-completion-list".into())
                            .role(Role::ListBox)
                            .aria_label(title)
                            .p_2()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px_2()
                                    .py_1()
                                    .text_size(px(10.))
                                    .text_color(content(0.4))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .id("composer-completion-rows")
                                    .min_h_0()
                                    .max_h(px(320.))
                                    .pr_1()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.scroll)
                                    .children(rows),
                            )
                            .when(self.choices.is_empty(), |d| {
                                d.child(
                                    div()
                                        .px_2()
                                        .py_3()
                                        .text_size(px(12.))
                                        .text_color(content(0.45))
                                        .child(SharedString::from(if self.loading {
                                            "Loading…".to_owned()
                                        } else {
                                            self.error
                                                .clone()
                                                .unwrap_or_else(|| "No matches".into())
                                        })),
                                )
                            })
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .mt_1()
                                    .px_2()
                                    .pt_1p5()
                                    .pb_1()
                                    .border_t_1()
                                    .border_color(content(0.07))
                                    .text_size(px(10.))
                                    .text_color(content(0.35))
                                    .child("↑ ↓ navigate    ↵ / Tab select    Esc dismiss"),
                            ),
                    ),
            );
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, WindowHandle};
    fn window(cx: &mut TestAppContext) -> WindowHandle<Completion> {
        let window = cx.update(|cx| {
            input::bind(cx);
            bind(cx);
            crate::popover::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let input = cx.new(input::Input::new);
                    let mut completion = Completion::new(input, "/not-a-real-project".into(), cx);
                    completion.loaded = Some(Instant::now());
                    completion
                })
            })
            .unwrap()
        });
        window
            .update(cx, |v, w, cx| w.focus(&v.input.focus_handle(cx), cx))
            .unwrap();
        cx.run_until_parked();
        window
    }
    #[gpui::test]
    fn bare_slash_prioritizes_local_modes_and_typing_filters_skills(cx: &mut TestAppContext) {
        let window = window(cx);
        window
            .update(cx, |view, _, cx| {
                view.skills = ["zebra", "alpha"]
                    .into_iter()
                    .map(|name| DiscoveredSkill {
                        name: name.into(),
                        description: String::new(),
                        path: format!("/skills/{name}/SKILL.md"),
                        scope: "project".into(),
                        source: "codex".into(),
                    })
                    .collect();
                view.input
                    .update(cx, |input, cx| input.set_text("/".into(), cx));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                let commands: Vec<_> = view.choices.iter().map(|c| c.insertion.as_str()).collect();
                assert_eq!(
                    commands,
                    [
                        "/draft",
                        "/orchestrator",
                        "/operator",
                        "/plan",
                        "/btw",
                        "/compact",
                        "/add-to-folder",
                        "/mcp",
                        "/stage",
                        "/commit",
                        "/visualize",
                        "/alpha",
                        "/zebra",
                    ]
                );
                view.input
                    .update(cx, |input, cx| input.set_text("/alpha".into(), cx));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert_eq!(view.choices.len(), 1);
                assert_eq!(view.choices[0].insertion, "/alpha");
            })
            .unwrap();
        window
            .update(cx, |view, _, cx| {
                view.input
                    .update(cx, |input, cx| input.set_text("/o".into(), cx));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert_eq!(view.choices[0].insertion, "/operator");
                assert_eq!(view.choices[1].insertion, "/orchestrator");
            })
            .unwrap();
    }
    #[gpui::test]
    fn a_users_skill_wins_over_the_git_command_of_its_name(cx: &mut TestAppContext) {
        let window = window(cx);
        window
            .update(cx, |view, _, cx| {
                assert!(!view.has_user_command("commit"));
                view.skills = vec![DiscoveredSkill {
                    name: "commit".into(),
                    description: "My own commit flow".into(),
                    path: "/skills/commit/SKILL.md".into(),
                    scope: "project".into(),
                    source: "claude".into(),
                }];
                assert!(view.has_user_command("Commit"));
                assert!(!view.has_user_command("stage"));
                view.input
                    .update(cx, |input, cx| input.set_text("/commit".into(), cx));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                let commit: Vec<_> = view
                    .choices
                    .iter()
                    .filter(|c| c.insertion == "/commit")
                    .map(|c| c.detail.as_str())
                    .collect();
                assert_eq!(commit, ["My own commit flow"]);
            })
            .unwrap();
    }
    #[gpui::test]
    fn slash_accept_keeps_input_focus_and_does_not_submit(cx: &mut TestAppContext) {
        let window = window(cx);
        let submitted = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = submitted.clone();
        let _submission = window
            .update(cx, |v, _, cx| {
                cx.subscribe(&v.input, move |_, _, _: &input::Submitted, _| {
                    count.set(count.get() + 1)
                })
            })
            .unwrap();
        window
            .update(cx, |v, _, cx| {
                v.input.update(cx, |i, cx| i.set_text("/pla".into(), cx))
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        window
            .update(cx, |v, w, cx| {
                assert_eq!(v.input.read(cx).content, "/plan ");
                assert!(v.token.is_none());
                assert!(v.input.focus_handle(cx).is_focused(w));
            })
            .unwrap();
        assert_eq!(submitted.get(), 0);
    }
    #[gpui::test]
    fn visualize_completion_inserts_a_command_without_selecting_a_mode(cx: &mut TestAppContext) {
        let window = window(cx);
        window
            .update(cx, |view, _, cx| {
                view.input
                    .update(cx, |input, cx| input.set_text("/vis".into(), cx));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert_eq!(view.choices[0].insertion, "/visualize");
                assert_eq!(view.choices[0].mode, None);
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                assert_eq!(view.input.read(cx).content, "/visualize ");
            })
            .unwrap();
    }
    #[gpui::test]
    fn compact_completion_inserts_a_local_command_without_mode_selection(cx: &mut TestAppContext) {
        let window = window(cx);
        window
            .update(cx, |view, _, cx| {
                view.input
                    .update(cx, |input, cx| input.set_text("/comp".into(), cx));
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                assert_eq!(view.input.read(cx).content, "/compact ")
            })
            .unwrap();
    }
    #[gpui::test]
    fn mcp_selection_replaces_the_command_and_only_present_tags_add_context(
        cx: &mut TestAppContext,
    ) {
        let window = window(cx);
        window
            .update(cx, |view, window, cx| {
                view.input
                    .update(cx, |input, cx| input.set_text("/mcp".into(), cx));
                view.loading = true;
                view.mcp = vec![monocode_core::mcp::McpConnection {
                    provider: "codex".into(),
                    name: "docs".into(),
                    scope: "project".into(),
                    config_path: "/repo/config.toml".into(),
                    transport: "stdio".into(),
                    enabled: true,
                }];
                view.open_mcp(cx);
                view.pick(0, window, cx);
                assert_eq!(view.input.read(cx).content, "@mcp/docs ");
                assert!(
                    view.prepare_prompt("Use @mcp/docs.")
                        .starts_with("MCP context: Use the configured server ")
                );
                assert_eq!(
                    view.prepare_prompt("Use @mcp/docs-extra"),
                    "Use @mcp/docs-extra"
                );
                assert_eq!(view.prepare_prompt("Ordinary request"), "Ordinary request");
                assert_eq!(view.prepare_prompt("> Use @mcp/docs"), "> Use @mcp/docs");
                assert_eq!(
                    view.prepare_prompt("Quote:\n  > @mcp/docs\nOrdinary request"),
                    "Quote:\n  > @mcp/docs\nOrdinary request"
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn ime_composition_does_not_open_a_picker_or_send(cx: &mut TestAppContext) {
        use gpui::EntityInputHandler;
        let window = window(cx);
        window
            .update(cx, |v, w, cx| {
                v.input.update(cx, |i, cx| {
                    i.replace_and_mark_text_in_range(None, "/pla", None, w, cx)
                })
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        window
            .update(cx, |v, _, cx| {
                assert!(v.input.read(cx).is_composing());
                assert!(v.token.is_none());
                assert_eq!(v.input.read(cx).content, "/pla");
            })
            .unwrap();
    }
    #[gpui::test]
    fn mention_accept_replaces_only_the_caret_token_and_escape_dismisses(cx: &mut TestAppContext) {
        let window = window(cx);
        window
            .update(cx, |v, _, cx| {
                v.files = vec![ProjectFile {
                    name: "my file.rs".into(),
                    relative: "src/my file.rs".into(),
                    path: "/repo/src/my file.rs".into(),
                }];
                v.input.update(cx, |i, cx| {
                    i.set_text("Use @my trailing".into(), cx);
                    i.select_range(7..7, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "tab");
        cx.run_until_parked();
        window
            .update(cx, |v, _, cx| {
                assert_eq!(v.input.read(cx).content, "Use @\"src/my file.rs\" trailing");
                v.input.update(cx, |i, cx| i.set_text("@my".into(), cx));
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        window
            .update(cx, |v, _, cx| {
                assert_eq!(v.input.read(cx).content, "@my");
                assert!(v.token.is_none());
            })
            .unwrap();
    }
    #[gpui::test]
    fn unique_file_names_insert_short_and_expand_on_send(cx: &mut TestAppContext) {
        let window = window(cx);
        let file = |relative: &str| ProjectFile {
            name: relative.rsplit('/').next().unwrap().into(),
            relative: relative.into(),
            path: format!("/repo/{relative}"),
        };
        window
            .update(cx, |v, _, cx| {
                v.files = vec![
                    file("app/apps/README.md"),
                    file("src/mod.rs"),
                    file("lib/mod.rs"),
                ];
                v.short_mentions = short_mentions(&v.files);
                v.input.update(cx, |i, cx| {
                    i.set_text("@READ".into(), cx);
                    i.select_range(5..5, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "tab");
        cx.run_until_parked();
        window
            .update(cx, |v, _, cx| {
                assert_eq!(v.input.read(cx).content.trim_end(), "@README.md");
                assert_eq!(v.mention_label(&v.files[1]), "@src/mod.rs");
                assert_eq!(
                    v.expand_mentions("see @README.md and @src/mod.rs\n> @README.md"),
                    "see @app/apps/README.md and @src/mod.rs\n> @README.md"
                );
            })
            .unwrap();
    }
}
