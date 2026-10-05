//! Tauri commands over `monocode_git::search`.
use monocode_git::search::{self, SearchOptions, SearchResult};
use monocode_platform::expand_home;

#[tauri::command]
pub fn cancel_project_search(cwd: String, search_id: String) {
    search::cancel_project_search(cwd, search_id)
}

#[tauri::command]
pub async fn search_project(options: SearchOptions) -> Result<SearchResult, String> {
    if options.query.trim().is_empty() {
        return Ok(SearchResult {
            matches: Vec::new(),
            truncated: false,
        });
    }
    let root = expand_home(&options.cwd);
    if !root.is_dir() {
        return Err(format!("{}: Not a directory", root.display()));
    }
    let search_id = options.search_id.clone();
    let token = search::begin_search(&root, &search_id);
    let result = match tauri::async_runtime::spawn_blocking({
        let root = root.clone();
        let token = token.clone();
        move || search::search_project_sync(&root, &options, &token)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => {
            search::finish_search(&root, &search_id, &token);
            return Err(error.to_string());
        }
    };
    search::finish_search(&root, &search_id, &token);
    result
}
