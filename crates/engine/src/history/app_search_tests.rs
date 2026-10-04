//! Port of src/features/search/model/appSearch.test.ts.

use super::*;
use monocode_core::harness::RuntimeMode;

const NOW: i64 = 1_800_000_000_000;

fn summary(id: &str, cwd: &str, title: &str, updated_at: i64) -> SessionSummary {
    SessionSummary {
        model: "gpt-5".into(),
        runtime_mode: RuntimeMode::Supervised,
        title: title.into(),
        created_at: updated_at,
        updated_at,
        additions: Some(0),
        deletions: Some(0),
        ..SessionSummary::new(id, cwd, HarnessId::Cursor)
    }
}

fn conversation(id: &str, cwd: &str, title: &str, score: i64) -> AppSearchHit {
    AppSearchHit::Conversation(ConversationHit {
        id: format!("conversation:{id}"),
        session_id: id.into(),
        cwd: cwd.into(),
        harness: HarnessId::Cursor,
        title: title.into(),
        updated_at: 1,
        score,
        positions: Vec::new(),
    })
}

fn file(index: usize, score: i64) -> AppSearchHit {
    AppSearchHit::File(FileHit {
        id: format!("file:{index}"),
        path: format!("/tmp/a/{index}.ts"),
        relative: format!("{index}.ts"),
        name: format!("{index}.ts"),
        score,
        positions: Vec::new(),
    })
}

#[test]
fn matches_intl_recent_project_ranking_punctuation_and_accents() {
    for locale in ["en", "fr", "ja", "ar"] {
        monocode_locale::with_locale(locale, || {
            for (input, expected) in [
                (
                    ["file.a", "file-a", "file_a"],
                    ["file_a", "file-a", "file.a"],
                ),
                (["filez", "fileé", "filee"], ["filee", "fileé", "filez"]),
            ] {
                let paths: Vec<_> = input.iter().map(|name| format!("/tmp/{name}")).collect();
                let ranked = search_recent_projects(&paths, "file");
                assert_eq!(ranked.len(), input.len());
                assert!(ranked.windows(2).all(|pair| pair[0].score == pair[1].score));
                assert_eq!(
                    ranked
                        .iter()
                        .map(|hit| hit.name.as_str())
                        .collect::<Vec<_>>(),
                    expected
                );
            }
        })
        .unwrap();
    }
}

#[test]
fn matches_intl_grouped_search_canonical_equivalence_and_score_priority() {
    for locale in ["en", "fr", "ja", "ar"] {
        monocode_locale::with_locale(locale, || {
            let input = [
                "fileé.rs",
                "filee\u{301}.rs",
                "filez.rs",
                "filee.rs",
                "file.a.rs",
                "file-a.rs",
                "file_a.rs",
            ];
            let mut hits: Vec<_> = input
                .iter()
                .map(|name| {
                    AppSearchHit::File(FileHit {
                        id: format!("file:{name}"),
                        path: format!("/tmp/{name}"),
                        relative: (*name).into(),
                        name: (*name).into(),
                        score: 10,
                        positions: vec![0, 1, 2, 3],
                    })
                })
                .collect();
            hits.push(AppSearchHit::File(FileHit {
                id: "high".into(),
                path: "/tmp/high".into(),
                relative: "high".into(),
                name: "high".into(),
                score: 11,
                positions: vec![],
            }));
            let grouped = group_hits(&hits, SearchScope::Files);
            assert_eq!(
                grouped
                    .files
                    .iter()
                    .map(|hit| hit.relative.as_str())
                    .collect::<Vec<_>>(),
                [
                    "high",
                    "file_a.rs",
                    "file-a.rs",
                    "file.a.rs",
                    "filee.rs",
                    "fileé.rs",
                    "filee\u{301}.rs",
                    "filez.rs"
                ]
            );
            assert!(
                grouped
                    .files
                    .iter()
                    .skip(1)
                    .all(|hit| hit.score == 10 && hit.positions == [0, 1, 2, 3])
            );
        })
        .unwrap();
    }
}

