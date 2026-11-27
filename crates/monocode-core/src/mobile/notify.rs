//! Notifications for paired phones, sent through Expo's push service so they
//! arrive while the app is closed. The desktop sends them itself; there is no
//! server of ours in between.
use serde_json::{json, Value};
use std::io::Read;

const ENDPOINT: &str = "https://exp.host/--/api/v2/push/send";
/// Expo takes at most this many messages per request.
const MAX_BATCH: usize = 100;
const MAX_RESPONSE_BYTES: u64 = 1 << 20;
/// The phone's Android channel for session alerts.
const CHANNEL: &str = "sessions";

pub struct Notice {
    pub title: String,
    /// Under the title on iOS, as the project a session is in.
    pub subtitle: Option<String>,
    pub body: String,
    /// What the phone opens when it is tapped.
    pub data: Value,
}

/// Sends `notice` to each token. Returns the tokens Expo says no longer reach
/// a phone, to forget.
pub fn send(tokens: &[String], notice: &Notice) -> Result<Vec<String>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build();
    let mut gone = Vec::new();
    for batch in tokens.chunks(MAX_BATCH) {
        let messages: Vec<Value> = batch.iter().map(|to| message(to, notice)).collect();
        let response = agent
            .post(ENDPOINT)
            .set("Accept", "application/json")
            .set("Content-Type", "application/json")
            .send_string(&Value::from(messages).to_string());
        let response = match response {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(error) => return Err(format!("Could not reach Expo push: {error}")),
        };
        let status = response.status();
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(MAX_RESPONSE_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_slice(&bytes).unwrap_or_default();
        if status != 200 {
            return Err(format!("Expo push returned HTTP {status}: {value}"));
        }
        gone.extend(unregistered(batch, &value));
    }
    Ok(gone)
}

fn message(to: &str, notice: &Notice) -> Value {
    let mut message = json!({
        "to": to,
        "title": notice.title,
        "body": notice.body,
        "data": notice.data,
        "sound": "default",
        "priority": "high",
        "channelId": CHANNEL,
    });
    if let Some(subtitle) = &notice.subtitle {
        message["subtitle"] = json!(subtitle);
    }
    message
}

/// Expo answers each message in order with a ticket; `DeviceNotRegistered`
/// means the app is gone from that phone.
fn unregistered(tokens: &[String], response: &Value) -> Vec<String> {
    let Some(tickets) = response["data"].as_array() else {
        return Vec::new();
    };
    tokens
        .iter()
        .zip(tickets)
        .filter(|(_, ticket)| ticket["details"]["error"] == "DeviceNotRegistered")
        .map(|(token, _)| token.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_carry_the_tap_target_and_unregistered_phones_are_found_in_order() {
        let notice = Notice {
            title: "Fix the build".into(),
            subtitle: Some("monocode".into()),
            body: "Finished".into(),
            data: json!({"sessionId":"s"}),
        };
        let message = message("ExponentPushToken[a]", &notice);
        assert_eq!(message["data"]["sessionId"], "s");
        assert_eq!(message["subtitle"], "monocode");
        let tokens = vec!["ExponentPushToken[a]".into(), "ExponentPushToken[b]".into()];
        let response = json!({"data":[
            {"status":"ok","id":"1"},
            {"status":"error","details":{"error":"DeviceNotRegistered"}},
        ]});
        assert_eq!(unregistered(&tokens, &response), ["ExponentPushToken[b]"]);
    }
}
