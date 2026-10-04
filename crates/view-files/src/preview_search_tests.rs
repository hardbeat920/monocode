//! Port of src/features/files/ui/FilePreviewSearch.test.ts. The content is
//! a real `MarkdownView`, which reports the runs it painted.

use gpui::{Entity, KeyBinding, NoAction, TestAppContext, VisualTestContext};
use monocode_markdown::MarkdownView;

use super::*;

const OPTIONS: SearchOptions = SearchOptions {
    case_sensitive: false,
    whole_word: false,
    regexp: false,
};

#[test]
fn javascript_preview_regex_accepts_lookaround_and_backreferences() {
    let options = SearchOptions {
        regexp: true,
        ..OPTIONS
    };
    for (pattern, expected) in [
        (r"foo(?=bar)", 0..3),
        (r"(?<=foo)bar", 3..6),
        (r"(foo)\1", 7..13),
    ] {
        let result = find_preview_text_matches("foobar foofoo", pattern, options);
        assert!(!result.invalid, "JavaScript accepts {pattern}");
        assert_eq!(result.matches, vec![expected], "{pattern}");
    }
    let result = find_preview_text_matches("foobar foofoo", r"(?<=f+)oo", options);
    assert!(
        !result.invalid,
        "JavaScript accepts a variable-width lookbehind"
    );
    assert_eq!(result.matches, vec![1..3, 8..10, 11..13]);
}

#[test]
fn javascript_preview_regex_keeps_unicode_character_class_rules() {
    let options = SearchOptions {
        regexp: true,
        ..OPTIONS
    };
    for (pattern, text, expected) in [
        (r"\w+", "é foo ٣", 3..6),
        (r"\d+", "٣ 3", 3..4),
        (r"\s", "a\u{feff}b\u{85}c", 1..4),
    ] {
        let result = find_preview_text_matches(text, pattern, options);
        assert!(!result.invalid);
        assert_eq!(result.matches, vec![expected], "{pattern}");
    }
    assert!(find_preview_text_matches("foo", "(?i)foo", options).invalid);
}

#[test]
fn preview_search_keeps_literal_punctuation_and_single_line_anchors() {
    assert_eq!(
        find_preview_text_matches("a-b", "a-b", OPTIONS).matches,
        vec![0..3]
    );
    let options = SearchOptions {
        regexp: true,
        ..OPTIONS
    };
    assert!(
        find_preview_text_matches("a\nb", "^b", options)
            .matches
            .is_empty()
    );
    assert!(
        find_preview_text_matches("a\nb", "a$", options)
            .matches
            .is_empty()
    );
}

#[test]
fn finds_literal_text_case_insensitively() {
    assert_eq!(
        find_preview_text_matches("Alpha beta ALPHA", "alpha", OPTIONS).matches,
        vec![0..5, 11..16]
    );
}

#[test]
fn supports_whole_word_and_regular_expression_searches() {
    let whole = SearchOptions {
        whole_word: true,
        ..OPTIONS
    };
    assert_eq!(
        find_preview_text_matches("cat scatter cat", "cat", whole).matches,
        vec![0..3, 12..15]
    );
    let regexp = SearchOptions {
        regexp: true,
        ..OPTIONS
    };
    assert_eq!(
        find_preview_text_matches("item-12 item-aa", "item-\\d+", regexp).matches,
        vec![0..7]
    );
}

#[test]
fn reports_an_invalid_regular_expression() {
    let regexp = SearchOptions {
        regexp: true,
        ..OPTIONS
    };
    let result = find_preview_text_matches("text", "[", regexp);
    assert!(result.matches.is_empty());
    assert!(result.invalid);
}

#[test]
fn caps_the_match_count() {
    let text = "a".repeat(MATCH_CAP + 5);
    let result = find_preview_text_matches(&text, "a", OPTIONS);
    assert_eq!(result.matches.len(), MATCH_CAP);
    assert!(result.capped);
}

