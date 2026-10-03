//! File views: the explorer tree, quick open with the `>` command palette,
//! the file pane and its editor and preview surfaces, the find bar over
//! rendered previews, and the explorer menu. Port of src/features/files/ui.
//!
//! The views read and change files through [`FilesData`]. The engine's
//! workspace package implements it over its `Files` global; [`LocalFiles`]
//! is a standalone implementation on the local disk.
//!
//! Call [`init`] once after `monocode_ui::init` and `monocode_editor::init`.

pub mod binary_view;
pub mod data;
pub mod explorer_menu;
pub mod file_action_error;
pub mod file_editor;
pub mod file_name;
pub mod file_pane;
pub mod file_picker;
pub mod file_preview;
pub mod file_tree;
pub mod fuzzy;
pub mod markdown_shell;
pub mod match_text;
pub mod paths;
pub mod plan_surface;
pub mod preview_search;

#[cfg(test)]
pub(crate) mod test_support;

pub use binary_view::BinaryFileSurface;
pub use data::{
    DataTask, EditorNavigation, ExplorerCache, FileOpenOptions, FilesData, FsEntry, GitStatusMap,
    Listener, LocalFiles, ProjectFile, RankedFile,
};
pub use explorer_menu::{
    ExplorerMenu, ExplorerMenuEvent, ExplorerMenuItem, MenuAction, MenuAnchor,
};
pub use file_action_error::{FileActionError, file_action_error};
pub use file_editor::{EditorCodeSelection, EditorSettings, FileEditorEvent, FileEditorSurface};
pub use file_pane::{
    ExternalSurface, FilePane, FilePaneEvent, SurfaceFactory, SurfaceRequest, TabStrip,
};
pub use file_picker::{FilePicker, FilePickerEvent};
pub use file_preview::{FilePreview, PreviewStatus, PreviewVariant, file_preview};
pub use file_tree::{DraggedExplorerFile, FileTree, FileTreeEvent, TreeRow};
pub use plan_surface::PlanSurface;
pub use preview_search::FilePreviewSearch;

/// Bind the keys of the explorer, its menu, the file picker, and the
/// preview find bar.
pub fn init(cx: &mut gpui::App) {
    explorer_menu::init(cx);
    file_tree::init(cx);
    preview_search::init(cx);
}

/// The editor palette for the current theme, so the editor and previews
/// follow the app's colors and fonts.
pub fn editor_theme(cx: &gpui::App) -> monocode_editor::EditorTheme {
    let theme = monocode_ui::Theme::of(cx);
    let scheme = if theme.is_dark() {
        monocode_editor::ColorScheme::Dark
    } else {
        monocode_editor::ColorScheme::Light
    };
    let mut editor = monocode_editor::EditorTheme::new(
        scheme,
        theme.colors.background_base,
        theme.colors.content,
    );
    editor.accent = theme.colors.accent;
    editor.mono_font = theme.fonts.mono.clone();
    editor.ui_font = theme.fonts.sans.clone();
    editor
}
