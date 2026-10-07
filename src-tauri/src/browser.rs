//! Embedded browser tabs. Each tab is a native child webview laid over a DOM
//! placeholder in its workspace window; the page owns layout and tells us
//! where to draw. Browsed pages never get IPC: they load remote origins, and
//! no capability grants remote URLs access.
use serde::{Deserialize, Serialize};
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewBuilder,
    WebviewUrl, Window,
};

const LABEL_PREFIX: &str = "browser-";
const EVENT: &str = "browser-event";

#[derive(Clone, Copy, Deserialize)]
pub struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
enum BrowserEvent {
    /// A main-frame load started or finished at `url`.
    Load {
        id: String,
        url: String,
        loading: bool,
    },
    Title {
        id: String,
        title: String,
    },
    /// The page asked for a new window (`target=_blank`, `window.open`).
    OpenTab {
        id: String,
        url: String,
    },
}

fn label(id: &str) -> Result<String, String> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !valid {
        return Err("Invalid browser tab id".into());
    }
    Ok(format!("{LABEL_PREFIX}{id}"))
}

/// Only web pages. `file:`, `tauri:`, `javascript:` and friends stay out.
fn parse_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value.trim()).map_err(|_| "Invalid URL".to_string())?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        "about" if url.as_str() == "about:blank" => Ok(url),
        scheme => Err(format!("Unsupported URL scheme: {scheme}")),
    }
}

/// In dev the app itself is served from localhost. A tab pointed there would
/// load the app with the app's own IPC permissions, so refuse that origin.
fn is_app_origin(app: &AppHandle, url: &Url) -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    app.config()
        .build
        .dev_url
        .as_ref()
        .is_some_and(|dev| dev.origin() == url.origin())
}

fn find(app: &AppHandle, id: &str) -> Result<Webview, String> {
    app.get_webview(&label(id)?)
        .ok_or_else(|| "Browser tab is not open".to_string())
}

fn place(webview: &Webview, bounds: Bounds) -> Result<(), String> {
    webview
        .set_position(LogicalPosition::new(bounds.x, bounds.y))
        .map_err(|e| e.to_string())?;
    webview
        .set_size(LogicalSize::new(
            bounds.width.max(1.0),
            bounds.height.max(1.0),
        ))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_open(
    app: AppHandle,
    window: Window,
    id: String,
    url: String,
    bounds: Bounds,
    visible: bool,
) -> Result<(), String> {
    let label = label(&id)?;
    if let Some(webview) = app.get_webview(&label) {
        place(&webview, bounds)?;
        return if visible {
            webview.show().map_err(|e| e.to_string())
        } else {
            webview.hide().map_err(|e| e.to_string())
        };
    }
    let url = parse_url(&url)?;
    if is_app_origin(&app, &url) {
        return Err("That address serves MonoCode itself".into());
    }
    let owner = window.label().to_string();

    let emit = {
        let app = app.clone();
        let owner = owner.clone();
        move |event: BrowserEvent| {
            let _ = app.emit_to(owner.as_str(), EVENT, event);
        }
    };
    let on_load = {
        let emit = emit.clone();
        let id = id.clone();
        move |_: Webview, payload: tauri::webview::PageLoadPayload<'_>| {
            emit(BrowserEvent::Load {
                id: id.clone(),
                url: payload.url().to_string(),
                loading: matches!(payload.event(), PageLoadEvent::Started),
            });
        }
    };
    let on_title = {
        let emit = emit.clone();
        let id = id.clone();
        move |_: Webview, title: String| {
            emit(BrowserEvent::Title {
                id: id.clone(),
                title,
            });
        }
    };
    let on_new_window = {
        let emit = emit.clone();
        let id = id.clone();
        move |url: Url, _| {
            if parse_url(url.as_str()).is_ok() {
                emit(BrowserEvent::OpenTab {
                    id: id.clone(),
                    url: url.to_string(),
                });
            }
            NewWindowResponse::Deny
        }
    };
    let on_navigation = {
        let app = app.clone();
        move |url: &Url| !is_app_origin(&app, url)
    };

    let builder = WebviewBuilder::new(&label, WebviewUrl::External(url))
        .on_page_load(on_load)
        .on_document_title_changed(on_title)
        .on_new_window(on_new_window)
        .on_navigation(on_navigation);
    let webview = window
        .add_child(
            builder,
            LogicalPosition::new(bounds.x, bounds.y),
            LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)),
        )
        .map_err(|e| e.to_string())?;
    if !visible {
        webview.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn browser_set_bounds(app: AppHandle, id: String, bounds: Bounds) -> Result<(), String> {
    place(&find(&app, &id)?, bounds)
}

#[tauri::command]
pub fn browser_set_visible(app: AppHandle, id: String, visible: bool) -> Result<(), String> {
    let webview = find(&app, &id)?;
    if visible {
        webview.show()
    } else {
        webview.hide()
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_focus(app: AppHandle, id: String) -> Result<(), String> {
    find(&app, &id)?.set_focus().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_navigate(app: AppHandle, id: String, url: String) -> Result<(), String> {
    let url = parse_url(&url)?;
    if is_app_origin(&app, &url) {
        return Err("That address serves MonoCode itself".into());
    }
    find(&app, &id)?.navigate(url).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryAction {
    Back,
    Forward,
    Reload,
    Stop,
}

#[tauri::command]
pub fn browser_history(app: AppHandle, id: String, action: HistoryAction) -> Result<(), String> {
    let webview = find(&app, &id)?;
    match action {
        HistoryAction::Reload => webview.reload(),
        HistoryAction::Back => webview.eval("history.back()"),
        HistoryAction::Forward => webview.eval("history.forward()"),
        HistoryAction::Stop => webview.eval("window.stop()"),
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_close(app: AppHandle, id: String) -> Result<(), String> {
    match app.get_webview(&label(&id)?) {
        Some(webview) => webview.close().map_err(|e| e.to_string()),
        None => Ok(()),
    }
}

/// Close this window's tabs that the page no longer knows about. A webview
/// reload keeps native children alive while the page forgets them.
#[tauri::command]
pub fn browser_retain(window: Window, ids: Vec<String>) -> Result<(), String> {
    let keep: Vec<String> = ids.iter().filter_map(|id| label(id).ok()).collect();
    for webview in window.webviews() {
        let name = webview.label();
        if name.starts_with(LABEL_PREFIX) && !keep.iter().any(|label| label == name) {
            let _ = webview.close();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_reject_anything_but_plain_ids() {
        assert_eq!(label("abc-1_2").unwrap(), "browser-abc-1_2");
        assert!(label("").is_err());
        assert!(label("../main").is_err());
        assert!(label(&"x".repeat(65)).is_err());
    }

    #[test]
    fn only_web_urls_open() {
        assert!(parse_url("https://example.com").is_ok());
        assert!(parse_url("http://localhost:3000/a?b").is_ok());
        assert!(parse_url("about:blank").is_ok());
        assert!(parse_url("file:///etc/passwd").is_err());
        assert!(parse_url("javascript:alert(1)").is_err());
        assert!(parse_url("tauri://localhost").is_err());
        assert!(parse_url("not a url").is_err());
    }
}
