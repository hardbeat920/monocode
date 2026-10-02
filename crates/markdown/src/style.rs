//! Caller-supplied look for rendered markdown.
//!
//! Values in the [`MarkdownStyle::dark`] and [`MarkdownStyle::light`] presets
//! come from the `.agent-markdown` rules in `src/styles/index.css`, the
//! Tailwind classes that Streamdown 2.5 puts on each element, and the
//! `github-dark` and `github-light` Shiki themes that `@streamdown/code` uses.
//! The app maps its own theme tokens onto this struct; this crate never reads
//! a theme on its own.

use gpui::{Hsla, Pixels, Rgba, SharedString, px, rgb, rgba};

/// Colors for syntax tokens in fenced code blocks. Each field is the color for
/// one class of TextMate scope (see `highlight::TokenKind`).
#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxColors {
    /// Text that matches no token class.
    pub plain: Hsla,
    pub comment: Hsla,
    pub keyword: Hsla,
    pub string: Hsla,
    /// Numbers, booleans, language constants, and support names.
    pub constant: Hsla,
    /// Function, class, and type names.
    pub function: Hsla,
    /// Variables and parameters that the grammar marks as such.
    pub variable: Hsla,
    /// Markup and JSX tag names.
    pub tag: Hsla,
    /// Attribute names and object keys.
    pub attribute: Hsla,
    /// Inserted lines in diffs.
    pub inserted: Hsla,
    /// Deleted lines in diffs.
    pub deleted: Hsla,
    /// Markdown headings inside code, and other emphasized markup.
    pub heading: Hsla,
}

impl SyntaxColors {
    /// The `github-dark` Shiki theme.
    pub fn github_dark() -> Self {
        Self {
            plain: color(rgb(0xe1e4e8)),
            comment: color(rgb(0x6a737d)),
            keyword: color(rgb(0xf97583)),
            string: color(rgb(0x9ecbff)),
            constant: color(rgb(0x79b8ff)),
            function: color(rgb(0xb392f0)),
            variable: color(rgb(0xffab70)),
            tag: color(rgb(0x85e89d)),
            attribute: color(rgb(0xb392f0)),
            inserted: color(rgb(0x85e89d)),
            deleted: color(rgb(0xfdaeb7)),
            heading: color(rgb(0x79b8ff)),
        }
    }

    /// The `github-light` Shiki theme.
    pub fn github_light() -> Self {
        Self {
            plain: color(rgb(0x24292e)),
            comment: color(rgb(0x6a737d)),
            keyword: color(rgb(0xd73a49)),
            string: color(rgb(0x032f62)),
            constant: color(rgb(0x005cc5)),
            function: color(rgb(0x6f42c1)),
            variable: color(rgb(0xe36209)),
            tag: color(rgb(0x22863a)),
            attribute: color(rgb(0x6f42c1)),
            inserted: color(rgb(0x22863a)),
            deleted: color(rgb(0xb31d28)),
            heading: color(rgb(0x005cc5)),
        }
    }
}

/// Space above and below one kind of block. Neighboring margins collapse the
/// way CSS block margins do: the gap between two blocks is the larger of the
/// first block's bottom margin and the second block's top margin. The first
/// block of a message gets no top margin and the last gets no bottom margin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockMargins {
    pub top: Pixels,
    pub bottom: Pixels,
}

impl BlockMargins {
    pub const fn new(top: f32, bottom: f32) -> Self {
        Self {
            top: px(top),
            bottom: px(bottom),
        }
    }
}

