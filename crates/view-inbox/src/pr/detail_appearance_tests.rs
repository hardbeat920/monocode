use super::*;
use gpui::TestAppContext;
use monocode_ui::{AppearanceSettings, ThemePreference, set_appearance};

use crate::data::{Loadable, PrDiff, PrFile};
use crate::fixtures::{FakeDetail, FakeServices, NOW};
use crate::pr::diff::editor_theme;

#[gpui::test]
fn cached_inbox_pr_diff_follows_appearance_without_reloading_or_resetting_expansion(
    cx: &mut TestAppContext,
) {
    cx.skip_drawing();
    cx.update(|cx| {
        crate::tests::init(cx);
        set_appearance(
            AppearanceSettings {
                theme_preference: ThemePreference::Dark,
                ..Default::default()
            },
            cx,
        );
    });
    let item = InboxItem::github(InboxKind::Pr, "acme/project", 42, "A controlled patch");
    let model = PrDiff {
        additions: 2,
        deletions: 2,
        files: vec![
            PrFile { path: "first.txt".into(), additions: 1, deletions: 1 },
            PrFile { path: "second.txt".into(), additions: 1, deletions: 1 },
        ],
        patch: "diff --git a/first.txt b/first.txt\n--- a/first.txt\n+++ b/first.txt\n@@ -1 +1 @@\n-before\n+after\ndiff --git a/second.txt b/second.txt\n--- a/second.txt\n+++ b/second.txt\n@@ -1 +1 @@\n-old\n+new\n".into(),
        truncated: false,
    };
    let data = FakeDetail::new(
        item.clone(),
        InboxDetailState {
            diff: Loadable::ready(model.clone()),
            ..Default::default()
        },
    );
    let services = FakeServices::new(NOW);
    services.state.borrow_mut().details.insert(42, data.clone());
    let window = cx.add_window(|window, cx| {
        InboxDetailView::new(
            services,
            item,
            DetailProps {
                cwd: "/isolated-project".into(),
                visible: true,
                ..Default::default()
            },
            window,
            cx,
        )
    });
    let diff = window
        .update(cx, |detail, window, cx| {
            detail.set_tab(DetailTab::Code, window, cx);
            detail.diff_view.as_ref().unwrap().2.clone()
        })
        .unwrap();
    diff.update(cx, |diff, cx| diff.expand_all(cx));
    cx.run_until_parked();
    let (original_theme, files, expanded) = diff.read_with(cx, |diff, _| {
        assert_eq!(diff.files().len(), 2);
        assert!(diff.files().iter().all(|file| !file.hunks.is_empty()));
        assert_eq!(diff.expanded_files(), &[0, 1].into());
        (
            diff.theme().clone(),
            diff.files()
                .iter()
                .map(|file| file.diff.clone())
                .collect::<Vec<_>>(),
            diff.expanded_files().clone(),
        )
    });
    let calls = data.calls.borrow().clone();
    cx.update(|cx| {
        set_appearance(
            AppearanceSettings {
                theme_preference: ThemePreference::Light,
                accent_color: Some("#cc5500".into()),
                ..Default::default()
            },
            cx,
        );
    });
    cx.run_until_parked();
    window
        .update(cx, |detail, _, cx| {
            let (cached_model, full_file, cached) = detail.diff_view.as_ref().unwrap();
            assert_eq!(cached_model, &model);
            assert_eq!(detail.state.diff.value.as_ref(), Some(&model));
            assert!(!full_file);
            assert_eq!(detail.tab(), DetailTab::Code);
            assert_eq!(cached.entity_id(), diff.entity_id());
            let cached = cached.read(cx);
            assert_eq!(cached.expanded_files(), &expanded);
            assert_eq!(
                cached
                    .files()
                    .iter()
                    .map(|file| file.diff.clone())
                    .collect::<Vec<_>>(),
                files,
            );
            assert_eq!(cached.theme(), &editor_theme(cx));
            assert_ne!(cached.theme(), &original_theme);
        })
        .unwrap();
    assert_eq!(*data.calls.borrow(), calls);
}