fn mount<'a>(
    markdown: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<FilePreviewSearch>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        crate::test_support::init(cx);
        monocode_markdown::init(cx);
    });
    let markdown = markdown.to_string();
    let (search, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| MarkdownView::with_text(markdown, cx));
        let source_view = view.clone();
        let source: RunsSource = Rc::new(move |cx: &App| source_view.read(cx).rendered_text());
        FilePreviewSearch::new(view.into(), source, None, window, cx)
    });
    cx.update(|window, cx| {
        let handle = search.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    cx.run_until_parked();
    (search, cx)
}

fn type_query(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(text);
    cx.run_until_parked();
}

fn count(search: &Entity<FilePreviewSearch>, cx: &mut VisualTestContext) -> String {
    search.read_with(cx, |search, cx| search.count_label(cx))
}

#[gpui::test]
fn opens_from_the_find_shortcut_and_navigates_rendered_matches(cx: &mut TestAppContext) {
    let (search, cx) = mount("Alpha **beta**\n\nalpha", cx);
    cx.simulate_keystrokes("secondary-f");
    assert!(search.read_with(cx, |search, _| search.is_open()));

    type_query("Alpha beta", cx);
    assert_eq!(count(&search, cx), "1 of 1");

    type_query("alpha", cx);
    assert_eq!(count(&search, cx), "1 of 2");

    cx.simulate_keystrokes("enter");
    assert_eq!(count(&search, cx), "2 of 2");
    cx.simulate_keystrokes("shift-enter");
    assert_eq!(count(&search, cx), "1 of 2");
    cx.simulate_keystrokes("secondary-g secondary-g");
    assert_eq!(count(&search, cx), "1 of 2");

    // Match Case drops the capitalized match.
    cx.simulate_keystrokes("alt-c");
    assert_eq!(count(&search, cx), "1 of 1");

    cx.simulate_keystrokes("escape");
    assert!(!search.read_with(cx, |search, _| search.is_open()));
    // Escape with the bar closed goes on to the owner.
    cx.simulate_keystrokes("escape");
    assert!(!search.read_with(cx, |search, _| search.is_open()));
}

#[gpui::test]
fn opens_from_a_custom_find_shortcut_instead_of_the_default(cx: &mut TestAppContext) {
    let (search, cx) = mount("Alpha", cx);
    cx.update(|_, cx| {
        cx.bind_keys([
            KeyBinding::new("ctrl-shift-m", OpenFind, Some(KEY_CONTEXT)),
            KeyBinding::new("secondary-f", NoAction, Some(KEY_CONTEXT)),
        ]);
    });
    cx.simulate_keystrokes("ctrl-shift-m");
    assert!(search.read_with(cx, |search, _| search.is_open()));
    cx.simulate_keystrokes("escape");
    assert!(!search.read_with(cx, |search, _| search.is_open()));
    cx.simulate_keystrokes("secondary-f");
    assert!(!search.read_with(cx, |search, _| search.is_open()));
}

#[gpui::test]
fn reports_no_results_and_invalid_patterns(cx: &mut TestAppContext) {
    let (search, cx) = mount("Alpha", cx);
    cx.simulate_keystrokes("secondary-f");
    type_query("zeta", cx);
    assert_eq!(count(&search, cx), "No results");
    cx.simulate_keystrokes("alt-r");
    type_query("[", cx);
    assert_eq!(count(&search, cx), "Invalid regex");
}

#[gpui::test]
fn closes_when_the_preview_hides(cx: &mut TestAppContext) {
    let (search, cx) = mount("Alpha", cx);
    cx.simulate_keystrokes("secondary-f");
    type_query("alpha", cx);
    search.update(cx, |search, cx| search.set_active(false, cx));
    search.read_with(cx, |search, _| {
        assert!(!search.is_open());
        assert!(search.matches().is_empty());
    });
}
