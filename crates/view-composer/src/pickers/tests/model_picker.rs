//! Port of src/features/sessions/ui/ModelPicker.test.ts.

use std::rc::Rc;

use gpui::{
    AnyView, AppContext as _, Entity, Focusable as _, TestAppContext, VisualTestContext, px,
};
use monocode_core::models::{
    AgentModel, ModelCatalog, ModelPickerTab, ModelPrefs, ModelProvider, ModelSetting,
    ModelSettingChoice, ModelSettingKind,
};
use monocode_core::{HarnessId, ModelSettings, ProjectProviders};
use monocode_ui::IconName;

use super::{Calls, Host, bounds, click, draw, exists, hover, init, keys, right_click};
use crate::pickers::model_logic::{self, ControlPill, model_groups, provenance};
use crate::pickers::model_pills::select_pill_icon;
use crate::pickers::model_source::{LocalModelSource, all_available};
use crate::pickers::{MenuEntry, ModelControlPills, ModelPicker, ModelPickerProps, Submenu};

struct Fixture {
    picker: Entity<ModelPicker>,
    pills: Option<Entity<ModelControlPills>>,
    changes: Calls<(HarnessId, String)>,
    settings: Calls<ModelSettings>,
    favorites: Calls<Vec<String>>,
}

fn values(pairs: &[(&str, &str)]) -> ModelSettings {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn choices(pairs: &[(&str, &str)]) -> Vec<ModelSettingChoice> {
    pairs
        .iter()
        .map(|(value, label)| ModelSettingChoice {
            value: (*value).into(),
            label: (*label).into(),
        })
        .collect()
}

fn setting(
    id: &str,
    label: &str,
    kind: ModelSettingKind,
    value: &str,
    options: &[(&str, &str)],
) -> ModelSetting {
    ModelSetting {
        id: id.into(),
        label: label.into(),
        kind,
        value: value.into(),
        options: choices(options),
        description: None,
    }
}

fn fast() -> ModelSetting {
    setting(
        "fast",
        "Fast",
        ModelSettingKind::Toggle,
        "false",
        &[("false", "Off"), ("true", "On")],
    )
}

fn model(id: &str, harness: HarnessId, name: &str, native: &str) -> AgentModel {
    AgentModel::new(id, harness, name).with_native_id(native)
}

fn with_provider(mut model: AgentModel, id: &str, name: &str) -> AgentModel {
    model.provider = Some(ModelProvider {
        id: id.into(),
        name: name.into(),
    });
    model
}

fn with_settings(mut model: AgentModel, settings: Vec<ModelSetting>) -> AgentModel {
    model.settings = Some(settings);
    model
}

/// Mounts a picker (and the toolbar pills, when `pills`) in a test window.
fn mount(
    cx: &mut TestAppContext,
    props: ModelPickerProps,
    catalog: ModelCatalog,
    prefs: ModelPrefs,
    pills: bool,
) -> (Fixture, &mut VisualTestContext) {
    init(cx);
    let source: Rc<dyn crate::pickers::ModelSource> =
        Rc::new(LocalModelSource::new(catalog, all_available()));
    let changes = Calls::new();
    let settings = Calls::new();
    let favorites = Calls::new();
    let (on_change, on_settings, on_favorites, on_pill_settings) = (
        changes.recorder(),
        settings.recorder(),
        favorites.recorder(),
        settings.recorder(),
    );
    let (host, cx) = cx.add_window_view(|window, cx| {
        let harness = props.harness.unwrap_or(HarnessId::Claude);
        let (model_id, model_values) = (props.model.clone(), props.values.clone());
        let picker_source = source.clone();
        let picker = cx.new(|cx| {
            ModelPicker::new(
                props,
                picker_source,
                prefs,
                ProjectProviders::default(),
                window,
                cx,
            )
            .on_change(move |harness, id, _, _| on_change((harness, id.to_string())))
            .on_settings_change(move |next, _, _| on_settings(next.clone()))
            .on_favorites_change(move |next, _, _| on_favorites(next.to_vec()))
        });
        let mut children: Vec<AnyView> = vec![picker.into()];
        if pills {
            let pills = cx.new(|cx| {
                ModelControlPills::new(harness, model_id, model_values, source, cx)
                    .on_settings_change(move |next, _, _| on_pill_settings(next.clone()))
            });
            children.push(pills.into());
        }
        Host { children }
    });
    let children = host.read_with(cx, |host, _| host.children.clone());
    let picker = children[0].clone().downcast::<ModelPicker>().unwrap();
    let pills = children
        .get(1)
        .map(|view| view.clone().downcast::<ModelControlPills>().unwrap());
    draw(cx);
    (
        Fixture {
            picker,
            pills,
            changes,
            settings,
            favorites,
        },
        cx,
    )
}

fn props(harness: HarnessId, model: &str, pairs: &[(&str, &str)]) -> ModelPickerProps {
    ModelPickerProps {
        harness: Some(harness),
        model: model.into(),
        values: values(pairs),
        ..Default::default()
    }
}

fn beside(mut props: ModelPickerProps) -> ModelPickerProps {
    props.hide_settings = true;
    props
}

fn option_names(f: &Fixture, cx: &mut VisualTestContext) -> Vec<String> {
    f.picker
        .read_with(cx, |picker, _| picker.visible_models())
        .into_iter()
        .map(|item| item.name)
        .collect()
}

#[gpui::test]
fn shows_the_model_name_and_effort_in_the_combined_picker(cx: &mut TestAppContext) {
    let (f, cx) = mount(
        cx,
        props(HarnessId::Grok, "grok:grok-4.6", &[("effort", "high")]),
        ModelCatalog::new(),
        ModelPrefs::default(),
        false,
    );
    let (label, effort) = f.picker.read_with(cx, |picker, _| {
        let current = picker.current();
        let effort = model_logic::trigger_effort_label(&current, &picker.props.values, false);
        (
            model_logic::trigger_label(&current, effort.as_deref()),
            effort,
        )
    });
    assert_eq!(label, "Grok Build Grok 4.6, effort High");
    assert_eq!(effort.as_deref(), Some("High"));

    click(cx, "model-picker-trigger");
    assert!(exists(cx, "model-menu"));
    assert!(exists(cx, "model-menu-model"));
    let entries = f.picker.read_with(cx, |picker, _| picker.entries());
    assert!(matches!(&entries[0], MenuEntry::Setting(s) if s.id == "effort"));
    assert_eq!(entries[1], MenuEntry::Model);

    hover(cx, "model-menu-model");
    assert_eq!(bounds(cx, "model-flyout").size.height, px(440.));
    assert!(exists(cx, "model-tab-favorites"));
    assert!(exists(cx, "model-tab-antigravity"));
    hover(cx, "model-tab-grok");
    assert_eq!(
        f.picker.read_with(cx, |picker, _| picker.visible_tab()),
        ModelPickerTab::Harness(HarnessId::Grok)
    );
    assert!(exists(cx, "model-option-Grok 4.6"));
    assert!(exists(cx, "model-favorite-Grok 4.6"));

    hover(cx, "model-menu-effort");
    assert!(matches!(
        f.picker.read_with(cx, |picker, _| picker.submenu().cloned()),
        Some(Submenu::Setting(s)) if s.id == "effort"
    ));
    click(cx, "model-setting-option-Extra High");
    assert_eq!(f.settings.all(), vec![values(&[("effort", "xhigh")])]);
    assert_eq!(f.changes.len(), 0);
}

#[gpui::test]
fn lists_claude_opus_5_5_after_opus_5_in_the_built_in_claude_catalog(cx: &mut TestAppContext) {
    let (f, cx) = mount(
        cx,
        props(HarnessId::Claude, "claude:opus-5-5", &[]),
        ModelCatalog::new(),
        ModelPrefs::default(),
        false,
    );
    let name = f.picker.read_with(cx, |picker, _| picker.current().name);
    assert_eq!(name, "Claude Opus 5.5");
    click(cx, "model-picker-trigger");
    hover(cx, "model-menu-model");
    let options = option_names(&f, cx);
    let opus = options
        .iter()
        .position(|name| name == "Claude Opus 5")
        .unwrap();
    assert_eq!(options[opus + 1], "Claude Opus 5.5");
}

fn opencode_catalog() -> ModelCatalog {
    let mut catalog = ModelCatalog::new();
    catalog.set_harness_models(
        HarnessId::Opencode,
        vec![
            with_provider(
                model(
                    "opencode:opencode-go/gpt-5.6-luna",
                    HarnessId::Opencode,
                    "GPT-5.6 Luna",
                    "opencode-go/gpt-5.6-luna",
                ),
                "opencode-go",
                "OpenCode Go",
            ),
            with_provider(
                model(
                    "opencode:openai/gpt-5.6-luna",
                    HarnessId::Opencode,
                    "GPT-5.6 Luna",
                    "openai/gpt-5.6-luna",
                ),
                "openai",
                "OpenAI",
            ),
            with_provider(
                model(
                    "opencode:openai/gpt-5.6-luna-fast",
                    HarnessId::Opencode,
                    "GPT-5.6 Luna Fast",
                    "openai/gpt-5.6-luna-fast",
                ),
                "openai",
                "OpenAI",
            ),
        ],
    );
    catalog
}

#[gpui::test]
fn groups_opencode_models_by_provider_and_searches_provider_names(cx: &mut TestAppContext) {
    let (f, cx) = mount(
        cx,
        props(
            HarnessId::Opencode,
            "opencode:opencode-go/gpt-5.6-luna",
            &[],
        ),
        opencode_catalog(),
        ModelPrefs::default(),
        false,
    );
    let label = f.picker.read_with(cx, |picker, _| {
        model_logic::trigger_label(&picker.current(), None)
    });
    assert_eq!(label, "OpenCode, OpenCode Go, GPT-5.6 Luna");
    click(cx, "model-picker-trigger");
    hover(cx, "model-menu-model");
    let groups = |f: &Fixture, cx: &mut VisualTestContext| {
        f.picker.read_with(cx, |picker, _| {
            model_groups(picker.visible_tab(), &picker.visible_models())
                .into_iter()
                .map(|group| (group.name.unwrap_or_default(), group.models.len()))
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(
        groups(&f, cx),
        vec![("OpenCode Go".to_string(), 1), ("OpenAI".to_string(), 2)]
    );

    click(cx, "model-search");
    cx.simulate_input("OpenAI");
    draw(cx);
    assert_eq!(groups(&f, cx), vec![("OpenAI".to_string(), 2)]);
    assert!(f.picker.read_with(cx, |picker, _| picker.is_open()));
}

#[gpui::test]
fn names_the_source_of_same_name_favorites_from_different_providers(cx: &mut TestAppContext) {
    let mut catalog = ModelCatalog::new();
    catalog.set_harness_models(
        HarnessId::Cursor,
        vec![
            model("cursor:auto", HarnessId::Cursor, "Auto", "auto"),
            model(
                "cursor:muse-spark-1.3",
                HarnessId::Cursor,
                "Muse Spark 1.3",
                "muse-spark-1.3",
            ),
        ],
    );
    catalog.set_harness_models(
        HarnessId::Opencode,
        vec![with_provider(
            model(
                "opencode:opencode-go/muse-spark-1.3",
                HarnessId::Opencode,
                "Muse Spark 1.3",
                "opencode-go/muse-spark-1.3",
            ),
            "opencode-go",
            "OpenCode Go",
        )],
    );
    let prefs = ModelPrefs {
        favorite_models: vec![
            "cursor:auto".into(),
            "cursor:muse-spark-1.3".into(),
            "opencode:opencode-go/muse-spark-1.3".into(),
        ],
        ..Default::default()
    };
    let (f, cx) = mount(
        cx,
        props(HarnessId::Cursor, "cursor:auto", &[]),
        catalog,
        prefs,
        false,
    );
    click(cx, "model-picker-trigger");
    hover(cx, "model-menu-model");
    click(cx, "model-tab-favorites");
    let labels: Vec<String> = f.picker.read_with(cx, |picker, _| {
        picker
            .visible_models()
            .iter()
            .map(|item| format!("{}, {}", item.name, provenance(item)))
            .collect()
    });
    assert_eq!(
        labels,
        vec![
            "Auto, Cursor",
            "Muse Spark 1.3, Cursor",
            "Muse Spark 1.3, OpenCode Go",
        ]
    );
}

#[gpui::test]
fn can_move_effort_into_a_dedicated_composer_control(cx: &mut TestAppContext) {
    let (f, cx) = mount(
        cx,
        beside(props(
            HarnessId::Grok,
            "grok:grok-4.6",
            &[("effort", "high")],
        )),
        ModelCatalog::new(),
        ModelPrefs::default(),
        true,
    );
    let label = f.picker.read_with(cx, |picker, _| {
        let current = picker.current();
        let effort = model_logic::trigger_effort_label(&current, &picker.props.values, true);
        model_logic::trigger_label(&current, effort.as_deref())
    });
    assert_eq!(label, "Grok Build Grok 4.6");
    click(cx, "model-picker-trigger");
    assert_eq!(
        f.picker.read_with(cx, |picker, _| picker.entries()),
        vec![MenuEntry::Model]
    );

    let pills = f.pills.clone().unwrap();
    assert!(exists(cx, "model-pill-effort"));
    click(cx, "model-pill-effort");
    assert_eq!(
        pills.read_with(cx, |pills, _| pills.open_menu_label()),
        Some("Effort".to_string())
    );
    keys(cx, "up enter");
    assert_eq!(f.settings.last(), Some(values(&[("effort", "xhigh")])));
}

fn composer_catalog() -> ModelCatalog {
    let mut catalog = ModelCatalog::new();
    catalog.set_harness_models(
        HarnessId::Cursor,
        vec![with_settings(
            model(
                "cursor:composer-2.5",
                HarnessId::Cursor,
                "Composer 2.5",
                "composer-2.5",
            ),
            vec![fast()],
        )],
    );
    catalog
}

#[gpui::test]
fn opens_the_model_list_with_no_intermediate_menu_when_settings_live_beside_the_picker(
    cx: &mut TestAppContext,
) {
    let (f, cx) = mount(
        cx,
        beside(props(
            HarnessId::Cursor,
            "cursor:composer-2.5",
            &[("fast", "false")],
        )),
        composer_catalog(),
        ModelPrefs::default(),
        true,
    );
    click(cx, "model-picker-trigger");
    assert!(!exists(cx, "model-menu"));
    assert!(exists(cx, "model-flyout"));

    let pills = f.pills.clone().unwrap();
    let pill_kinds = pills.read_with(cx, |pills, _| pills.pills());
    assert!(matches!(&pill_kinds[..], [ControlPill::Toggle(s)] if s.id == "fast"));
    click(cx, "model-pill-fast");
    assert_eq!(f.settings.last(), Some(values(&[("fast", "true")])));
}

#[gpui::test]
fn renders_the_opencode_variant_as_a_beside_picker_pill(cx: &mut TestAppContext) {
    let mut catalog = ModelCatalog::new();
    catalog.set_harness_models(
        HarnessId::Opencode,
        vec![with_settings(
            with_provider(
                model(
                    "opencode:some-cloud/spark-1",
                    HarnessId::Opencode,
                    "Spark 1",
                    "some-cloud/spark-1",
                ),
                "some-cloud",
                "Some Cloud",
            ),
            vec![setting(
                "variant",
                "Variant",
                ModelSettingKind::Select,
                "high",
                &[
                    ("low", "Low"),
                    ("medium", "Medium"),
                    ("high", "High"),
                    ("xhigh", "Extra High"),
                ],
            )],
        )],
    );
    let (f, cx) = mount(
        cx,
        beside(props(
            HarnessId::Opencode,
            "opencode:some-cloud/spark-1",
            &[("variant", "high")],
        )),
        catalog,
        ModelPrefs::default(),
        true,
    );
    let pills = f.pills.clone().unwrap();
    click(cx, "model-pill-variant");
    assert_eq!(
        pills.read_with(cx, |pills, _| pills.open_menu_label()),
        Some("Variant".to_string())
    );
    assert!(exists(cx, "model-pill-option-variant-High"));
}

#[test]
fn shows_an_effort_icon_for_pi_and_omp_thinking_levels() {
    for harness in [HarnessId::Pi, HarnessId::Omp] {
        let id = format!("{harness}:gpt-5.4-mini");
        let model = with_settings(
            model(&id, harness, "GPT-5.4 mini", "gpt-5.4-mini"),
            vec![setting(
                "thinking",
                "Thinking",
                ModelSettingKind::Select,
                "xhigh",
                &[("high", "High"), ("xhigh", "Extra High")],
            )],
        );
        let pills = model_logic::control_pills(&model);
        let [ControlPill::Select { setting, .. }] = &pills[..] else {
            panic!("one select pill for {harness}");
        };
        assert_eq!(
            model_logic::setting_value_label(setting, &values(&[("thinking", "xhigh")])),
            "Extra High"
        );
        assert_eq!(select_pill_icon(setting), Some(IconName::Gauge));
    }
}

#[test]
fn shows_a_speed_icon_on_the_service_tier_pill() {
    let model = with_settings(
        model(
            "codex:gpt-5.6-luna",
            HarnessId::Codex,
            "GPT-5.6 Luna",
            "gpt-5.6-luna",
        ),
        vec![setting(
            "serviceTier",
            "Service Tier",
            ModelSettingKind::Select,
            "default",
            &[("default", "Standard"), ("fast", "Fast")],
        )],
    );
    let pills = model_logic::control_pills(&model);
    let [ControlPill::Select { setting, grouped }] = &pills[..] else {
        panic!("one select pill");
    };
    assert!(grouped.is_empty());
    assert_eq!(select_pill_icon(setting), Some(IconName::Zap));
}

#[gpui::test]
fn groups_the_service_tier_inside_the_effort_popover(cx: &mut TestAppContext) {
    let mut catalog = ModelCatalog::new();
    catalog.set_harness_models(
        HarnessId::Codex,
        vec![with_settings(
            model(
                "codex:gpt-5.6-sol",
                HarnessId::Codex,
                "GPT-5.6 Sol",
                "gpt-5.6-sol",
            ),
            vec![
                setting(
                    "reasoningEffort",
                    "Reasoning",
                    ModelSettingKind::Select,
                    "high",
                    &[("high", "High"), ("xhigh", "Extra High")],
                ),
                setting(
                    "serviceTier",
                    "Service Tier",
                    ModelSettingKind::Select,
                    "default",
                    &[("default", "Standard"), ("fast", "Fast")],
                ),
            ],
        )],
    );
    let (f, cx) = mount(
        cx,
        beside(props(
            HarnessId::Codex,
            "codex:gpt-5.6-sol",
            &[("reasoningEffort", "high"), ("serviceTier", "default")],
        )),
        catalog,
        ModelPrefs::default(),
        true,
    );
    assert!(!exists(cx, "model-pill-serviceTier"));
    click(cx, "model-pill-reasoningEffort");
    let pills = f.pills.clone().unwrap();
    assert_eq!(
        pills.read_with(cx, |pills, _| pills.open_menu_label()),
        Some("Reasoning and Service Tier".to_string())
    );
    assert!(exists(cx, "model-pill-option-serviceTier-Standard"));
    click(cx, "model-pill-option-serviceTier-Fast");
    assert_eq!(
        f.settings.last(),
        Some(values(&[
            ("reasoningEffort", "high"),
            ("serviceTier", "fast")
        ]))
    );
}

#[gpui::test]
fn groups_fast_mode_inside_the_effort_popover(cx: &mut TestAppContext) {
    let mut catalog = ModelCatalog::new();
    catalog.set_harness_models(
        HarnessId::Claude,
        vec![with_settings(
            model(
                "claude:opus-5",
                HarnessId::Claude,
                "Opus 5",
                "claude-opus-5",
            ),
            vec![
                setting(
                    "effort",
                    "Reasoning",
                    ModelSettingKind::Select,
                    "high",
                    &[("medium", "Medium"), ("high", "High")],
                ),
                fast(),
            ],
        )],
    );
    let (f, cx) = mount(
        cx,
        beside(props(
            HarnessId::Claude,
            "claude:opus-5",
            &[("effort", "high"), ("fast", "false")],
        )),
        catalog,
        ModelPrefs::default(),
        true,
    );
    assert!(!exists(cx, "model-pill-fast"));
    click(cx, "model-pill-effort");
    let pills = f.pills.clone().unwrap();
    assert_eq!(
        pills.read_with(cx, |pills, _| pills.open_menu_label()),
        Some("Effort and Fast".to_string())
    );
    assert!(exists(cx, "model-pill-option-fast-Off"));
    click(cx, "model-pill-option-fast-On");
    assert_eq!(
        f.settings.last(),
        Some(values(&[("effort", "high"), ("fast", "true")]))
    );
}

#[gpui::test]
fn opens_the_model_list_directly_when_settings_live_beside_the_picker(cx: &mut TestAppContext) {
    let (f, cx) = mount(
        cx,
        beside(props(
            HarnessId::Cursor,
            "cursor:composer-2.5",
            &[("fast", "false")],
        )),
        composer_catalog(),
        ModelPrefs::default(),
        false,
    );
    click(cx, "model-picker-trigger");
    assert!(!exists(cx, "model-menu"));
    assert!(exists(cx, "model-option-Composer 2.5"));
    // Search takes focus once the flyout is on screen.
    assert!(search_focused(&f, cx));
    click(cx, "model-option-Composer 2.5");
    assert_eq!(
        f.changes.all(),
        vec![(HarnessId::Cursor, "cursor:composer-2.5".to_string())]
    );
    assert!(!f.picker.read_with(cx, |picker, _| picker.is_open()));
}

fn first_second() -> ModelCatalog {
    let mut catalog = ModelCatalog::new();
    catalog.set_harness_models(
        HarnessId::Cursor,
        vec![
            model("cursor:first", HarnessId::Cursor, "First", "first"),
            model("cursor:second", HarnessId::Cursor, "Second", "second"),
        ],
    );
    catalog
}

#[gpui::test]
fn picks_the_highlighted_model_on_enter(cx: &mut TestAppContext) {
    let (f, cx) = mount(
        cx,
        beside(props(HarnessId::Cursor, "cursor:first", &[])),
        first_second(),
        ModelPrefs::default(),
        false,
    );
    click(cx, "model-picker-trigger");
    assert_eq!(option_names(&f, cx), vec!["First", "Second"]);
    keys(cx, "down enter");
    assert_eq!(
        f.changes.all(),
        vec![(HarnessId::Cursor, "cursor:second".to_string())]
    );
}

#[gpui::test]
fn the_favorite_toggle_does_not_pick_the_model(cx: &mut TestAppContext) {
    let (f, cx) = mount(
        cx,
        beside(props(HarnessId::Cursor, "cursor:first", &[])),
        first_second(),
        ModelPrefs::default(),
        false,
    );
    click(cx, "model-picker-trigger");
    click(cx, "model-favorite-Second");
    assert_eq!(f.favorites.all(), vec![vec!["cursor:second".to_string()]]);
    assert_eq!(f.changes.len(), 0);
    assert!(f.picker.read_with(cx, |picker, _| picker.is_open()));
}

#[gpui::test]
fn returns_to_the_selected_models_harness_when_reopened(cx: &mut TestAppContext) {
    let (f, cx) = mount(
        cx,
        props(HarnessId::Grok, "grok:grok-4.6", &[("effort", "high")]),
        ModelCatalog::new(),
        ModelPrefs::default(),
        false,
    );
    click(cx, "model-picker-trigger");
    hover(cx, "model-menu-model");
    hover(cx, "model-tab-opencode");
    assert_eq!(
        f.picker.read_with(cx, |picker, _| picker.visible_tab()),
        ModelPickerTab::Harness(HarnessId::Opencode)
    );
    click(cx, "model-picker-trigger");
    assert!(!f.picker.read_with(cx, |picker, _| picker.is_open()));
    click(cx, "model-picker-trigger");
    hover(cx, "model-menu-model");
    assert_eq!(
        f.picker.read_with(cx, |picker, _| picker.visible_tab()),
        ModelPickerTab::Harness(HarnessId::Grok)
    );
}

#[gpui::test]
fn quick_switches_between_recently_used_models_on_right_click(cx: &mut TestAppContext) {
    let mut prefs = ModelPrefs::default();
    prefs.save_recent_model_choice(HarnessId::Claude, "claude:opus-5");
    prefs.save_recent_model_choice(HarnessId::Cursor, "cursor:composer-2.5");
    let mut picker_props = props(HarnessId::Grok, "grok:grok-4.6", &[("effort", "high")]);
    picker_props.hotkeys = true;
    let (f, cx) = mount(cx, picker_props, ModelCatalog::new(), prefs, false);

    right_click(cx, "model-picker-trigger");
    assert!(exists(cx, "model-recent-menu"));
    let names: Vec<String> = f.picker.read_with(cx, |picker, _| {
        picker
            .recent_models()
            .unwrap()
            .iter()
            .map(|item| item.name.clone())
            .collect()
    });
    assert_eq!(names, vec!["Composer 2.5", "Claude Opus 5", "Grok 4.6"]);
    let active =
        |f: &Fixture, cx: &mut VisualTestContext| f.picker.read_with(cx, |p, _| p.recent_active());
    assert_eq!(active(&f, cx), 2);
    keys(cx, "up");
    assert_eq!(active(&f, cx), 1);
    keys(cx, "down");
    assert_eq!(active(&f, cx), 2);
    keys(cx, "down");
    assert_eq!(active(&f, cx), 0);
    keys(cx, "enter");
    assert_eq!(
        f.changes.all(),
        vec![(HarnessId::Cursor, "cursor:composer-2.5".to_string())]
    );
    assert!(!exists(cx, "model-recent-menu"));

    #[cfg(target_os = "macos")]
    keys(cx, "cmd-.");
    #[cfg(not(target_os = "macos"))]
    keys(cx, "ctrl-.");
    assert!(exists(cx, "model-recent-menu"));
    assert!(!exists(cx, "model-menu"));
}

#[gpui::test]
fn escape_closes_the_menu_and_restores_focus(cx: &mut TestAppContext) {
    let (f, cx) = mount(
        cx,
        props(HarnessId::Grok, "grok:grok-4.6", &[("effort", "high")]),
        ModelCatalog::new(),
        ModelPrefs::default(),
        false,
    );
    click(cx, "model-picker-trigger");
    keys(cx, "down");
    assert_eq!(f.picker.read_with(cx, |p, _| p.active_entry()), 1);
    keys(cx, "right");
    assert_eq!(
        f.picker.read_with(cx, |p, _| p.submenu().cloned()),
        Some(Submenu::Models)
    );
    keys(cx, "left escape");
    assert!(!f.picker.read_with(cx, |p, _| p.is_open()));
}

fn search_focused(f: &Fixture, cx: &mut VisualTestContext) -> bool {
    let search = f.picker.read_with(cx, |picker, _| picker.search.clone());
    cx.update(|window, cx| search.read(cx).focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn focuses_the_model_search_when_the_models_submenu_opens(cx: &mut TestAppContext) {
    let (f, cx) = mount(
        cx,
        props(HarnessId::Cursor, "cursor:composer-2.5", &[]),
        composer_catalog(),
        ModelPrefs::default(),
        false,
    );
    click(cx, "model-picker-trigger");
    assert!(!search_focused(&f, cx));
    hover(cx, "model-menu-model");
    assert!(exists(cx, "model-search"));
    assert!(search_focused(&f, cx));
}

fn codex_efforts() -> ModelCatalog {
    let mut catalog = ModelCatalog::new();
    catalog.set_harness_models(
        HarnessId::Codex,
        vec![with_settings(
            model("codex:gpt-5.6", HarnessId::Codex, "GPT-5.6", "gpt-5.6"),
            vec![setting(
                "reasoningEffort",
                "Reasoning",
                ModelSettingKind::Select,
                "high",
                &[
                    ("low", "Low"),
                    ("high", "High"),
                    ("max", "Max"),
                    ("ultra", "Ultra"),
                ],
            )],
        )],
    );
    catalog
}

#[gpui::test]
fn shimmers_only_codex_max_and_ultra_effort_options(cx: &mut TestAppContext) {
    let (_f, cx) = mount(
        cx,
        props(
            HarnessId::Codex,
            "codex:gpt-5.6",
            &[("reasoningEffort", "high")],
        ),
        codex_efforts(),
        ModelPrefs::default(),
        false,
    );
    click(cx, "model-picker-trigger");
    hover(cx, "model-menu-reasoningEffort");
    hover(cx, "model-setting-option-High");
    assert!(!exists(cx, "model-effort-reasoningEffort-high-tiles"));
    hover(cx, "model-setting-option-Max");
    assert!(exists(cx, "model-effort-reasoningEffort-max-tiles"));
    assert!(!exists(cx, "model-effort-reasoningEffort-ultra-tiles"));
    hover(cx, "model-setting-option-Ultra");
    assert!(exists(cx, "model-effort-reasoningEffort-ultra-tiles"));
    assert!(!exists(cx, "model-effort-reasoningEffort-max-tiles"));
}

#[gpui::test]
fn shimmers_the_max_effort_in_the_beside_picker_pill(cx: &mut TestAppContext) {
    let (_f, cx) = mount(
        cx,
        beside(props(
            HarnessId::Codex,
            "codex:gpt-5.6",
            &[("reasoningEffort", "high")],
        )),
        codex_efforts(),
        ModelPrefs::default(),
        true,
    );
    click(cx, "model-pill-reasoningEffort");
    hover(cx, "model-pill-option-reasoningEffort-Max");
    assert!(exists(cx, "model-pill-effort-reasoningEffort-max-tiles"));
    hover(cx, "model-pill-option-reasoningEffort-Low");
    assert!(!exists(cx, "model-pill-effort-reasoningEffort-max-tiles"));
    assert!(!exists(cx, "model-pill-effort-reasoningEffort-low-tiles"));
}

#[gpui::test]
fn keeps_a_long_model_name_whole_on_the_trigger(cx: &mut TestAppContext) {
    let mut catalog = ModelCatalog::new();
    catalog.set_harness_models(
        HarnessId::Cursor,
        vec![model(
            "cursor:long",
            HarnessId::Cursor,
            "Composer 2.5 Extended Thinking Preview",
            "long",
        )],
    );
    let (_f, cx) = mount(
        cx,
        beside(props(HarnessId::Cursor, "cursor:long", &[])),
        catalog,
        ModelPrefs::default(),
        false,
    );
    // The old `max-w-40` cap truncated this name at 160px.
    assert!(bounds(cx, "model-picker-trigger").size.width > px(200.));
}
