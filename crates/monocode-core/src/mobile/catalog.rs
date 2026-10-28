//! The workspace catalog a phone polls, and the session pages it browses.
use serde_json::{json, Value};
use std::collections::HashMap;

/// How many of a project's sessions the workspace poll carries; the sessions
/// sheet pages through the rest with `sessions.list`.
const RECENT_SESSIONS: usize = 20;
const SESSION_PAGE_MAX: usize = 100;

/// The workspace poll's sessions: each project's most recent, every busy one,
/// and every Mono's. `sessions` is newest first, as the catalog is sorted.
pub fn recent_sessions(sessions: &Value) -> Vec<Value> {
    let mut per_project: HashMap<&str, usize> = HashMap::new();
    sessions
        .as_array()
        .into_iter()
        .flatten()
        .filter(|session| {
            let count = per_project
                .entry(session["projectId"].as_str().unwrap_or(""))
                .or_default();
            *count += 1;
            *count <= RECENT_SESSIONS || session["busy"] == true || session["monoId"].is_string()
        })
        .cloned()
        .collect()
}

/// One page of a project's sessions, newest first, optionally matching a query
/// against title, model and branch.
pub fn session_page(sessions: &Value, params: &Value) -> Value {
    let project = params["projectId"].as_str().unwrap_or("");
    let query = params["query"].as_str().unwrap_or("").trim().to_lowercase();
    let offset = params["offset"].as_u64().unwrap_or(0) as usize;
    let limit = (params["limit"].as_u64().unwrap_or(30) as usize).clamp(1, SESSION_PAGE_MAX);
    // The Sessions screen's filters: active since a time, and providers left out.
    let since = params["since"].as_f64().unwrap_or(0.0);
    let hidden: Vec<&str> = params["hiddenHarnesses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let matches: Vec<&Value> = sessions
        .as_array()
        .into_iter()
        .flatten()
        .filter(|session| session["projectId"] == project)
        .filter(|session| since <= 0.0 || session["updatedAt"].as_f64().unwrap_or(0.0) >= since)
        .filter(|session| {
            !session["harness"]
                .as_str()
                .is_some_and(|harness| hidden.contains(&harness))
        })
        .filter(|session| {
            query.is_empty()
                || ["title", "model", "branch"].iter().any(|field| {
                    session[*field]
                        .as_str()
                        .is_some_and(|value| value.to_lowercase().contains(&query))
                })
        })
        .collect();
    let page: Vec<&Value> = matches.iter().skip(offset).take(limit).copied().collect();
    json!({
        "sessions": page,
        "hasMore": offset + page.len() < matches.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_poll_carries_recent_sessions_and_pages_serve_the_rest() {
        let sessions: Vec<Value> = (0..30)
            .map(|i| json!({"id":format!("s{i}"),"projectId":"p","title":format!("Task {i}"),"busy":i == 25}))
            .chain([json!({"id":"m","projectId":"p","monoId":"mono"})])
            .collect();
        let sessions = json!(sessions);
        let recent = recent_sessions(&sessions);
        assert_eq!(recent.len(), RECENT_SESSIONS + 2);
        assert!(recent.iter().any(|s| s["id"] == "s25"));

        let page = session_page(&sessions, &json!({"projectId":"p","offset":28,"limit":10}));
        assert_eq!(page["sessions"].as_array().unwrap().len(), 3);
        assert_eq!(page["hasMore"], false);
        let page = session_page(&sessions, &json!({"projectId":"p","limit":2}));
        assert_eq!(page["hasMore"], true);
        let found = session_page(&sessions, &json!({"projectId":"p","query":"task 2"}));
        assert_eq!(found["sessions"].as_array().unwrap().len(), 11);
        let filtered: Vec<Value> = [
            json!({"id":"a","projectId":"p","harness":"claude","updatedAt":200}),
            json!({"id":"b","projectId":"p","harness":"codex","updatedAt":300}),
            json!({"id":"c","projectId":"p","harness":"claude","updatedAt":100}),
        ]
        .into();
        let filtered = json!(filtered);
        let ids = |params: Value| -> Vec<String> {
            session_page(&filtered, &params)["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["id"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(ids(json!({"projectId":"p","since":150})), ["a", "b"]);
        assert_eq!(
            ids(json!({"projectId":"p","hiddenHarnesses":["codex"]})),
            ["a", "c"]
        );
        assert!(
            session_page(&sessions, &json!({"projectId":"q"}))["sessions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
