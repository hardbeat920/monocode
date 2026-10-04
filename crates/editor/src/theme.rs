//! Colors and fonts for the editor and diff views.
//!
//! The syntax palettes are ported from `HIGHLIGHT_PALETTE` in
//! src/features/files/editor/editorLanguage.ts. The chrome colors come from
//! editorChrome.ts, editorGit.ts, editorSearch.ts, and UnifiedDiffView.tsx,
//! resolved against the default tokens in src/styles/index.css. The app
//! builds an [`EditorTheme`] from `monocode-ui`'s theme, including the diff
//! palette through [`EditorTheme::with_diff_colors`]; the defaults here keep
//! this crate usable on its own.

use std::sync::Arc;

use gpui::{Hsla, Pixels, SharedString, px, rgb};
use gpui_base::input::{HighlightStyleResolver, InputEditorStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorScheme {
    Dark,
    Light,
}

/// `HighlightPalette`: one color per tag group.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyntaxPalette {
    pub keyword: Hsla,
    pub heading: Hsla,
    pub callable: Hsla,
    pub string: Hsla,
    pub type_: Hsla,
    pub number: Hsla,
    pub comment: Hsla,
    pub property: Hsla,
    pub meta: Hsla,
    pub invalid: Hsla,
}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

fn with_alpha(color: Hsla, alpha: f32) -> Hsla {
    Hsla { a: alpha, ..color }
}

impl SyntaxPalette {
    /// `HIGHLIGHT_PALETTE.dark`. `heading` is `--color-markdown-heading`.
    pub fn dark() -> Self {
        Self {
            keyword: hex(0xff8ffd),
            heading: hex(0xf9a8c9),
            callable: hex(0xa5d5fe),
            string: hex(0xb4fa72),
            type_: hex(0xff8272),
            number: hex(0xb4fa72),
            comment: hex(0xfefdc2),
            property: hex(0xd0d1fe),
            meta: hex(0x8e8e8e),
            invalid: hex(0xffc4bd),
        }
    }

    /// `HIGHLIGHT_PALETTE.light`.
    pub fn light() -> Self {
        Self {
            keyword: hex(0xa626a4),
            heading: hex(0xbe185d),
            callable: hex(0x4078f2),
            string: hex(0x50a14f),
            type_: hex(0xc18401),
            number: hex(0x986801),
            comment: hex(0x8a9199),
            property: hex(0xe45649),
            meta: hex(0x5c6370),
            invalid: hex(0xcf222e),
        }
    }

    pub fn for_scheme(scheme: ColorScheme) -> Self {
        match scheme {
            ColorScheme::Dark => Self::dark(),
            ColorScheme::Light => Self::light(),
        }
    }

    /// The palette color for a tree-sitter capture name, following the
    /// Lezer tag groups in `HIGHLIGHT_TAGS`. `language` decides captures that
    /// mean different tags per grammar, such as `attribute`.
    pub fn color_for_capture(&self, name: &str, language: &str) -> Option<Hsla> {
        let head = name.split('.').next().unwrap_or(name);
        match name {
            "constant.builtin" | "variable.special" | "variable.builtin" => {
                return Some(self.keyword);
            }
            "string.special.symbol" => return Some(self.keyword),
            "punctuation.special" | "punctuation.list_marker" => return Some(self.meta),
            "function.macro" => return Some(self.callable),
            _ => {}
        }
        match head {
            "keyword" | "boolean" => Some(self.keyword),
            "function" | "label" => Some(self.callable),
            "string" | "character" | "escape" => Some(self.string),
            "type" | "tag" | "enum" | "variant" | "constructor" | "namespace" | "module" => {
                Some(self.type_)
            }
            "number" | "float" => Some(self.number),
            "comment" => Some(self.comment),
            "property" => Some(self.property),
            "attribute" => {
                // Rust and C# attributes are annotations (`meta`); in markup
                // and JSX the capture is an attribute name (`property`).
                if matches!(
                    language,
                    "rust" | "csharp" | "java" | "kotlin" | "swift" | "python"
                ) {
                    Some(self.meta)
                } else {
                    Some(self.property)
                }
            }
            "preproc" | "annotation" | "decorator" => Some(self.meta),
            "title" => Some(self.heading),
            _ => None,
        }
    }
}

/// Resolves capture names to colors for one language.
#[derive(Debug, Clone)]
pub struct SyntaxResolver {
    palette: SyntaxPalette,
    language: SharedString,
}

impl SyntaxResolver {
    pub fn new(palette: SyntaxPalette, language: impl Into<SharedString>) -> Self {
        Self {
            palette,
            language: language.into(),
        }
    }
}

