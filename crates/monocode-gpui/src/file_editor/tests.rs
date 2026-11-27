use super::*;
use gpui::{EntityInputHandler, TestAppContext};

#[gpui::test]
fn replace_is_one_undo_and_autosave_formats_with_original_encoding(cx: &mut TestAppContext) {
    let path = std::env::temp_dir().join(format!(
        "monocode-editor-parity-{}.json",
        std::process::id()
    ));
    let original = "\u{feff}{\"word\":\"hello\",\"other\":\"hello\"}\r\n";
    std::fs::write(&path, original).unwrap();
    cx.update(|cx| cx.set_global(crate::settings::Preferences::default()));
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            let git = cx
                .new(|cx| ProjectGit::new(std::env::temp_dir().to_string_lossy().into_owned(), cx));
            cx.new(|cx| FileEditor::new(path.to_string_lossy().into_owned(), git, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    window
        .update(cx, |e, w, cx| {
            e.open_search(w, cx);
            e.replace_open = true;
            e.search.update(cx, |i, cx| i.set_text("hello".into(), cx));
            e.replacement
                .update(cx, |i, cx| i.set_text("world".into(), cx));
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |e, _, cx| {
            assert_eq!(e.matches.len(), 2);
            e.replace_matches(true, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    window
        .update(cx, |e, w, cx| {
            assert!(e.dirty);
            e.dismiss_search(w, cx);
            w.dispatch_action(Box::new(input::Undo), cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |e, w, cx| {
            assert!(!e.dirty);
            e.open_search(w, cx);
            e.find_matches(cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |e, _, cx| {
            assert_eq!(e.matches.len(), 2);
            e.replace_matches(true, cx);
        })
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| cx.update_global::<crate::settings::Preferences, _>(|p, _| p.autosave = true));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(999));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "\u{feff}{ \"word\": \"world\", \"other\": \"world\" }\r\n"
    );
    window
        .update(cx, |e, _, _| assert!(!e.dirty && !e.saving))
        .unwrap();
    std::fs::remove_file(path).unwrap();
}

#[gpui::test]
fn autosave_refuses_external_edits_even_before_the_file_poll(cx: &mut TestAppContext) {
    let path = std::env::temp_dir().join(format!(
        "monocode-autosave-conflict-{}.txt",
        std::process::id()
    ));
    std::fs::write(&path, "original\n").unwrap();
    cx.update(|cx| {
        cx.set_global(crate::settings::Preferences {
            autosave: true,
            ..Default::default()
        })
    });
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            let git = cx
                .new(|cx| ProjectGit::new(std::env::temp_dir().to_string_lossy().into_owned(), cx));
            cx.new(|cx| FileEditor::new(path.to_string_lossy().into_owned(), git, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    window
        .update(cx, |e, w, cx| {
            e.input
                .update(cx, |i, cx| i.replace_text_in_range(None, "draft ", w, cx))
        })
        .unwrap();
    cx.run_until_parked();
    std::fs::write(&path, "external\n").unwrap();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    window
        .update(cx, |e, _, _| {
            assert!(e.dirty);
            assert!(e.error.as_ref().unwrap().contains("changed on disk"));
        })
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "external\n");
    std::fs::remove_file(path).unwrap();
}

#[gpui::test]
fn saving_a_clean_file_still_applies_the_format_on_save_preference(cx: &mut TestAppContext) {
    let path =
        std::env::temp_dir().join(format!("monocode-clean-format-{}.json", std::process::id()));
    std::fs::write(&path, "{\"value\":1}").unwrap();
    cx.update(|cx| cx.set_global(crate::settings::Preferences::default()));
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            let git = cx
                .new(|cx| ProjectGit::new(std::env::temp_dir().to_string_lossy().into_owned(), cx));
            cx.new(|cx| FileEditor::new(path.to_string_lossy().into_owned(), git, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    window
        .update(cx, |e, _, cx| {
            assert!(!e.dirty);
            e.save(cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "{ \"value\": 1 }\n"
    );
    std::fs::remove_file(path).unwrap();
}
