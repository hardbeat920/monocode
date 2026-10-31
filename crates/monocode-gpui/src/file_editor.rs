//! Native FileEditor/FilePane previews. Disk work and syntax parsing stay off
//! the UI thread; drafts, selections and scrolling belong to each live tab.
use crate::{
    git_status::{GitChanged, ProjectGit},
    input::{self, Input},
    right_panel::{icon_action, stroke},
    theme::*,
    transcript::TranscriptView,
};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Image, ImageCacheError,
    ImageFormat, ImageSource, KeyBinding, ObjectFit, PromptLevel, RenderImage, Role, SharedString,
    Subscription, Task, Window, actions, div, img, prelude::*, px, rgb,
};
use monocode_core::files::document::{self, Document, TextDocument};
use std::{path::PathBuf, sync::Arc, time::Duration};

mod chrome;
mod diff;
mod media;
#[cfg(test)]
#[path = "file_editor/tests.rs"]
mod parity_tests;
mod search;
mod selection;

actions!(
    file_editor,
    [
        ToggleAutosave,
        ToggleWordWrap,
        ReloadFile,
        Save,
        Find,
        Replace,
        FindNext,
        FindPrevious,
        DismissSearch,
        MatchCase,
        WholeWord,
        Regex,
        Format,
        PreviousChange,
        NextChange,
        RevertChange
    ]
);
pub(crate) fn bind(cx: &mut App) {
    let m = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([
        KeyBinding::new(&format!("{m}-s"), Save, Some("FileEditor")),
        KeyBinding::new(&format!("{m}-f"), Find, Some("FileEditor")),
        KeyBinding::new(&format!("{m}-alt-f"), Replace, Some("FileEditor")),
        KeyBinding::new("alt-shift-f", Format, Some("FileEditor")),
        KeyBinding::new("alt-z", ToggleWordWrap, Some("FileEditor")),
        KeyBinding::new("alt-c", MatchCase, Some("FileEditor")),
        KeyBinding::new("alt-w", WholeWord, Some("FileEditor")),
        KeyBinding::new("alt-r", Regex, Some("FileEditor")),
        KeyBinding::new("f3", FindNext, Some("FileEditor")),
        KeyBinding::new("shift-f3", FindPrevious, Some("FileEditor")),
        KeyBinding::new("alt-up", PreviousChange, Some("FileEditor")),
        KeyBinding::new("alt-down", NextChange, Some("FileEditor")),
        KeyBinding::new(&format!("{m}-g"), FindNext, Some("FileEditor")),
        KeyBinding::new(&format!("{m}-shift-g"), FindPrevious, Some("FileEditor")),
        KeyBinding::new("escape", DismissSearch, Some("FileEditor")),
    ]);
}
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = file_editor, no_json)]
pub(crate) struct OpenPath(pub String);

/// File links may include a one-based line and column (`:12:4` or `#L12`).
pub(crate) fn file_location(raw: &str, cwd: &str) -> (String, Option<(usize, usize)>) {
    let mut path = raw.strip_prefix("file://").unwrap_or(raw).to_owned();
    let mut location = None;
    if let Some((file, line)) = path.rsplit_once("#L")
        && let Ok(line) = line.split('-').next().unwrap_or("").parse::<usize>()
    {
        location = Some((line.max(1), 1));
        path = file.into();
    } else if let Some((prefix, number)) = path.rsplit_once(':')
        && let Ok(last) = number.parse::<usize>()
    {
        if let Some((file, line)) = prefix.rsplit_once(':')
            && let Ok(line) = line.parse::<usize>()
        {
            location = Some((line.max(1), last.max(1)));
            path = file.into();
        } else {
            location = Some((last.max(1), 1));
            path = prefix.into();
        }
    }
    if raw.starts_with("file://") || raw.contains('%') {
        let bytes = path.as_bytes();
        let mut decoded = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] == b'%'
                && at + 2 < bytes.len()
                && let Ok(hex) = std::str::from_utf8(&bytes[at + 1..at + 3])
                && let Ok(byte) = u8::from_str_radix(hex, 16)
            {
                decoded.push(byte);
                at += 3;
            } else {
                decoded.push(bytes[at]);
                at += 1;
            }
        }
        path = String::from_utf8_lossy(&decoded).into_owned();
    }
    let path = document::resolve_path(std::path::Path::new(&path), std::path::Path::new(cwd));
    (path.to_string_lossy().into_owned(), location)
}

pub(crate) enum FileEvent {
    Dirty,
    Saved,
    SaveFailed,
    WordWrap(bool),
    AddToChat(String),
}

