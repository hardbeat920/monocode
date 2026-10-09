// Protocol adapted from cortexkit/antigravity-auth; see LICENSE in this directory.
use futures_util::StreamExt;
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::time::Duration;
use tokio::sync::watch;

pub const DAILY: &str = "https://daily-cloudcode-pa.googleapis.com";
pub const PROD: &str = "https://cloudcode-pa.googleapis.com";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo?alt=json";
pub const REDIRECT: &str = "http://localhost:51121/oauth-callback";
pub const CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
// Public desktop OAuth client, not a user credential.
pub const CLIENT_SECRET: &str = "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";
pub const SCOPES: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
}

impl NativeError {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retry_after: None,
        }
    }
    pub fn cancelled() -> Self {
        Self::new("cancelled", "Antigravity operation cancelled.")
    }
    pub fn auth() -> Self {
        Self::new(
            "auth",
            "Authentication required. Please sign in to Antigravity with Google.",
        )
    }
    pub fn storage() -> Self {
        Self::new("storage", "Could not read or save Antigravity data.")
    }
}

pub type Result<T> = std::result::Result<T, NativeError>;

#[derive(Clone)]
pub struct Cancel(watch::Sender<bool>);

impl Default for Cancel {
    fn default() -> Self {
        Self(watch::channel(false).0)
    }
}

impl Cancel {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }
    pub async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        while !*receiver.borrow_and_update() {
            if receiver.changed().await.is_err() {
                break;
            }
        }
    }
    pub async fn run<T>(
        &self,
        duration: Duration,
        future: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        tokio::select! {
            biased;
            _ = self.cancelled() => Err(NativeError::cancelled()),
            value = tokio::time::timeout(duration, future) => value.unwrap_or_else(|_| Err(NativeError::new("timeout", "Antigravity request timed out. Please retry."))),
        }
    }
}

#[derive(Clone)]
pub struct Transport {
    pub client: Client,
    pub endpoints: Vec<String>,
    pub token_url: String,
    pub userinfo_url: String,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            client: Client::builder()
                .http1_only()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(20))
                .gzip(true)
                .build()
                .expect("HTTP client"),
            endpoints: vec![DAILY.into(), PROD.into()],
            token_url: TOKEN_URL.into(),
            userinfo_url: USERINFO_URL.into(),
        }
    }
}

pub fn user_agent() -> String {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        other => other,
    };
    format!("antigravity/cli/1.1.24 (aidev_client; os_type=windows; arch={arch}; cl=974782877; auth_method=consumer)")
}

fn status_error(response: &Response) -> NativeError {
    let mut error = match response.status() {
        StatusCode::UNAUTHORIZED | StatusCode::BAD_REQUEST if response.url().as_str().contains("/token") => NativeError::auth(),
        StatusCode::UNAUTHORIZED => NativeError::auth(),
        StatusCode::FORBIDDEN => NativeError::new("ineligible", "This Google account is not eligible for Antigravity or its project permissions are missing."),
        StatusCode::TOO_MANY_REQUESTS => NativeError::new("quota", "Antigravity quota is exhausted. Wait before retrying."),
        status if status.is_server_error() => NativeError::new("network", "Antigravity is temporarily unavailable. Please retry."),
        _ => NativeError::new("protocol", "Antigravity rejected this request. The native transport may be incompatible with this model."),
    };
    error.retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    error
}

pub async fn checked(response: std::result::Result<Response, reqwest::Error>) -> Result<Response> {
    let response = response.map_err(|_| {
        NativeError::new(
            "network",
            "Could not reach Antigravity. Check your network and retry.",
        )
    })?;
    if !response.status().is_success() {
        return Err(status_error(&response));
    }
    Ok(response)
}

