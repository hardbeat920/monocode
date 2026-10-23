use super::*;
use gpui::{
    Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, point,
    size,
};

fn window(section: Section, cx: &mut TestAppContext) -> WindowHandle<SettingsView> {
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(1200.), px(800.)),
                ))),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| {
                    let mut view = SettingsView::new(
                        section,
                        Preferences::default(),
                        SettingsContext::default(),
                        cx,
                    );
                    view.discovery_task = None;
                    view.discovery_loading = false;
                    view
                })
            },
        )
        .unwrap()
    });
    cx.refresh().unwrap();
    cx.run_until_parked();
    window
}

#[gpui::test]
fn workspace_style_is_only_offered_under_the_new_layout(cx: &mut TestAppContext) {
    let window = window(Section::Appearance, cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    for classic in [false, true, false] {
        window
            .update(visual, |view, _, cx| {
                view.ctx.classic = classic;
                view.ctx.new_layout_boxy = true;
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        assert_eq!(
            visual
                .debug_bounds("settings-new-layout-style-boxy")
                .is_some(),
            !classic
        );
        assert_eq!(
            visual
                .debug_bounds("settings-new-layout-style-rounded")
                .is_some(),
            !classic
        );
        assert!(
            visual
                .debug_bounds("settings-collapsed-project-rail-hidden")
                .is_none()
        );
    }
}

#[gpui::test]
fn provider_models_load_without_chat_catalogs_and_empty_selects_stay_closed(
    cx: &mut TestAppContext,
) {
    use monocode_core::provider::Kind;
    let window = window(Section::Providers, cx);
    window
        .update(cx, |view, _, cx| {
            view.discovery.binaries = vec![(Kind::Codex, Some("/fixture/codex".into()))];
            view.prefs
                .default_models
                .insert("codex".into(), "model-39".into());
            view.load_provider_models(Kind::Codex, false, cx);
            assert!(view.provider_models[&Kind::Codex].loading);
            cx.notify();
        })
        .unwrap();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    let trigger = visual.debug_bounds("settings-select-codex").unwrap();
    visual.simulate_click(trigger.center(), Default::default());
    visual.run_until_parked();
    window
        .update(visual, |view, _, cx| {
            assert!(view.select_open.is_none());
            let generation = view.provider_models[&Kind::Codex].generation;
            view.provider_models_finished(
                Kind::Codex,
                generation,
                Ok((0..40)
                    .map(|i| (format!("model-{i}"), format!("Model {i}")))
                    .collect()),
                cx,
            );
        })
        .unwrap();
    visual.run_until_parked();
    let trigger = visual.debug_bounds("settings-select-codex").unwrap();
    visual.simulate_click(trigger.center(), Default::default());
    visual.run_until_parked();
    // GPUI tests have no platform frame loop to deliver the initial scroll callback.
    visual.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    visual.run_until_parked();
    let list = visual.debug_bounds("settings-select-list-codex").unwrap();
    assert!(
        list.size.height > px(28.) && list.size.height <= px(310.),
        "{list:?}"
    );
    let last = visual.debug_bounds("settings-option-codex-39").unwrap();
    assert_eq!(last.size.height, px(28.));
    assert!(
        last.top() >= list.top() && last.bottom() <= list.bottom(),
        "list={list:?}, last={last:?}"
    );
    visual.simulate_keystrokes("home");
    visual.run_until_parked();
    let list = visual.debug_bounds("settings-select-list-codex").unwrap();
    let first = visual.debug_bounds("settings-option-codex-0").unwrap();
    assert!(
        first.top() >= list.top() && first.bottom() <= list.bottom(),
        "list={list:?}, first={first:?}"
    );
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    window
        .update(visual, |view, _, _| {
            assert_eq!(view.prefs.default_models["codex"], "model-0");
            assert!(view.select_open.is_none());
        })
        .unwrap();
}

#[gpui::test]
fn provider_model_errors_and_empty_results_offer_retry_and_ignore_stale_results(
    cx: &mut TestAppContext,
) {
    use monocode_core::provider::Kind;
    let window = window(Section::Providers, cx);
    let old_generation = window
        .update(cx, |view, _, cx| {
            view.discovery.binaries = vec![(Kind::Codex, Some("/fixture/codex".into()))];
            view.load_provider_models(Kind::Codex, false, cx);
            let generation = view.provider_models[&Kind::Codex].generation;
            view.provider_models_finished(
                Kind::Codex,
                generation,
                Err("Sign in required".into()),
                cx,
            );
            generation
        })
        .unwrap();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    let retry = visual.debug_bounds("settings-retry-models-codex").unwrap();
    visual.simulate_click(retry.center(), Default::default());
    visual.run_until_parked();
    window
        .update(visual, |view, _, cx| {
            let state = &view.provider_models[&Kind::Codex];
            assert!(state.loading);
            assert!(state.error.is_none());
            let generation = state.generation;
            assert!(generation > old_generation);
            view.provider_models_finished(
                Kind::Codex,
                old_generation,
                Ok(vec![("stale-model".into(), "Stale model".into())]),
                cx,
            );
            assert!(view.provider_models[&Kind::Codex].models.is_empty());
            view.provider_models_finished(Kind::Codex, generation, Ok(vec![]), cx);
        })
        .unwrap();
    visual.run_until_parked();
    assert!(visual.debug_bounds("settings-retry-models-codex").is_some());
    window
        .update(visual, |view, _, cx| {
            view.discovery.binaries[0].1 = Some("/fixture/new-codex".into());
            view.load_provider_models(Kind::Codex, false, cx);
            let state = &view.provider_models[&Kind::Codex];
            assert!(state.loading);
            assert_eq!(state.binary, std::path::PathBuf::from("/fixture/new-codex"));
            assert!(state.generation > old_generation);
            let previous_account_generation = state.generation;
            view.provider_models
                .get_mut(&Kind::Codex)
                .unwrap()
                .profile
                .id = "other-account".into();
            view.load_provider_models(Kind::Codex, false, cx);
            assert_eq!(
                view.provider_models[&Kind::Codex].profile,
                Default::default()
            );
            view.provider_models_finished(
                Kind::Codex,
                previous_account_generation,
                Ok(vec![("other-account-model".into(), "Other account".into())]),
                cx,
            );
            assert!(view.provider_models[&Kind::Codex].models.is_empty());
        })
        .unwrap();
}

#[gpui::test]
fn skills_document_has_its_own_full_height_pane_and_compact_actions(cx: &mut TestAppContext) {
    let window = window(Section::Skills, cx);
    window
        .update(cx, |view, _, cx| {
            let path = std::path::PathBuf::from("/project/.agents/skills/review/SKILL.md");
            view.discovery.skills = vec![data::Skill {
                name: "review".into(),
                description: "Review changes".into(),
                path: path.clone(),
                scope: "project".into(),
                provider: "agents".into(),
            }];
            view.skill_preview = Some(path);
            view.skill_preview_text = Some(Ok(skill_document::DocumentData::new(format!(
                "---\nname: review\n---\n# Review\n\n{}",
                "Instructions.\n\n".repeat(150)
            ))));
            cx.notify();
        })
        .unwrap();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    let left = visual.debug_bounds("skills-list-scroll").unwrap();
    let right = visual.debug_bounds("skill-preview-pane").unwrap();
    let body = visual.debug_bounds("skill-document").unwrap();
    assert_eq!(left.size.height, px(800.));
    assert_eq!(right.size.height, px(800.));
    assert!(right.left() >= left.right());
    assert!(body.bottom() <= right.bottom());
    assert!(body.size.height > px(600.));
    assert!(visual.debug_bounds("transcript").is_none());
    for id in [
        "skill-preview-action",
        "skill-copy-action",
        "skill-reveal-action",
    ] {
        assert_eq!(
            visual.debug_bounds(id).unwrap().size,
            size(px(20.), px(20.))
        );
    }
    let source = visual
        .debug_bounds("settings-skill-document-source")
        .unwrap();
    assert!(source.size.width < px(100.));
    visual.simulate_click(source.center(), Default::default());
    visual.run_until_parked();
    window
        .update(visual, |view, _, _| {
            assert_eq!(view.prefs.skill_source_paths.len(), 1)
        })
        .unwrap();
    visual.simulate_resize(size(px(700.), px(800.)));
    visual.run_until_parked();
    // Deliver the frame that applies the measured Skills container breakpoint.
    visual.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    visual.run_until_parked();
    let left = visual.debug_bounds("skills-list-scroll").unwrap();
    let right = visual.debug_bounds("skill-preview-pane").unwrap();
    assert!(
        right.top() >= left.bottom(),
        "list={left:?}, preview={right:?}"
    );
    assert_eq!(left.size.width, px(700.));
    assert_eq!(right.size.width, px(700.));
    assert!(visual.debug_bounds("skill-document").unwrap().size.height > px(200.));
}

#[gpui::test]
fn mono_mascot_is_beside_name_and_sidebar_switch_stays_visible(cx: &mut TestAppContext) {
    let window = window(Section::Monos, cx);
    window
        .update(cx, |view, _, cx| {
            view.ctx.monos = vec![MonoSetting {
                id: "layout-mono".into(),
                name: "Big Boss".into(),
                mascot: "ghost".into(),
                color: 0xee99bb,
                projects: vec!["monocode".into(), "mono-web".into()],
                show: false,
                folders: false,
            }];
            cx.notify();
        })
        .unwrap();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    let mascot = visual.debug_bounds("settings-mono-mascot").unwrap();
    let switch = visual
        .debug_bounds("settings-toggle-mono-sidebar-show")
        .unwrap();
    let reset = visual.debug_bounds("settings-mono-reset").unwrap();
    assert_eq!(mascot.size, size(px(16.), px(16.)));
    assert_eq!(switch.size, size(px(36.), px(20.)));
    let name = visual.debug_bounds("settings-mono-name").unwrap();
    assert!(mascot.right() < name.left());
    assert!(mascot.left() < px(200.));
    assert!(switch.left() > px(800.));
    assert!(switch.right() <= reset.left());
    assert!(reset.right() <= px(1200.));
    assert!((mascot.top() - switch.top()).abs() < px(4.));
    assert!(
        visual
            .debug_bounds("settings-toggle-mono-sidebar-folders")
            .is_none()
    );
    visual.simulate_click(reset.center(), Default::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("settings-management-dialog").is_some());
    window
        .update(visual, |view, window, cx| {
            view.close_management(window, cx);
            cx.notify();
        })
        .unwrap();
    visual.simulate_resize(size(px(700.), px(800.)));
    visual.run_until_parked();
    let switch = visual
        .debug_bounds("settings-toggle-mono-sidebar-show")
        .unwrap();
    assert_eq!(switch.size, size(px(36.), px(20.)));
    assert!(switch.right() <= px(700.));
}
