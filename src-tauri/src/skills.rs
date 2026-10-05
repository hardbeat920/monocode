//! Tauri commands over `monocode_process::skills`.
use monocode_process::skills::{self, DiscoveredSkill};

/// Excludes disabled paths before deduplication so lower-priority enabled
/// same-name files can fall through.
#[tauri::command(async)]
pub fn list_skills(
    cwd: String,
    disabled_paths: Option<Vec<String>>,
) -> Result<Vec<DiscoveredSkill>, String> {
    skills::list_skills(cwd, disabled_paths)
}
