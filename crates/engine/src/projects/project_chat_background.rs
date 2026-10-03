//! Port of src/features/projects/model/projectChatBackground.ts: each
//! project's chat background override (image, visibility, scope, effect).
//!
//! The module state (`revision`, `imageRevision`) and the
//! `monocode:project-chat-background-changed` event live in
//! `ProjectChatBackgrounds`. The `Projects` entity holds one and emits
//! `ProjectsEvent::ChatBackgroundChanged` for each notification.

use monocode_core::appearance::{
    CHAT_BACKGROUND_EMPTY_OPACITY_KEY, CHAT_BACKGROUND_OPACITY_DEFAULT,
    CHAT_BACKGROUND_OPACITY_KEY, CHAT_BACKGROUND_SCOPE_KEY, CHAT_BACKGROUND_SESSION_OPACITY_KEY,
    ChatBackgroundScope, NEW_THREAD_BACKGROUND_EFFECT_DEFAULT, NewThreadBackgroundEffect,
    clamp_chat_background_opacity,
};
use monocode_core::js;
use monocode_core::settings::read_number;
use monocode_layout::tab_groups::JsRecord;
use monocode_settings::Kv;
use serde::Serialize;
use serde_json::Value;

use super::js_object::{finite_number, parse_object_or_array, stringify};
use super::now_ms;

pub const KEY: &str = "monocode:project-chat-backgrounds";

/// `ProjectChatBackground`: the older single-opacity shape.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectChatBackground {
    pub path: String,
    pub opacity: f64,
    pub scope: ChatBackgroundScope,
}

/// `ProjectChatBackgroundSettings`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectChatBackgroundSettings {
    pub path: String,
    pub empty_opacity: f64,
    pub session_opacity: f64,
    pub scope: ChatBackgroundScope,
    pub effect: NewThreadBackgroundEffect,
}

/// What `saveProjectChatBackgroundSettings` writes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredSettings {
    path: String,
    empty_opacity: f64,
    session_opacity: f64,
    scope: ChatBackgroundScope,
    effect: NewThreadBackgroundEffect,
}

/// What `saveProjectChatBackground` writes.
#[derive(Debug, Clone, Serialize)]
struct StoredLegacy {
    path: String,
    opacity: f64,
    scope: ChatBackgroundScope,
}

/// One stored entry: as read, or as this module writes it.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum Stored {
    Raw(Value),
    Settings(StoredSettings),
    Legacy(StoredLegacy),
}

impl Stored {
    fn field(&self, key: &str) -> Option<Value> {
        match self {
            Stored::Raw(value) => value.get(key).cloned(),
            other => serde_json::to_value(other)
                .ok()
                .and_then(|value| value.get(key).cloned()),
        }
    }
}

/// `read`.
fn read(kv: &Kv) -> JsRecord<Stored> {
    let Some(raw) = kv.get_item(KEY).filter(|raw| !raw.is_empty()) else {
        return JsRecord::new();
    };
    let Some(parsed) = parse_object_or_array(&raw) else {
        return JsRecord::new();
    };
    let mut out = JsRecord::new();
    for (key, value) in parsed.iter() {
        out.insert(key, Stored::Raw(value.clone()));
    }
    out
}

/// `write`.
fn write(kv: &Kv, value: &JsRecord<Stored>) {
    kv.set_item(KEY, &stringify(value));
}

fn valid_scope(value: Option<&Value>) -> Option<ChatBackgroundScope> {
    value
        .and_then(Value::as_str)
        .and_then(ChatBackgroundScope::from_str_opt)
}

fn valid_effect(value: Option<&Value>) -> Option<NewThreadBackgroundEffect> {
    value
        .and_then(Value::as_str)
        .and_then(NewThreadBackgroundEffect::from_str_opt)
}

/// `storedOpacity`.
fn stored_opacity(value: Option<&Value>, fallback: f64) -> f64 {
    finite_number(value)
        .map(clamp_chat_background_opacity)
        .unwrap_or(fallback)
}

/// `loadChatBackgroundOpacityValue` from appearance.ts.
fn global_opacity(kv: &Kv, key: &str) -> f64 {
    let number = |key: &str| read_number(kv.get_item(key).as_deref());
    clamp_chat_background_opacity(
        number(key)
            .or_else(|| number(CHAT_BACKGROUND_OPACITY_KEY))
            .unwrap_or(CHAT_BACKGROUND_OPACITY_DEFAULT),
    )
}

/// `loadChatBackgroundEmptyOpacity`.
fn global_empty_opacity(kv: &Kv) -> f64 {
    global_opacity(kv, CHAT_BACKGROUND_EMPTY_OPACITY_KEY)
}

