use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use base64::Engine;
use serde_json::{Value, json};
use url::Url;

use crate::core::child::{Children, HttpRequest, SseEvent, SseEvents};
use crate::core::task::timeout;

#[derive(Debug)]
pub struct HttpError {
    pub status: u16,
    pub tag: String,
    pub message: String,
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OpenCode 2 HTTP {}: {}", self.status, self.message)
    }
}

impl std::error::Error for HttpError {}

#[derive(Clone)]
pub struct Client {
    base: Url,
    pub directory: String,
    authorization: String,
    children: Children,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenCodeV2Client")
            .field("base", &self.base)
            .field("directory", &self.directory)
            .finish()
    }
}

impl Client {
    pub fn new(base: &str, directory: &str, password: &str, children: Children) -> Result<Self> {
        let base = Url::parse(base)?;
        if base.scheme() != "http"
            || !matches!(base.host_str(), Some("127.0.0.1" | "localhost"))
            || !base.username().is_empty()
            || base.password().is_some()
            || password.is_empty()
        {
            bail!("OpenCode 2 requires an authenticated loopback server");
        }
        Ok(Self {
            base,
            directory: directory.into(),
            authorization: format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"))
            ),
            children,
        })
    }

    fn headers(&self) -> HashMap<String, String> {
        HashMap::from([
            ("Authorization".into(), self.authorization.clone()),
            ("Content-Type".into(), "application/json".into()),
        ])
    }

    pub async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        query: &[(&str, &str)],
    ) -> Result<Value> {
        let mut url = self.base.join(path)?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query.iter().copied());
        }
        let response = self
            .children
            .harness_http(HttpRequest {
                url: url.to_string(),
                method: method.into(),
                headers: Some(self.headers()),
                body: body.map(|body| body.to_string()),
                timeout_ms: Some(45_000),
            })
            .await?;
        let value = if response.body.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&response.body).map_err(|_| {
                anyhow!(
                    "OpenCode 2 returned a non-JSON response with status {}",
                    response.status
                )
            })?
        };
        if response.status >= 400 {
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("OpenCode request failed");
            return Err(HttpError {
                status: response.status,
                tag: value["_tag"].as_str().unwrap_or_default().into(),
                message: message.into(),
            }
            .into());
        }
        Ok(if value.get("cursor").is_some() {
            value
        } else {
            value.get("data").cloned().unwrap_or(value)
        })
    }

    pub async fn models(&self) -> Result<Value> {
        self.request(
            "GET",
            "/api/model",
            None,
            &[("location[directory]", &self.directory)],
        )
        .await
    }

    pub async fn agents(&self) -> Result<Value> {
        self.request(
            "GET",
            "/api/agent",
            None,
            &[("location[directory]", &self.directory)],
        )
        .await
    }

    pub async fn create_session(&self, body: Value) -> Result<Value> {
        self.request("POST", "/api/session", Some(body), &[]).await
    }

    pub async fn session(&self, id: &str) -> Result<Value> {
        self.request("GET", &format!("/api/session/{}", enc(id)), None, &[])
            .await
    }

    pub async fn update_session(&self, id: &str, body: Value) -> Result<Value> {
        self.request(
            "PATCH",
            &format!("/api/session/{}", enc(id)),
            Some(body),
            &[],
        )
        .await
    }

    pub async fn prompt(&self, id: &str, body: Value) -> Result<Value> {
        self.request(
            "POST",
            &format!("/api/session/{}/prompt", enc(id)),
            Some(body),
            &[],
        )
        .await
    }

    pub async fn switch_model(&self, id: &str, model: Value) -> Result<Value> {
        self.request(
            "POST",
            &format!("/api/session/{}/model", enc(id)),
            Some(json!({"model":model})),
            &[],
        )
        .await
    }

    pub async fn switch_agent(&self, id: &str, agent: &str) -> Result<Value> {
        self.request(
            "POST",
            &format!("/api/session/{}/agent", enc(id)),
            Some(json!({"agent":agent})),
            &[],
        )
        .await
    }

    pub async fn interrupt(&self, id: &str) -> Result<Value> {
        self.request(
            "POST",
            &format!("/api/session/{}/interrupt", enc(id)),
            None,
            &[],
        )
        .await
    }

    pub async fn compact(&self, id: &str) -> Result<Value> {
        self.request(
            "POST",
            &format!("/api/session/{}/compact", enc(id)),
            Some(json!({})),
            &[],
        )
        .await
    }

    pub async fn stage_revert(&self, id: &str, message: &str) -> Result<Value> {
        self.request(
            "POST",
            &format!("/api/session/{}/revert/stage", enc(id)),
            Some(json!({"messageID":message,"files":false})),
            &[],
        )
        .await
    }

    pub async fn active(&self) -> Result<Value> {
        self.request("GET", "/api/session/active", None, &[]).await
    }

    pub async fn pending_forms(&self, id: &str) -> Result<Value> {
        self.request("GET", &format!("/api/session/{}/form", enc(id)), None, &[])
            .await
    }

    pub async fn pending_permissions(&self) -> Result<Value> {
        self.request(
            "GET",
            "/api/permission/request",
            None,
            &[("location[directory]", &self.directory)],
        )
        .await
    }

    pub async fn commit_revert(&self, id: &str) -> Result<Value> {
        self.request(
            "POST",
            &format!("/api/session/{}/revert/commit", enc(id)),
            None,
            &[],
        )
        .await
    }

    pub async fn fork(&self, id: &str) -> Result<Value> {
        self.request(
            "POST",
            &format!("/api/session/{}/fork", enc(id)),
            Some(json!({})),
            &[],
        )
        .await
    }

    pub async fn move_to(&self, id: &str) -> Result<Value> {
        self.request(
            "POST",
            &format!("/api/session/{}/move", enc(id)),
            Some(json!({"directory":self.directory})),
            &[],
        )
        .await
    }

    pub async fn reply_permission(
        &self,
        session: &str,
        request: &str,
        decision: &str,
    ) -> Result<Value> {
        self.request(
            "POST",
            &format!(
                "/api/session/{}/permission/{}/reply",
                enc(session),
                enc(request)
            ),
            Some(json!({"decision":decision})),
            &[],
        )
        .await
    }

    pub async fn reply_form(&self, session: &str, form: &str, answer: Value) -> Result<Value> {
        self.request(
            "POST",
            &format!("/api/session/{}/form/{}/reply", enc(session), enc(form)),
            Some(json!({"answer":answer})),
            &[],
        )
        .await
    }

    pub async fn cancel_form(&self, session: &str, form: &str) -> Result<Value> {
        self.request(
            "DELETE",
            &format!("/api/session/{}/form/{}", enc(session), enc(form)),
            None,
            &[],
        )
        .await
    }

    pub async fn messages(&self, session: &str) -> Result<Vec<Value>> {
        let path = format!("/api/session/{}/message", enc(session));
        let mut messages = Vec::new();
        let mut cursor = None;
        let mut seen = HashSet::new();
        loop {
            let query = cursor
                .as_deref()
                .map(|cursor| vec![("cursor", cursor)])
                .unwrap_or_else(|| vec![("order", "asc")]);
            let page = self.request("GET", &path, None, &query).await?;
            let list = page
                .get("data")
                .or_else(|| page.get("items"))
                .or_else(|| page.get("messages"))
                .unwrap_or(&page);
            let rows = list
                .as_array()
                .ok_or_else(|| anyhow!("OpenCode 2 returned invalid session messages"))?;
            messages.extend(rows.iter().cloned());
            let next = page
                .pointer("/cursor/next")
                .and_then(Value::as_str)
                .map(str::to_string);
            if next.is_none() {
                break;
            }
            if !seen.insert(next.clone()) {
                bail!("OpenCode 2 repeated a message cursor");
            }
            cursor = next;
        }
        Ok(messages)
    }

    pub async fn subscribe(&self, stream: &str) -> Result<SseEvents> {
        let events = self.children.watch_sse(stream);
        let opened = self
            .children
            .open_harness_sse(
                stream,
                self.base.join("/api/event")?.as_str(),
                Some(self.headers()),
            )
            .await;
        if let Err(error) = opened {
            self.close_events(stream).await;
            return Err(error);
        }
        // The HTTP worker returns before the server has installed its subscriber.
        let ready = timeout(Duration::from_secs(10), async {
            while let Ok(event) = events.recv().await {
                match event {
                    SseEvent::Data(data)
                        if serde_json::from_str::<Value>(&data)
                            .ok()
                            .is_some_and(|event| event["type"] == "server.connected") =>
                    {
                        return Ok(());
                    }
                    SseEvent::End(error) => {
                        return Err(anyhow!(
                            "OpenCode 2 event stream ended before subscription: {}",
                            error.unwrap_or_default()
                        ));
                    }
                    _ => {}
                }
            }
            Err(anyhow!(
                "OpenCode 2 event stream closed before subscription"
            ))
        })
        .await;
        if !matches!(ready, Some(Ok(()))) {
            self.close_events(stream).await;
            return Err(ready
                .and_then(Result::err)
                .unwrap_or_else(|| anyhow!("OpenCode 2 event subscription timed out")));
        }
        Ok(events)
    }

    /// Recover durable boundaries missed while the volatile event stream was down.
    pub async fn log(&self, session: &str, after: u64) -> Result<Vec<Value>> {
        let stream = format!("opencode-v2-log-{}", uuid::Uuid::new_v4());
        let mut url = self
            .base
            .join(&format!("/api/experimental/session/{}/log", enc(session)))?;
        url.query_pairs_mut()
            .append_pair("after", &after.to_string())
            .append_pair("follow", "false");
        let events = self.children.watch_sse(&stream);
        let result = async {
            self.children
                .open_harness_sse(&stream, url.as_str(), Some(self.headers()))
                .await?;
            let read = timeout(Duration::from_secs(15), async {
                let mut values = Vec::new();
                while let Ok(event) = events.recv().await {
                    match event {
                        SseEvent::Data(data) => values
                            .push(serde_json::from_str(&data).map_err(|_| {
                                anyhow!("OpenCode 2 returned an invalid log event")
                            })?),
                        SseEvent::End(None) => return Ok(values),
                        SseEvent::End(Some(error)) => {
                            return Err(anyhow!("OpenCode 2 log failed: {error}"));
                        }
                    }
                }
                Err(anyhow!("OpenCode 2 log closed before completion"))
            })
            .await;
            read.ok_or_else(|| anyhow!("OpenCode 2 log recovery timed out"))?
        }
        .await;
        self.close_events(&stream).await;
        result
    }

    pub async fn close_events(&self, stream: &str) {
        let _ = self.children.close_harness_sse(stream).await;
        self.children.unwatch_sse(stream);
    }
}

fn enc(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
