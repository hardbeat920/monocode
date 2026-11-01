//! FileTypeIcon.tsx's Material Icon Theme lookup, from the same
//! react-material-icon-theme 1.2.0 tables (scripts/generate-material-icons.mjs).
use gpui::{AnyElement, img, prelude::*, px};
use serde::Deserialize;
use std::{collections::HashMap, sync::OnceLock};

#[derive(Deserialize)]
struct Table {
    files: HashMap<String, String>,
    exts: HashMap<String, String>,
    folders: HashMap<String, String>,
    svgs: HashMap<String, String>,
}
fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/material-icons.json"))
            .expect("bundled icon table")
    })
}
/// The package matches the whole lowercased name, then each compound suffix
/// (`d.ts`, then `ts`).
pub(crate) fn file_icon_name(name: &str) -> &'static str {
    let table = table();
    let key = name.to_lowercase();
    if let Some(icon) = table.files.get(&key) {
        return icon;
    }
    let parts: Vec<&str> = key.split('.').collect();
    let start = usize::from(parts.first() == Some(&""));
    for i in start + 1..parts.len() {
        if let Some(icon) = table.exts.get(&parts[i..].join(".")) {
            return icon;
        }
    }
    "file"
}
/// `getFolderIcon` with the specific theme, never as a root.
pub(crate) fn folder_icon_name(name: &str, open: bool) -> String {
    let base = table().folders.get(name).map_or("folder", String::as_str);
    if open {
        format!("{base}-open")
    } else {
        base.to_owned()
    }
}
/// `mi/<icon>.svg` asset bytes.
pub(crate) fn svg(asset: &str) -> Option<&'static [u8]> {
    let name = asset.strip_prefix("mi/")?.strip_suffix(".svg")?;
    table().svgs.get(name).map(|svg| svg.as_bytes())
}
/// Colored, so drawn as an image rather than a monochrome SVG mask.
pub(crate) fn file_type_icon(name: &str, is_dir: bool, open: bool, side: f32) -> AnyElement {
    let icon = if is_dir {
        folder_icon_name(name, open)
    } else {
        file_icon_name(name).to_owned()
    };
    img(format!("mi/{icon}.svg"))
        .size(px(side))
        .flex_shrink_0()
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_then_compound_extensions_then_folders() {
        assert_eq!(
            file_icon_name("Package.JSON"),
            table().files["package.json"]
        );
        assert_eq!(file_icon_name("Cargo.toml"), "toml");
        assert_eq!(file_icon_name("main.rs"), "rust");
        assert_eq!(file_icon_name("index.d.ts"), table().exts["d.ts"]);
        assert_eq!(file_icon_name("no-extension"), "file");
        assert_eq!(folder_icon_name("src", false), "folder-src");
        assert_eq!(folder_icon_name("src", true), "folder-src-open");
        assert_eq!(folder_icon_name("zzz-unknown", true), "folder-open");
        for icon in ["rust", "file", "folder", "folder-open", "folder-src-open"] {
            assert!(svg(&format!("mi/{icon}.svg")).is_some(), "{icon}");
        }
    }
}
