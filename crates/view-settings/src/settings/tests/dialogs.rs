//! Ports of JiraSettings.test.ts and ProjectBackgroundDialog.test.ts.

use std::collections::HashMap;

use gpui::{AppContext as _, ParentElement as _, Render, Styled as _, TestAppContext, Window};
use monocode_core::Platform;
use monocode_core::appearance::{ChatBackgroundScope, NewThreadBackgroundEffect};
use monocode_core::settings::SettingsSectionId;

use super::*;
use crate::settings::jira::JiraSettings;
use crate::settings::project_background_dialog::ProjectBackgroundDialog;
use crate::settings::store::{JIRA_HIDDEN_PROJECTS_KEY, load_hidden_ids};
use monocode_settings::display_prefs::{MASK_EMAILS_KEY, save_mask_emails};

fn jira(page: &Entity<SettingsPage>, cx: &mut VisualTestContext) -> Entity<JiraSettings> {
    let SectionBody::Inbox(inbox) = body(page, cx) else {
        panic!("inbox section");
    };
    inbox.read_with(cx, |inbox, _| inbox.jira().clone())
}

fn type_into(
    cx: &mut VisualTestContext,
    input: &Entity<gpui_component::input::InputState>,
    value: &str,
) {
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.focus(window, cx);
        })
    });
    cx.simulate_input(value);
    draw(cx);
}

fn jira_setup() -> Setup {
    let setup = Setup::new(Platform::Linux);
    *setup.host.jira_projects.borrow_mut() = vec![JiraProject {
        id: "10000".into(),
        key: "ENG".into(),
        name: "Engineering".into(),
    }];
    setup
}

#[gpui::test]
fn connects_jira_synchronizes_project_filters_and_disconnects(cx: &mut TestAppContext) {
    let setup = jira_setup();
    save_mask_emails(&setup.kv, true);
    let (page, cx) = mount(cx, SettingsSectionId::Inbox, &setup);
    let jira = jira(&page, cx);
    let [site, email, token] = jira.read_with(cx, |jira, _| jira.fields().map(Clone::clone));
    type_into(cx, &site, "acme.atlassian.net");
    type_into(cx, &email, "ada@example.com");
    type_into(cx, &token, "secret");
    keys(cx, "enter");
    assert_eq!(
        setup.host.jira_saves.borrow().as_slice(),
        &[(
            "acme.atlassian.net".to_string(),
            "ada@example.com".to_string(),
            "secret".to_string()
        )]
    );
    // The email stays hidden until clicked.
    assert!(exists(cx, "email:Reveal email"));
    click(cx, "email:Reveal email");
    assert!(exists(cx, "email:Hide email"));
    click(cx, "email:Hide email");
    assert!(exists(cx, "email:Reveal email"));
    // Turning masking off in any window shows the email as plain text.
    setup.kv.set_item(MASK_EMAILS_KEY, "0");
    cx.run_until_parked();
    draw(cx);
    assert!(!exists(cx, "email:Reveal email"));
    assert!(exists(cx, "email-text:ada@example.com"));
    // The form is gone once connected.
    assert!(!exists(cx, "jira-form"));
    assert!(exists(cx, "jira-project:Engineering"));
    assert!(jira.read_with(cx, |jira, _| jira.hidden_ids().is_empty()));
    click(cx, "jira-project:Engineering");
    assert_eq!(
        load_hidden_ids(&setup.kv, JIRA_HIDDEN_PROJECTS_KEY),
        vec!["10000".to_string()]
    );
    assert_eq!(
        jira.read_with(cx, |jira, _| jira.hidden_ids().to_vec()),
        vec!["10000".to_string()]
    );

    click(cx, "button:jira-disconnect");
    assert_eq!(
        setup.host.jira_saves.borrow().last(),
        Some(&(String::new(), String::new(), String::new()))
    );
    assert!(exists(cx, "jira-form"));
    assert_eq!(
        token.read_with(cx, |token, _| token.value().to_string()),
        ""
    );
}

#[gpui::test]
fn shows_authentication_errors_without_claiming_a_successful_connection(cx: &mut TestAppContext) {
    let setup = jira_setup();
    setup
        .host
        .jira_failures
        .borrow_mut()
        .push_back("Jira email or API token is invalid".into());
    let (page, cx) = mount(cx, SettingsSectionId::Inbox, &setup);
    let jira = jira(&page, cx);
    let [site, email, token] = jira.read_with(cx, |jira, _| jira.fields().map(Clone::clone));
    type_into(cx, &site, "acme");
    type_into(cx, &email, "ada@example.com");
    type_into(cx, &token, "bad-token");
    click(cx, "button:jira-connect");
    assert!(
        jira.read_with(cx, |jira, _| jira.error().map(str::to_string))
            .unwrap()
            .contains("invalid")
    );
    assert!(exists(cx, "jira-alert"));
    assert!(exists(cx, "jira-form"));
    assert!(!exists(cx, "jira-projects"));
}

/// Stands in for the projects package's per-project settings.
#[derive(Default)]
struct FakeBackgrounds {
    settings: RefCell<HashMap<String, ProjectBackgroundSettings>>,
}

impl ProjectBackgroundHost for FakeBackgrounds {
    fn load_settings(&self, project: &str, _: &App) -> Option<ProjectBackgroundSettings> {
        self.settings.borrow().get(project).cloned()
    }

