use super::*;
use crate::{
    store::Store,
    transcript::{Row, Transcript},
};
use std::sync::Arc;

fn row(role: &str, text: &str, first: bool) -> Row {
    Row {
        role: role.into(),
        text: text.into(),
        first,
        sent_at: None,
    }
}

#[test]
fn desktop_title_prefixes_are_hidden_before_search_highlighting() {
    let store = Store::open_monocode(std::path::Path::new(":memory:")).unwrap();
    let mut session = store.create("codex", "/project").unwrap();
    session.title = "codex · Café repair".into();
    let hits = titles(&[session.clone()], "café");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "Café repair");
    assert!(hits[0]
        .highlights
        .iter()
        .all(|range| hits[0].title.get(range.clone()).is_some()));
    assert_eq!(hits[0].highlights[0].start, 0);
    let hit = message_hit(&session, 0, "user", "needle", "needle", false).unwrap();
    assert_eq!(hit.meta, "Café repair");
    assert_eq!(session.title, "codex · Café repair");
}

#[test]
fn saved_search_rejoins_messages_is_literal_and_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sessions.db");
    let mut store = Store::open(&path).unwrap();
    let session = store.create("codex", "/other/project").unwrap();
    store
        .write_rows(
            session.id,
            0,
            &[
                row("user", "find the café and ", true),
                row("user", "100%_literal", false),
                row("reasoning", "private reasoning 100%_literal", true),
                row("assistant", "nothing matching", true),
            ],
        )
        .unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let (hits, truncated) = store
        .search_sessions("CAFÉ and 100%_literal", cancel.clone())
        .unwrap();
    assert!(!truncated);
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].target,
        Target::Message {
            session: session.id,
            row: 0
        }
    );
    assert!(hits[0].title.contains("café and 100%_literal"));
    assert!(store
        .search_sessions("100Xliteral", cancel.clone())
        .unwrap()
        .0
        .is_empty());
    store.set_archived(session.id, true).unwrap();
    assert!(store
        .search_sessions("100%_literal", cancel)
        .unwrap()
        .0
        .is_empty());
}

#[test]
fn cancelled_session_search_does_not_interrupt_later_store_operations() {
    let mut store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let s = store.create("claude", "/a").unwrap();
    store
        .write_rows(s.id, 0, &[row("user", "needle", true)])
        .unwrap();
    assert!(store
        .search_sessions("needle", Arc::new(AtomicBool::new(true)))
        .unwrap()
        .0
        .is_empty());
    store.title(s.id, "renamed").unwrap();
    assert_eq!(
        store
            .search_sessions("needle", Arc::new(AtomicBool::new(false)))
            .unwrap()
            .0
            .len(),
        1
    );
}

#[test]
fn live_search_finds_split_phrases_tools_and_skips_reasoning() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let s = store.create("codex", "/a").unwrap();
    let mut transcript = Transcript::default();
    transcript.begin("assistant");
    transcript.append(&format!(
        "{}café needle",
        "a".repeat(crate::transcript::ROW_BYTES - 3)
    ));
    transcript.begin("reasoning");
    transcript.append("café needle hidden");
    transcript.begin("tool");
    transcript.append(
        &serde_json::to_string(&crate::tool::ToolCall {
            id: "x".into(),
            name: "Read".into(),
            input: "{\"path\":\"café needle.rs\"}".into(),
            output: "tool output".into(),
            state: crate::tool::ToolState::Completed,
            panel: None,
            display: None,
        })
        .unwrap(),
    );
    let hits = messages(&s, &transcript.rows, "café needle", &AtomicBool::new(false));
    assert_eq!(hits.len(), 2);
    assert!(hits[0].title.contains("café needle"));
    assert!(hits[1].title.contains("café needle.rs"));
}

#[test]
fn grouping_matches_react_limits_order_and_live_deduplication() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let mut s = store.create("codex", "/a").unwrap();
    s.title = "Search parity".into();
    let mut hits = titles(&[s.clone()], "search");
    for i in 0..30 {
        hits.push(message_hit(&s, i, "assistant", "search here", "search", false).unwrap());
        hits.push(message_hit(&s, i, "assistant", "search live", "search", true).unwrap());
    }
    let all = grouped(hits.clone(), Scope::All);
    assert_eq!(all.len(), 9);
    assert!(matches!(all[0].target, Target::Conversation(_)));
    assert!(all[1..].iter().all(|h| h.title == "search live"));
    assert_eq!(grouped(hits.clone(), Scope::Conversations).len(), 25);
    assert!(grouped(hits, Scope::Files).is_empty());
}

#[test]
fn fuzzy_tokens_filename_priority_and_utf8_ranges_match_react() {
    assert!(fuzzy("İ", "İstanbul").is_some());
    assert!(fuzzy("side srch", "Fix sidebar search").is_some());
    let (_, ranges) = fuzzy("café", "Café search").unwrap();
    assert_eq!(&"Café search"[ranges[3].clone()], "é");
    let files = vec![
        ProjectFile {
            name: "app.rs".into(),
            path: "/a/search/app.rs".into(),
            relative: "search/app.rs".into(),
        },
        ProjectFile {
            name: "search.rs".into(),
            path: "/a/search.rs".into(),
            relative: "search.rs".into(),
        },
    ];
    assert_eq!(filenames(&files, "search", &[])[0].title, "search.rs");
    let preview = snippet(
        &format!("{}café needle{}", "🙂 ".repeat(90), " end".repeat(90)),
        "CAFÉ needle",
    );
    assert!(preview.starts_with('…') && preview.ends_with('…'));
    assert!(preview.contains("café needle"));
}

#[test]
fn unopened_titles_are_searchable_and_the_reader_sees_new_writes() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("search.db")).unwrap();
    let session = store.create("codex", "/another-project").unwrap();
    store
        .title(session.id, "Unopened needle conversation")
        .unwrap();
    store
        .write_rows(session.id, 0, &[row("user", "unrelated body", true)])
        .unwrap();
    let reader = store.search_reader().unwrap().unwrap();
    let flag = Arc::new(AtomicBool::new(false));
    let hits = reader.search_sessions("needle", flag.clone()).unwrap().0;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].target, Target::Conversation(session.id));
    store
        .write_rows(
            session.id,
            1,
            &[row("assistant", "new needle message", true)],
        )
        .unwrap();
    let hits = reader.search_sessions("needle", flag).unwrap().0;
    assert!(hits.iter().any(|h| h.target
        == Target::Message {
            session: session.id,
            row: 1
        }));
}
