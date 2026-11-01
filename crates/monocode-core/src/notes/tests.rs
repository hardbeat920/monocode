//! Behaviour parity with the desktop app, `src/features/notes/notes.ts`.
//!
//! Every expectation here is what the desktop app does. Each group names the
//! desktop function it covers. Cases copied from the desktop's own suites carry a
//! `desktop_` prefix; the rest cover inputs those suites leave out, resolved the
//! same way, from the desktop's code.
//!
//! The point is not coverage. It is that these rules — the ones that decide what
//! a note is called, how it is tagged, what its preview shows and what gets
//! pasted into a prompt — cannot quietly drift away from the desktop app again.

use super::*;

fn note(id: &str, slug: &str, title: &str) -> Note {
    Note {
        id: id.into(),
        slug: slug.into(),
        title: title.into(),
        body: String::new(),
        tags: Vec::new(),
        source_session_id: None,
        source_cwd: None,
        slug_pending: false,
        created_at: 1,
        updated_at: 1,
    }
}

fn with_body(note: Note, body: &str) -> Note {
    Note {
        body: body.into(),
        ..note
    }
}

fn with_project(note: Note, cwd: &str) -> Note {
    Note {
        source_cwd: Some(cwd.into()),
        ..note
    }
}

fn tagged(note: Note, tags: &[&str]) -> Note {
    Note {
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        ..note
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_string()).collect()
}

mod title_rules {
    //! Mirrors `noteTitle` in `src/features/notes/notes.ts`.

    use super::*;

    #[test]
    fn desktop_the_first_heading_wins_over_the_intro_line() {
        assert_eq!(title("intro\n# Auth approach\n\nbody"), "Auth approach");
    }

    #[test]
    fn desktop_the_first_prose_line_is_the_fallback() {
        assert_eq!(
            title("Ship the notes overlay first."),
            "Ship the notes overlay first."
        );
    }

    #[test]
    fn desktop_spacing_and_code_alone_leave_a_note_untitled() {
        assert_eq!(title("   \n```\ncode\n```\n"), "Untitled");
    }

    #[test]
    fn emphasis_links_and_ticks_are_stripped_from_the_name() {
        assert_eq!(
            title("# **Ship** the [plan](https://example.com) `now`"),
            "Ship the plan now"
        );
    }

    #[test]
    fn a_heading_indented_past_three_spaces_is_prose_and_keeps_its_hashes() {
        // `noteTitle` allows at most three spaces of indent, so a four-space
        // line is not a heading here either; it becomes the first prose line.
        assert_eq!(title("    # Not a title\nReal title"), "# Not a title");
    }

    #[test]
    fn seven_hashes_are_prose_not_a_heading() {
        assert_eq!(title("####### seven"), "####### seven");
    }

    #[test]
    fn a_rule_and_an_image_only_line_are_skipped() {
        assert_eq!(title("---\n![shot](notes/a.png)\nReal title"), "Real title");
    }

    #[test]
    fn a_heading_inside_a_code_fence_still_names_the_note_like_the_desktop() {
        // Both apps scan for headings without tracking fences, so a `# comment`
        // in a code block becomes the name. Pinned so a "fix" on one side alone
        // cannot split the two apps.
        assert_eq!(title("```sh\n# install\n```\nbody"), "install");
    }

    #[test]
    fn the_name_stops_at_the_maximum_length() {
        assert_eq!(title(&"a".repeat(MAX_TITLE + 40)), "a".repeat(MAX_TITLE));
    }
}

mod preview_rules {
    //! Mirrors `notePreview` in `src/features/notes/notes.ts`.

    use super::*;

    #[test]
    fn desktop_the_title_heading_never_repeats_in_the_preview() {
        let note = with_body(note("n", "auth", "Auth"), "# Auth\n\nKeep it global.");
        assert_eq!(note.preview(), "Keep it global.");
    }