pub(crate) struct FileEditor {
    pub path: String,
    git: Entity<ProjectGit>,
    cwd: String,
    git_original: Option<String>,
    git_staged: bool,
    git_line_ending: &'static str,
    git_task: Option<Task<()>>,
    analysis: Option<Task<()>>,
    /// The Tree-sitter parser and last tree, reused by each incremental pass.
    syntax: Arc<std::sync::Mutex<Option<crate::editor_syntax::Session>>>,
    focus: FocusHandle,
    pub input: Entity<Input>,
    selection: selection::Selection,
    search: Entity<Input>,
    search_open: bool,
    replacement: Entity<Input>,
    replace_open: bool,
    match_case: bool,
    whole_word: bool,
    regex: bool,
    search_invalid: bool,
    search_task: Option<Task<()>>,
    matches: Vec<crate::editor_analysis::SearchMatch>,
    autosave: Option<Task<()>>,
    formatting: Option<Task<()>>,
    pub show_diff: bool,
    change_index: usize,
    match_index: usize,
    document: Option<TextDocument>,
    image: Option<Arc<Image>>,
    media: Option<Entity<media::MediaPreview>>,
    media_events: Option<Subscription>,
    /// Each edit renders a fresh sprite; the one it replaces is given back.
    svg_preview: Option<Arc<RenderImage>>,
    svg_failed: bool,
    svg_render: Option<Task<()>>,
    unsupported: Option<String>,
    error: Option<String>,
    disk_changed: bool,
    loading: bool,
    pub saving: bool,
    pub dirty: bool,
    visible: bool,
    preview: bool,
    markdown: Option<Entity<TranscriptView>>,
    markdown_source: SharedString,
    navigation: Option<(usize, usize)>,
    _subscriptions: Vec<Subscription>,
    _poll: Task<()>,
}
impl EventEmitter<FileEvent> for FileEditor {}
impl Focusable for FileEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl FileEditor {
    pub(crate) fn new(path: String, git: Entity<ProjectGit>, cx: &mut Context<Self>) -> Self {
        let mut this = Self::new_deferred(path, git, cx);
        this.load(false, cx);
        this
    }
    pub(crate) fn new_deferred(
        path: String,
        git: Entity<ProjectGit>,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(Input::editor);
        let search = cx.new(|cx| {
            let mut field = Input::field("Find", "Find", content(0.35), cx);
            field.set_font_size(12.);
            field
        });
        let replacement = cx.new(|cx| {
            let mut field = Input::field("Replace", "Replace", content(0.35), cx);
            field.set_font_size(12.);
            field
        });
        input.update(cx, |input, _| input.set_editor_path(path.clone()));
        let subscriptions = vec![
            cx.subscribe(&input, |this, _, _: &input::Changed, cx| this.edited(cx)),
            cx.subscribe(&input, |this, _, action: &input::CodeAction, cx| {
                let (index, stage) = match action {
                    input::CodeAction::Stage(index) => (*index, true),
                    input::CodeAction::Revert(index) => (*index, false),
                };
                this.change_index = index;
                this.apply_change(stage, cx);
            }),
            cx.observe(&input, |this, _, cx| {
                this.observe_code_selection(cx);
                cx.notify();
            }),
            cx.subscribe(&input, |this, _, event: &input::Rejected, cx| {
                this.error = Some(event.0.into());
                cx.notify();
            }),
            cx.subscribe(&search, |this, _, _: &input::Changed, cx| {
                this.find_matches(cx)
            }),
            cx.subscribe(&search, |this, _, _: &input::Submitted, cx| {
                this.step_match(false, cx)
            }),
            cx.subscribe(&replacement, |this, _, _: &input::Changed, cx| {
                this.find_matches(cx)
            }),
            cx.subscribe(&replacement, |this, _, _: &input::Submitted, cx| {
                this.replace_matches(false, cx)
            }),
            cx.subscribe(&git, |this, _, _: &GitChanged, cx| {
                if this.visible {
                    this.refresh_git(cx);
                }
            }),
            cx.observe_global::<crate::settings::Preferences>(|this, cx| {
                let wrap = cx.global::<crate::settings::Preferences>().file_word_wrap;
                this.input
                    .update(cx, |input, cx| input.set_word_wrap(wrap, cx));
                this.schedule_autosave(cx);
                cx.notify();
            }),
        ];
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                if this
                    .update(cx, |this, cx| {
                        if this.visible && !this.saving {
                            this.load(false, cx);
                            this.refresh_git(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        cx.on_release(|this, cx| {
            if let Some(image) = this.svg_preview.take() {
                cx.drop_image(image, None);
            }
        })
        .detach();
        Self {
            path,
            cwd: git.read(cx).project().to_owned(),
            git,
            git_original: None,
            git_staged: false,
            git_line_ending: "\n",
            git_task: None,
            analysis: None,
            syntax: Default::default(),
            focus: cx.focus_handle(),
            input,
            selection: selection::Selection::default(),
            search,
            search_open: false,
            replacement,
            replace_open: false,
            match_case: false,
            whole_word: false,
            regex: false,
            search_invalid: false,
            search_task: None,
            autosave: None,
            formatting: None,
            show_diff: false,
            change_index: 0,
            matches: Vec::new(),
            match_index: 0,
            document: None,
            image: None,
            media: None,
            media_events: None,
            svg_preview: None,
            svg_failed: false,
            svg_render: None,
            unsupported: None,
            error: None,
            disk_changed: false,
            loading: false,
            saving: false,
            dirty: false,
            visible: false,
            preview: false,
            markdown: None,
            markdown_source: "".into(),
            navigation: None,
            _subscriptions: subscriptions,
            _poll: poll,
        }
    }
    pub(crate) fn navigate(&mut self, line: usize, column: usize, cx: &mut Context<Self>) {
        self.navigation = Some((line, column));
        self.apply_navigation(cx);
    }
    fn apply_navigation(&mut self, cx: &mut Context<Self>) {
        if self.document.is_none() {
            return;
        }
        let Some((line, column)) = self.navigation.take() else {
            return;
        };
        self.preview = false;
        self.input.update(cx, |input, cx| {
            let mut start = 0;
            for _ in 1..line.min(input.content.len() + 1) {
                if let Some(n) = input.content[start..].find('\n') {
                    start += n + 1;
                } else {
                    start = input.content.len();
                    break;
                }
            }
            let end = input.content[start..]
                .find('\n')
                .map_or(input.content.len(), |n| start + n);
            let at = start
                + input.content[start..end]
                    .char_indices()
                    .nth(column.saturating_sub(1))
                    .map_or(end - start, |(at, _)| at);
            input.select_range(at..at, cx);
        });
        cx.notify();
    }
    pub(crate) fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        let was = self.visible;
        self.visible = visible;
        if !visible {
            self.selection.dismiss();
        }
        if let Some(media) = &self.media {
            media.update(cx, |m, _| m.set_visible(visible));
        }
        if visible && !was {
            self.load(false, cx);
            self.refresh_git(cx);
        }
    }
    fn is_markdown(&self) -> bool {
        matches!(
            PathBuf::from(&self.path)
                .extension()
                .and_then(|s| s.to_str()),
            Some("md" | "mdx" | "markdown")
        )
    }
    fn load(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.loading || self.saving {
            return;
        }
        self.loading = true;
        let path = PathBuf::from(&self.path);
        let work = cx
            .background_executor()
            .spawn(async move { document::read_document(&path).and_then(media::prepare_document) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(Document::Text(doc)) => {
                        if !force
                            && this
                                .document
                                .as_ref()
                                .is_some_and(|old| old.disk == doc.disk)
                        {
                            return;
                        }
                        if this.dirty && !force {
                            this.disk_changed = true;
                            cx.notify();
                            return;
                        }
                        this.error = None;
                        this.disk_changed = false;
                        this.image = None;
                        this.media = None;
                        this.unsupported = None;
                        let first = this.document.is_none();
                        let text = doc.text.clone();
                        this.document = Some(doc);
                        this.input.update(cx, |input, cx| {
                            input.reload_file(text, !first && !force, cx);
                        });
                        this.dirty = false;
                        if first && !this.show_diff && (this.is_markdown() || this.is_svg()) {
                            this.preview = cx
                                .try_global::<crate::settings::Preferences>()
                                .is_none_or(|prefs| prefs.file_view_mode != "source");
                        }
                        this.apply_navigation(cx);
                        this.update_preview(cx);
                        this.schedule_analysis(cx);
                        cx.emit(FileEvent::Dirty);
                    }
                    Ok(Document::Image {
                        bytes,
                        format,
                        width,
                        height,
                        original_size,
                    }) => {
                        if this.dirty && !force {
                            this.disk_changed = true;
                            cx.notify();
                            return;
                        }
                        let byte_size = original_size;
                        let file_format = if format == "tiff" { "avif" } else { format };
                        let format = match format {
                            "jpeg" => ImageFormat::Jpeg,
                            "webp" => ImageFormat::Webp,
                            "gif" => ImageFormat::Gif,
                            "bmp" => ImageFormat::Bmp,
                            "ico" => ImageFormat::Ico,
                            "tiff" => ImageFormat::Tiff,
                            _ => ImageFormat::Png,
                        };
                        let image = Arc::new(Image::from_bytes(format, bytes));
                        if this
                            .image
                            .as_ref()
                            .is_some_and(|old| old.id() == image.id())
                        {
                            return;
                        }
                        this.media = Some(cx.new(|cx| {
                            media::MediaPreview::image(
                                this.path.clone(),
                                image.clone(),
                                width,
                                height,
                                byte_size,
                                file_format,
                                cx,
                            )
                        }));
                        this.image = Some(image);
                        this.dirty = false;
                        this.disk_changed = false;
                        cx.emit(FileEvent::Dirty);
                        this.document = None;
                        this.unsupported = None;
                        this.error = None;
                    }
                    Ok(Document::Video {
                        size,
                        modified,
                        source,
                    }) => {
                        if this.dirty && !force {
                            this.disk_changed = true;
                            cx.notify();
                            return;
                        }
                        let same = this
                            .media
                            .as_ref()
                            .is_some_and(|m| m.read(cx).same_video(size, modified));
                        if !same || force {
                            this.media = Some(cx.new(|cx| {
                                media::MediaPreview::video(
                                    this.path.clone(),
                                    size,
                                    modified,
                                    source,
                                    cx,
                                )
                            }));
                        }
                        if let Some(media) = &this.media {
                            media.update(cx, |m, _| m.set_visible(this.visible));
                            this.media_events =
                                Some(cx.subscribe(media, |this, _, _: &media::MediaEvent, cx| {
                                    this.load(true, cx)
                                }));
                        }
                        this.document = None;
                        this.image = None;
                        this.unsupported = None;
                        this.error = None;
                        this.dirty = false;
                        this.disk_changed = false;
                        cx.emit(FileEvent::Dirty);
                    }
                    Ok(Document::Unsupported(reason)) => {
                        if this.dirty && !force {
                            this.disk_changed = true;
                            cx.notify();
                            return;
                        }
                        this.media = None;
                        this.unsupported = Some(reason);
                        this.disk_changed = false;
                        this.document = None;
                        this.image = None;
                        this.error = None;
                        this.dirty = false;
                        cx.emit(FileEvent::Dirty);
                    }
                    Err(error) => {
                        this.error = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn edited(&mut self, cx: &mut Context<Self>) {
        self.selection.dismiss();
        let Some(doc) = &self.document else { return };
        let dirty = self.input.read(cx).content.as_ref() != doc.text;
        if self.dirty != dirty {
            self.dirty = dirty;
            cx.emit(FileEvent::Dirty);
        }
        self.schedule_analysis(cx);
        self.schedule_autosave(cx);
        if self.dirty {
            self.input
                .update(cx, |input, cx| input.schedule_completion(false, cx));
        }
        if self.search_open {
            self.find_matches(cx);
        }
        cx.notify();
    }
    fn refresh_git(&mut self, cx: &mut Context<Self>) {
        let cwd = PathBuf::from(&self.cwd);
        let Ok(relative) = PathBuf::from(&self.path)
            .strip_prefix(&cwd)
            .map(|p| p.to_string_lossy().into_owned())
        else {
            return;
        };
        let work = cx.background_executor().spawn(async move {
            let files = monocode_core::scm::diff_files(&cwd).ok()?;
            let staged = files
                .files
                .iter()
                .find(|f| f.relative == relative)
                .is_some_and(|f| f.staged && !f.unstaged);
            let diff = monocode_core::scm::file_diff(&cwd, &relative, staged).ok()?;
            (!diff.binary && !diff.too_large).then(|| {
                (
                    diff.original.replace("\r\n", "\n").replace('\r', "\n"),
                    staged,
                    if diff.original.contains("\r\n") {
                        "\r\n"
                    } else if diff.original.contains('\r') {
                        "\r"
                    } else {
                        "\n"
                    },
                )
            })
        });
        self.git_task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                let original = result.map(|(original, staged, line_ending)| {
                    this.git_staged = staged;
                    this.git_line_ending = line_ending;
                    original
                });
                if this.git_original != original {
                    this.git_original = original;
                    this.schedule_analysis(cx);
                }
            });
        }));
    }
    fn schedule_analysis(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let syntax = crate::editor_syntax::supports(&path).then(|| self.syntax.clone());
        self.analysis = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(if syntax.is_some() {
                    40
                } else {
                    150
                }))
                .await;
            let Ok((text, original)) = this.update(cx, |this, cx| {
                (
                    Arc::<str>::from(this.input.read(cx).content.as_str()),
                    this.git_original.as_deref().map(Arc::<str>::from),
                )
            }) else {
                return;
            };
            let source = text.clone();
            let base = original.clone();
            let work = cx.background_executor().spawn(async move {
                match syntax {
                    Some(syntax) => {
                        crate::editor_syntax::analyze(&syntax, &path, &source, base.as_deref())
                    }
                    None => crate::editor_analysis::analyze(&path, &source, base.as_deref()),
                }
            });
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.input.read(cx).content.as_str() == text.as_ref()
                    && this.git_original.as_deref() == original.as_deref()
                {
                    match result {
                        Ok(analysis) => this
                            .input
                            .update(cx, |input, cx| input.set_editor_analysis(analysis, cx)),
                        Err(error) => eprintln!("monocode: editor analysis failed: {error}"),
                    }
                }
            });
        }));
    }
    fn toggle_word_wrap(&mut self, cx: &mut Context<Self>) {
        let wrap = !self.input.read(cx).word_wrap();
        self.input
            .update(cx, |input, cx| input.set_word_wrap(wrap, cx));
        cx.emit(FileEvent::WordWrap(wrap));
        cx.notify();
    }
    fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        self.autosave = None;
        if !self.dirty
            || self.disk_changed
            || !cx
                .try_global::<crate::settings::Preferences>()
                .is_some_and(|p| p.autosave)
        {
            return;
        }
        self.autosave = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |this, cx| {
                if this.dirty
                    && !this.disk_changed
                    && cx
                        .try_global::<crate::settings::Preferences>()
                        .is_some_and(|p| p.autosave)
                {
                    if this.input.read(cx).is_composing() || this.saving {
                        this.schedule_autosave(cx);
                    } else {
                        this.save(cx);
                    }
                }
            });
        }));
    }
    fn format_document(&mut self, cx: &mut Context<Self>) {
        if self.document.is_none() || self.saving || self.input.read(cx).is_composing() {
            return;
        }
        let source = self.input.read(cx).content.clone();
        let at = self.input.read(cx).selection_range().end;
        let path = self.path.clone();
        self.formatting = Some(cx.spawn(async move |this, cx| {
            let text = source.clone();
            let result = cx
                .background_executor()
                .spawn(async move { crate::editor_analysis::format(&path, &text, at) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.input.read(cx).content == source
                    && !this.input.read(cx).is_composing()
                    && let Ok(Some(result)) = result
                    && result.formatted != source.as_ref()
                {
                    this.input.update(cx, |input, cx| {
                        input.replace_bytes(0..source.len(), &result.formatted, result.cursor, cx)
                    });
                }
            });
        }));
    }
    pub(crate) fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving || self.input.read(cx).is_composing() || self.document.is_none() {
            return;
        }
        let source = self.input.read(cx).content.clone();
        let at = self.input.read(cx).selection_range().end;
        let path = self.path.clone();
        let format = cx
            .try_global::<crate::settings::Preferences>()
            .is_none_or(|p| p.format_on_save);
        self.saving = true;
        self.error = None;
        cx.spawn(async move |this, cx| {
            if format {
                let text = source.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { crate::editor_analysis::format(&path, &text, at) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.input.read(cx).content == source
                        && !this.input.read(cx).is_composing()
                        && let Ok(Some(result)) = result
                        && result.formatted != source.as_ref()
                    {
                        this.input.update(cx, |input, cx| {
                            input.replace_bytes(
                                0..source.len(),
                                &result.formatted,
                                result.cursor,
                                cx,
                            )
                        });
                    }
                });
            }
            let Ok(Some((path, doc, saved))) = this.update(cx, |this, cx| {
                if this.input.read(cx).is_composing() {
                    this.saving = false;
                    this.schedule_autosave(cx);
                    cx.emit(FileEvent::SaveFailed);
                    return None;
                }
                Some((
                    PathBuf::from(&this.path),
                    this.document.clone()?,
                    this.input.read(cx).content.to_string(),
                ))
            }) else {
                return;
            };
            let text = saved.clone();
            let result = cx
                .background_executor()
                .spawn(async move { document::save_document(&path, &doc, &text) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(disk) => {
                        if let Some(doc) = &mut this.document {
                            doc.disk = disk;
                            doc.text = saved;
                        }
                        this.disk_changed = false;
                        this.edited(cx);
                        this.git.update(cx, |git, cx| git.changed(cx));
                        cx.emit(FileEvent::Saved);
                    }
                    Err(error) => {
                        this.error = Some(error);
                        cx.emit(FileEvent::SaveFailed);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.dirty {
            self.load(true, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            "Discard your edits and reload this file?",
            None,
            &["Reload", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| this.load(true, cx));
            }
        })
        .detach();
    }
    fn is_svg(&self) -> bool {
        PathBuf::from(&self.path)
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("svg"))
    }
    fn update_preview(&mut self, cx: &mut Context<Self>) {
        if self.preview && self.is_svg() {
            let bytes = self.input.read(cx).content.as_bytes().to_vec();
            let renderer = cx.svg_renderer();
            let work = cx.background_executor().spawn(async move {
                Image::from_bytes(ImageFormat::Svg, bytes)
                    .to_image_data(renderer)
                    .ok()
            });
            self.svg_render = Some(cx.spawn(async move |this, cx| {
                let image = work.await;
                let _ = this.update(cx, |this, cx| {
                    this.svg_failed = image.is_none();
                    if let Some(old) = std::mem::replace(&mut this.svg_preview, image) {
                        cx.drop_image(old, None);
                    }
                    cx.notify();
                });
            }));
        }
        if self.preview && self.is_markdown() {
            let text = self.input.read(cx).content.clone();
            if self.markdown.is_none() || self.markdown_source != text {
                self.markdown_source = text.clone();
                let base = PathBuf::from(&self.path)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned());
                self.markdown = Some(cx.new(|cx| {
                    let mut view = TranscriptView::document(text.to_string(), cx);
                    view.file_link_base = base;
                    view
                }));
            }
        }
    }
    fn toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selection.dismiss();
        self.preview = !self.preview;
        if cx.has_global::<crate::settings::Preferences>() {
            cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
                prefs.file_view_mode = if self.preview { "preview" } else { "source" }.into();
            });
        }
        self.update_preview(cx);
        window.focus(&self.focus_handle(cx), cx);
        cx.notify();
    }
    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.document.is_none() {
            return;
        }
        self.preview = false;
        if !self.search_open {
            let input = self.input.read(cx);
            let selected = input.content[input.selection_range()].to_owned();
            if !selected.is_empty() && !selected.contains('\n') {
                self.search.update(cx, |i, cx| i.set_text(selected, cx));
            }
        }
        self.search_open = true;
        window.focus(&self.search.focus_handle(cx), cx);
        cx.notify();
    }
    fn find_matches(&mut self, cx: &mut Context<Self>) {
        let source = self.input.read(cx).content.clone();
        let query = self.search.read(cx).content.clone();
        let replacement = self.replacement.read(cx).content.clone();
        let (case, word, regex) = (self.match_case, self.whole_word, self.regex);
        let previous = self
            .matches
            .get(self.match_index)
            .map(|m| m.from)
            .unwrap_or(0);
        self.matches.clear();
        self.search_task = Some(cx.spawn(async move |this, cx| {
            let text = source.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    crate::editor_analysis::search(&text, &query, &replacement, case, word, regex)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.input.read(cx).content != source {
                    return;
                }
                match result {
                    Ok(result) => {
                        this.matches = result.matches;
                        this.search_invalid = result.invalid;
                    }
                    Err(_) => {
                        this.matches.clear();
                        this.search_invalid = true;
                    }
                }
                this.match_index = this
                    .matches
                    .iter()
                    .position(|m| m.from >= previous)
                    .unwrap_or(0);
                let ranges = this.matches.iter().map(|m| m.from..m.to).collect();
                this.input
                    .update(cx, |i, cx| i.set_search_matches(ranges, cx));
                if let Some(m) = this.matches.get(this.match_index) {
                    this.input
                        .update(cx, |i, cx| i.select_range(m.from..m.to, cx));
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn step_match(&mut self, backwards: bool, cx: &mut Context<Self>) {
        if self.matches.is_empty() {
            return;
        }
        self.match_index = (self.match_index as isize + if backwards { -1 } else { 1 })
            .rem_euclid(self.matches.len() as isize) as usize;
        let m = &self.matches[self.match_index];
        self.input
            .update(cx, |i, cx| i.select_range(m.from..m.to, cx));
        cx.notify();
    }
    fn replace_matches(&mut self, all: bool, cx: &mut Context<Self>) {
        if self.input.read(cx).is_composing() || self.matches.is_empty() {
            return;
        }
        let mut text = self.input.read(cx).content.to_string();
        let cursor = if all {
            for m in self.matches.iter().rev() {
                text.replace_range(m.from..m.to, &m.insert);
            }
            0
        } else {
            let m = &self.matches[self.match_index];
            text.replace_range(m.from..m.to, &m.insert);
            m.from + m.insert.len()
        };
        self.input.update(cx, |i, cx| {
            i.replace_bytes(0..i.content.len(), &text, cursor, cx)
        });
    }
    fn step_change(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let changes = &self.input.read(cx).editor_analysis().changes;
        if changes.is_empty() {
            return;
        }
        self.change_index = if backwards {
            self.change_index.saturating_sub(1)
        } else {
            (self.change_index + 1).min(changes.len() - 1)
        };
        let from = changes[self.change_index].from;
        self.input
            .update(cx, |i, cx| i.select_range(from..from, cx));
        cx.notify();
    }
    fn revert_change(&mut self, cx: &mut Context<Self>) {
        self.apply_change(false, cx);
    }
    fn dismiss_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open {
            self.search_open = false;
            self.search_task = None;
            self.input
                .update(cx, |i, cx| i.set_search_matches(Vec::new(), cx));
            window.focus(&self.input.focus_handle(cx), cx);
            cx.notify();
        } else {
            cx.propagate();
        }
    }
}
impl gpui::Render for FileEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.document.is_some();
        let controls = ready.then(|| {
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_1p5()
                .when(!self.preview, |d| d.child(self.word_wrap_control(cx)))
                .when(self.is_markdown() || self.is_svg(), |d| {
                    d.child(self.view_mode_control(cx))
                })
                .into_any_element()
        });
        let selection_menu = self.code_selection_menu(window, cx);
        self.input.update(cx, |i, _| {
            i.set_editor_diff(self.show_diff, !self.git_staged)
        });
        if ((!ready || self.preview) && self.input.focus_handle(cx).is_focused(window))
            || (!self.search_open
                && (self.search.focus_handle(cx).is_focused(window)
                    || self.replacement.focus_handle(cx).is_focused(window)))
        {
            window.focus(&self.focus, cx);
        }
        if ready && !self.preview && self.focus.is_focused(window) {
            window.focus(&self.input.focus_handle(cx), cx);
        }
        let preview_image: Option<ImageSource> = match &self.image {
            Some(image) => Some(image.clone().into()),
            None if self.preview && self.svg_failed => {
                Some(ImageSource::Custom(Arc::new(|_, _| {
                    Some(Err(ImageCacheError::Asset("Invalid SVG".into())))
                })))
            }
            None if self.preview => self.svg_preview.clone().map(ImageSource::Render),
            None => None,
        };
        let body: AnyElement = if let Some(media) = &self.media {
            media.clone().into_any_element()
        } else if let Some(image) = preview_image {
            div()
                .size_full()
                .p_4()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    img(image)
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .with_fallback(|| {
                            div()
                                .text_size(px(12.))
                                .text_color(content(0.5))
                                .child("Couldn’t decode this image")
                                .into_any_element()
                        }),
                )
                .into_any_element()
        } else if ready {
            if self.preview {
                self.markdown
                    .clone()
                    .map(|m| m.into_any_element())
                    .unwrap_or_else(|| div().into_any_element())
            } else {
                self.input.clone().into_any_element()
            }
        } else {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .p_6()
                .text_size(px(12.))
                .text_color(content(0.5))
                .child(
                    self.unsupported
                        .clone()
                        .or(self.error.clone())
                        .unwrap_or_else(|| "Opening file…".into()),
                )
                .when(!self.loading, |d| {
                    d.child(
                        div()
                            .id("open-file-externally")
                            .role(Role::Button)
                            .aria_label("Open externally")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .hover(|s| s.bg(content(0.1)))
                            .child("Open externally")
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.open_url(&format!(
                                    "file://{}",
                                    this.path
                                        .replace('%', "%25")
                                        .replace(' ', "%20")
                                        .replace('#', "%23")
                                        .replace('?', "%3F")
                                ))
                            })),
                    )
                })
                .when(!self.loading, |d| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id("retry-file")
                                    .role(Role::Button)
                                    .aria_label("Retry")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .bg(content(0.1))
                                    .child("Retry")
                                    .on_click(cx.listener(|this, _, _, cx| this.load(true, cx))),
                            )
                            .child(
                                div()
                                    .id("reveal-file")
                                    .role(Role::Button)
                                    .aria_label("Reveal")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .bg(content(0.1))
                                    .child("Reveal")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let path = PathBuf::from(&this.path);
                                        cx.background_executor()
                                            .spawn(async move {
                                                let _ = monocode_core::files::reveal_path(&path);
                                            })
                                            .detach();
                                    })),
                            )
                            .child(
                                div()
                                    .id("copy-file-path")
                                    .role(Role::Button)
                                    .aria_label("Copy path")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .bg(content(0.1))
                                    .child("Copy path")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            this.path.clone(),
                                        ))
                                    })),
                            ),
                    )
                })
                .into_any_element()
        };
        div()
            .id("file-editor")
            .relative()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .key_context("FileEditor")
            .track_focus(&self.focus)
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.finish_code_selection(window, cx)),
            )
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.selection.dismiss();
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, w, cx| {
                if this.search_open
                    && event.keystroke.key == "enter"
                    && event.keystroke.modifiers.shift
                    && this.search.focus_handle(cx).is_focused(w)
                {
                    this.step_match(true, cx);
                    cx.stop_propagation();
                    return;
                }
                if let Some(media) = &this.media {
                    media.update(cx, |m, cx| m.zoom_key(event, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &Save, _, cx| this.save(cx)))
            .on_action(cx.listener(|this, _: &ToggleWordWrap, _, cx| this.toggle_word_wrap(cx)))
            .on_action(cx.listener(|this, _: &ReloadFile, w, cx| this.reload(w, cx)))
            .on_action(cx.listener(|this, _: &Find, w, cx| this.open_search(w, cx)))
            .on_action(cx.listener(|this, _: &Replace, w, cx| {
                this.open_search(w, cx);
                this.replace_open = true;
            }))
            .on_action(cx.listener(|this, _: &Format, _, cx| this.format_document(cx)))
            .on_action(cx.listener(|this, _: &MatchCase, _, cx| {
                if this.search_open {
                    this.match_case = !this.match_case;
                    this.find_matches(cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &WholeWord, _, cx| {
                if this.search_open {
                    this.whole_word = !this.whole_word;
                    this.find_matches(cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &Regex, _, cx| {
                if this.search_open {
                    this.regex = !this.regex;
                    this.find_matches(cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &PreviousChange, _, cx| this.step_change(true, cx)))
            .on_action(cx.listener(|this, _: &NextChange, _, cx| this.step_change(false, cx)))
            .on_action(cx.listener(|this, _: &RevertChange, _, cx| this.revert_change(cx)))
            .on_action(cx.listener(|this, _: &FindNext, _, cx| this.step_match(false, cx)))
            .on_action(cx.listener(|this, _: &FindPrevious, _, cx| this.step_match(true, cx)))
            .on_action(cx.listener(|this, _: &DismissSearch, w, cx| this.dismiss_search(w, cx)))
            .child(self.path_header(controls, cx))
            .when(self.show_diff && ready && !self.preview, |d| {
                d.child(self.diff_toolbar(cx))
            })
            .when(self.search_open, |d| d.child(self.search_panel(cx)))
            .when_some(
                self.error.clone().filter(|_| ready).or_else(|| {
                    self.disk_changed
                        .then(|| "File changed on disk. Reload to use the new version.".into())
                }),
                |d, error| {
                    d.child(
                        div()
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgb(0xfbbf24))
                            .child(error),
                    )
                },
            )
            .child(div().flex_1().min_h_0().min_w_0().child(body))
            .when(ready && !self.preview, |d| d.child(self.status_footer(cx)))
            .child(self.selection_surface(cx))
            .children(selection_menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        Bounds, EntityInputHandler, TestAppContext, VisualTestContext, WindowBounds, WindowOptions,
        point, size,
    };
    #[gpui::test]
    fn file_overview_tracks_unsaved_syntax_and_staged_or_unstaged_git_baselines(
        cx: &mut TestAppContext,
    ) {
        let root =
            std::env::temp_dir().join(format!("monocode-file-overview-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let result = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.com",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        };
        git(&["init"]);
        let path = root.join("sample.js");
        let base = "const value = 1;\n";
        std::fs::write(&path, base).unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "base"]);
        let changed = "const value = ;\n";
        std::fs::write(&path, changed).unwrap();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                let project = cx.new(|cx| ProjectGit::new(root.to_string_lossy().into_owned(), cx));
                cx.new(|cx| FileEditor::new(path.to_string_lossy().into_owned(), project, cx))
            })
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| view.set_visible(true, cx))
            .unwrap();
        let finish_analysis = |cx: &mut TestAppContext| {
            cx.run_until_parked();
            cx.executor().advance_clock(Duration::from_millis(200));
            cx.run_until_parked();
        };
        finish_analysis(cx);
        window
            .update(cx, |view, w, cx| {
                assert_eq!(view.git_original.as_deref(), Some(base));
                let input = view.input.read(cx);
                assert_eq!(input.editor_analysis().changes.len(), 1);
                assert!(!input.editor_analysis().diagnostics.is_empty());
                view.input.update(cx, |input, cx| {
                    input.select_range(0..input.content.len(), cx);
                    input.replace_text_in_range(None, base, w, cx);
                });
            })
            .unwrap();
        finish_analysis(cx);
        window
            .update(cx, |view, _, cx| {
                assert!(view.dirty);
                assert!(view.input.read(cx).editor_analysis().changes.is_empty());
                assert!(view.input.read(cx).editor_analysis().diagnostics.is_empty());
            })
            .unwrap();
        git(&["add", "."]);
        window
            .update(cx, |view, _, cx| view.refresh_git(cx))
            .unwrap();
        finish_analysis(cx);
        window
            .update(cx, |view, _, _| {
                assert_eq!(
                    view.git_original.as_deref(),
                    Some(base),
                    "staged-only changes compare to HEAD"
                )
            })
            .unwrap();
        std::fs::write(&path, "const value = 2;\n").unwrap();
        window
            .update(cx, |view, _, cx| view.refresh_git(cx))
            .unwrap();
        finish_analysis(cx);
        window
            .update(cx, |view, _, cx| {
                assert_eq!(
                    view.git_original.as_deref(),
                    Some(changed),
                    "mixed staged/unstaged changes compare to the index"
                );
                assert_eq!(view.input.read(cx).editor_analysis().changes.len(), 1);
            })
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn file_links_resolve_relative_paths_and_line_locations() {
        assert_eq!(
            file_location("src/../main.rs:12:4", "/project"),
            ("/project/main.rs".into(), Some((12, 4)))
        );
        assert_eq!(
            file_location("file:///project/my%20file.rs#L3", "/elsewhere"),
            ("/project/my file.rs".into(), Some((3, 1)))
        );
        assert_eq!(
            file_location("./README.md", "/project"),
            ("/project/README.md".into(), None)
        );
        assert_eq!(
            file_location("/tmp/a.rs:0", "/project"),
            ("/tmp/a.rs".into(), Some((1, 1)))
        );
    }

    #[gpui::test]
    fn remembered_preview_mode_keeps_focus_and_find_switches_to_source(cx: &mut TestAppContext) {
        for (extension, source_mode) in [("md", false), ("svg", false), ("md", true), ("svg", true)]
        {
            let path = std::env::temp_dir().join(format!(
                "monocode-preview-focus-{}.{extension}",
                std::process::id()
            ));
            std::fs::write(
                &path,
                if extension == "md" {
                    "# Preview\n"
                } else {
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>"
                },
            )
            .unwrap();
            let file = path.to_string_lossy().into_owned();
            let window = cx.update(|cx| {
                let prefs = crate::settings::Preferences {
                    file_view_mode: if source_mode { "source" } else { "preview" }.into(),
                    ..Default::default()
                };
                cx.set_global(prefs);
                input::bind(cx);
                bind(cx);
                cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(0.), px(0.)),
                            size(px(600.), px(400.)),
                        ))),
                        ..Default::default()
                    },
                    |w, cx| {
                        let git = cx.new(|cx| ProjectGit::new("/tmp".into(), cx));
                        let view = cx.new(|cx| FileEditor::new(file, git, cx));
                        w.focus(&view.focus_handle(cx), cx);
                        view
                    },
                )
                .unwrap()
            });
            let cx = &mut VisualTestContext::from_window(window.into(), cx);
            cx.run_until_parked();
            window
                .update(cx, |e, w, cx| {
                    assert_eq!(e.preview, !source_mode);
                    assert!(e.focus.contains_focused(w, cx));
                    assert_eq!(e.input.focus_handle(cx).is_focused(w), source_mode);
                })
                .unwrap();
            let m = if cfg!(target_os = "macos") {
                "cmd"
            } else {
                "ctrl"
            };
            cx.simulate_keystrokes(&format!("{m}-f"));
            cx.run_until_parked();
            window
                .update(cx, |e, w, cx| {
                    assert!(e.search_open);
                    assert!(!e.preview);
                    assert!(e.search.focus_handle(cx).is_focused(w));
                })
                .unwrap();
            std::fs::remove_file(path).unwrap();
        }
    }

    #[gpui::test]
    fn editor_load_find_save_and_external_edits_preserve_the_draft(cx: &mut TestAppContext) {
        let path = std::env::temp_dir().join(format!("monocode-editor-{}.rs", std::process::id()));
        std::fs::write(&path, "fn hello() {}\r\n// hello 🙂\r\n").unwrap();
        let file = path.to_string_lossy().into_owned();
        let window = cx.update(|cx| {
            input::bind(cx);
            bind(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(600.), px(400.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    let git = cx.new(|cx| {
                        ProjectGit::new(std::env::temp_dir().to_string_lossy().into_owned(), cx)
                    });
                    cx.new(|cx| FileEditor::new(file, git, cx))
                },
            )
            .unwrap()
        });
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        window
            .update(cx, |e, w, cx| {
                assert!(e.document.is_some());
                assert!(!e.dirty);
                assert_eq!(
                    e.input.read(cx).content.as_ref(),
                    "fn hello() {}\n// hello 🙂\n"
                );
                e.open_search(w, cx);
                e.search.update(cx, |i, cx| i.set_text("hello".into(), cx));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |e, w, cx| {
                assert_eq!(e.matches.len(), 2);
                e.step_match(false, cx);
                assert_eq!(e.match_index, 1);
                e.dismiss_search(w, cx);
                e.input
                    .update(cx, |i, cx| i.replace_text_in_range(None, "world", w, cx));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |e, _, cx| {
                assert!(e.dirty);
                e.save(cx);
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "fn hello() {}\r\n// world 🙂\r\n"
        );
        window
            .update(cx, |e, w, cx| {
                assert!(!e.dirty);
                e.input
                    .update(cx, |i, cx| i.replace_text_in_range(None, " draft", w, cx));
            })
            .unwrap();
        cx.run_until_parked();
        std::fs::write(&path, "external\n").unwrap();
        window.update(cx, |e, _, cx| e.load(false, cx)).unwrap();
        cx.run_until_parked();
        window
            .update(cx, |e, _, cx| {
                assert!(e.disk_changed);
                assert!(e.dirty);
                assert!(e.input.read(cx).content.contains("draft"));
                e.save(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |e, _, _| {
                assert!(e.dirty);
                assert!(e.error.as_ref().unwrap().contains("changed on disk"));
            })
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external\n");
        std::fs::write(&path, include_bytes!("../assets/monocode.png")).unwrap();
        window.update(cx, |e, _, cx| e.load(true, cx)).unwrap();
        cx.run_until_parked();
        window
            .update(cx, |e, _, _| {
                assert!(e.image.is_some());
                assert!(!e.dirty);
                assert!(e.document.is_none());
            })
            .unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
