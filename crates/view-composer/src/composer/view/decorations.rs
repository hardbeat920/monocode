//! Turns the highlight model into prompt decorations: `text-skill`,
//! `text-mention`, the mode colors, and the icons over hidden glyphs.

use std::rc::Rc;

use gpui::{App, IntoElement as _, ParentElement as _, Styled as _, div};
use monocode_ui::{IconName, Theme, file_type_icon, folder_type_icon, icon, u};

use super::super::model::highlight::{
    IconSlot, SpanKind, composer_highlight, is_note_mention_path,
};
use super::super::model::mode_commands::{MODE_COMMAND_INDENT, leading_mode_command};
use super::super::prompt_input::{GlyphOverlay, OverlayPlacement, PromptDecorations};
use super::Composer;
use super::colors::mode_text;

impl Composer {
    /// Hands the prompt a decorator that reads this composer's skill names,
    /// mention index, and MCP tags.
    pub(crate) fn install_decorator(&mut self, cx: &mut gpui::Context<Self>) {
        let composer = cx.entity().downgrade();
        let decorator: super::super::prompt_input::Decorator = Rc::new(move |text, cx| {
            composer
                .upgrade()
                .map(|composer| composer.read(cx).decorations_for(text, cx))
                .unwrap_or_default()
        });
        self.prompt
            .update(cx, |prompt, cx| prompt.set_decorator(Some(decorator), cx));
    }

    /// `ComposerHighlight` for `text`.
    pub(crate) fn decorations_for(&self, text: &str, cx: &App) -> PromptDecorations {
        let theme = Theme::of(cx);
        let names = self.skill_names();
        let mode = leading_mode_command(text, &names);
        let highlight = composer_highlight(
            text,
            mode,
            &names,
            &self.mention_index.labels,
            &self.selected_mcp,
        );
        let spans = highlight
            .spans
            .into_iter()
            .map(|(range, kind)| {
                let color = match kind {
                    SpanKind::Mode(mode) => mode_text(mode, theme),
                    SpanKind::Skill => theme.colors.skill,
                    SpanKind::McpTag | SpanKind::Mention => theme.colors.mention,
                };
                (range, color)
            })
            .collect();
        let overlays = highlight
            .icons
            .into_iter()
            .map(|slot| match slot {
                IconSlot::Mode { at, mode } => {
                    let color = mode_text(mode, theme);
                    GlyphOverlay {
                        range: at,
                        placement: OverlayPlacement::Before(MODE_COMMAND_INDENT),
                        size: 14.,
                        render: Rc::new(move |_, _| {
                            icon(mode.icon())
                                .size(u(14.))
                                .text_color(color)
                                .into_any_element()
                        }),
                    }
                }
                IconSlot::File { at, file } => {
                    let note = is_note_mention_path(&file.path);
                    let ink = theme.colors.mention;
                    GlyphOverlay {
                        range: at,
                        placement: OverlayPlacement::Center,
                        size: if note { 14. } else { 13. },
                        render: Rc::new(move |_, _| {
                            if note {
                                icon(IconName::StickyNote)
                                    .size(u(14.))
                                    .text_color(ink)
                                    .into_any_element()
                            } else if file.is_dir {
                                div()
                                    .size(u(13.))
                                    .child(
                                        folder_type_icon(file.name.clone(), false, false).size(13.),
                                    )
                                    .into_any_element()
                            } else {
                                div()
                                    .size(u(13.))
                                    .child(file_type_icon(file.name.clone()).size(13.))
                                    .into_any_element()
                            }
                        }),
                    }
                }
            })
            .collect();
        PromptDecorations {
            spans,
            hidden: highlight.hidden,
            overlays,
            first_line_indent: if highlight.indented {
                MODE_COMMAND_INDENT
            } else {
                0.
            },
        }
    }
}
