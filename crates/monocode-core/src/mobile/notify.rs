//! Pushes for paired phones: an alert when an agent stops for the user or
//! finishes, and a silent one that wakes the app to redraw its Live Activity.
//! The desktop sees each change as it publishes the catalog and sends it
//! through Expo's push service, so a phone hears it with the app closed.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

const ENDPOINT: &str = "https://exp.host/--/api/v2/push/send";

/// Where a phone wants its alerts, and which kinds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushTarget {
    /// An Expo push token, `ExponentPushToken[…]`.
    pub token: String,
    /// The phone's enabled categories: `agentInput`, `agentFinished`.
    #[serde(default)]
    pub categories: Vec<String>,
    /// The phone shows a Live Activity and wants waking as sessions change.
    #[serde(default)]
    pub activity: bool,
}

impl PushTarget {
    pub(super) fn from_params(params: &Value) -> Result<Option<Self>, String> {
        let Some(token) = params["token"].as_str() else {
            return Ok(None);
        };
        let valid = token.len() <= 256
            && (token.starts_with("ExponentPushToken[") || token.starts_with("ExpoPushToken["))
            && token.ends_with(']');
        if !valid {
            return Err("Invalid push token".into());
        }
        let categories = params["categories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|id| ["agentInput", "agentFinished"].contains(id))
            .map(str::to_owned)
            .collect();
        Ok(Some(Self {
            token: token.to_owned(),
            categories,
            activity: params["activity"] == true,
        }))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Alert {
    pub category: &'static str,
    pub session_id: String,
    pub project_id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub body: String,
    /// Lifts it past Focus; for an agent blocked on the user.
    pub urgent: bool,
}

/// What changed between two catalogs that a phone would want to hear about.
/// A session missing from `before` is new to us, so it says nothing.
pub(super) fn alerts(before: &Value, after: &Value) -> Vec<Alert> {
    let previous: std::collections::HashMap<&str, &Value> = before["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|session| Some((session["id"].as_str()?, session)))
        .collect();
    let project_name = |id: &str| {
        after["projects"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|project| project["id"] == id)
            .and_then(|project| project["name"].as_str())
            .map(str::to_owned)
    };
    let mut alerts = Vec::new();
    for session in after["sessions"].as_array().into_iter().flatten() {
        let Some(id) = session["id"].as_str() else {
            continue;
        };
        let Some(last) = previous.get(id) else {
            continue;
        };
        let waiting = session["waiting"].as_str();
        let busy = session["busy"] == true;
        let (category, body, urgent) = match waiting {
            Some(kind) if last["waiting"].as_str() != Some(kind) => {
                let lead = if kind == "approval" {
                    "Needs your approval"
                } else {
                    "Has a question for you"
                };
                // Say what it asks, when the desktop knows.
                let body = match session["request"]["label"].as_str().map(str::trim) {
                    Some(label) if !label.is_empty() => format!("{lead}: {label}"),
                    _ => lead.to_owned(),
                };
                ("agentInput", body, true)
            }
            None if last["busy"] == true && !busy => {
                ("agentFinished", "Finished".to_owned(), false)
            }
            _ => continue,
        };
        let project_id = session["projectId"].as_str().unwrap_or_default();
        let title = session["title"]
            .as_str()
            .filter(|t| !t.is_empty())
            .unwrap_or("Session");
        alerts.push(Alert {
            category,
            session_id: id.to_owned(),
            project_id: project_id.to_owned(),
            title: title.to_owned(),
            subtitle: project_name(project_id),
            body,
            urgent,
        });
    }
    alerts
}

/// Sessions a Live Activity shows, at most this many.
const ACTIVITY_SESSIONS: usize = 4;

/// What a phone's Live Activity shows: each session working or waiting on the
/// user, newest first, plus any that just finished. Small enough to ride in a
/// push, so a woken phone redraws without reaching the desktop.
pub(super) fn activity(before: &Value, after: &Value) -> Value {
    let finished: Vec<String> = alerts(before, after)
        .into_iter()
        .filter(|alert| alert.category == "agentFinished")
        .map(|alert| alert.session_id)
        .collect();
    let projects = after["projects"].as_array().cloned().unwrap_or_default();
    let sessions: Vec<Value> = after["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|session| {
            let id = session["id"].as_str()?;
            let status = match session["waiting"].as_str() {
                Some("approval") => "approval",
                Some(_) => "question",
                None if session["busy"] == true => "working",
                None if finished.iter().any(|done| done == id) => "done",
                None => return None,
            };
            let project = projects
                .iter()
                .find(|project| project["id"] == session["projectId"]);
            let field = |key: &str| {
                project
                    .and_then(|project| project[key].as_str())
                    .unwrap_or_default()
            };
            let title: String = session["title"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(60)
                .collect();
            // The project's path, color and mascot pick draw its mascot, as
            // the phone's project rail does.
            let mut shown = json!({
                "id": id,
                "projectId": session["projectId"],
                "project": field("name"),
                "projectPath": field("path"),
                "color": field("color"),
                "mascot": field("mascot"),
                "title": title,
                "model": session["modelName"].as_str().unwrap_or_default(),
                "status": status,
            });
            // What it waits on, so the activity can show it and answer it.
            if status == "approval" || status == "question" {
                let request = &session["request"];
                let label: String = request["label"]
                    .as_str()
                    .unwrap_or_default()
                    .chars()
                    .take(120)
                    .collect();
                shown["request"] = json!({
                    "id": request["id"].as_str().unwrap_or_default(),
                    "label": label,
                    "count": request["count"].as_u64().unwrap_or(1),
                });
            }
            Some(shown)
        })
        .take(ACTIVITY_SESSIONS)
        .collect();
    Value::from(sessions)
}

/// Where Apple delivers this phone's Live Activity pushes: the running
/// activity's token and the one that starts a new activity (iOS 17.2+).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_token: Option<String>,
    /// The app's bundle ID, which names the push topic.
    pub bundle_id: String,
    /// Development builds get their pushes from Apple's sandbox.
    #[serde(default)]
    pub sandbox: bool,
}

impl LiveTarget {
    /// Applies `device.activity`: each token present replaces the last, and
    /// null clears it, as when the activity ends on the phone.
    pub(super) fn merge(current: Option<Self>, params: &Value) -> Result<Self, String> {
        let token = |key: &str, last: Option<String>| -> Result<Option<String>, String> {
            match &params[key] {
                Value::Null if params.get(key).is_some() => Ok(None),
                Value::Null => Ok(last),
                Value::String(token)
                    if (16..=512).contains(&token.len())
                        && token.chars().all(|c| c.is_ascii_hexdigit()) =>
                {
                    Ok(Some(token.to_ascii_lowercase()))
                }
                _ => Err("Invalid activity token".into()),
            }
        };
        let bundle_id = params["bundleId"].as_str().unwrap_or_default();
        let valid = !bundle_id.is_empty()
            && bundle_id.len() <= 155
            && bundle_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if !valid {
            return Err("Invalid bundle ID".into());
        }
        let current = current.unwrap_or_default();
        Ok(Self {
            activity_token: token("activityToken", current.activity_token)?,
            start_token: token("startToken", current.start_token)?,
            bundle_id: bundle_id.to_owned(),
            sandbox: params["environment"] == "sandbox",
        })
    }
}

/// Seconds from 1970 to 2001, where Swift's `Date` counts from.
const REFERENCE_DATE: u64 = 978_307_200;

/// The activity as the widget decodes it (MonocodeActivityAttributes
/// .ContentState): most urgent first, each mascot drawn out, and `since` as
/// Swift's default `Date` encoding.
pub(super) fn live_state(
    activity: &Value,
    since: &std::collections::HashMap<String, u64>,
) -> Value {
    use crate::project_appearance::{ProjectAppearance, MASCOT_FRAMES};
    let urgency = |status: &str| match status {
        "approval" => 0,
        "question" => 1,
        "working" => 2,
        _ => 3,
    };
    let mut sessions: Vec<&Value> = activity.as_array().into_iter().flatten().collect();
    sessions.sort_by_key(|session| urgency(session["status"].as_str().unwrap_or_default()));
    let sessions: Vec<Value> = sessions
        .into_iter()
        .map(|session| {
            let text = |key: &str| session[key].as_str().unwrap_or_default();
            let id = text("id");
            let path = Some(text("projectPath"))
                .filter(|path| !path.is_empty())
                .unwrap_or(text("projectId"));
            let appearance = ProjectAppearance {
                mascot: Some(text("mascot").to_owned()).filter(|name| !name.is_empty()),
                ..ProjectAppearance::default()
            };
            let started = since.get(id).copied().unwrap_or(REFERENCE_DATE);
            json!({
                "id": id,
                "projectId": text("projectId"),
                "project": text("project"),
                "title": text("title"),
                "model": text("model"),
                "color": Some(text("color")).filter(|c| !c.is_empty()).unwrap_or("#ffffff"),
                "grid": MASCOT_FRAMES[appearance.mascot(path)].concat(),
                "status": text("status"),
                "request": session["request"]["label"].as_str().unwrap_or_default(),
                "requestId": session["request"]["id"].as_str().unwrap_or_default(),
                "requests": session["request"]["count"].as_u64().unwrap_or(0),
                "since": started.saturating_sub(REFERENCE_DATE) as f64,
            })
        })
        .collect();
    json!({ "sessions": sessions })
}

/// A push that only wakes the app, carrying the activity to redraw.
pub(super) fn silent(token: &str, activity: &Value) -> Value {
    json!({
        "to": token,
        "_contentAvailable": true,
        // Background pushes must go at normal priority, or APNs drops them.
        "priority": "normal",
        "data": { "activity": activity },
    })
}

pub(super) fn message(token: &str, alert: &Alert) -> Value {
    let mut message = json!({
        "to": token,
        "title": alert.title,
        "body": alert.body,
        "sound": "default",
        "priority": "high",
        // One per session in Notification Center: "finished" replaces
        // "needs approval" rather than stacking under it.
        "collapseId": alert.session_id,
        "threadId": alert.session_id,
        "interruptionLevel": if alert.urgent { "time-sensitive" } else { "active" },
        "data": { "projectId": alert.project_id, "sessionId": alert.session_id },
    });
    if let Some(subtitle) = &alert.subtitle {
        message["subtitle"] = json!(subtitle);
    }
    message
}

pub(super) enum SendError {
    /// The phone uninstalled the app or turned its token over; forget it.
    Unregistered,
    Other(String),
}

/// Sends the messages in one request, as Expo takes up to 100 at a time.
pub(super) fn send(messages: &[Value]) -> Result<(), SendError> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build();
    let response = match agent
        .post(ENDPOINT)
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .send_string(&Value::from(messages.to_vec()).to_string())
    {
        Ok(response) | Err(ureq::Error::Status(_, response)) => response,
        Err(error) => return Err(SendError::Other(error.to_string())),
    };
    let body: Value = response
        .into_string()
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or_else(|| SendError::Other("Invalid push service response".into()))?;
    let tickets = body["data"].as_array().cloned().unwrap_or_default();
    if tickets
        .iter()
        .any(|ticket| ticket["details"]["error"] == "DeviceNotRegistered")
    {
        return Err(SendError::Unregistered);
    }
    match tickets.iter().find(|ticket| ticket["status"] == "error") {
        Some(ticket) => Err(SendError::Other(
            ticket["message"]
                .as_str()
                .unwrap_or("Push failed")
                .to_owned(),
        )),
        None if body["errors"].is_array() => Err(SendError::Other(body["errors"].to_string())),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(sessions: Value) -> Value {
        json!({"projects":[{"id":"p","name":"monocode","path":"/code/monocode","color":"#7aa2ff"}],"monos":[],"sessions":sessions})
    }

    #[test]
    fn an_agent_stopping_for_the_user_or_finishing_raises_one_alert_each() {
        let before = catalog(json!([
            {"id":"a","projectId":"p","title":"Fix push","busy":true},
            {"id":"b","projectId":"p","title":"Docs","busy":true},
            {"id":"c","projectId":"p","title":"Idle","busy":false},
        ]));
        let after = catalog(json!([
            {"id":"a","projectId":"p","title":"Fix push","busy":true,"waiting":"approval","request":{"id":"r1","label":"Run cargo test","count":1}},
            {"id":"b","projectId":"p","title":"Docs","busy":false},
            {"id":"c","projectId":"p","title":"Idle","busy":false},
            {"id":"d","projectId":"p","title":"New","busy":false},
        ]));
        let alerts = alerts(&before, &after);
        assert_eq!(alerts.len(), 2);
        assert_eq!(
            (
                alerts[0].category,
                alerts[0].body.as_str(),
                alerts[0].urgent
            ),
            ("agentInput", "Needs your approval: Run cargo test", true)
        );
        assert_eq!(alerts[0].subtitle.as_deref(), Some("monocode"));
        assert_eq!(
            (alerts[1].category, alerts[1].session_id.as_str()),
            ("agentFinished", "b")
        );
    }

    #[test]
    fn a_session_still_waiting_or_first_seen_says_nothing() {
        let waiting = catalog(json!([{"id":"a","projectId":"p","busy":true,"waiting":"question"}]));
        assert!(alerts(&waiting, &waiting).is_empty());
        assert!(alerts(&catalog(json!([])), &waiting).is_empty());
        // Answering a question lets it carry on; that is not "finished".
        let resumed = catalog(json!([{"id":"a","projectId":"p","busy":true}]));
        assert!(alerts(&waiting, &resumed).is_empty());
    }

    #[test]
    fn the_activity_carries_working_and_waiting_sessions_and_the_ones_that_just_finished() {
        let before = catalog(json!([
            {"id":"a","projectId":"p","title":"Fix push","busy":true},
            {"id":"b","projectId":"p","title":"Docs","busy":true},
            {"id":"c","projectId":"p","title":"Idle","busy":false},
        ]));
        let after = catalog(json!([
            {"id":"a","projectId":"p","title":"Fix push","busy":true,"waiting":"approval","modelName":"Opus 5.5","request":{"id":"r1","label":"Run cargo test","count":2}},
            {"id":"b","projectId":"p","title":"Docs","busy":false},
            {"id":"c","projectId":"p","title":"Idle","busy":false},
        ]));
        let shown = activity(&before, &after);
        let statuses: Vec<_> = shown
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["id"].as_str().unwrap(), s["status"].as_str().unwrap()))
            .collect();
        assert_eq!(statuses, [("a", "approval"), ("b", "done")]);
        assert_eq!(shown[0]["project"], "monocode");
        assert_eq!(shown[0]["model"], "Opus 5.5");
        assert_eq!(
            shown[0]["request"],
            json!({"id":"r1","label":"Run cargo test","count":2})
        );
        assert!(shown[1]["request"].is_null());
        assert_eq!(
            (
                &shown[0]["projectPath"],
                &shown[0]["color"],
                &shown[0]["mascot"]
            ),
            (&json!("/code/monocode"), &json!("#7aa2ff"), &json!(""))
        );
        // Nothing changed, nothing is done any more: only what still runs.
        assert_eq!(activity(&after, &after).as_array().unwrap().len(), 1);
    }

    #[test]
    fn the_pushed_state_matches_the_widget_most_urgent_first() {
        let activity = json!([
            {"id":"w","projectId":"p","project":"monocode","projectPath":"/code/monocode","color":"#7aa2ff","mascot":"","title":"Docs","model":"Opus 5.5","status":"working"},
            {"id":"a","projectId":"p","project":"monocode","projectPath":"/code/monocode","color":"","mascot":"ghost","title":"Fix push","model":"Opus 5.5","status":"approval","request":{"id":"r1","label":"Run cargo test","count":1}},
        ]);
        let since = std::collections::HashMap::from([("w".to_owned(), REFERENCE_DATE + 60)]);
        let state = live_state(&activity, &since);
        let sessions = state["sessions"].as_array().unwrap();
        assert_eq!(sessions[0]["id"], "a");
        assert_eq!(
            (
                &sessions[0]["request"],
                &sessions[0]["requestId"],
                &sessions[0]["requests"]
            ),
            (&json!("Run cargo test"), &json!("r1"), &json!(1))
        );
        assert_eq!(sessions[0]["color"], "#ffffff");
        // A chosen mascot wins; otherwise the path picks one, as on the phone.
        let ghost = crate::project_appearance::MASCOT_FRAMES[1].concat();
        assert_eq!(sessions[0]["grid"], ghost);
        assert_eq!(sessions[1]["grid"].as_str().unwrap().len(), 64);
        assert_eq!(sessions[1]["since"], json!(60.0));
        assert_eq!(sessions[1]["request"], "");
        let keys: Vec<&String> = sessions[1].as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 12, "every field the widget's Session decodes");
    }