    #[test]
    fn fences_rules_and_bullet_markers_are_dropped() {
        let note = with_body(
            note("n", "s", "S"),
            "# S\n\n```\ncode\n```\n---\n- first\n* second\n\nthird",
        );
        assert_eq!(note.preview(), "first second third");
    }

    #[test]
    fn lines_are_joined_until_the_budget_then_ellipsized() {
        let body = "word ".repeat(40);
        let note = with_body(note("n", "t", "T"), &format!("# T\n\n{body}"));
        let kept = note
            .preview()
            .strip_suffix('…')
            .expect("an over-budget preview ends in an ellipsis")
            .to_owned();
        assert_eq!(kept.chars().count(), 119);
        assert!(
            body.trim().starts_with(&kept),
            "what survives is the opening of the body, not a re-join of it"
        );
    }

    #[test]
    fn a_heading_indented_past_three_spaces_stays_in_the_preview() {
        // `notePreview` tests its heading pattern against the raw line, so this
        // indented line is prose there and prose here.
        let note = with_body(note("n", "a", "A"), "    # Not a title\n\nSome text.");
        assert_eq!(note.preview(), "# Not a title Some text.");
    }

    #[test]
    fn the_budget_counts_characters_not_bytes() {
        // The desktop counts UTF-16 units, native counts characters. The two are
        // identical outside the astral planes, which is as far as parity can go:
        // slicing a surrogate pair in half is not representable in a Rust string.
        let note = with_body(note("n", "u", "Ü"), &format!("# Ü\n\n{}", "é".repeat(130)));
        assert_eq!(note.preview(), format!("{}…", "é".repeat(119)));
    }

    #[test]
    fn a_body_that_only_repeats_the_name_previews_empty() {
        let note = with_body(note("n", "x", "Same"), "Same");
        assert_eq!(note.preview(), "");
    }
}

mod tag_rules {
    //! Mirrors `normalizeNoteTags` in `src/features/notes/notes.ts`. The store now
    //! calls this function, so these are the tags that reach disk.

    use super::*;

    #[test]
    fn desktop_hashes_and_spacing_normalize_and_duplicates_drop() {
        assert_eq!(
            normalize_tags(&strings(&[" Ideas ", "#Project Docs", "ideas", "###"])),
            strings(&["ideas", "project-docs"])
        );
    }

    #[test]
    fn at_most_twenty_tags_survive_and_they_are_the_first_twenty() {
        let tags = (0..25)
            .map(|index| format!("tag-{index}"))
            .collect::<Vec<_>>();
        let expected = (0..MAX_TAGS)
            .map(|index| format!("tag-{index}"))
            .collect::<Vec<_>>();
        assert_eq!(normalize_tags(&tags), expected);
    }

    #[test]
    fn a_cut_tag_does_not_end_on_a_dash() {
        let long = format!("{} tail", "x".repeat(MAX_TAG_LENGTH - 1));
        let tags = normalize_tags(&[long]);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0], "x".repeat(MAX_TAG_LENGTH - 1));
    }
}

mod search_rules {
    //! Mirrors the filter in `src/features/notes/ui/NotesView.tsx`.

    use super::*;

    #[test]
    fn a_blank_query_keeps_every_note() {
        assert!(note("n", "a", "A").matches("   "));
    }

    #[test]
    fn the_query_reaches_the_name_the_body_the_slug_and_the_tags() {
        let note = tagged(
            with_body(note("n", "auth-plan", "Auth"), "Use a cookie."),
            &["security"],
        );
        assert!(note.matches("AUTH"));
        assert!(note.matches("cookie"));
        assert!(note.matches("plan"));
        assert!(note.matches("#security"));
        assert!(note.matches("security"));
        assert!(!note.matches("missing"));
    }

    #[test]
    fn one_leading_hash_is_stripped_from_a_tag_query_and_no_more() {
        let note = tagged(note("n", "a", "A"), &["security"]);
        assert!(note.matches("#security"));
        assert!(!note.matches("##security"));
    }

