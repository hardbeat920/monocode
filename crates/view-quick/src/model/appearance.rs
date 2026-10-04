//! The data half of src/features/quick-composer/ui/QuickProjectIcon.tsx:
//! a project's saved logo, else its mascot in the project color.

use monocode_layout::paths::{project_key, project_name};
use monocode_layout::tab_groups::{
    JsRecord, resolve_tab_group_color, resolve_tab_group_logo, resolve_tab_group_mascot,
};

/// `loadQuickProjectAppearance`: the tab group logos, mascots, palette
/// colors, and custom colors, keyed by project key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectAppearance {
    pub logos: JsRecord<String>,
    pub mascots: JsRecord<String>,
    pub colors: JsRecord<usize>,
    pub custom_colors: JsRecord<String>,
}

/// What a project icon draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectIcon {
    /// A saved logo file. The view falls back to `mascot` if it fails to
    /// load.
    Logo {
        path: String,
        mascot: MascotIcon,
    },
    Mascot(MascotIcon),
}

/// A mascot: the hash seed, the saved mascot name, and the color as hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MascotIcon {
    pub seed: String,
    pub name: Option<String>,
    pub color: String,
}

impl ProjectIcon {
    pub fn mascot(&self) -> &MascotIcon {
        match self {
            ProjectIcon::Logo { mascot, .. } | ProjectIcon::Mascot(mascot) => mascot,
        }
    }
}

/// Project paths identify appearance settings; only saved logo files are
/// images, so a project directory is never loaded as one.
pub fn project_icon(project_path: &str, appearance: &ProjectAppearance) -> ProjectIcon {
    let key = project_key(project_path);
    let seed = project_name(project_path);
    let mascot = MascotIcon {
        name: resolve_tab_group_mascot(&key, Some(&appearance.mascots)),
        color: hex_color(&resolve_tab_group_color(
            &key,
            Some(&appearance.colors),
            Some(&appearance.custom_colors),
            Some(&seed),
        )),
        seed,
    };
    match resolve_tab_group_logo(&key, Some(&appearance.logos)).filter(|path| !path.is_empty()) {
        Some(path) => ProjectIcon::Logo { path, mascot },
        None => ProjectIcon::Mascot(mascot),
    }
}

/// Converts the project's palette color to the hex format native icons draw.
pub fn hex_color(value: &str) -> String {
    let Some(hsl) = value.strip_prefix("hsl(").and_then(|v| v.strip_suffix(')')) else {
        return value.to_string();
    };
    let parts: Vec<f64> = hsl
        .split_whitespace()
        .filter_map(|v| v.trim_end_matches('%').parse().ok())
        .collect();
    if parts.len() != 3 {
        return value.to_string();
    }
    let rgb = monocode_ui::color::hsl_to_rgb(parts[0], parts[1], parts[2]);
    format!("#{:02x}{:02x}{:02x}", rgb.r, rgb.g, rgb.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &str = "/Users/me/code/agent-terminal";

    #[test]
    fn renders_a_mascot_instead_of_trying_to_load_a_project_directory_as_an_image() {
        let icon = project_icon(PATH, &ProjectAppearance::default());
        let ProjectIcon::Mascot(mascot) = icon else {
            panic!("expected a mascot");
        };
        assert_eq!(mascot.seed, "agent-terminal");
        assert_eq!(mascot.name, None);
        assert!(mascot.color.starts_with('#'));
    }

    #[test]
    fn uses_the_projects_saved_mascot_and_color() {
        let mut appearance = ProjectAppearance::default();
        appearance
            .mascots
            .insert(project_key(PATH), "cat".to_string());
        appearance
            .custom_colors
            .insert(project_key(PATH), "#ff0000".to_string());
        let icon = project_icon(PATH, &appearance);
        assert_eq!(icon.mascot().name.as_deref(), Some("cat"));
        assert_eq!(icon.mascot().color, "#ff0000");
    }

    #[test]
    fn loads_only_saved_logos() {
        let mut appearance = ProjectAppearance::default();
        appearance
            .logos
            .insert(project_key(PATH), "/app-data/logos/project.png".to_string());
        let ProjectIcon::Logo { path, .. } = project_icon(PATH, &appearance) else {
            panic!("expected a logo");
        };
        assert_eq!(path, "/app-data/logos/project.png");
    }
}
