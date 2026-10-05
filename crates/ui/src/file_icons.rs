//! Port of src/features/files/ui/FileTypeIcon.tsx and the matcher it calls in
//! react-material-icon-theme (`getFileIcon`, `getFolderIcon`).
//!
//! `assets/file-icons/mapping.json` holds the package's tables as FileTypeIcon
//! sees them (`iconPack: ""`, so pack-specific icons are left out). File names
//! and extensions map to the first icon that lists them, which is what the
//! package's `Array.find` returns.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use gpui::{App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, img};
use serde::Deserialize;

use crate::u;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Mapping {
    file_names: HashMap<String, String>,
    file_extensions: HashMap<String, String>,
    folders: Vec<FolderRule>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderRule {
    name: String,
    folder_names: Vec<String>,
    root_folder_names: Option<Vec<String>>,
}

fn mapping() -> &'static Mapping {
    static MAPPING: OnceLock<Mapping> = OnceLock::new();
    MAPPING.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/file-icons/mapping.json"))
            .expect("file-icons/mapping.json is valid")
    })
}

/// `compoundExtensions`: `a.d.ts` gives `d.ts`, then `ts`. A leading dot is
/// part of the name, so `.env.local` gives only `local`.
fn compound_extensions(file_name: &str) -> Vec<String> {
    let parts: Vec<&str> = file_name.split('.').collect();
    let start = if parts.first() == Some(&"") { 1 } else { 0 };
    (start + 1..parts.len())
        .map(|i| parts[i..].join("."))
        .collect()
}

/// `resolveFileIcon`: the full lowercase name, then each compound suffix,
/// then the generic `file` icon.
pub fn file_icon_name(file_name: &str) -> &'static str {
    let map = mapping();
    let key = file_name.to_lowercase();
    if let Some(name) = map.file_names.get(&key) {
        return name;
    }
    for ext in compound_extensions(&key) {
        // `findFileIconByExtension` strips one leading dot.
        let clean = ext.strip_prefix('.').unwrap_or(&ext);
        if let Some(name) = map.file_extensions.get(clean) {
            return name;
        }
    }
    "file"
}

/// The prefix and suffix variants `findFolderIcon` also accepts.
fn folder_variant_matches(name: &str, folder_name: &str) -> bool {
    name == folder_name
        || name == format!(".{folder_name}")
        || name == format!("_{folder_name}")
        || name == format!("-{folder_name}")
        || name == format!("__{folder_name}__")
        || folder_name == format!(".{name}")
        || folder_name == format!("_{name}")
        || folder_name == format!("-{name}")
        || folder_name == format!("__{name}__")
}

fn find_folder_icon(folder_name: &str, is_root: bool) -> Option<&'static FolderRule> {
    mapping().folders.iter().find(|icon| {
        if is_root && let Some(root_names) = &icon.root_folder_names {
            // A root rule never falls through to the plain folder names.
            return root_names
                .iter()
                .any(|name| folder_variant_matches(name, folder_name));
        }
        icon.folder_names.iter().any(|name| name == folder_name)
            || icon
                .folder_names
                .iter()
                .any(|name| folder_variant_matches(name, folder_name))
    })
}

/// `getFolderIcon` with the "specific" theme.
pub fn folder_icon_name(folder_name: &str, is_open: bool, is_root: bool) -> String {
    if !folder_name.is_empty()
        && let Some(icon) = find_folder_icon(folder_name, is_root)
    {
        return if is_open {
            format!("{}-open", icon.name)
        } else {
            icon.name.clone()
        };
    }
    let base = if is_root { "folder-root" } else { "folder" };
    if is_open {
        format!("{base}-open")
    } else {
        base.to_string()
    }
}