    #[test]
    fn the_query_reaches_the_project_folder_name() {
        let note = with_project(note("n", "a", "A"), "/tmp/monocode-lab/agent-terminal");
        assert!(note.matches("agent"));
        assert!(!note.matches("elsewhere"));
    }
}

mod project_detection_rules {
    //! Mirrors `looksLikeProject` in `src/features/projects/model/recents.ts`.
    //! The desktop reaches the same verdict for the home shapes through
    //! `prettyCwd`, which folds any `/Users/<name>` or `/home/<name>` into `~`.

    use super::*;

    #[test]
    fn a_project_is_a_real_folder_not_a_root_or_an_app_bundle() {
        for path in ["", "~", "/", "C:", "/Applications/MonoCode.app/Contents"] {
            assert!(!is_project(path), "{path} must not count as a project");
        }
    }

    #[test]
    fn a_users_home_is_not_a_project_even_though_it_is_a_folder() {
        assert!(!is_project("/Users/someone"));
        assert!(!is_project("/home/someone"));
    }

    #[test]
    fn a_nested_folder_is_a_project() {
        assert!(is_project("/Users/someone/code/agent-terminal"));
        assert!(is_project("/Users/someone/code/agent-terminal/"));
    }

    #[test]
    fn desktop_a_note_in_a_project_is_named_after_its_folder() {
        assert_eq!(
            with_project(note("n", "a", "A"), "/Users/someone/code/agent-terminal")
                .project_name()
                .as_deref(),
            Some("agent-terminal")
        );
    }

    #[test]
    fn desktop_a_note_outside_a_project_has_no_name() {
        assert_eq!(note("n", "a", "A").project_name(), None);
        assert_eq!(with_project(note("n", "a", "A"), "~").project_name(), None);
        assert_eq!(with_project(note("n", "a", "A"), "/").project_name(), None);
    }
}

mod prompt_rules {
    //! Mirrors `injectNotePrompt` and `composeNoteMessage` in
    //! `src/features/notes/notes.ts`.

    use super::*;

    #[test]
    fn desktop_a_referenced_note_follows_the_prompt_after_a_rule() {
        let note = with_body(note("n1", "auth", "Auth"), "Use a cookie.");
        assert_eq!(
            inject_prompt("Use this.", &[note]),
            "Use this.\n\n---\nReferenced note \"Auth\":\n\nUse a cookie."
        );
    }

    #[test]
    fn without_a_note_the_prompt_is_untouched() {
        assert_eq!(inject_prompt("Keep me", &[]), "Keep me");
    }

    #[test]
    fn a_blank_name_reads_as_untitled() {
        let note = with_body(note("n", "u", "   "), "Body");
        assert_eq!(
            inject_prompt("Ask", &[note]),
            "Ask\n\n---\nReferenced note \"Untitled\":\n\nBody"
        );
    }

    #[test]
    fn two_notes_follow_each_other_without_a_gap() {
        let first = with_body(note("n1", "a", "A"), "First body.");
        let second = with_body(note("n2", "b", "B"), "Second body.");
        assert_eq!(
            inject_prompt("Go.", &[first, second]),
            "Go.\n\n---\nReferenced note \"A\":\n\nFirst body.\nReferenced note \"B\":\n\nSecond body."
        );
    }

    #[test]
    fn desktop_a_blank_draft_becomes_use_this_note() {
        let note = with_body(note("n1", "auth", "Auth"), "Use a cookie.");
        assert_eq!(
            compose_message(&note, "  "),
            "Use this note.\n\n---\nReferenced note \"Auth\":\n\nUse a cookie."
        );
    }

    #[test]
    fn desktop_a_written_draft_keeps_the_users_words() {
        let note = with_body(note("n1", "auth", "Auth"), "Use a cookie.");
        assert_eq!(
            compose_message(&note, "Start with the cookie."),
            "Start with the cookie.\n\n---\nReferenced note \"Auth\":\n\nUse a cookie."
        );
    }
}

mod mention_rules {
    //! Mirrors `noteSlugsInText` in `src/features/notes/notes.ts`.

