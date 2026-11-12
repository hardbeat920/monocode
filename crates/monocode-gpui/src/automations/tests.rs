//! Trigger filter editing, plus screenshots of it with the Metal renderer:
//! `TRIGGER_FILTER_SHOTS=/tmp/shots cargo test … trigger_filter_shots -- --ignored`
use super::*;
use gpui::{HeadlessAppContext, TestAppContext, size};

const PROJECT: &str = "/Users/you/Dev/acme-app";

fn filter(field: &str, op: &str, values: &[&str]) -> TriggerFilter {
    TriggerFilter {
        field: field.into(),
        op: op.into(),
        values: values.iter().map(|value| value.to_string()).collect(),
    }
}

#[test]
#[ignore = "Requires a Metal device; writes screenshots"]
fn trigger_filter_shots() {
    let dir = std::env::var("TRIGGER_FILTER_SHOTS")
        .unwrap_or_else(|_| "/tmp/trigger-filter-shots".into());
    std::fs::create_dir_all(&dir).unwrap();
    let database = std::path::Path::new(&dir).join("automations.sqlite");
    let _ = std::fs::remove_file(&database);
    let service = Arc::new(Mutex::new(Service::open(&database).unwrap()));
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(gpui_apple::AppleTextSystem::new()),
        Arc::new(crate::assets::Assets),
        || {
            Ok(Some(Box::new(
                gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
            )))
        },
    );
    cx.update(crate::popover::bind);
    let (width, height) = (980u32, 1100u32);
    let window = cx
        .open_window(size(px(width as f32), px(height as f32)), |_, cx| {
            cx.new(|cx| Automations::new(service, PROJECT.into(), vec![PROJECT.into()], cx))
        })
        .unwrap();
    let frame = |cx: &mut HeadlessAppContext| {
        for _ in 0..4 {
            std::thread::sleep(std::time::Duration::from_millis(120));
            cx.run_until_parked();
            cx.update_window(window.into(), |_, w, _| w.refresh())
                .unwrap();
            cx.run_until_parked();
        }
    };
    let show = |cx: &mut HeadlessAppContext, triggers: Vec<AutomationTrigger>| {
        window
            .update(cx, |view, _, cx| {
                view.set_trigger_suggestions(HashMap::from([
                    (
                        "github:labels".to_string(),
                        ["bug", "docs", "enhancement", "good first issue", "p1"]
                            .map(String::from)
                            .to_vec(),
                    ),
                    (
                        "github:repo".to_string(),
                        vec!["acme/app".into(), "acme/site".into()],
                    ),
                ]));
                let mut draft =
                    AutomationUpsert::new(PROJECT.into(), "claude".into(), String::new());
                draft.name = "Triage new bugs".into();
                draft.prompt =
                    "Reproduce the bug, find the cause and open a draft PR with a fix.".into();
                draft.triggers = Some(triggers);
                view.set_editor(draft, false, cx);
            })
            .unwrap();
    };
    // The Triggers section: from its heading to `bottom` points down the window.
    let shot = |cx: &mut HeadlessAppContext, name: &str, bottom: u32| {
        frame(cx);
        let image = cx.capture_screenshot(window.into()).unwrap();
        let mut flat = image::RgbaImage::from_pixel(
            image.width(),
            image.height(),
            image::Rgba([20, 20, 20, 255]),
        );
        image::imageops::overlay(&mut flat, &image, 0, 0);
        let scale = image.width() / width;
        let (x, y) = (296, 124);
        image::imageops::crop_imm(
            &flat,
            x * scale,
            y * scale,
            (width - 16 - x) * scale,
            (bottom - y) * scale,
        )
        .to_image()
        .save(format!("{dir}/{name}.png"))
        .unwrap();
    };

    show(
        &mut cx,
        vec![AutomationTrigger {
            actor: "people".into(),
            filters: vec![
                filter("labels", "any", &["bug", "p1"]),
                filter("assignee", "is", &[automations::UNASSIGNED]),
            ],
            ..AutomationTrigger::new("github", "issue_opened")
        }],
    );
    shot(&mut cx, "1-github-issue-filters", 410);

    show(
        &mut cx,
        vec![
            AutomationTrigger {
                repos: vec!["acme/app".into()],
                filters: vec![filter("labels", "any", &["good first issue"])],
                ..AutomationTrigger::new("github", "label_added")
            },
            AutomationTrigger {
                repos: vec!["ENG".into()],
                filters: vec![
                    filter("labels", "any", &["regression"]),
                    filter("status", "is_not", &["Done", "Canceled"]),
                    filter("title", "not_contains", &["[spike]"]),
                ],
                ..AutomationTrigger::new("linear", "issue_created")
            },
        ],
    );
    shot(&mut cx, "2-label-added-and-linear", 578);

    show(
        &mut cx,
        vec![AutomationTrigger {
            actor: "octocat, hubot".into(),
            ..AutomationTrigger::new("github", "pull_request_opened")
        }],
    );
    shot(&mut cx, "3-specific-people", 380);
    // Open "Add filter" with a real click; the point is its place in shot 3.
    if let Ok(point) = std::env::var("TRIGGER_FILTER_CLICK") {
        let (x, y) = point.split_once(',').unwrap();
        let position = gpui::point(px(x.parse().unwrap()), px(y.parse().unwrap()));
        for event in [
            gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                position,
                modifiers: Default::default(),
                button: gpui::MouseButton::Left,
                click_count: 1,
                first_mouse: false,
            }),
            gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                position,
                modifiers: Default::default(),
                button: gpui::MouseButton::Left,
                click_count: 1,
            }),
        ] {
            cx.update_window(window.into(), |_, w, cx| {
                w.dispatch_event(event, cx);
            })
            .unwrap();
            cx.run_until_parked();
        }
        shot(&mut cx, "4-add-filter-menu", 520);
    }
}

