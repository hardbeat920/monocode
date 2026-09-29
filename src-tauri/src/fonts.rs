/// Scan off the UI thread and return family names, not one entry per weight.
#[tauri::command]
pub async fn list_font_families() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut database = fontdb::Database::new();
        database.load_system_fonts();
        let mut families: Vec<String> = database
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .filter(|name| !name.is_empty() && !name.starts_with('.'))
            .collect();
        families.sort_unstable();
        families.dedup();
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
        assert!(!families.is_empty());
        assert!(families.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(families
            .iter()
            .all(|name| !name.is_empty() && !name.starts_with('.')));
        #[cfg(target_os = "macos")]
        assert!(families.iter().any(|name| name == "Menlo"));
        println!("Discovered {} installed font families", families.len());
    }
}