    fn save_settings(
        &self,
        project: &str,
        settings: &ProjectBackgroundSettings,
        _: bool,
        _: &mut App,
    ) {
        self.settings
            .borrow_mut()
            .insert(project.to_string(), settings.clone());
    }
}

struct DialogHost {
    dialog: Entity<ProjectBackgroundDialog>,
}

impl Render for DialogHost {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        gpui::div().size_full().child(self.dialog.clone())
    }
}

fn background(path: &str, effect: NewThreadBackgroundEffect) -> ProjectBackgroundSettings {
    ProjectBackgroundSettings {
        path: path.into(),
        empty_opacity: 0.2,
        session_opacity: 0.3,
        scope: ChatBackgroundScope::All,
        effect,
    }
}

fn open_dialog(
    cx: &mut TestAppContext,
    host: Rc<FakeBackgrounds>,
) -> (
    Entity<ProjectBackgroundDialog>,
    &'static mut VisualTestContext,
) {
    init(cx);
    let kv = Kv::in_memory();
    let window = cx.open_window(gpui::size(px(900.), px(800.)), move |_, cx| {
        let dialog =
            cx.new(|cx| ProjectBackgroundDialog::new("/work/alpha", "Alpha", kv, host, cx));
        DialogHost { dialog }
    });
    let root = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    draw(cx);
    let dialog = root.read_with(cx, |root, _| root.dialog.clone());
    (dialog, cx)
}

#[gpui::test]
fn changes_the_effect_only_for_the_selected_project(cx: &mut TestAppContext) {
    let host = Rc::new(FakeBackgrounds::default());
    host.save_settings_raw(
        "/work/alpha",
        background("/backgrounds/alpha.png", NewThreadBackgroundEffect::None),
    );
    host.save_settings_raw(
        "/work/beta",
        background("/backgrounds/beta.png", NewThreadBackgroundEffect::Ascii),
    );
    let (dialog, cx) = open_dialog(cx, host.clone());
    assert!(exists(cx, "project-background-dialog"));
    assert_eq!(
        NewThreadBackgroundEffect::None.description(),
        "Shows the original artwork."
    );
    let select = dialog.read_with(cx, |dialog, _| dialog.effect_select().clone());
    let label =
        |cx: &mut VisualTestContext| select.read_with(cx, |select, _| select.trigger_label());
    assert_eq!(label(cx), "Project background effect: None");
    // The effect sits under its heading, right of the description.
    let description = bounds(cx, "project-effect-description");
    let trigger = bounds(cx, "select:Project background effect");
    assert!(trigger.left() > description.left());

    click(cx, "select:Project background effect");
    click(cx, "option:Project background effect:Dither");
    assert_eq!(label(cx), "Project background effect: Dither");
    assert_eq!(
        host.settings.borrow()["/work/alpha"].effect,
        NewThreadBackgroundEffect::Dither
    );
    assert_eq!(
        host.settings.borrow()["/work/beta"].effect,
        NewThreadBackgroundEffect::Ascii
    );
}

#[gpui::test]
fn previews_a_project_haze_background_with_its_own_image_and_visibility(cx: &mut TestAppContext) {
    let host = Rc::new(FakeBackgrounds::default());
    host.save_settings_raw(
        "/work/alpha",
        ProjectBackgroundSettings {
            path: "/backgrounds/alpha.png".into(),
            empty_opacity: 0.35,
            session_opacity: 0.55,
            scope: ChatBackgroundScope::Empty,
            effect: NewThreadBackgroundEffect::GradientBlur,
        },
    );
    let (dialog, cx) = open_dialog(cx, host.clone());
    assert!(exists(cx, "gradient-blur-background"));
    dialog.read_with(cx, |dialog, _| {
        assert_eq!(dialog.empty_opacity(), 0.35);
        assert_eq!(dialog.path(), Some("/backgrounds/alpha.png"));
        assert_eq!(dialog.effect(), NewThreadBackgroundEffect::GradientBlur);
    });
    let select = dialog.read_with(cx, |dialog, _| dialog.effect_select().clone());
    assert_eq!(
        select.read_with(cx, |select, _| select.trigger_label()),
        "Project background effect: Haze"
    );
    assert_eq!(
        host.settings.borrow()["/work/alpha"].effect,
        NewThreadBackgroundEffect::GradientBlur
    );
}

#[gpui::test]
fn moves_visibility_and_scope_for_the_project(cx: &mut TestAppContext) {
    let host = Rc::new(FakeBackgrounds::default());
    host.save_settings_raw(
        "/work/alpha",
        background("/backgrounds/alpha.png", NewThreadBackgroundEffect::None),
    );
    let (_, cx) = open_dialog(cx, host.clone());
    click(cx, "radio:Show project background on:empty");
    assert_eq!(
        host.settings.borrow()["/work/alpha"].scope,
        ChatBackgroundScope::Empty
    );
    // Press at the right end of the empty-chat slider.
    let track = bounds(cx, "slider:Project background visibility in empty chats");
    cx.simulate_click(
        gpui::point(track.right() - px(1.), track.center().y),
        Modifiers::none(),
    );
    draw(cx);
    assert_eq!(host.settings.borrow()["/work/alpha"].empty_opacity, 0.65);
}

impl FakeBackgrounds {
    fn save_settings_raw(&self, project: &str, settings: ProjectBackgroundSettings) {
        self.settings
            .borrow_mut()
            .insert(project.to_string(), settings);
    }
}
