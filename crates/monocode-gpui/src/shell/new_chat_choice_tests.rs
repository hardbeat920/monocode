//! The new-chat resolver: each step of the order and hidden skipping.
use super::new_chat_choice::*;
use super::*;
use crate::settings::{ModelChoice, Preferences};

const LOCAL: &str = "/work/project";
const REMOTE: &str = "remote://env/work/project";

fn recent(provider: &str, model: Option<&str>) -> ModelChoice {
    ModelChoice {
        provider: provider.into(),
        model: model.map(Into::into),
    }
}

/// The project's own provider, models and hidden list.
fn project(
    prefs: &mut Preferences,
    cwd: &str,
    provider: Option<&str>,
    models: &[(&str, &str)],
    hidden: &[&str],
) {
    let p = prefs.project_providers.entry(cwd.into()).or_default();
    p.provider = provider.map(Into::into);
    p.models = models
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    p.hidden = hidden.iter().map(|s| s.to_string()).collect();
}

fn pick(prefs: &Preferences, cwd: &str) -> (Kind, Option<String>) {
    let c = resolve(prefs, cwd);
    (c.kind, c.model)
}

#[test]
fn nothing_set_starts_codex() {
    assert_eq!(pick(&Preferences::default(), LOCAL), (Kind::Codex, None));
}

#[test]
fn project_provider_beats_settings_and_recents() {
    let mut prefs = Preferences {
        default_provider: Some("claude".into()),
        default_models: [("grok".into(), "grok-default".into())].into(),
        recent_models: vec![recent("pi", Some("pi-1"))],
        ..Default::default()
    };
    project(
        &mut prefs,
        LOCAL,
        Some("grok"),
        &[("grok", "grok-project")],
        &[],
    );
    assert_eq!(
        pick(&prefs, LOCAL),
        (Kind::Grok, Some("grok-project".into()))
    );
    project(&mut prefs, LOCAL, Some("grok"), &[], &[]);
    assert_eq!(
        pick(&prefs, LOCAL),
        (Kind::Grok, Some("grok-default".into()))
    );
}

#[test]
fn settings_provider_beats_recents_and_takes_its_default_model() {
    let prefs = Preferences {
        default_provider: Some("claude".into()),
        default_models: [("claude".into(), "opus".into())].into(),
        recent_models: vec![recent("pi", Some("pi-1"))],
        ..Default::default()
    };
    assert_eq!(pick(&prefs, LOCAL), (Kind::Claude, Some("opus".into())));
}

#[test]
fn last_choice_is_used_and_defaults_win_over_its_model() {
    let mut prefs = Preferences {
        recent_models: vec![recent("claude", Some("opus")), recent("pi", None)],
        ..Default::default()
    };
    assert_eq!(pick(&prefs, LOCAL), (Kind::Claude, Some("opus".into())));
    prefs
        .default_models
        .insert("claude".into(), "sonnet".into());
    assert_eq!(pick(&prefs, LOCAL), (Kind::Claude, Some("sonnet".into())));
}

#[test]
fn hidden_providers_are_skipped_step_by_step() {
    let mut prefs = Preferences {
        default_provider: Some("claude".into()),
        recent_models: vec![recent("pi", Some("pi-1"))],
        hidden_providers: vec!["claude".into(), "pi".into()],
        ..Default::default()
    };
    assert_eq!(pick(&prefs, LOCAL), (Kind::Codex, None));
    // The first provider left when Codex is hidden too.
    prefs.hidden_providers.push("codex".into());
    assert_eq!(pick(&prefs, LOCAL).0, Kind::Cursor);
    // Everything hidden falls back to Codex.
    prefs.hidden_providers = Kind::ALL.iter().map(|k| k.id().into()).collect();
    assert_eq!(pick(&prefs, LOCAL).0, Kind::Codex);
    // A project's own hidden list counts for that project.
    let mut prefs = Preferences {
        recent_models: vec![recent("pi", Some("pi-1"))],
        ..Default::default()
    };
    project(&mut prefs, LOCAL, None, &[], &["pi"]);
    assert_eq!(pick(&prefs, LOCAL).0, Kind::Codex);
    assert_eq!(pick(&prefs, "/work/other").0, Kind::Pi);
}

#[test]
fn remote_projects_ignore_the_hidden_list() {
    let prefs = Preferences {
        default_provider: Some("claude".into()),
        hidden_providers: vec!["claude".into()],
        ..Default::default()
    };
    assert_eq!(pick(&prefs, LOCAL).0, Kind::Codex);
    assert_eq!(pick(&prefs, REMOTE).0, Kind::Claude);
}

#[test]
fn explicit_provider_takes_the_project_then_default_model() {
    let mut prefs = Preferences {
        default_models: [("claude".into(), "sonnet".into())].into(),
        ..Default::default()
    };
    let c = NewChatChoice::explicit(&prefs, LOCAL, Kind::Claude);
    assert_eq!((c.kind, c.model.as_deref()), (Kind::Claude, Some("sonnet")));
    project(&mut prefs, LOCAL, None, &[("claude", "opus")], &[]);
    assert_eq!(
        NewChatChoice::explicit(&prefs, LOCAL, Kind::Claude)
            .model
            .as_deref(),
        Some("opus")
    );
}

// --- Through the real Shell ---

use gpui::TestAppContext;

/// The newest session's provider and saved model after a new chat.
fn created(shell: &Shell, cx: &App) -> (String, Option<String>) {
    let id = shell.workspace.active.expect("a new chat is active");
    let provider = shell.session_metadata(id, cx).summary.provider.clone();
    let model = shell
        .store
        .lock()
        .unwrap()
        .agent_settings(id)
        .unwrap()
        .and_then(|settings| settings.model);
    (provider, model)
}

#[gpui::test]
fn ordinary_shell_new_chat_follows_the_last_choice(cx: &mut TestAppContext) {
    let (window, _, _) = super::tests::window(cx);
    let new_chat = |cx: &mut TestAppContext, recents: Vec<ModelChoice>, hidden: &[&str]| {
        window
            .update(cx, |shell, _, cx| {
                shell.workspace.preferences.recent_models = recents;
                shell.workspace.preferences.hidden_providers =
                    hidden.iter().map(|id| id.to_string()).collect();
                shell.new_chat(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| created(shell, cx))
            .unwrap()
    };
    assert_eq!(new_chat(cx, vec![], &[]), ("codex".into(), None));
    assert_eq!(
        new_chat(cx, vec![recent("claude", Some("opus"))], &[]),
        ("claude".into(), Some("opus".into()))
    );
    // A hidden last choice is skipped.
    assert_eq!(
        new_chat(cx, vec![recent("claude", Some("opus"))], &["claude"]),
        ("codex".into(), None)
    );
}
