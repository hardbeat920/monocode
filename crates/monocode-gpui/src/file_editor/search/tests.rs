use super::*;
use gpui::TestAppContext;

#[gpui::test]
fn stale_searches_cannot_move_selection_or_replace_the_live_buffer(cx: &mut TestAppContext) {
    let path = std::env::temp_dir().join(format!(
        "monocode-search-snapshot-{}.txt",
        std::process::id()
    ));
    let source = "🙂 foo FOO\nfoo";
    std::fs::write(&path, source).unwrap();
    let window = cx.update(|cx| {
        cx.set_global(crate::settings::Preferences::default());
        cx.open_window(Default::default(), |_, cx| {
            let git = cx
                .new(|cx| ProjectGit::new(std::env::temp_dir().to_string_lossy().into_owned(), cx));
            cx.new(|cx| FileEditor::new(path.to_string_lossy().into_owned(), git, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();

    for change in [
        "query",
        "replacement",
        "options",
        "buffer",
        "generation",
        "closed",
    ] {
        window
            .update(cx, |e, w, cx| {
                e.input.update(cx, |i, cx| {
                    i.set_text(source.into(), cx);
                    i.select_range(0..0, cx);
                });
                e.match_case = false;
                e.open_search(w, cx);
                e.search.update(cx, |i, cx| i.set_text("foo".into(), cx));
                e.replacement.update(cx, |i, cx| i.set_text("x".into(), cx));
                e.find_matches(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |e, w, cx| {
                assert_eq!(e.matches.len(), 3);
                e.step_match(true, cx);
                assert_eq!(e.match_index, 2);
                e.step_match(false, cx);
                assert_eq!(e.match_index, 0);
                // Deliver a genuine result after changing its inputs. Explicit
                // ordering makes the race check independent of scheduler luck.
                let snapshot = e.capture_search(cx);
                let result = snapshot.run();
                match change {
                    "query" => e.search.update(cx, |i, cx| i.set_text("absent".into(), cx)),
                    "replacement" => e
                        .replacement
                        .update(cx, |i, cx| i.set_text("longer".into(), cx)),
                    "options" => e.match_case = true,
                    "buffer" => e.input.update(cx, |i, cx| i.set_text("🙂".into(), cx)),
                    "generation" => e.find_matches(cx),
                    "closed" => {
                        // Changed is delivered after Dismiss; it must not restart
                        // a hidden search or restore its highlights/selection.
                        e.search.update(cx, |i, cx| i.set_text("FOO".into(), cx));
                        e.dismiss_search(w, cx);
                    }
                    _ => unreachable!(),
                }
                let text = e.input.read(cx).content.clone();
                let selection = e.input.read(cx).selection_range();
                assert!(!e.apply_search_result(snapshot, result, 0, cx), "{change}");
                e.step_match(false, cx);
                e.replace_matches(false, cx);
                e.replace_matches(true, cx);
                assert_eq!(e.input.read(cx).content, text, "{change}");
                assert_eq!(e.input.read(cx).selection_range(), selection, "{change}");
            })
            .unwrap();
        cx.run_until_parked();
    }
    window
        .update(cx, |e, w, cx| {
            assert!(!e.search_open && e.matches.is_empty());
            e.input.update(cx, |i, cx| i.select_range(0..0, cx));
            e.open_search(w, cx);
            e.search.update(cx, |i, cx| i.set_text("foo".into(), cx));
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |e, _, cx| {
            // A timeout must never become a partial Replace All, even if an engine
            // implementation returns matches alongside its incomplete status.
            let snapshot = e.capture_search(cx);
            let mut result = snapshot.run();
            assert!(!result.matches.is_empty());
            result.timed_out = true;
            assert!(e.apply_search_result(snapshot, result, 0, cx));
            assert!(e.search_timed_out && e.matches.is_empty());
            e.replace_matches(true, cx);
            assert_eq!(e.input.read(cx).content.as_ref(), source);
        })
        .unwrap();
    std::fs::remove_file(path).unwrap();
}
