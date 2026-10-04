//! Code editor and diff views for MonoCode, built on gpui-base's editor.
//!
//! The pure modules ([`git_diff`], [`search`], [`unified_diff`], [`doc`],
//! [`language`]) hold the ported logic and have no GPUI state. The views
//! ([`CodeEditor`], [`DiffView`], [`ImageView`], [`PdfView`]) take file
//! contents, the git base text, and git actions as parameters and callbacks,
//! so this crate does not depend on the git or theme crates.
//!
//! Call [`init`] once after `gpui_component::init`.

pub mod code_editor;
pub mod diff_view;
pub mod doc;
mod find_bar;
pub mod git_diff;
mod git_gutter;
pub mod highlighter;
mod icons;
pub mod image_view;
pub mod language;
pub mod pdf_view;
pub mod search;
pub mod theme;
pub mod unified_diff;
pub mod viewer;

pub use code_editor::{
    CodeEditor, CodeEditorEvent, CommentHandler, Formatter, RevertHandler, SaveHandler,
    SaveRequest, SaveState, StageHandler,
};
pub use diff_view::{
    DiffAction, DiffFile, DiffFileActions, DiffView, HunkActionRequest, InitialExpansion,
    parse_diff,
};
pub use image_view::ImageView;
pub use pdf_view::PdfView;
pub use theme::{ColorScheme, DiffColors, EditorTheme, SyntaxPalette};

/// Bind the editor keys.
pub fn init(cx: &mut gpui::App) {
    code_editor::init(cx);
}
