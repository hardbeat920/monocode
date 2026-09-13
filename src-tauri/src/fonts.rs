use std::collections::BTreeMap;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SystemFont {
    pub family: String,
    pub monospace: bool,
}

fn collect_system_fonts_sync() -> Vec<SystemFont> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    // Family -> monospace. A family counts as monospace if any of its faces
    // is marked monospaced (covers regular/bold/italic splits).
    let mut by_family: BTreeMap<String, bool> = BTreeMap::new();
    // Case-insensitive dedupe while preserving the first-seen display name.
    let mut seen_lower: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for face in db.faces() {
        for (name, _) in face.families.iter() {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            let lower = trimmed.to_lowercase();
            let display = seen_lower
                .entry(lower.clone())
                .or_insert_with(|| trimmed.to_string())
                .clone();
            let entry = by_family.entry(display).or_insert(false);
            if face.monospaced {
                *entry = true;
            }
        }
    }

    let mut out: Vec<SystemFont> = by_family
        .into_iter()
        .map(|(family, monospace)| SystemFont { family, monospace })
        .collect();
    out.sort_by(|a, b| {
        a.family
            .to_lowercase()
            .cmp(&b.family.to_lowercase())
            .then_with(|| a.family.cmp(&b.family))
    });
    out
}

#[tauri::command]
pub async fn list_system_fonts() -> Result<Vec<SystemFont>, String> {
    tauri::async_runtime::spawn_blocking(collect_system_fonts_sync)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupes_families_case_insensitively_and_sorts() {
        // Simulate the merge logic without touching the real system fonts.
        let mut by_family: BTreeMap<String, bool> = BTreeMap::new();
        let mut seen_lower: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (name, mono) in [
            ("JetBrains Mono", true),
            ("jetbrains mono", true),
            ("Inter", false),
            ("  Inter  ", false),
            ("", false),
        ] {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            let lower = trimmed.to_lowercase();
            let display = seen_lower
                .entry(lower)
                .or_insert_with(|| trimmed.to_string())
                .clone();
            let entry = by_family.entry(display).or_insert(false);
            if mono {
                *entry = true;
            }
        }
        let mut out: Vec<SystemFont> = by_family
            .into_iter()
            .map(|(family, monospace)| SystemFont { family, monospace })
            .collect();
        out.sort_by(|a, b| {
            a.family
                .to_lowercase()
                .cmp(&b.family.to_lowercase())
                .then_with(|| a.family.cmp(&b.family))
        });
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].family, "Inter");
        assert!(!out[0].monospace);
        assert_eq!(out[1].family, "JetBrains Mono");
        assert!(out[1].monospace);
    }

    #[test]
    fn system_scan_returns_sorted_unique_families() {
        let fonts = collect_system_fonts_sync();
        // An empty result is valid (minimal containers may have no fonts);
        // uniqueness and ordering must hold regardless.
        let mut last = String::new();
        let mut seen = std::collections::HashSet::new();
        for font in &fonts {
            assert!(!font.family.trim().is_empty());
            assert!(seen.insert(font.family.to_lowercase()), "duplicate family");
            assert!(
                font.family.to_lowercase() >= last,
                "fonts not sorted: {} after {}",
                font.family,
                last
            );
            last = font.family.to_lowercase();
        }
    }
}
