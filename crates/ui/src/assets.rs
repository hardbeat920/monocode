//! The GPUI asset source for MonoCode's icons, provider logos, and file-type
//! icons. Everything under `crates/ui/assets` is embedded in the binary and
//! served under the `monocode/` prefix, so the paths cannot collide with
//! gpui-component's own `icons/` set, which this source also serves.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use rust_embed::RustEmbed;

/// Prefix for every MonoCode asset path.
pub const PREFIX: &str = "monocode/";

#[derive(RustEmbed)]
#[folder = "assets"]
#[exclude = "file-icons/mapping.json"]
struct Embedded;

/// Serves MonoCode assets, then gpui-component's bundled icons.
/// Pass it to `Application::with_assets`.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(rest) = path.strip_prefix(PREFIX) {
            return Ok(Embedded::get(rest).map(|file| file.data));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut out: Vec<SharedString> = Embedded::iter()
            .map(|name| format!("{PREFIX}{name}"))
            .filter(|name| name.starts_with(path))
            .map(SharedString::from)
            .collect();
        out.extend(gpui_component_assets::Assets.list(path)?);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_prefixed_assets_and_falls_back() {
        let assets = Assets;
        assert!(assets.load("monocode/icons/plus.svg").unwrap().is_some());
        assert!(
            assets
                .load("monocode/providers/claude.svg")
                .unwrap()
                .is_some()
        );
        assert!(
            assets
                .load("monocode/file-icons/typescript.svg")
                .unwrap()
                .is_some()
        );
        assert!(assets.load("monocode/icons/nope.svg").unwrap().is_none());
        assert!(!assets.list("monocode/icons/").unwrap().is_empty());
    }
}