impl HighlightStyleResolver for SyntaxResolver {
    fn style(&self, name: &str) -> Option<gpui::HighlightStyle> {
        let color = self.palette.color_for_capture(name, &self.language)?;
        Some(gpui::HighlightStyle {
            color: Some(color),
            ..Default::default()
        })
    }
}

/// The `--color-diff-*` tokens: solid marker hues, text readable on the
/// background, and row and gutter tints. The app's diff palette setting
/// swaps them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffColors {
    pub add: Hsla,
    pub add_fg: Hsla,
    pub add_bg: Hsla,
    pub add_gutter: Hsla,
    pub del: Hsla,
    pub del_fg: Hsla,
    pub del_bg: Hsla,
    pub del_gutter: Hsla,
}

impl DiffColors {
    /// The default palette: emerald and rose, with darker text in light mode.
    pub fn default_for(scheme: ColorScheme) -> Self {
        let add = hex(0x10b981);
        let del = hex(0xf43f5e);
        let (add_fg, del_fg) = match scheme {
            ColorScheme::Dark => (hex(0x6ee7b7), hex(0xfda4af)),
            ColorScheme::Light => (hex(0x047857), hex(0xbe123c)),
        };
        Self {
            add,
            add_fg,
            add_bg: with_alpha(add, 0.15),
            add_gutter: with_alpha(add, 0.25),
            del,
            del_fg,
            del_bg: with_alpha(del, 0.15),
            del_gutter: with_alpha(del, 0.25),
        }
    }
}

/// Every color and font the editor and diff views draw with.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorTheme {
    pub scheme: ColorScheme,
    /// Behind the text. CodeMirror's editor was transparent over the pane.
    pub background: Hsla,
    /// `--color-content`.
    pub foreground: Hsla,
    /// `--color-background-base`, for floating panels.
    pub panel_background: Hsla,
    /// `--color-accent`.
    pub accent: Hsla,
    /// Line numbers: content at 38%.
    pub line_number: Hsla,
    /// The current line number: content at 70%.
    pub line_number_active: Hsla,
    /// Current line background: content at 10%.
    pub active_line: Hsla,
    /// Separators (`--color-stroke`): content at 7%.
    pub stroke: Hsla,
    /// Control borders: content at 12%.
    pub border: Hsla,
    /// Selection: accent at 35%.
    pub selection: Hsla,
    pub caret: Hsla,
    /// Secondary text: content at 45% to 62%.
    pub muted: Hsla,
    /// Hover fill: content at 10%.
    pub hover: Hsla,
    /// Error text, `#f87171`.
    pub danger: Hsla,
    /// `.cm-searchMatch`.
    pub search_match: Hsla,
    /// `.cm-searchMatch-selected`.
    pub search_match_selected: Hsla,
    /// `--color-diff-add`: the added bar in the gutter and overview ruler.
    pub git_added: Hsla,
    /// `--color-diff-del`: the deleted bar in the gutter and overview ruler.
    pub git_deleted: Hsla,
    /// `.cm-gitInsertedLine`: `--color-diff-add-bg`.
    pub inserted_line: Hsla,
    /// `.cm-gitDeletedLine`: `--color-diff-del-bg`.
    pub deleted_line: Hsla,
    /// Diff view rows: `bg-diff-add-bg` and `bg-diff-del-bg`.
    pub diff_added_row: Hsla,
    pub diff_deleted_row: Hsla,
    /// Diff view gutter tint: `bg-diff-add-gutter` and `bg-diff-del-gutter`.
    pub diff_added_gutter: Hsla,
    pub diff_deleted_gutter: Hsla,
    /// `text-diff-add-fg` and `text-diff-del-fg`: line numbers and +/-
    /// marks on changed rows, and the change counts.
    pub diff_added_number: Hsla,
    pub diff_deleted_number: Hsla,
    pub syntax: SyntaxPalette,
    pub mono_font: SharedString,
    pub ui_font: SharedString,
    /// Editor text size, 13px in CodeMirror.
    pub font_size: Pixels,
    /// Line height as a multiple of the font size, 1.6 in CodeMirror.
    pub line_height: f32,
}