/// Everything the renderer needs to know about fonts, colors, sizes, and
/// spacing. Build one from the app theme, or start from a preset and change
/// the fields that differ.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkdownStyle {
    // Fonts.
    /// Body font family. `.SystemUIFont` is the platform UI font.
    pub font_family: SharedString,
    /// Font family for inline code and code blocks. CSS asks for
    /// `ui-monospace`, which has no portable name, so the presets use Menlo.
    pub mono_font_family: SharedString,

    // Body text.
    /// `text-sm`.
    pub text_size: Pixels,
    /// `leading-6`.
    pub line_height: Pixels,
    /// Full-strength content color. Strong and emphasized text use it.
    pub text: Hsla,
    /// Paragraph and list text (`content` at 78%).
    pub body_text: Hsla,
    /// Paragraph text in a reasoning block (`content` at 48%). Used when the
    /// view is in reasoning mode.
    pub reasoning_text: Hsla,
    /// Strong and emphasized text in a reasoning block (`content` at 62%).
    pub reasoning_emphasis: Hsla,

    // Headings.
    pub heading: Hsla,
    /// Font sizes for `h1` to `h6`. `h1` and `h2` are overridden in index.css;
    /// `h3` to `h6` keep Streamdown's Tailwind sizes.
    pub heading_sizes: [Pixels; 6],
    /// Line heights for `h1` to `h6`, from each Tailwind size's ratio.
    pub heading_line_heights: [Pixels; 6],

    // Links.
    /// `text-sky-400/90`.
    pub link: Hsla,
    /// `hover:text-sky-300`, painted as the hover underline.
    pub link_hover: Hsla,

    // Inline code.
    /// Inline code size (`0.8em` of the body size).
    pub inline_code_size: Pixels,
    pub inline_code_text: Hsla,
    /// `bg-content/8`.
    pub inline_code_background: Hsla,
    pub inline_code_radius: Pixels,

    // Code blocks.
    /// Code block body size (index.css sets 12px).
    pub code_size: Pixels,
    pub code_line_height: Pixels,
    /// `bg-content/6`.
    pub code_background: Hsla,
    /// `border-content/10`, also the line under the header.
    pub code_border: Hsla,
    pub code_radius: Pixels,
    /// Language label color (`content` at 65%).
    pub code_label: Hsla,
    pub code_label_size: Pixels,
    pub code_header_height: Pixels,
    /// Line number color (`content` at 35%).
    pub line_number: Hsla,
    pub line_number_size: Pixels,
    /// Width of the line number column.
    pub line_number_width: Pixels,
    /// Copy button icon color (`content` at 45%).
    pub copy_icon: Hsla,
    /// Copy button icon color on hover.
    pub copy_icon_hover: Hsla,
    /// Copy button background on hover (`content` at 10%).
    pub copy_hover_background: Hsla,
    pub syntax: SyntaxColors,

    // Block quotes.
    /// `border-inline-start` color (`content` at 20%).
    pub quote_border: Hsla,
    pub quote_border_width: Pixels,
    pub quote_padding: Pixels,

    // Lists.
    /// Indent of list content (`padding-inline-start: 1.5rem`).
    pub list_indent: Pixels,
    /// `py-1` on each list item.
    pub list_item_padding: Pixels,
    /// Task checkbox border and fill.
    pub checkbox_border: Hsla,
    pub checkbox_checked: Hsla,
    pub checkbox_check: Hsla,

    // Tables.
    pub table_background: Hsla,
    pub table_border: Hsla,
    /// Lines between rows (`content` at 5%).
    pub table_row_border: Hsla,
    pub table_radius: Pixels,
    pub table_text_size: Pixels,
    pub table_line_height: Pixels,
    pub table_cell_padding_x: Pixels,
    pub table_cell_padding_y: Pixels,

    // Rules and images.
    /// `border-top` color of a horizontal rule (`content` at 10%).
    pub rule: Hsla,
    /// Largest height an image may take.
    pub image_max_height: Pixels,
    pub image_radius: Pixels,

    // Selection.
    pub selection: Hsla,

    // Spacing between blocks.
    pub paragraph_margins: BlockMargins,
    pub heading_margins: BlockMargins,
    pub list_margins: BlockMargins,
    pub code_margins: BlockMargins,
    pub quote_margins: BlockMargins,
    pub table_margins: BlockMargins,
    pub rule_margins: BlockMargins,

    // Motion.
    /// How long one word takes to fade in (`WORD_FADE_MS` in wordFade.tsx).
    pub word_fade_ms: f32,
}

impl MarkdownStyle {
    /// The dark theme, with `content` at 92% lightness.
    pub fn dark() -> Self {
        let content = gpui::hsla(0., 0., 0.92, 1.);
        Self::with_content(
            content,
            color(rgb(0xf9a8c9)),
            color(rgba(0x38bdf8e6)),
            color(rgb(0x7dd3fc)),
            SyntaxColors::github_dark(),
        )
    }