    use super::*;

    #[test]
    fn desktop_note_tokens_are_collected_once_and_in_order() {
        assert_eq!(
            slugs_in_text("See @note/auth and @note/auth plus @note/plan-2."),
            strings(&["auth", "plan-2"])
        );
    }

    #[test]
    fn a_token_glued_to_a_word_is_not_a_mention() {
        assert!(slugs_in_text("mail@note/auth").is_empty());
        assert!(slugs_in_text("@note/").is_empty());
        assert_eq!(slugs_in_text("start @note/auth"), strings(&["auth"]));
    }
}

mod image_rules {
    //! Mirrors `noteImageMarkdown` in `src/features/notes/noteImages.ts`.

    use super::*;

    #[test]
    fn desktop_an_image_name_escapes_its_brackets() {
        let asset = NoteImageAsset {
            name: "Architecture [draft].png".into(),
            markdown_path: "/note-assets/note-1/123-architecture-draft.png".into(),
        };
        assert_eq!(
            image_markdown(&asset),
            "![Architecture \\[draft\\].png](/note-assets/note-1/123-architecture-draft.png)"
        );
    }

    #[test]
    fn a_backslash_in_a_name_is_escaped_as_well() {
        let asset = NoteImageAsset {
            name: "a\\c.png".into(),
            markdown_path: "notes/img/x.png".into(),
        };
        assert_eq!(image_markdown(&asset), "![a\\\\c.png](notes/img/x.png)");
    }

    #[test]
    fn a_multi_line_name_collapses_into_one_space() {
        let asset = NoteImageAsset {
            name: "shot\r\none.png".into(),
            markdown_path: "p.png".into(),
        };
        assert_eq!(image_markdown(&asset), "![shot one.png](p.png)");
    }
}

mod upsert_rules {
    //! Mirrors `NoteUpsert` and `upsertNote` in `src/features/notes/notes.ts`.

    use super::*;

    #[test]
    fn the_upsert_carries_the_saved_fields_and_the_finalize_flag() {
        let note = tagged(
            with_project(
                with_body(note("n1", "auth", "Auth"), "Body"),
                "/tmp/lab/one",
            ),
            &["one", "two"],
        );
        let upsert = note.upsert(true);
        assert_eq!(upsert.id, "n1");
        assert_eq!(upsert.title, "Auth");
        assert_eq!(upsert.body, "Body");
        assert_eq!(upsert.tags, strings(&["one", "two"]));
        assert_eq!(upsert.source_cwd.as_deref(), Some("/tmp/lab/one"));
        assert!(upsert.finalize_slug);
        assert!(!note.upsert(false).finalize_slug);
    }
}

mod storage_rules {
    //! The stored row is the desktop's `Note` in `src/features/notes/notes.ts`:
    //! camelCase keys, `slugPending` optional, and no key at all for an absent
    //! project or session.

    use super::*;

    #[test]
    fn the_stored_json_uses_the_desktops_keys() {
        let note = tagged(
            with_project(
                with_body(note("n1", "auth", "Auth"), "Body"),
                "/tmp/lab/one",
            ),
            &["one"],
        );
        let json = serde_json::to_value(&note).unwrap();
        let keys = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            [
                "body",
                "createdAt",
                "id",
                "slug",
                "slugPending",
                "sourceCwd",
                "tags",
                "title",
                "updatedAt"
            ],
            "the desktop's keys, with no absent project or session key written"
        );
        assert_eq!(json["sourceCwd"], "/tmp/lab/one");
        assert_eq!(json["slugPending"], false);
    }

    #[test]
    fn a_row_without_the_optional_keys_still_reads() {
        let json =
            r#"{"id":"n","slug":"auth","title":"Auth","body":"","createdAt":1,"updatedAt":1}"#;
        let note: Note = serde_json::from_str(json).unwrap();
        assert_eq!(note.tags, Vec::<String>::new());
        assert!(!note.slug_pending);
        assert!(note.source_cwd.is_none());
    }
}
