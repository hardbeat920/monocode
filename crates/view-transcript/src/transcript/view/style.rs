//! Colors and sizes the transcript reads, in one place.
//!
//! Theme tokens come from `monocode_ui::Theme`. A few React classes use
//! fixed Tailwind palette colors that are not theme tokens (the diff line
//! tints in FilePreview.tsx, the task states in TaskListPreview.tsx). They
//! live here, named after their Tailwind class.

use gpui::{Hsla, Styled, px};
use monocode_markdown::{MarkdownStyle, SyntaxColors};
use monocode_ui::color::{hex, mix, with_alpha};
use monocode_ui::{Theme, u};

/// The transcript column (`max-w-4xl`).
pub const COLUMN_MAX_WIDTH: f32 = 896.;
/// The user bubble in the chat layout (`max-w-xl`).
pub const BUBBLE_MAX_WIDTH: f32 = 576.;
/// The live phase window (`max-height: min(17.5rem, 45vh)`).
pub const LIVE_WINDOW_MAX_HEIGHT: f32 = 280.;
/// `pb-8` under the last turn.
pub const BOTTOM_PADDING: f32 = 32.;

/// Tailwind palette colors the React classes name directly.
pub mod palette {
    use super::*;

    /// `teal-300`, FilePreview keywords.
    pub fn teal_300() -> Hsla {
        hex(0x46ecd5)
    }
    /// `teal-400`, added-line bar and mark.
    pub fn teal_400() -> Hsla {
        hex(0x00d5be)
    }
    /// `teal-800/20`, added-line tint.
    pub fn teal_800_20() -> Hsla {
        with_alpha(hex(0x005f5a), 0.2)
    }
    /// `rose-400`, removed-line bar and mark.
    pub fn rose_400() -> Hsla {
        hex(0xff637e)
    }
    /// `rose-800/20`, removed-line tint.
    pub fn rose_800_20() -> Hsla {
        with_alpha(hex(0xa50036), 0.2)
    }
    /// `amber-200/90`, FilePreview type names.
    pub fn amber_200_90() -> Hsla {
        with_alpha(hex(0xfee685), 0.9)
    }
    /// `amber-300/80`, "Mixed changes".
    pub fn amber_300_80() -> Hsla {
        with_alpha(hex(0xffd230), 0.8)
    }
    /// `emerald-300`, a completed task's check.
    pub fn emerald_300() -> Hsla {
        hex(0x5ee9b5)
    }
    /// `sky-300`, an in-progress task and a hovered file chip.
    pub fn sky_300() -> Hsla {
        hex(0x74d4ff)
    }
    /// `violet-400`, a pull request chip.
    pub fn violet_400() -> Hsla {
        hex(0xa684ff)
    }
    /// `yellow-100`, a hovered plan title.
    pub fn yellow_100() -> Hsla {
        hex(0xfef9c2)
    }
}

/// `--zen-rail`: content at 14% over the background, opaque so the curve and
/// the spine do not stack where they meet.
pub fn rail_color(theme: &Theme) -> Hsla {
    mix(theme.colors.content, theme.colors.background_base, 0.14)
}

/// Font size and line height shortcuts for the Tailwind sizes the
/// transcript uses. Line heights follow Tailwind's defaults.
pub trait TextSizes: Styled + Sized {
    /// `text-sm`: 14px on 20px.
    fn text_sm_ui(self) -> Self {
        self.text_size(u(14.)).line_height(u(20.))
    }
    /// `text-xs`: 12px on 16px.
    fn text_xs_ui(self) -> Self {
        self.text_size(u(12.)).line_height(u(16.))
    }
    /// `text-[Npx]` on the transcript's `leading-5`.
    fn text_px_l5(self, size: f32) -> Self {
        self.text_size(u(size)).line_height(u(20.))
    }
}

impl<T: Styled> TextSizes for T {}

/// How a markdown block reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkdownVariant {
    /// An answer or note at full strength.
    Normal,
    /// `.zen-fold-prose`: prose the open fold holds, one notch quieter, with
    /// headings demoted to bold lines and tighter spacing.
    FoldProse,
}

/// The markdown style for the current theme (`.agent-markdown` in index.css).
pub fn markdown_style(theme: &Theme, variant: MarkdownVariant) -> MarkdownStyle {
    let content = theme.colors.content;
    let syntax = if theme.is_dark() {
        SyntaxColors::github_dark()
    } else {
        SyntaxColors::github_light()
    };
    // `text-sky-400/90` links, `hover:text-sky-300`.
    let mut style = MarkdownStyle::with_content(
        content,
        theme.colors.markdown_heading,
        with_alpha(hex(0x00bcff), 0.9),
        theme.colors.link,
        syntax,
    );
    style.font_family = theme.fonts.sans.clone();
    style.mono_font_family = theme.fonts.mono.clone();
    style.selection = theme.accent(0.35);
    if variant == MarkdownVariant::FoldProse {
        style.body_text = with_alpha(content, 0.65);
        style.text = with_alpha(content, 0.80);
        style.heading = with_alpha(content, 0.80);
        style.heading_sizes = [px(14.); 6];
        style.heading_line_heights = [px(20.); 6];
        style.heading_margins = monocode_markdown::BlockMargins::new(12., 4.);
        style.paragraph_margins = monocode_markdown::BlockMargins::new(8., 0.);
        style.list_margins = monocode_markdown::BlockMargins::new(8., 0.);
        style.code_margins = monocode_markdown::BlockMargins::new(8., 8.);
        style.quote_margins = monocode_markdown::BlockMargins::new(8., 8.);
        style.table_margins = monocode_markdown::BlockMargins::new(8., 8.);
        style.rule_margins = monocode_markdown::BlockMargins::new(12., 12.);
        style.code_border = with_alpha(content, 0.08);
        style.code_background = with_alpha(content, 0.04);
        style.table_border = with_alpha(content, 0.08);
        style.table_background = with_alpha(content, 0.04);
    }
    scale_markdown(style, theme.ui_scale())
}

/// The markdown view sizes in pixels, so follow the interface scale here.
fn scale_markdown(mut style: MarkdownStyle, scale: f32) -> MarkdownStyle {
    if (scale - 1.).abs() < f32::EPSILON {
        return style;
    }
    let s = |value: gpui::Pixels| value * scale;
    style.text_size = s(style.text_size);
    style.line_height = s(style.line_height);
    style.heading_sizes = style.heading_sizes.map(s);
    style.heading_line_heights = style.heading_line_heights.map(s);
    style.inline_code_size = s(style.inline_code_size);
    style.code_size = s(style.code_size);
    style.code_line_height = s(style.code_line_height);
    style.code_label_size = s(style.code_label_size);
    style.code_header_height = s(style.code_header_height);
    style.line_number_size = s(style.line_number_size);
    style.line_number_width = s(style.line_number_width);
    style.list_indent = s(style.list_indent);
    style.list_item_padding = s(style.list_item_padding);
    style.table_text_size = s(style.table_text_size);
    style.table_line_height = s(style.table_line_height);
    style.table_cell_padding_x = s(style.table_cell_padding_x);
    style.table_cell_padding_y = s(style.table_cell_padding_y);
    style.quote_padding = s(style.quote_padding);
    for margins in [
        &mut style.paragraph_margins,
        &mut style.heading_margins,
        &mut style.list_margins,
        &mut style.code_margins,
        &mut style.quote_margins,
        &mut style.table_margins,
        &mut style.rule_margins,
    ] {
        margins.top = s(margins.top);
        margins.bottom = s(margins.bottom);
    }
    style
}
