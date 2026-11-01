//! modeCommands.tsx's inline icon, tinted command name and first-line spacing.
//! The editor keeps the original `/command` bytes for clipboard, IME and edits.
use super::*;

// A 1.5-em blank replaces the slash when shaping. Its byte mapping is shared by
// hit testing, selection, caret movement and IME geometry; wrapped lines retain
// the full field width rather than inheriting a separate prefix column.
const ICON_SLOT: &str = "\u{2003}\u{2002}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Plan,
    Operator,
    Orchestrator,
}
impl Command {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "plan" => Some(Self::Plan),
            "operator" => Some(Self::Operator),
            "orchestrator" => Some(Self::Orchestrator),
            _ => None,
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Operator => "operator",
            Self::Orchestrator => "orchestrator",
        }
    }
    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::Plan => "ai-idea",
            Self::Operator => "cursor-magic",
            Self::Orchestrator => "share",
        }
    }
    pub(crate) fn color(self) -> Hsla {
        let dark = crate::theme::content(1.).l > 0.5;
        let mut color: Hsla = rgb(match (self, dark) {
            (Self::Plan, true) => 0xfff085,
            (Self::Plan, false) => 0xa65f00,
            (Self::Operator, true) => 0xb8e6fe,
            (Self::Operator, false) => 0x0069a8,
            (Self::Orchestrator, true) => 0xf6cfff,
            (Self::Orchestrator, false) => 0xa800b7,
        })
        .into();
        if dark {
            color.a = 0.9;
        }
        color
    }
    pub(crate) fn leading(text: &str) -> Option<Self> {
        let token = text.strip_prefix('/')?.split(char::is_whitespace).next()?;
        Self::from_name(token)
    }
}

pub(crate) struct CommandRemoved(pub &'static str);
impl EventEmitter<CommandRemoved> for Input {}

#[derive(Clone, Copy)]
pub(super) struct CommandLayout {
    pub mode: Command,
    /// Actual `/command` replaces one byte; a retained selected mode is virtual.
    source_prefix: usize,
    display_prefix: usize,
}
impl CommandLayout {
    pub fn to_display(self, at: usize) -> usize {
        if self.source_prefix == 0 {
            at + self.display_prefix
        } else if at == 0 {
            0
        } else {
            at + ICON_SLOT.len() - 1
        }
    }
    pub fn to_source(self, at: usize) -> usize {
        if self.source_prefix == 0 {
            at.saturating_sub(self.display_prefix)
        } else if at == 0 {
            0
        } else {
            at.saturating_sub(ICON_SLOT.len() - 1).max(1)
        }
    }
    pub fn name_range(self) -> Range<usize> {
        ICON_SLOT.len()..ICON_SLOT.len() + self.mode.name().len()
    }
    pub fn icon_range(self) -> Range<usize> {
        0..ICON_SLOT.len()
    }
}

impl Input {
    /// The compact composer in BtwSheet; Enter sends and Shift+Enter adds a line.
    pub(crate) fn side_question(cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.label = "By-the-way question".into();
        input.placeholder = "Ask a side question…".into();
        // `rows={1}`: the side composer has no workspace bar under it.
        input.min_height = 22.;
        input.height = 22.;
        input
    }

    pub(crate) fn enable_inline_commands(&mut self, cx: &mut Context<Self>) {
        self.inline_commands = true;
        cx.notify();
    }
    pub(crate) fn selected_command(&mut self, mode: Option<Command>, cx: &mut Context<Self>) {
        if self.selected_command != mode {
            self.selected_command = mode;
            cx.notify();
        }
    }
    pub(crate) fn has_message_text(&self) -> bool {
        let body = Command::leading(&self.content).map_or(self.content.as_ref(), |mode| {
            &self.content[mode.name().len() + 1..]
        });
        !body.trim().is_empty()
    }
    pub(super) fn command_layout(&self) -> Option<CommandLayout> {
        if !self.inline_commands || self.single_line || self.code.is_some() {
            return None;
        }
        if let Some(mode) = Command::leading(&self.content) {
            Some(CommandLayout {
                mode,
                source_prefix: 1,
                display_prefix: ICON_SLOT.len(),
            })
        } else {
            self.selected_command.map(|mode| CommandLayout {
                mode,
                source_prefix: 0,
                display_prefix: ICON_SLOT.len() + mode.name().len() + 1,
            })
        }
    }
    pub(super) fn display_text(&self) -> (SharedString, DisplayLayout) {
        if self.secret && !self.content.is_empty() {
            // Preserve byte positions used by the native selection handler.
            return (
                "*".repeat(self.content.len()).into(),
                DisplayLayout::default(),
            );
        }
        let text = if self.content.is_empty() {
            &self.placeholder
        } else {
            &self.content
        };
        let layout = self.display_layout();
        let display = match layout.command {
            Some(command) if command.source_prefix == 1 => {
                format!("{ICON_SLOT}{}", &text[1..]).into()
            }
            Some(command) => format!("{ICON_SLOT}{} {text}", command.mode.name()).into(),
            None => text.clone(),
        };
        (layout.apply(display), layout)
    }
    pub(super) fn command_removed(&self, previous: Option<Command>, cx: &mut Context<Self>) {
        if let Some(mode) = previous
            && Command::leading(&self.content) != Some(mode)
        {
            cx.emit(CommandRemoved(mode.name()));
        }
    }
}