#[gpui::test]
fn editing_filters_and_actor_saves_them_on_the_trigger(cx: &mut TestAppContext) {
    let database =
        std::env::temp_dir().join(format!("trigger-filters-{}.sqlite", automations::new_id()));
    let service = Arc::new(Mutex::new(Service::open(&database).unwrap()));
    let view = cx.new(|cx| Automations::new(service, PROJECT.into(), vec![PROJECT.into()], cx));
    let saved = view.update(cx, |view, cx| {
        let mut draft = AutomationUpsert::new(PROJECT.into(), "claude".into(), String::new());
        let trigger = AutomationTrigger::new("github", "issue_opened");
        let id = trigger.id.clone();
        draft.triggers = Some(vec![trigger]);
        view.set_editor(draft, false, cx);

        view.apply_choice(Choice::AddFilter(id.clone()), "title".into(), cx);
        view.apply_choice(Choice::AddFilter(id.clone()), "labels".into(), cx);
        let fields = &view.editor.as_ref().unwrap().fields[0];
        fields.filters[0].update(cx, |input, cx| input.set_text("crash".into(), cx));
        view.apply_choice(Choice::FilterValue(id.clone(), 1), "bug".into(), cx);
        view.apply_choice(Choice::FilterValue(id.clone(), 1), "p1".into(), cx);
        view.apply_choice(Choice::FilterOp(id.clone(), 1), "none".into(), cx);
        // Typed text survives a structural edit that rebuilds the fields.
        view.apply_choice(Choice::AddFilter(id.clone()), "assignee".into(), cx);
        view.apply_choice(Choice::Actor(id.clone()), "custom".into(), cx);
        let fields = &view.editor.as_ref().unwrap().fields[0];
        fields
            .actor
            .update(cx, |input, cx| input.set_text("octocat, @hubot".into(), cx));
        view.collected(cx).unwrap().triggers.unwrap().remove(0)
    });
    assert_eq!(saved.actor, "octocat, @hubot");
    // The assignee filter has no values, so it is not saved.
    assert_eq!(
        saved.filters,
        vec![
            filter("title", "contains", &["crash"]),
            filter("labels", "none", &["bug", "p1"]),
        ]
    );
    let _ = std::fs::remove_file(database);
}