    /// The light theme, with `content` at 18% lightness.
    pub fn light() -> Self {
        let content = gpui::hsla(0., 0., 0.18, 1.);
        Self::with_content(
            content,
            color(rgb(0xbe185d)),
            color(rgba(0x38bdf8e6)),
            gpui::hsla(211. / 360., 0.92, 0.40, 1.),
            SyntaxColors::github_light(),
        )
    }

    /// Derive every content-relative color from one content color, the way
    /// index.css derives them with `color-mix`.
    pub fn with_content(
        content: Hsla,
        heading: Hsla,
        link: Hsla,
        link_hover: Hsla,
        syntax: SyntaxColors,
    ) -> Self {
        let mix = |amount: f32| content.opacity(amount);
        Self {
            font_family: ".SystemUIFont".into(),
            mono_font_family: "Menlo".into(),
            text_size: px(14.),
            line_height: px(24.),
            text: content,
            body_text: mix(0.78),
            reasoning_text: mix(0.48),
            reasoning_emphasis: mix(0.62),
            heading,
            heading_sizes: [px(22.), px(18.), px(20.), px(18.), px(16.), px(14.)],
            heading_line_heights: [px(26.4), px(24.), px(28.), px(28.), px(24.), px(20.)],
            link,
            link_hover,
            inline_code_size: px(11.2),
            inline_code_text: content,
            inline_code_background: mix(0.08),
            inline_code_radius: px(6.),
            code_size: px(12.),
            code_line_height: px(18.),
            code_background: mix(0.06),
            code_border: mix(0.10),
            code_radius: px(10.),
            code_label: mix(0.65),
            code_label_size: px(12.),
            code_header_height: px(36.),
            line_number: mix(0.35),
            line_number_size: px(10.),
            line_number_width: px(24.),
            copy_icon: mix(0.45),
            copy_icon_hover: content,
            copy_hover_background: mix(0.10),
            syntax,
            quote_border: mix(0.20),
            quote_border_width: px(4.),
            quote_padding: px(16.),
            list_indent: px(24.),
            list_item_padding: px(4.),
            checkbox_border: mix(0.35),
            checkbox_checked: gpui::hsla(211. / 360., 0.92, 0.62, 1.),
            checkbox_check: gpui::white(),
            table_background: mix(0.06),
            table_border: mix(0.10),
            table_row_border: mix(0.05),
            table_radius: px(10.),
            table_text_size: px(12.),
            table_line_height: px(18.),
            table_cell_padding_x: px(10.),
            table_cell_padding_y: px(8.),
            rule: mix(0.10),
            image_max_height: px(480.),
            image_radius: px(8.),
            selection: gpui::hsla(211. / 360., 0.92, 0.62, 0.35),
            paragraph_margins: BlockMargins::new(16., 0.),
            heading_margins: BlockMargins::new(24., 8.),
            list_margins: BlockMargins::new(8., 0.),
            code_margins: BlockMargins::new(16., 16.),
            quote_margins: BlockMargins::new(16., 16.),
            table_margins: BlockMargins::new(16., 16.),
            rule_margins: BlockMargins::new(24., 24.),
            word_fade_ms: 320.,
        }
    }

    /// Font size and line height for a heading level (1 to 6).
    pub fn heading_metrics(&self, level: u8) -> (Pixels, Pixels) {
        let ix = (level.clamp(1, 6) - 1) as usize;
        (self.heading_sizes[ix], self.heading_line_heights[ix])
    }
}

impl Default for MarkdownStyle {
    fn default() -> Self {
        Self::dark()
    }
}

fn color(value: Rgba) -> Hsla {
    value.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_derive_content_mixes() {
        let dark = MarkdownStyle::dark();
        assert!((dark.body_text.a - 0.78).abs() < 1e-6);
        assert!((dark.code_border.a - 0.10).abs() < 1e-6);
        assert_eq!(dark.heading_metrics(1).0, px(22.));
        assert_eq!(dark.heading_metrics(9).0, px(14.));
        let light = MarkdownStyle::light();
        assert!(light.text.l < 0.5);
        assert_ne!(light.syntax, dark.syntax);
    }
}