    #[test]
    fn activity_tokens_update_one_at_a_time() {
        let first = LiveTarget::merge(
            None,
            &json!({"startToken":"ABCDEF0123456789","bundleId":"com.example.app","environment":"sandbox"}),
        )
        .unwrap();
        assert_eq!(first.start_token.as_deref(), Some("abcdef0123456789"));
        assert!(first.sandbox && first.activity_token.is_none());
        let running = LiveTarget::merge(
            Some(first),
            &json!({"activityToken":"00112233445566778899","bundleId":"com.example.app","environment":"production"}),
        )
        .unwrap();
        assert_eq!(running.start_token.as_deref(), Some("abcdef0123456789"));
        assert!(!running.sandbox);
        let ended = LiveTarget::merge(
            Some(running),
            &json!({"activityToken":null,"bundleId":"com.example.app"}),
        )
        .unwrap();
        assert!(ended.activity_token.is_none() && ended.start_token.is_some());
        assert!(
            LiveTarget::merge(None, &json!({"activityToken":"not hex!","bundleId":"a.b"})).is_err()
        );
        assert!(LiveTarget::merge(None, &json!({"bundleId":"a/b"})).is_err());
    }

    #[test]
    fn only_expo_tokens_and_known_categories_are_kept() {
        assert!(PushTarget::from_params(&json!({"token":"abc"})).is_err());
        assert_eq!(PushTarget::from_params(&json!({})).unwrap(), None);
        let target = PushTarget::from_params(
            &json!({"token":"ExponentPushToken[x]","categories":["agentInput","rm -rf"]}),
        )
        .unwrap()
        .unwrap();
        assert_eq!(target.categories, ["agentInput"]);
        assert!(!target.activity);
    }
}