/// `loadChatBackgroundSessionOpacity`.
fn global_session_opacity(kv: &Kv) -> f64 {
    global_opacity(kv, CHAT_BACKGROUND_SESSION_OPACITY_KEY)
}

/// `loadChatBackgroundScope`.
fn global_scope(kv: &Kv) -> ChatBackgroundScope {
    ChatBackgroundScope::parse(kv.get_item(CHAT_BACKGROUND_SCOPE_KEY).as_deref())
}

/// `loadProjectChatBackgroundSettings`: the project's override, or `None`
/// when it has no image. Missing values fall back to the older single
/// opacity, then to the global settings.
pub fn load_project_chat_background_settings(
    kv: &Kv,
    project: &str,
) -> Option<ProjectChatBackgroundSettings> {
    let all = read(kv);
    let stored = all.get(project)?;
    let path = stored
        .field("path")
        .and_then(|path| path.as_str().map(|path| js::trim(path).to_string()))
        .unwrap_or_default();
    if path.is_empty() {
        return None;
    }
    let opacity = stored.field("opacity");
    let empty_opacity = stored.field("emptyOpacity");
    let session_opacity = stored.field("sessionOpacity");
    let legacy_opacity = stored_opacity(opacity.as_ref(), global_empty_opacity(kv));
    let has_stored_opacity = finite_number(opacity.as_ref()).is_some();
    let has_stored_empty_opacity = finite_number(empty_opacity.as_ref()).is_some();
    let session_fallback = if has_stored_empty_opacity {
        stored_opacity(empty_opacity.as_ref(), legacy_opacity)
    } else if has_stored_opacity {
        legacy_opacity
    } else {
        global_session_opacity(kv)
    };
    Some(ProjectChatBackgroundSettings {
        path,
        empty_opacity: stored_opacity(empty_opacity.as_ref(), legacy_opacity),
        session_opacity: stored_opacity(session_opacity.as_ref(), session_fallback),
        scope: valid_scope(stored.field("scope").as_ref()).unwrap_or_else(|| global_scope(kv)),
        effect: valid_effect(stored.field("effect").as_ref())
            .unwrap_or(NEW_THREAD_BACKGROUND_EFFECT_DEFAULT),
    })
}

/// `loadProjectChatBackground`: the older single-opacity view.
pub fn load_project_chat_background(kv: &Kv, project: &str) -> Option<ProjectChatBackground> {
    let settings = load_project_chat_background_settings(kv, project)?;
    Some(ProjectChatBackground {
        path: settings.path,
        opacity: settings.empty_opacity,
        scope: settings.scope,
    })
}

/// The module state of projectChatBackground.ts: the revisions views use to
/// reload, and the change notifications not yet delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectChatBackgrounds {
    revision: i64,
    image_revision: i64,
    /// One entry per `notifyProjectChatBackgroundChanged`, with whether the
    /// image itself changed.
    events: Vec<bool>,
}

impl Default for ProjectChatBackgrounds {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectChatBackgrounds {
    /// Both revisions start at the current time, as `Date.now()` did.
    pub fn new() -> Self {
        let now = now_ms();
        Self {
            revision: now,
            image_revision: now,
            events: Vec::new(),
        }
    }

    /// `projectChatBackgroundRevision`.
    pub fn revision(&self) -> i64 {
        self.revision
    }

    /// `projectChatBackgroundImageRevision`.
    pub fn image_revision(&self) -> i64 {
        self.image_revision
    }

    /// The notifications since the last call, oldest first. Each is `true`
    /// when the image changed.
    pub fn take_events(&mut self) -> Vec<bool> {
        std::mem::take(&mut self.events)
    }

    /// `notifyProjectChatBackgroundChanged`.
    pub fn notify_changed(&mut self, image_changed: bool) {
        self.revision += 1;
        if image_changed {
            self.image_revision += 1;
        }
        self.events.push(image_changed);
    }

    /// `saveProjectChatBackgroundSettings`. Pass `image_changed` when the
    /// picture itself is new, so views reprocess it.
    pub fn save_settings(
        &mut self,
        kv: &Kv,
        project: &str,
        value: &ProjectChatBackgroundSettings,
        image_changed: bool,
    ) {
        let path = js::trim(&value.path);
        if project.is_empty() || path.is_empty() {
            return;
        }
        let mut next = read(kv);
        next.insert(
            project,
            Stored::Settings(StoredSettings {
                path: path.to_string(),
                empty_opacity: clamp_chat_background_opacity(value.empty_opacity),
                session_opacity: clamp_chat_background_opacity(value.session_opacity),
                scope: value.scope,
                effect: value.effect,
            }),
        );
        write(kv, &next);
        self.notify_changed(image_changed);
    }