/// The asset path for a material icon name, or `None` when the package has
/// no SVG for it (FileTypeIcon then renders an empty box).
pub fn file_icon_path(icon_name: &str) -> Option<SharedString> {
    static AVAILABLE: OnceLock<HashSet<SharedString>> = OnceLock::new();
    let available = AVAILABLE.get_or_init(|| {
        use gpui::AssetSource;
        let prefix = format!("{}file-icons/", crate::assets::PREFIX);
        crate::Assets
            .list(&prefix)
            .unwrap_or_default()
            .into_iter()
            .collect()
    });
    let path = SharedString::from(format!(
        "{}file-icons/{icon_name}.svg",
        crate::assets::PREFIX
    ));
    available.contains(&path).then_some(path)
}

/// `FileTypeIcon`: a file or folder icon matched by name, 16px by default.
#[derive(IntoElement)]
pub struct FileTypeIcon {
    name: SharedString,
    is_dir: bool,
    is_open: bool,
    is_root: bool,
    size: f32,
}

/// An icon for a file name.
pub fn file_type_icon(name: impl Into<SharedString>) -> FileTypeIcon {
    FileTypeIcon {
        name: name.into(),
        is_dir: false,
        is_open: false,
        is_root: false,
        size: 16.,
    }
}

/// An icon for a folder name.
pub fn folder_type_icon(
    name: impl Into<SharedString>,
    is_open: bool,
    is_root: bool,
) -> FileTypeIcon {
    FileTypeIcon {
        name: name.into(),
        is_dir: true,
        is_open,
        is_root,
        size: 16.,
    }
}

impl FileTypeIcon {
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for FileTypeIcon {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let icon_name = if self.is_dir {
            folder_icon_name(&self.name, self.is_open, self.is_root)
        } else {
            file_icon_name(&self.name).to_string()
        };
        let frame = div().flex_none().size(u(self.size));
        match file_icon_path(&icon_name) {
            Some(path) => frame.child(img(path).size_full()),
            None => frame,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values come from running FileTypeIcon's resolver against
    // react-material-icon-theme 1.2.0.
    #[test]
    fn files_resolve_like_the_typescript_package() {
        for (name, expected) in [
            ("package.json", "nodejs"),
            ("Cargo.toml", "toml"),
            ("README.md", "readme"),
            ("gridArcade.ts", "typescript"),
            ("gridArcade.test.ts", "test-ts"),
            ("types.d.ts", "typescript-def"),
            ("HarnessIcon.tsx", "react_ts"),
            (".gitignore", "git"),
            (".env.local", "tune"),
            ("main.rs", "rust"),
            ("Makefile", "makefile"),
            ("screenshot.jpg", "image"),
            ("noext", "file"),
            ("archive.tar.gz", "zip"),
            ("weird.", "file"),
            ("Dockerfile", "docker"),
            ("index.css", "css"),
        ] {
            assert_eq!(file_icon_name(name), expected, "{name}");
        }
    }

    #[test]
    fn folders_resolve_like_the_typescript_package() {
        for (name, open, root, expected) in [
            ("src", false, false, "folder-src"),
            ("src", true, false, "folder-src-open"),
            ("node_modules", false, false, "folder-node"),
            (".github", false, false, "folder-github"),
            ("_tests", false, false, "folder-test"),
            ("randomdir", false, false, "folder"),
            ("randomdir", true, false, "folder-open"),
            ("monocode", false, true, "folder-root"),
            ("src", false, true, "folder-src"),
            ("", false, false, "folder"),
            ("docs", true, false, "folder-docs-open"),
        ] {
            assert_eq!(folder_icon_name(name, open, root), expected, "{name}");
        }
    }

    #[test]
    fn compound_extensions_skip_a_leading_dot() {
        assert_eq!(compound_extensions("a.d.ts"), vec!["d.ts", "ts"]);
        assert_eq!(compound_extensions(".env.local"), vec!["local"]);
        assert!(compound_extensions("noext").is_empty());
    }

    #[test]
    fn resolved_icons_have_svgs() {
        assert!(file_icon_path(file_icon_name("main.rs")).is_some());
        assert!(file_icon_path(&folder_icon_name("src", true, false)).is_some());
        assert!(file_icon_path("folder-deprecated").is_none());
    }
}
