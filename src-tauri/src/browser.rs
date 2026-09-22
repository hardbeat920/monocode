//! An unprivileged native webview embedded in an existing app window.
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};
use tauri::{
    webview::WebviewBuilder, Manager, PhysicalPosition, PhysicalSize, Webview, WebviewUrl,
};

// WKWebView's URL may be nil during loading/teardown. Wry's url() unwraps it,
// so keep the address supplied by native navigation callbacks instead.
static URLS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn record_url(label: &str, url: &tauri::Url) {
    if let Some(current) = URLS.lock().unwrap().get_mut(label) {
        *current = url.to_string();
    }
}

pub fn restrict_commands(
    handler: impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static,
) -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke| {
        let label = invoke.message.webview_ref().label();
        if label.starts_with("embedded-browser-") {
            invoke
                .resolver
                .reject("Browser pages cannot call app commands");
            return true;
        }
        handler(invoke)
    }
}

#[derive(Clone, Copy, Deserialize)]
pub struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Bounds {
    fn validate(self) -> Result<Self, String> {
        if [self.x, self.y, self.width, self.height]
            .iter()
            .any(|n| !n.is_finite() || *n < 0.0 || *n > 100_000.0)
            || self.width < 1.0
            || self.height < 1.0
        {
            return Err("Invalid browser bounds".into());
        }
        Ok(self)
    }

    fn position(self) -> PhysicalPosition<f64> {
        PhysicalPosition::new(self.x, self.y)
    }

    fn size(self) -> PhysicalSize<f64> {
        PhysicalSize::new(self.width, self.height)
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum BrowserRequest {
    Open { url: String, bounds: Bounds },
    Layout { bounds: Bounds, visible: bool },
    Back,
    Forward,
    Reload,
    Close,
    Url,
}

fn allowed_url(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && !matches!(
            url.host_str(),
            Some("tauri.localhost" | "asset.localhost" | "ipc.localhost")
        )
}

pub fn label(window: &str) -> String {
    format!("embedded-browser-{window}")
}

// Must be async: add_child schedules work on the UI thread and waits for it.
#[tauri::command]
pub async fn embedded_browser(
    caller: Webview,
    request: BrowserRequest,
) -> Result<Option<String>, String> {
    let window = caller.window();
    if caller.label() != window.label() {
        return Err("Only the app UI may control the browser".into());
    }
    let app = caller.app_handle();
    let label = label(window.label());
    let existing = app.get_webview(&label);
    let result = match request {
        BrowserRequest::Open { url, bounds } => {
            let url = tauri::Url::parse(&url).map_err(|e| e.to_string())?;
            if !allowed_url(&url) {
                return Err("Enter an http or https address".into());
            }
            let bounds = bounds.validate()?;
            if let Some(view) = existing {
                view.navigate(url)
            } else {
                let popup_app = app.clone();
                let popup_label = label.clone();
                let navigation_label = label.clone();
                URLS.lock().unwrap().insert(label.clone(), url.to_string());
                let builder = WebviewBuilder::new(label.clone(), WebviewUrl::External(url))
                    .on_navigation(move |url| {
                        if !allowed_url(url) {
                            return false;
                        }
                        record_url(&navigation_label, url);
                        true
                    })
                    .on_page_load(|view, payload| record_url(view.label(), payload.url()))
                    .on_new_window(move |url, _| {
                        // target=_blank stays inside this browser, including its history.
                        if allowed_url(&url) {
                            let app = popup_app.clone();
                            let label = popup_label.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Some(view) = app.get_webview(&label) {
                                    let _ = view.navigate(url);
                                }
                            });
                        }
                        tauri::webview::NewWindowResponse::Deny
                    });
                let created = window
                    .add_child(builder, bounds.position(), bounds.size())
                    .map(|_| ());
                if created.is_err() {
                    URLS.lock().unwrap().remove(&label);
                }
                created
            }
        }
        BrowserRequest::Close => {
            let closed = existing.map_or(Ok(()), |view| view.close());
            if closed.is_ok() {
                URLS.lock().unwrap().remove(&label);
            }
            closed
        }
        BrowserRequest::Layout { bounds, visible } => {
            if let Some(view) = existing {
                if visible {
                    let bounds = bounds.validate()?;
                    view.set_bounds(tauri::Rect {
                        position: bounds.position().into(),
                        size: bounds.size().into(),
                    })
                    .and_then(|()| view.show())
                } else {
                    view.hide()
                }
            } else {
                Ok(())
            }
        }
        action => {
            let view = existing.ok_or("Open an address first")?;
            match action {
                BrowserRequest::Back => view.eval("history.back()"),
                BrowserRequest::Forward => view.eval("history.forward()"),
                BrowserRequest::Reload => view.reload(),
                BrowserRequest::Url => {
                    return Ok(URLS.lock().unwrap().get(&label).cloned());
                }
                _ => unreachable!(),
            }
        }
    };
    result.map(|()| None).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_navigation_stays_unprivileged() {
        for url in ["https://example.com", "http://localhost:3000"] {
            assert!(allowed_url(&tauri::Url::parse(url).unwrap()));
        }
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "tauri://localhost",
            "http://tauri.localhost",
            "https://asset.localhost/a",
            "http://ipc.localhost",
        ] {
            assert!(!allowed_url(&tauri::Url::parse(url).unwrap()));
        }
    }

    #[test]
    fn bounds_reject_invalid_native_dimensions() {
        let bounds = Bounds {
            x: 0.0,
            y: 40.0,
            width: 600.0,
            height: 400.0,
        };
        assert!(bounds.validate().is_ok());
        for width in [0.0, -1.0, f64::NAN, f64::INFINITY, 100_001.0] {
            assert!(Bounds { width, ..bounds }.validate().is_err());
        }
    }
}
