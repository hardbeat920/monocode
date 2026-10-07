//! Embedded browser tabs. Each tab is a native child webview laid over a DOM
//! placeholder in its workspace window; the page owns layout and tells us
//! where to draw. Browsed pages never get IPC: they load remote origins, and
//! no capability grants remote URLs access.
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewBuilder,
    WebviewUrl, Window,
};

const LABEL_PREFIX: &str = "browser-";
const EVENT: &str = "browser-event";
const CONSOLE_CAPTURE: &str = include_str!("browser_console.js");
const EVAL_TIMEOUT_MAX: Duration = Duration::from_secs(25);
const EVAL_POLL: Duration = Duration::from_millis(40);

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
        .on_navigation(on_navigation)
        .initialization_script(CONSOLE_CAPTURE);
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

/// Evaluate `js` and hand back its JSON-encoded result. Native evaluation
/// cannot return promises or arbitrary objects, so callers always produce a
/// string; wry encodes that string as JSON once more.
fn eval_string(webview: &Webview, js: String, timeout: Duration) -> Result<String, String> {
    let (tx, rx) = mpsc::channel();
    webview
        .eval_with_callback(js, move |raw| {
            let _ = tx.send(raw);
        })
        .map_err(|e| e.to_string())?;
    let raw = rx
        .recv_timeout(timeout)
        .map_err(|_| "The page did not respond".to_string())?;
    Ok(serde_json::from_str::<String>(&raw).unwrap_or_default())
}

/// Wrap an agent script as the body of an async function. A script with no
/// `return` is treated as one expression so `document.title` just works.
fn agent_script(key: &str, script: &str) -> String {
    let body = if script.contains("return") {
        script.to_string()
    } else {
        format!("return (\n{script}\n);")
    };
    let key = serde_json::to_string(key).unwrap_or_default();
    format!(
        r#"(function () {{
  var store = window.__monocodeResults || (window.__monocodeResults = {{}});
  var key = {key};
  function encode(value) {{
    var seen = new WeakSet();
    return JSON.stringify(value === undefined ? null : value, function (_, v) {{
      if (typeof Node !== "undefined" && v instanceof Node) {{
        var html = v.outerHTML || v.textContent || "";
        return html.length > 20000 ? html.slice(0, 20000) + "…" : html;
      }}
      if (typeof v === "function") return "[Function " + (v.name || "anonymous") + "]";
      if (typeof v === "bigint") return v.toString();
      if (v && typeof v === "object") {{
        if (seen.has(v)) return "[Circular]";
        seen.add(v);
      }}
      return v;
    }});
  }}
  store[key] = {{ pending: true }};
  (async function () {{
{body}
  }})().then(
    function (value) {{
      try {{ store[key] = {{ ok: true, json: encode(value) }}; }}
      catch (e) {{ store[key] = {{ ok: false, error: "Result is not serializable: " + e }}; }}
    }},
    function (e) {{ store[key] = {{ ok: false, error: String((e && e.stack) || e) }}; }}
  );
  return "started";
}})()"#
    )
}

fn result_poll(key: &str) -> String {
    let key = serde_json::to_string(key).unwrap_or_default();
    format!(
        r#"(function () {{
  var store = window.__monocodeResults, key = {key};
  var entry = store && store[key];
  if (!entry) return "lost";
  if (entry.pending) return "";
  delete store[key];
  return JSON.stringify(entry);
}})()"#
    )
}

fn run_agent_script(webview: &Webview, script: &str, timeout: Duration) -> Result<Value, String> {
    let key = uuid::Uuid::new_v4().simple().to_string();
    let started = eval_string(webview, agent_script(&key, script), Duration::from_secs(5))?;
    if started != "started" {
        return Err("The script did not start. Check it for syntax errors.".into());
    }
    let deadline = Instant::now() + timeout;
    loop {
        let reply = eval_string(webview, result_poll(&key), Duration::from_secs(5))?;
        match reply.as_str() {
            "" => {}
            "lost" => return Err("The page navigated away before the script finished".into()),
            entry => {
                let entry: Value =
                    serde_json::from_str(entry).map_err(|_| "Unreadable script result")?;
                if entry["ok"].as_bool() == Some(true) {
                    let json = entry["json"].as_str().unwrap_or("null");
                    return Ok(serde_json::from_str(json).unwrap_or(Value::Null));
                }
                return Err(entry["error"]
                    .as_str()
                    .unwrap_or("Script failed")
                    .to_string());
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Script still running after {} ms",
                timeout.as_millis()
            ));
        }
        std::thread::sleep(EVAL_POLL);
    }
}

/// Run an agent's script in a tab and return its JSON result.
#[tauri::command]
pub async fn browser_eval(
    app: AppHandle,
    id: String,
    script: String,
    timeout_ms: Option<u64>,
) -> Result<Value, String> {
    let webview = find(&app, &id)?;
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(10_000)).min(EVAL_TIMEOUT_MAX);
    tauri::async_runtime::spawn_blocking(move || run_agent_script(&webview, &script, timeout))
        .await
        .map_err(|e| e.to_string())?
}

/// The visible part of a tab as base64 PNG.
#[tauri::command]
pub async fn browser_screenshot(app: AppHandle, id: String) -> Result<String, String> {
    let webview = find(&app, &id)?;
    tauri::async_runtime::spawn_blocking(move || snapshot_png(&webview))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(target_os = "macos")]
fn snapshot_png(webview: &Webview) -> Result<String, String> {
    use base64::Engine;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use objc2_web_kit::WKWebView;

    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>();
    webview
        .with_webview(move |platform| {
            let view = platform.inner().cast::<WKWebView>();
            if view.is_null() {
                let _ = tx.send(Err("Browser view is gone".into()));
                return;
            }
            // SAFETY: wry hands us its live WKWebView on the main thread.
            let view = unsafe { &*view };
            let tx = tx.clone();
            let handler = block2::RcBlock::new(move |image: *mut NSImage, _: *mut NSError| {
                // SAFETY: WebKit passes a valid image or null.
                let png = unsafe { image.as_ref() }
                    .and_then(|image| image.TIFFRepresentation())
                    .and_then(|tiff| NSBitmapImageRep::imageRepWithData(&tiff))
                    .and_then(|rep| unsafe {
                        rep.representationUsingType_properties(
                            NSBitmapImageFileType::PNG,
                            &NSDictionary::new(),
                        )
                    })
                    .map(|data| data.to_vec())
                    .ok_or_else(|| "Could not capture the page".to_string());
                let _ = tx.send(png);
            });
            unsafe { view.takeSnapshotWithConfiguration_completionHandler(None, &handler) };
        })
        .map_err(|e| e.to_string())?;
    let png = rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "Timed out capturing the page".to_string())??;
    Ok(base64::engine::general_purpose::STANDARD.encode(png))
}

#[cfg(not(target_os = "macos"))]
fn snapshot_png(_webview: &Webview) -> Result<String, String> {
    Err("Browser screenshots are only available on macOS for now".into())
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
    fn agent_scripts_are_async_bodies_and_expressions_return() {
        let wrapped = agent_script("k1", "document.title");
        assert!(wrapped.contains("return (\ndocument.title\n);"));
        assert!(wrapped.contains("var key = \"k1\";"));
        let body = agent_script("k2", "const a = 1;\nreturn a;");
        assert!(body.contains("const a = 1;\nreturn a;"));
        assert!(!body.contains("return (\nconst"));
        assert!(result_poll("k\"x").contains(r#"key = "k\"x""#));
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
