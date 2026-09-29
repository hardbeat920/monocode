#[derive(serde::Serialize, Default)]
pub struct FontFamilies {
    all: Vec<String>,
    monospaced: Vec<String>,
}

/// Scan off the UI thread and return family names, not one entry per weight.
#[tauri::command]
pub async fn list_font_families() -> Result<FontFamilies, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut database = fontdb::Database::new();
        database.load_system_fonts();
        let mut families = FontFamilies::default();
        for face in database.faces() {
            for (name, _) in &face.families {
                if name.is_empty() || name.starts_with('.') {
                    continue;
                }
                families.all.push(name.clone());
                if face.monospaced {
                    families.monospaced.push(name.clone());
                }
            }
        }
        for names in [&mut families.all, &mut families.monospaced] {
            names.sort_unstable();
            names.dedup();
        }
        families
    })
    .await
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires a desktop with installed fonts"]
    fn discovers_installed_families_without_duplicate_weights() {
        let families = tauri::async_runtime::block_on(super::list_font_families()).unwrap();
        assert!(!families.all.is_empty());
        assert!(families.all.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(families
            .all
            .iter()
            .all(|name| !name.is_empty() && !name.starts_with('.')));
        #[cfg(target_os = "macos")]
        {
            assert!(families.monospaced.iter().any(|name| name == "Menlo"));
            assert!(families.all.iter().any(|name| name == "Georgia"));
            assert!(!families.monospaced.iter().any(|name| name == "Georgia"));
        }
        assert!(families.monospaced.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(families
            .monospaced
            .iter()
            .all(|name| families.all.contains(name)));
        println!("Discovered {} installed font families", families.all.len());
    }
}