    /// `saveProjectChatBackground`: the older single-opacity shape.
    pub fn save(&mut self, kv: &Kv, project: &str, value: &ProjectChatBackground) {
        let path = js::trim(&value.path);
        if project.is_empty() || path.is_empty() {
            return;
        }
        let mut next = read(kv);
        next.insert(
            project,
            Stored::Legacy(StoredLegacy {
                path: path.to_string(),
                opacity: clamp_chat_background_opacity(value.opacity),
                scope: value.scope,
            }),
        );
        write(kv, &next);
        self.notify_changed(true);
    }

    /// `clearProjectChatBackgroundSetting`.
    pub fn clear(&mut self, kv: &Kv, project: &str) {
        let mut next = read(kv);
        if next.remove(project).is_none() {
            return;
        }
        write(kv, &next);
        self.notify_changed(true);
    }

    /// `rebaseProjectChatBackgroundSetting`: follow a project rename.
    pub fn rebase(&mut self, kv: &Kv, from: &str, to: &str) {
        if from.is_empty() || to.is_empty() || from == to {
            return;
        }
        let mut next = read(kv);
        let Some(entry) = next.get(from).cloned() else {
            return;
        };
        if !next.contains_key(to) {
            next.insert(to, entry);
        }
        next.remove(from);
        write(kv, &next);
        self.notify_changed(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy(path: &str, opacity: f64, scope: ChatBackgroundScope) -> ProjectChatBackground {
        ProjectChatBackground {
            path: path.into(),
            opacity,
            scope,
        }
    }

    fn settings(path: &str) -> ProjectChatBackgroundSettings {
        ProjectChatBackgroundSettings {
            path: path.into(),
            empty_opacity: 0.2,
            session_opacity: 0.3,
            scope: ChatBackgroundScope::All,
            effect: NewThreadBackgroundEffect::None,
        }
    }

    #[test]
    fn stores_independent_overrides_for_each_project() {
        let kv = Kv::in_memory();
        let mut backgrounds = ProjectChatBackgrounds::new();
        let alpha = legacy("/backgrounds/alpha.webp", 0.22, ChatBackgroundScope::Empty);
        let beta = legacy("/backgrounds/beta.png", 0.48, ChatBackgroundScope::All);
        backgrounds.save(&kv, "/work/alpha", &alpha);
        backgrounds.save(&kv, "/work/beta", &beta);

        assert_eq!(
            load_project_chat_background(&kv, "/work/alpha"),
            Some(alpha)
        );
        assert_eq!(load_project_chat_background(&kv, "/work/beta"), Some(beta));
    }

    #[test]
    fn clamps_visibility_to_the_supported_range() {
        let kv = Kv::in_memory();
        let mut backgrounds = ProjectChatBackgrounds::new();
        backgrounds.save(
            &kv,
            "/work/alpha",
            &legacy("/backgrounds/alpha.webp", 1.0, ChatBackgroundScope::All),
        );
        backgrounds.save(
            &kv,
            "/work/beta",
            &legacy("/backgrounds/beta.webp", 0.0, ChatBackgroundScope::All),
        );

        assert_eq!(
            load_project_chat_background(&kv, "/work/alpha").map(|value| value.opacity),
            Some(0.65)
        );
        assert_eq!(
            load_project_chat_background(&kv, "/work/beta").map(|value| value.opacity),
            Some(0.05)
        );
    }

    #[test]
    fn falls_back_safely_when_stored_project_data_is_malformed() {
        let kv = Kv::in_memory();
        kv.set_item(
            KEY,
            r#"{"/work/alpha":{"path":"/backgrounds/alpha.webp","opacity":"bright","scope":"transcript"}}"#,
        );
        assert_eq!(
            load_project_chat_background(&kv, "/work/alpha"),
            Some(legacy(
                "/backgrounds/alpha.webp",
                0.24,
                ChatBackgroundScope::All
            ))
        );
    }

    #[test]
    fn clears_one_project_without_changing_the_others() {
        let kv = Kv::in_memory();
        let mut backgrounds = ProjectChatBackgrounds::new();
        backgrounds.save(
            &kv,
            "/work/alpha",
            &legacy("/backgrounds/alpha.webp", 0.2, ChatBackgroundScope::Empty),
        );
        backgrounds.save(
            &kv,
            "/work/beta",
            &legacy("/backgrounds/beta.webp", 0.3, ChatBackgroundScope::All),
        );

        backgrounds.clear(&kv, "/work/alpha");

        assert_eq!(load_project_chat_background(&kv, "/work/alpha"), None);
        assert_eq!(
            load_project_chat_background(&kv, "/work/beta").map(|value| value.path),
            Some("/backgrounds/beta.webp".into())
        );
    }

    #[test]
    fn stores_effects_independently_and_preserves_older_project_images() {
        let kv = Kv::in_memory();
        let mut backgrounds = ProjectChatBackgrounds::new();
        kv.set_item("monocode.newThreadBackgroundEffect", "ascii");
        backgrounds.save_settings(
            &kv,
            "/work/alpha",
            &ProjectChatBackgroundSettings {
                effect: NewThreadBackgroundEffect::Dither,
                ..settings("/backgrounds/alpha.webp")
            },
            false,
        );
        backgrounds.save(
            &kv,
            "/work/beta",
            &legacy("/backgrounds/beta.webp", 0.4, ChatBackgroundScope::Empty),
        );

        assert_eq!(
            load_project_chat_background_settings(&kv, "/work/alpha").map(|value| value.effect),
            Some(NewThreadBackgroundEffect::Dither)
        );
        assert_eq!(
            load_project_chat_background_settings(&kv, "/work/beta").map(|value| value.effect),
            Some(NewThreadBackgroundEffect::None)
        );

        kv.set_item(
            KEY,
            r#"{"/work/alpha":{"path":"/backgrounds/alpha.webp","effect":"unknown"}}"#,
        );
        assert_eq!(
            load_project_chat_background_settings(&kv, "/work/alpha").map(|value| value.effect),
            Some(NewThreadBackgroundEffect::None)
        );
    }

    #[test]
    fn does_not_reprocess_the_image_for_effect_and_visibility_updates() {
        let kv = Kv::in_memory();
        let mut backgrounds = ProjectChatBackgrounds::new();
        let image_revision = backgrounds.image_revision();
        backgrounds.save_settings(
            &kv,
            "/work/alpha",
            &settings("/backgrounds/alpha.webp"),
            true,
        );
        assert_eq!(backgrounds.image_revision(), image_revision + 1);

        backgrounds.save_settings(
            &kv,
            "/work/alpha",
            &ProjectChatBackgroundSettings {
                empty_opacity: 0.4,
                effect: NewThreadBackgroundEffect::Dither,
                ..settings("/backgrounds/alpha.webp")
            },
            false,
        );
        assert_eq!(backgrounds.image_revision(), image_revision + 1);
        assert_eq!(backgrounds.take_events(), [true, false]);
    }

    #[test]
    fn writes_the_typescript_shape_in_its_key_order() {
        let kv = Kv::in_memory();
        let mut backgrounds = ProjectChatBackgrounds::new();
        backgrounds.save_settings(&kv, "/work/alpha", &settings(" /a.png "), false);
        assert_eq!(
            kv.get_item(KEY).unwrap(),
            r#"{"/work/alpha":{"path":"/a.png","emptyOpacity":0.2,"sessionOpacity":0.3,"scope":"all","effect":"none"}}"#
        );
    }

    #[test]
    fn rebase_moves_the_override_once() {
        let kv = Kv::in_memory();
        let mut backgrounds = ProjectChatBackgrounds::new();
        backgrounds.save_settings(&kv, "/old", &settings("/a.png"), false);
        backgrounds.rebase(&kv, "/old", "/new");
        assert!(load_project_chat_background_settings(&kv, "/old").is_none());
        assert_eq!(
            load_project_chat_background_settings(&kv, "/new").map(|value| value.path),
            Some("/a.png".into())
        );
        backgrounds.take_events();
        backgrounds.rebase(&kv, "/old", "/new");
        assert!(backgrounds.take_events().is_empty());
    }

    #[test]
    fn falls_back_to_the_global_settings() {
        let kv = Kv::in_memory();
        kv.set_item(CHAT_BACKGROUND_EMPTY_OPACITY_KEY, "0.4");
        kv.set_item(CHAT_BACKGROUND_SESSION_OPACITY_KEY, "0.5");
        kv.set_item(CHAT_BACKGROUND_SCOPE_KEY, "empty");
        kv.set_item(KEY, r#"{"/p":{"path":"/a.png"}}"#);
        let loaded = load_project_chat_background_settings(&kv, "/p").unwrap();
        assert_eq!(loaded.empty_opacity, 0.4);
        assert_eq!(loaded.session_opacity, 0.5);
        assert_eq!(loaded.scope, ChatBackgroundScope::Empty);
    }
}