impl EditorTheme {
    pub fn new(scheme: ColorScheme, background_base: Hsla, content: Hsla) -> Self {
        let accent: Hsla = gpui::hsla(211. / 360., 0.92, 0.62, 1.);
        let danger = hex(0xf87171);
        let diff = DiffColors::default_for(scheme);
        Self {
            scheme,
            background: gpui::transparent_black(),
            foreground: content,
            panel_background: background_base,
            accent,
            line_number: with_alpha(content, 0.38),
            line_number_active: with_alpha(content, 0.70),
            active_line: with_alpha(content, 0.10),
            stroke: with_alpha(content, 0.07),
            border: with_alpha(content, 0.12),
            selection: with_alpha(accent, 0.35),
            caret: content,
            muted: with_alpha(content, 0.55),
            hover: with_alpha(content, 0.10),
            danger,
            search_match: with_alpha(hex(0xe2c08d), 0.46),
            search_match_selected: with_alpha(accent, 0.52),
            git_added: diff.add,
            git_deleted: diff.del,
            inserted_line: diff.add_bg,
            deleted_line: diff.del_bg,
            diff_added_row: diff.add_bg,
            diff_deleted_row: diff.del_bg,
            diff_added_gutter: diff.add_gutter,
            diff_deleted_gutter: diff.del_gutter,
            diff_added_number: diff.add_fg,
            diff_deleted_number: diff.del_fg,
            syntax: SyntaxPalette::for_scheme(scheme),
            mono_font: "Menlo".into(),
            ui_font: ".SystemUIFont".into(),
            font_size: px(13.),
            line_height: 1.6,
        }
    }

    /// The default dark tokens: background `hsl(240 0% 9%)`, content `hsl(240 0% 92%)`.
    pub fn dark() -> Self {
        Self::new(
            ColorScheme::Dark,
            gpui::hsla(240. / 360., 0., 0.09, 1.),
            gpui::hsla(240. / 360., 0., 0.92, 1.),
        )
    }

    /// The default light tokens: background `hsl(240 0% 97%)`, content `hsl(240 0% 18%)`.
    pub fn light() -> Self {
        Self::new(
            ColorScheme::Light,
            gpui::hsla(240. / 360., 0., 0.97, 1.),
            gpui::hsla(240. / 360., 0., 0.18, 1.),
        )
    }

    /// Every diff color from the app's diff palette.
    pub fn with_diff_colors(mut self, diff: DiffColors) -> Self {
        self.git_added = diff.add;
        self.git_deleted = diff.del;
        self.inserted_line = diff.add_bg;
        self.deleted_line = diff.del_bg;
        self.diff_added_row = diff.add_bg;
        self.diff_deleted_row = diff.del_bg;
        self.diff_added_gutter = diff.add_gutter;
        self.diff_deleted_gutter = diff.del_gutter;
        self.diff_added_number = diff.add_fg;
        self.diff_deleted_number = diff.del_fg;
        self
    }

    /// Content at `alpha`, for `text-content/NN` classes.
    pub fn content(&self, alpha: f32) -> Hsla {
        with_alpha(self.foreground, alpha)
    }

    pub fn line_height_px(&self) -> Pixels {
        (self.font_size * self.line_height).round()
    }

    /// The style gpui-base paints the editor with.
    pub fn input_style(&self, language: &str) -> InputEditorStyle {
        InputEditorStyle {
            foreground: self.foreground,
            muted_foreground: self.line_number,
            background: self.background,
            border: self.stroke,
            selection: self.selection,
            caret: self.caret,
            highlight_styles: Arc::new(SyntaxResolver::new(self.syntax, language.to_owned())),
            editor_invisible: Some(self.content(0.25)),
            editor_active_line: Some(self.active_line),
            editor_gutter_background: Some(gpui::transparent_black()),
            ..Default::default()
        }
    }
}

impl Default for EditorTheme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_palette_matches_codemirror() {
        let palette = SyntaxPalette::dark();
        let resolver = SyntaxResolver::new(palette, "typescript");
        assert_eq!(
            resolver.style("keyword").unwrap().color,
            Some(hex(0xff8ffd))
        );
        assert_eq!(resolver.style("string").unwrap().color, Some(hex(0xb4fa72)));
        assert_eq!(
            resolver.style("comment").unwrap().color,
            Some(hex(0xfefdc2))
        );
        assert_eq!(
            resolver.style("function.method").unwrap().color,
            Some(hex(0xa5d5fe))
        );
        assert_eq!(resolver.style("variable"), None);
        assert_eq!(resolver.style("punctuation.bracket"), None);
    }

    #[test]
    fn attribute_depends_on_the_language() {
        let palette = SyntaxPalette::light();
        assert_eq!(
            palette.color_for_capture("attribute", "rust"),
            Some(palette.meta)
        );
        assert_eq!(
            palette.color_for_capture("attribute", "html"),
            Some(palette.property)
        );
    }
}