#[test]
fn keeps_a_short_string() {
    assert_eq!(snippet_around("hello world", "hello"), "hello world");
}

#[test]
fn trims_around_a_later_match() {
    let text = format!(
        "{}sidebar chips {}",
        "alpha ".repeat(20),
        "omega ".repeat(20)
    );
    let snippet = snippet_around(&text, "sidebar");
    assert!(snippet.starts_with('…'));
    assert!(snippet.contains("sidebar chips"));
}

#[test]
fn shortens_text_without_a_match_and_collapses_whitespace() {
    assert_eq!(snippet_around("  a \n\t b  ", ""), "a b");
    let long = "x".repeat(100);
    assert_eq!(
        snippet_around(&long, "missing"),
        format!("{}…", "x".repeat(84))
    );
    assert_eq!(snippet_around("   ", "x"), "");
}

#[test]
fn fuzzy_matches_display_titles() {
    let rows = vec![
        ConversationRow {
            id: "s1".into(),
            cwd: "/tmp/a".into(),
            harness: HarnessId::Cursor,
            title: "cursor · Fix sidebar search".into(),
            updated_at: 10,
        },
        ConversationRow {
            id: "s2".into(),
            cwd: "/tmp/a".into(),
            harness: HarnessId::Cursor,
            title: "cursor · Unrelated".into(),
            updated_at: 11,
        },
    ];
    let hits = search_conversation_titles(&rows, "side srch", NOW);
    let ids: Vec<&str> = hits.iter().map(|hit| hit.session_id.as_str()).collect();
    assert_eq!(ids, vec!["s1"]);
    assert_eq!(hits[0].title, "Fix sidebar search");
}

#[test]
fn finds_matching_user_and_assistant_text() {
    let blocks = vec![
        Block::new(
            "u1",
            BlockRole::User,
            "Please search the sidebar filter chips",
        ),
        Block::new("a1", BlockRole::Assistant, "Opening the explorer next."),
        Block::new("r1", BlockRole::Reasoning, "sidebar internals"),
    ];
    let hits = search_session_messages(
        &[MessageSource {
            id: "s",
            cwd: "/tmp/a",
            harness: HarnessId::Cursor,
            title: "cursor",
            updated_at: 1,
            blocks: &blocks,
        }],
        "sidebar",
        NOW,
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].block_id, "u1");
    assert_eq!(hits[0].role, "user");
    assert!(hits[0].preview.to_lowercase().contains("sidebar"));
}

#[test]
fn matches_project_folder_names() {
    let hits = search_recent_projects(
        &[
            "/Users/me/code/agent-terminal".to_string(),
            "/Users/me/code/other".to_string(),
        ],
        "agent term",
    );
    let names: Vec<&str> = hits.iter().map(|hit| hit.name.as_str()).collect();
    assert_eq!(names, vec!["agent-terminal"]);
}

#[test]
fn dedupes_by_id_and_keeps_the_higher_score() {
    let low = conversation("s1", "/tmp/a", "One", 4);
    let high = conversation("s1", "/tmp/a", "Better", 20);
    let merged = merge_hits(&[std::slice::from_ref(&low), std::slice::from_ref(&high)]);
    assert_eq!(merged.len(), 1);
    let AppSearchHit::Conversation(hit) = &merged[0] else {
        panic!("expected a conversation");
    };
    assert_eq!((hit.title.as_str(), hit.score), ("Better", 20));
}

#[test]
fn limits_each_section_for_the_all_scope() {
    let files: Vec<AppSearchHit> = (0..20).map(|index| file(index, 10)).collect();
    let grouped = group_hits(&files, SearchScope::All);
    assert_eq!(grouped.files.len(), 10);
    assert_eq!(flatten_grouped(&grouped).len(), 10);
    assert_eq!(grouped_count(&grouped), 10);
}