impl Transport {
    pub async fn form(&self, fields: &[(&str, &str)], cancel: &Cancel) -> Result<Value> {
        cancel
            .run(Duration::from_secs(30), async {
                checked(self.client.post(&self.token_url).form(fields).send().await)
                    .await?
                    .json()
                    .await
                    .map_err(|_| {
                        NativeError::new("protocol", "Invalid Google authentication response.")
                    })
            })
            .await
    }
    pub async fn post(
        &self,
        endpoint: &str,
        action: &str,
        token: &str,
        body: &Value,
        cancel: &Cancel,
    ) -> Result<Response> {
        // Streaming body forces HTTP/1.1 chunked framing, matching agy-transport.
        let bytes = serde_json::to_vec(body)
            .map_err(|_| NativeError::new("protocol", "Invalid Antigravity request."))?;
        let request = self
            .client
            .post(format!("{endpoint}/v1internal:{action}"))
            .bearer_auth(token)
            .header("User-Agent", user_agent())
            .header("Content-Type", "application/json")
            .header("Accept-Encoding", "gzip");
        let request = if action.starts_with("streamGenerateContent") {
            request.body(reqwest::Body::wrap_stream(futures_util::stream::once(
                async move { Ok::<_, std::io::Error>(bytes) },
            )))
        } else {
            request.body(bytes)
        };
        cancel
            .run(Duration::from_secs(180), async {
                checked(request.send().await).await
            })
            .await
    }
    pub async fn json(
        &self,
        action: &str,
        token: &str,
        body: &Value,
        cancel: &Cancel,
        prod_first: bool,
    ) -> Result<Value> {
        let mut endpoints = self.endpoints.clone();
        if prod_first {
            endpoints.reverse();
        }
        let mut last = NativeError::new("network", "Antigravity is unavailable.");
        for endpoint in endpoints {
            match self.post(&endpoint, action, token, body, cancel).await {
                Ok(response) => {
                    return cancel
                        .run(Duration::from_secs(30), async {
                            response.json().await.map_err(|_| {
                                NativeError::new("protocol", "Invalid Antigravity response.")
                            })
                        })
                        .await
                }
                Err(error) if error.code == "network" => last = error,
                Err(error) => return Err(error),
            }
        }
        Err(last)
    }
    pub async fn stream(
        &self,
        token: &str,
        body: &Value,
        cancel: &Cancel,
        mut on_chunk: impl FnMut(Value) -> Result<()>,
    ) -> Result<()> {
        let mut response = None;
        let mut last = NativeError::new("network", "Antigravity is unavailable.");
        // Only retry before a response begins. Never replay a partially streamed generation.
        for endpoint in &self.endpoints {
            match self
                .post(
                    endpoint,
                    "streamGenerateContent?alt=sse",
                    token,
                    body,
                    cancel,
                )
                .await
            {
                Ok(value) => {
                    response = Some(value);
                    break;
                }
                Err(error) if error.code == "network" => last = error,
                Err(error) => return Err(error),
            }
        }
        let mut stream = response.ok_or(last)?.bytes_stream();
        let mut parser = SseDecoder::default();
        loop {
            let next = cancel
                .run(Duration::from_secs(180), async {
                    stream.next().await.transpose().map_err(|_| {
                        NativeError::new(
                            "network",
                            "Antigravity streaming was interrupted. Please retry.",
                        )
                    })
                })
                .await?;
            match next {
                Some(chunk) => {
                    for value in parser.push(&chunk)? {
                        on_chunk(value)?;
                    }
                }
                None => {
                    for value in parser.finish()? {
                        on_chunk(value)?;
                    }
                    return Ok(());
                }
            }
        }
    }
}

#[derive(Default)]
pub struct SseDecoder {
    buffer: Vec<u8>,
    data: Vec<String>,
    data_len: usize,
}

impl SseDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Value>> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > 8 * 1024 * 1024 {
            return Err(NativeError::new(
                "protocol",
                "Antigravity stream frame is too large.",
            ));
        }
        let mut result = Vec::new();
        while let Some(index) = self.buffer.iter().position(|&b| b == b'\n') {
            let bytes: Vec<_> = self.buffer.drain(..=index).collect();
            let line = std::str::from_utf8(&bytes)
                .map_err(|_| NativeError::new("protocol", "Invalid Antigravity stream encoding."))?
                .trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                self.dispatch(&mut result)?;
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data_len += data.len();
                if self.data_len > 8 * 1024 * 1024 {
                    return Err(NativeError::new(
                        "protocol",
                        "Antigravity stream frame is too large.",
                    ));
                }
                self.data
                    .push(data.strip_prefix(' ').unwrap_or(data).into());
            }
        }
        Ok(result)
    }
    fn dispatch(&mut self, output: &mut Vec<Value>) -> Result<()> {
        if self.data.is_empty() {
            return Ok(());
        }
        let text = std::mem::take(&mut self.data).join("\n");
        self.data_len = 0;
        if text != "[DONE]" {
            output.push(serde_json::from_str(&text).map_err(|_| {
                NativeError::new("protocol", "Invalid Antigravity streaming response.")
            })?);
        }
        Ok(())
    }
    pub fn finish(&mut self) -> Result<Vec<Value>> {
        let mut output = self.push(b"\n")?;
        self.dispatch(&mut output)?;
        Ok(output)
    }
}