#[test]
fn hides_non_file_hits_in_the_files_scope() {
    let grouped = group_hits(
        &[
            AppSearchHit::Project(ProjectHit {
                id: "project:/tmp/a".into(),
                path: "/tmp/a".into(),
                name: "a".into(),
                score: 10,
                positions: Vec::new(),
            }),
            AppSearchHit::File(FileHit {
                id: "file:1".into(),
                path: "/tmp/a/one.ts".into(),
                relative: "one.ts".into(),
                name: "one.ts".into(),
                score: 8,
                positions: Vec::new(),
            }),
        ],
        SearchScope::Files,
    );
    assert!(grouped.projects.is_empty());
    assert_eq!(grouped.files.len(), 1);
}

#[test]
fn normalizes_harness_titles() {
    let hits = hits_from_session_search(
        &[
            SearchHit {
                kind: "conversation".into(),
                session_id: "s1".into(),
                cwd: "/tmp/a".into(),
                harness: "cursor".into(),
                title: "cursor · Fix search".into(),
                updated_at: 1,
                block_id: None,
                role: None,
                preview: String::new(),
            },
            SearchHit {
                kind: "message".into(),
                session_id: "s1".into(),
                cwd: "/tmp/a".into(),
                harness: "cursor".into(),
                title: "cursor · Fix search".into(),
                updated_at: 1,
                block_id: Some("u1".into()),
                role: Some("user".into()),
                preview: "search chips".into(),
            },
        ],
        NOW,
    );
    let AppSearchHit::Conversation(first) = &hits[0] else {
        panic!("expected a conversation");
    };
    assert_eq!(first.title, "Fix search");
    let AppSearchHit::Message(second) = &hits[1] else {
        panic!("expected a message");
    };
    assert_eq!(second.block_id, "u1");
    assert_eq!(second.preview, "search chips");
}

#[test]
fn keeps_line_and_preview() {
    let hits = hits_from_content_matches(&[SearchMatch {
        path: "/tmp/a/App.tsx".into(),
        relative: "src/App.tsx".into(),
        line: 12,
        column: 4,
        preview: "const searchOpen = true;  ".into(),
    }]);
    assert_eq!(hits[0].name, "App.tsx");
    assert_eq!(hits[0].line, 12);
    assert_eq!(hits[0].preview, "const searchOpen = true;");
    assert_eq!(hits[0].id, "content:/tmp/a/App.tsx:12:4");
}

#[test]
fn prefers_live_session_titles_over_history() {
    let mut live = Session::blank("s1", HarnessId::Cursor, "cursor:auto", "/tmp/a");
    live.title = "cursor · Live title".into();
    let rows = conversation_rows_from(
        &[summary("s1", "/tmp/a", "cursor · Old title", 5)],
        &[live],
        NOW,
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "cursor · Live title");
    assert_eq!(rows[0].updated_at, 5);
}

#[test]
fn keeps_current_project_conversations() {
    let hits = vec![
        conversation("s1", "/tmp/a", "A", 1),
        conversation("s2", "/tmp/b", "B", 1),
    ];
    let kept: Vec<String> = filter_hits_by_project(&hits, Some("/tmp/a"))
        .iter()
        .map(|hit| hit.id().to_string())
        .collect();
    assert_eq!(kept, vec!["conversation:s1"]);
    assert_eq!(filter_hits_by_project(&hits, Some("~")).len(), 2);
}

#[test]
fn fades_the_recency_bonus_over_a_week() {
    let week = 7 * 24 * 60 * 60 * 1000;
    assert_eq!(recency_bonus(NOW, NOW), 24);
    assert_eq!(recency_bonus(NOW - week / 2, NOW), 12);
    assert_eq!(recency_bonus(NOW - 2 * week, NOW), 0);
    assert_eq!(recency_bonus(0, NOW), 0);
}
