//! Embedded browser tabs. Each tab is a native child webview laid over a DOM
//! placeholder in its workspace window; the page owns layout and tells us
//! where to draw. Browsed pages never get IPC: they load remote origins, and
//! no capability grants remote URLs access.
use std::sync::{mpsc, Mutex};
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

/// Serializes "is the tab already open?" with creating its child webview, so
/// two opens of one id cannot both miss the lookup and race on the label.
static OPEN_LOCK: Mutex<()> = Mutex::new(());

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

/// The app's own origins: the dev server and any external URL a window is
/// configured with. Read from config, not from live webviews: `Webview::url`
/// can fail (and panics on macOS before the native URL exists).
fn app_origins(app: &AppHandle) -> Vec<Url> {
    app_origins_from(app.config(), tauri::is_dev())
}

/// The dev server is the app's origin only in a dev build (no
/// `custom-protocol`); otherwise the app is served from `tauri://` and the dev
/// server's address is an ordinary page a user may be developing.
fn app_origins_from(config: &tauri::Config, dev: bool) -> Vec<Url> {
    let mut own: Vec<Url> = if dev {
        config.build.dev_url.iter().cloned().collect()
    } else {
        Vec::new()
    };
    for window in &config.app.windows {
        if let WebviewUrl::External(url) = &window.url {
            own.push(url.clone());
        }
    }
    own
}

/// A tab on any app origin would run with the app's IPC permissions, so none
/// may be browsed to, in any build or on any platform.
fn is_app_origin(app: &AppHandle, url: &Url) -> bool {
    matches_app_origin(url, &app_origins(app))
}

fn matches_app_origin(url: &Url, own: &[Url]) -> bool {
    is_internal_origin(url) || own.iter().any(|app| app.origin() == url.origin())
}

/// Origins Tauri serves bundled assets and IPC from: `tauri://localhost`,
/// `ipc://`/`asset:`, and `http(s)://{tauri,ipc,asset}.localhost`
/// (Windows/Android), including subdomains.
fn is_internal_origin(url: &Url) -> bool {
    match url.scheme() {
        "tauri" | "ipc" | "asset" => true,
        _ => url.host_str().is_some_and(|host| {
            ["tauri", "ipc", "asset"].iter().any(|name| {
                let internal = format!("{name}.localhost");
                host == internal || host.ends_with(&format!(".{internal}"))
            })
        }),
    }
}

/// Every navigation, including redirects and link clicks, passes through here.
fn allow_navigation(app: &AppHandle, url: &Url) -> bool {
    navigation_allowed(url, &app_origins(app))
}

/// On macOS and Linux this also sees subframe navigations, with no frame to
/// tell them apart. So it admits the documents frames are built from, which
/// take their origin from a page already allowed: `about:blank` and
/// `about:srcdoc` with any query or fragment, and `blob:` URLs of a web origin
/// other than the app's. `data:` stays out: it would also open top-level.
fn navigation_allowed(url: &Url, own: &[Url]) -> bool {
    match url.scheme() {
        "about" => matches!(url.path(), "blank" | "srcdoc"),
        "blob" => Url::parse(url.path()).is_ok_and(|inner| {
            matches!(inner.scheme(), "http" | "https") && !matches_app_origin(&inner, own)
        }),
        _ => parse_url(url.as_str()).is_ok() && !matches_app_origin(url, own),
    }
}

/// Run in every frame before page scripts. Wry's navigation hook only sees
/// main-frame navigations on Windows (WebView2 `NavigationStarting`), so a
/// frame pointed at an app origin would load it; empty such frames instead.
fn frame_guard_script(own: &[Url]) -> String {
    let origins: Vec<String> = own
        .iter()
        .map(|url| url.origin().ascii_serialization())
        .collect();
    let origins = serde_json::to_string(&origins).unwrap_or_else(|_| "[]".into());
    format!(
        r#"(function () {{
  if (window.top === window) return;
  var own = {origins};
  var p = location.protocol, h = location.hostname;
  var internal = p === "tauri:" || p === "ipc:" || p === "asset:" ||
    /(^|\.)(tauri|ipc|asset)\.localhost$/.test(h);
  if (!internal && own.indexOf(location.origin) < 0) return;
  try {{ window.stop(); }} catch (e) {{}}
  // Runs before the document is parsed, so there may be no root yet.
  try {{ location.replace("about:blank"); }} catch (e) {{}}
  if (document.documentElement) document.documentElement.textContent = "";
}})();"#
    )
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

/// Async so creation runs off the main thread: on Windows, building a child
/// webview from a synchronous command (which runs on the main thread) deadlocks.
#[tauri::command]
pub async fn browser_open(
    app: AppHandle,
    window: Window,
    id: String,
    url: String,
    bounds: Bounds,
    visible: bool,
) -> Result<(), String> {
    let label = label(&id)?;
    let _open = OPEN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        move |url: &Url| allow_navigation(&app, url)
    };

    let builder = WebviewBuilder::new(&label, WebviewUrl::External(url))
        .on_page_load(on_load)
        .on_document_title_changed(on_title)
        .on_new_window(on_new_window)
        .on_navigation(on_navigation)
        .initialization_script_for_all_frames(frame_guard_script(&app_origins(&app)))
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

/// The page's current URL. Same-document navigation (`pushState`, hash
/// changes) emits no load event, so the page is asked directly.
#[tauri::command]
pub async fn browser_url(app: AppHandle, id: String) -> Result<String, String> {
    let webview = find(&app, &id)?;
    tauri::async_runtime::spawn_blocking(move || {
        eval_string(&webview, "location.href".into(), Duration::from_secs(2))
    })
    .await
    .map_err(|e| e.to_string())?
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

/// How long a fragment target waits for the page to say whether it stays in the
/// document. A page that does not answer (hung, still blank) cannot apply a
/// fragment either, so it is sent to the webview like any other target.
const FRAGMENT_PROBE: Duration = Duration::from_secs(1);

/// The page refuses to start a probe this much before the native side gives up
/// on it, so a probe that was submitted but ran late does not begin changing
/// the page after the target was handed to the webview instead. It cannot stop
/// a probe that started in time from finishing late: the page's synchronous
/// handlers run inside it for as long as they take.
const PROBE_MARGIN: Duration = Duration::from_millis(200);

/// Runs in the page. When `target` differs from the page's URL only by its
/// fragment, the page applies it itself and reports what its URL is afterwards.
/// `location.href` changes synchronously only for same-document navigations
/// (fragments, `pushState`, `replaceState`); a document load changes it when it
/// commits, later. So:
///
/// - "same:<url>": the URL changed, so the page stayed in its document, at
///   `<url>`: the target, or wherever its handlers rewrote it to.
/// - "unchanged": the URL did not change. A handler refused the navigation,
///   or replaced it with a document load that has not committed yet.
/// - "unknown": the assignment threw; the page may have done anything.
/// - "other": the target is a different document; nothing was changed.
/// - "expired": the probe started after `deadline` (ms since the epoch) and
///   did nothing.
///
/// Both sides are normalized by the page, so the comparison cannot be fooled by
/// how the target was spelled. The page's own scripts can replace `Date`, `URL`
/// or `location` behavior, and handlers that react later change the URL
/// unseen; the answer is only as good as the page's built-ins.
fn fragment_probe(target: &Url, deadline_ms: u128) -> String {
    let target = serde_json::to_string(target.as_str()).unwrap_or_default();
    format!(
        r##"(function () {{
  var assigned = false;
  try {{
    if (Date.now() > {deadline_ms}) return "expired";
    var to = new URL({target}), from = new URL(location.href);
    var toBase = new URL(to.href), fromBase = new URL(from.href);
    toBase.hash = ""; fromBase.hash = "";
    if (to.href.indexOf("#") < 0 || toBase.href !== fromBase.href) return "other";
    assigned = true;
    location.href = {target};
    var now = new URL(location.href).href;
    return now === from.href && now !== to.href ? "unchanged" : "same:" + now;
  }} catch (e) {{
    return assigned ? "unknown" : "other";
  }}
}})()"##
    )
}

/// What the native side knows about a navigation it was asked to make.
#[derive(Serialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// The page stayed in its document: no load follows, and `url` is where
    /// the page's synchronous handlers left it.
    Applied,
    /// The webview was told to load a document. Load events report what
    /// happens next, but a load that fails before it commits or is denied may
    /// report none.
    Document,
    /// It is not known whether a load follows: the page refused the target or
    /// replaced it with a load not yet committed, or the native navigation of a
    /// fragment target may land in the same document without any event.
    Unknown,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct Navigation {
    outcome: Outcome,
    /// The page's URL right after an applied navigation.
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

impl Navigation {
    fn of(outcome: Outcome) -> Self {
        Navigation { outcome, url: None }
    }
}

/// What to do with the page's answer to a probe.
#[derive(Debug, PartialEq, Eq)]
enum Probed {
    /// The page's answer is the outcome; the webview is not navigated.
    Settled(Navigation),
    /// The target is another document: navigate the webview.
    Document,
    /// No usable answer (none in time, expired, unexpected): navigate the
    /// webview without knowing whether the page stays in its document.
    Unanswered,
}

fn judge_probe(answer: &Result<String, String>) -> Probed {
    match answer.as_deref() {
        Ok(answer) => {
            if let Some(url) = answer.strip_prefix("same:") {
                return Probed::Settled(Navigation {
                    outcome: Outcome::Applied,
                    url: Some(url.to_string()),
                });
            }
            match answer {
                // The page already acted (or refused); navigating natively
                // would be a second action.
                "unchanged" | "unknown" => Probed::Settled(Navigation::of(Outcome::Unknown)),
                "other" => Probed::Document,
                _ => Probed::Unanswered,
            }
        }
        Err(_) => Probed::Unanswered,
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

/// Point the tab at `url`. A target with a fragment is first offered to the
/// page, because the webview's navigation hook is not told about same-document
/// navigations on every platform and reports only a URL, never which request or
/// frame it belongs to; the page's own answer is the one completion that is
/// known. Everything else, including an unanswered probe, is handed to the
/// webview.
///
/// The frontend sends one navigation per tab at a time, so they are dispatched
/// in the order they were made. That orders dispatch only: it does not order
/// every eventual effect on the page, such as a load that starts after the next
/// navigation was dispatched.
#[tauri::command]
pub async fn browser_navigate(
    app: AppHandle,
    id: String,
    url: String,
) -> Result<Navigation, String> {
    let url = parse_url(&url)?;
    if is_app_origin(&app, &url) {
        return Err("That address serves MonoCode itself".into());
    }
    let webview = find(&app, &id)?;
    let mut outcome = Outcome::Document;
    if url.fragment().is_some() {
        let deadline = now_ms() + (FRAGMENT_PROBE - PROBE_MARGIN).as_millis();
        let probe = fragment_probe(&url, deadline);
        let page = webview.clone();
        let answer =
            tauri::async_runtime::spawn_blocking(move || eval_string(&page, probe, FRAGMENT_PROBE))
                .await
                .map_err(|e| e.to_string())?;
        match judge_probe(&answer) {
            Probed::Settled(navigation) => return Ok(navigation),
            Probed::Document => {}
            // Whether the target stays in the current document is not known.
            // The webview's own URL cannot settle it: `Webview::url` panics on
            // macOS while a hung or blank page has no URL yet.
            Probed::Unanswered => outcome = Outcome::Unknown,
        }
    }
    webview.navigate(url).map_err(|e| e.to_string())?;
    Ok(Navigation::of(outcome))
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

/// Wrap an agent script as the body of an async function. With `expression`
/// the script is returned as one expression (`document.title`,
/// `[1,2].map(x => { return x * 2; })`); otherwise it runs as a function body
/// (statements, an explicit `return`). Whether a script is an expression is for
/// the page's JavaScript parser to say, so `run_agent_script` tries the
/// expression form first and falls back when the page cannot compile it.
fn agent_script(key: &str, script: &str, expression: bool) -> String {
    let body = if expression {
        let expr = script
            .trim()
            .trim_end_matches(|c: char| c == ';' || c.is_whitespace());
        format!("return (\n{expr}\n);")
    } else {
        script.to_string()
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
  var pending = {{ pending: true }};
  store[key] = pending;
  function settle(entry) {{
    // Publish only while this run still owns the key; an abandoned run (entry
    // deleted) or a reused key must not be overwritten by a late result.
    if (store[key] === pending) store[key] = entry;
  }}
  (async function () {{
{body}
  }})().then(
    function (value) {{
      try {{ settle({{ ok: true, json: encode(value) }}); }}
      catch (e) {{ settle({{ ok: false, error: "Result is not serializable: " + e }}); }}
    }},
    function (e) {{ settle({{ ok: false, error: String((e && e.stack) || e) }}); }}
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

/// Forget a run the caller stopped waiting for. The entry is deleted at once;
/// a run still going sees it is no longer the owner and drops its late result.
fn result_abandon(key: &str) -> String {
    let key = serde_json::to_string(key).unwrap_or_default();
    format!(
        r#"(function () {{
  var store = window.__monocodeResults, key = {key};
  if (store) delete store[key];
  return "";
}})()"#
    )
}

fn run_agent_script(webview: &Webview, script: &str, timeout: Duration) -> Result<Value, String> {
    let key = uuid::Uuid::new_v4().simple().to_string();
    let result = run_agent_script_keyed(webview, &key, script, timeout);
    if result.is_err() {
        let _ = eval_string(webview, result_abandon(&key), Duration::from_secs(1));
    }
    result
}

fn run_agent_script_keyed(
    webview: &Webview,
    key: &str,
    script: &str,
    timeout: Duration,
) -> Result<Value, String> {
    let key = key.to_string();
    let mut started = String::new();
    for expression in [true, false] {
        started = eval_string(
            webview,
            agent_script(&key, script, expression),
            Duration::from_secs(5),
        )?;
        if started == "started" {
            break;
        }
    }
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
    fn fragment_probe_embeds_the_target_as_a_json_string() {
        let target = Url::parse("https://a.test/p?q=\"x\"#frag\"</script>").unwrap();
        let script = fragment_probe(&target, 123);
        let quoted = serde_json::to_string(target.as_str()).unwrap();
        assert_eq!(script.matches(&quoted).count(), 2);
        assert!(script.contains("location.href = "));
        assert!(script.contains("Date.now() > 123"));
    }

    #[test]
    fn probe_answers_map_to_outcomes() {
        let ok = |s: &str| Ok::<String, String>(s.to_string());
        assert_eq!(
            judge_probe(&ok("same:https://a.test/#c")),
            Probed::Settled(Navigation {
                outcome: Outcome::Applied,
                url: Some("https://a.test/#c".into()),
            })
        );
        for unknown in ["unchanged", "unknown"] {
            assert_eq!(
                judge_probe(&ok(unknown)),
                Probed::Settled(Navigation::of(Outcome::Unknown))
            );
        }
        assert_eq!(judge_probe(&ok("other")), Probed::Document);
        assert_eq!(judge_probe(&ok("expired")), Probed::Unanswered);
        assert_eq!(judge_probe(&ok("")), Probed::Unanswered);
        assert_eq!(
            judge_probe(&Err("The page did not respond".into())),
            Probed::Unanswered
        );
    }

    #[test]
    fn navigation_serializes_like_the_frontend_expects() {
        let applied = Navigation {
            outcome: Outcome::Applied,
            url: Some("https://a.test/#b".into()),
        };
        assert_eq!(
            serde_json::to_value(&applied).unwrap(),
            serde_json::json!({ "outcome": "applied", "url": "https://a.test/#b" })
        );
        assert_eq!(
            serde_json::to_value(Navigation::of(Outcome::Document)).unwrap(),
            serde_json::json!({ "outcome": "document" })
        );
        assert_eq!(
            serde_json::to_value(Navigation::of(Outcome::Unknown)).unwrap(),
            serde_json::json!({ "outcome": "unknown" })
        );
    }

    /// Run the generated probe in Node against a stub `location` whose setter
    /// behaves like the page's handlers would. `None` when Node is unavailable.
    fn run_probe(cases: &[(&str, &str, &str, u128)]) -> Option<Vec<Value>> {
        let driver = r##"
          const cases = JSON.parse(process.argv[1]);
          const results = [];
          for (const { start, handler, script } of cases) {
            let href = start, assigned = 0;
            globalThis.location = {
              get href() { return href; },
              set href(to) {
                assigned++;
                const next = new URL(to, href).href;
                if (handler === "apply") href = next;
                else if (handler === "rewrite-fragment") href = new URL("#c", href).href;
                else if (handler === "rewrite-path") href = new URL("/other#x", href).href;
                else if (handler === "throw") { href = next; throw new Error("handler"); }
                // "refuse": the URL stays; a document load would commit later.
              },
            };
            results.push({ answer: (0, eval)(script), href, assigned });
          }
          console.log(JSON.stringify(results));
        "##;
        let payload: Vec<Value> = cases
            .iter()
            .map(|(start, target, handler, deadline)| {
                serde_json::json!({
                    "start": start,
                    "handler": handler,
                    "script": fragment_probe(&Url::parse(target).unwrap(), *deadline),
                })
            })
            .collect();
        let out = run_node(driver, &serde_json::to_string(&payload).unwrap())?;
        Some(serde_json::from_slice(&out.stdout).unwrap())
    }

    #[test]
    fn fragment_probe_executes() {
        let future = now_ms() + 60_000;
        let Some(results) = run_probe(&[
            ("https://a.test/p", "https://a.test/p#b", "apply", future),
            ("https://a.test/p", "https://a.test/p#b", "refuse", future),
            (
                "https://a.test/p",
                "https://a.test/p#b",
                "rewrite-fragment",
                future,
            ),
            (
                "https://a.test/p",
                "https://a.test/p#b",
                "rewrite-path",
                future,
            ),
            ("https://a.test/p", "https://a.test/p#b", "throw", future),
            ("https://a.test/p", "https://b.test/p#b", "apply", future),
            ("https://a.test/p#b", "https://a.test/p#b", "refuse", future),
            ("https://a.test/p", "https://a.test/p#", "apply", future),
            ("https://a.test/p", "https://a.test/p#b", "apply", 0),
        ]) else {
            return;
        };
        let got: Vec<(String, u64)> = results
            .iter()
            .map(|r| {
                (
                    r["answer"].as_str().unwrap().to_string(),
                    r["assigned"].as_u64().unwrap(),
                )
            })
            .collect();
        let want = [
            ("same:https://a.test/p#b", 1),
            ("unchanged", 1),
            ("same:https://a.test/p#c", 1),
            ("same:https://a.test/other#x", 1),
            ("unknown", 1),
            ("other", 0),
            // Already at the target: nothing to refuse.
            ("same:https://a.test/p#b", 1),
            ("same:https://a.test/p#", 1),
            // A probe that starts late does not touch the page.
            ("expired", 0),
        ];
        let want: Vec<(String, u64)> = want.iter().map(|(a, n)| (a.to_string(), *n)).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn only_targets_with_a_fragment_are_probed() {
        let has = |s: &str| Url::parse(s).unwrap().fragment().is_some();
        assert!(has("https://a.test/#"));
        assert!(has("https://a.test/p#x"));
        assert!(!has("https://a.test/p"));
    }

    #[test]
    fn labels_reject_anything_but_plain_ids() {
        assert_eq!(label("abc-1_2").unwrap(), "browser-abc-1_2");
        assert!(label("").is_err());
        assert!(label("../main").is_err());
        assert!(label(&"x".repeat(65)).is_err());
    }

    #[test]
    fn agent_scripts_wrap_expressions_and_bodies() {
        let expr = agent_script("k1", "document.title;\n", true);
        assert!(expr.contains("return (\ndocument.title\n);"));
        assert!(expr.contains("var key = \"k1\";"));
        let call = agent_script("k1", "[1,2].map(x => { return x*2; })", true);
        assert!(call.contains("return (\n[1,2].map(x => { return x*2; })\n);"));
        let body = agent_script("k2", "const a = 1;\nreturn a;", false);
        assert!(body.contains("const a = 1;\nreturn a;"));
        assert!(!body.contains("return (\nconst"));
        assert!(result_poll("k\"x").contains(r#"key = "k\"x""#));
    }

    fn run_node(driver: &str, args: &str) -> Option<std::process::Output> {
        match std::process::Command::new("node")
            .args(["-e", driver, args])
            .output()
        {
            Ok(out) => {
                assert!(
                    out.status.success(),
                    "Node execution failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
                Some(out)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                assert!(std::env::var_os("CI").is_none(), "Node is required in CI");
                eprintln!("Skipping Node execution test: node is unavailable ({error})");
                None
            }
            Err(error) => panic!("Failed to run Node: {error}"),
        }
    }

    /// Run the wrapped script in Node the way `run_agent_script` runs it in a
    /// page: expression form first, body form when that will not compile.
    /// `None` when Node is unavailable.
    fn execute(script: &str) -> Option<Value> {
        let driver = r#"
          globalThis.window = globalThis;
          const [first, second, poll] = JSON.parse(process.argv[1]);
          let started = "";
          for (const wrapper of [first, second]) {
            try { started = (0, eval)(wrapper); } catch (e) { started = ""; }
            if (started === "started") break;
          }
          setTimeout(() => console.log(started === "started" ? (0, eval)(poll) : "not started"), 50);
        "#;
        let args = serde_json::to_string(&[
            agent_script("k", script, true),
            agent_script("k", script, false),
            result_poll("k"),
        ])
        .unwrap();
        let out = run_node(driver, &args)?;
        let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Some(match serde_json::from_str::<Value>(&line) {
            Ok(entry) => entry,
            Err(_) => Value::String(line),
        })
    }

    #[test]
    fn expressions_and_bodies_execute() {
        let result = |script: &str| execute(script).map(|v| v["json"].clone());
        if execute("1").is_none() {
            return;
        }
        let ok = |json: &str| Some(Value::String(json.to_string()));
        assert_eq!(result("1 + 2;\n"), ok("3"));
        assert_eq!(result("[1,2].map(x => { return x * 2; })"), ok("[2,4]"));
        assert_eq!(result("const a = 2;\nreturn a * 3;"), ok("6"));
        assert_eq!(result("return 'return'"), ok("\"return\""));
        assert_eq!(result("await Promise.resolve(7)"), ok("7"));
    }

    #[test]
    fn runtime_errors_report_once_and_do_not_rerun() {
        if execute("1").is_none() {
            return;
        }
        // An expression that throws must not fall through to the body form
        // and run its side effects a second time.
        let entry = execute(
            "(globalThis.n = (globalThis.n || 0) + 1, (() => { throw new Error('boom'); })())",
        )
        .unwrap();
        assert_eq!(entry["ok"], Value::Bool(false));
        assert!(entry["error"].as_str().unwrap().contains("boom"));
        let count =
            execute("globalThis.n = (globalThis.n || 0) + 1; return globalThis.n;").unwrap();
        assert_eq!(count["json"], Value::String("1".into()));
        let broken = execute("let = = ;").unwrap();
        assert_eq!(broken, Value::String("not started".into()));
    }

    #[test]
    fn app_and_internal_origins_never_browse() {
        let own = [Url::parse("tauri://localhost/index.html").unwrap()];
        let dev = [Url::parse("http://localhost:1420").unwrap()];
        for blocked in [
            "tauri://localhost/",
            "http://tauri.localhost/",
            "https://tauri.localhost/x",
            "https://a.tauri.localhost/",
            "http://asset.localhost/f",
            "https://asset.localhost/f",
            "http://ipc.localhost/cmd",
            "https://ipc.localhost/cmd",
            "ipc://localhost/cmd",
            "asset://localhost/f",
        ] {
            assert!(
                matches_app_origin(&Url::parse(blocked).unwrap(), &own),
                "{blocked}"
            );
        }
        assert!(matches_app_origin(
            &Url::parse("http://localhost:1420/a").unwrap(),
            &dev
        ));
        assert!(!matches_app_origin(
            &Url::parse("http://localhost:3000").unwrap(),
            &dev
        ));
        assert!(!matches_app_origin(
            &Url::parse("https://example.com").unwrap(),
            &own
        ));
    }

    #[test]
    fn dev_server_is_an_app_origin_only_in_dev_builds() {
        let config: tauri::Config = serde_json::from_value(serde_json::json!({
            "identifier": "test.monocode",
            "build": { "devUrl": "http://localhost:1420" },
            "app": { "windows": [{ "url": "https://app.example.com/" }] },
        }))
        .unwrap();
        let dev_server = Url::parse("http://localhost:1420/").unwrap();
        let external = Url::parse("https://app.example.com/x").unwrap();
        let dev = app_origins_from(&config, true);
        assert!(matches_app_origin(&dev_server, &dev));
        assert!(matches_app_origin(&external, &dev));
        let bundled = app_origins_from(&config, false);
        assert!(!matches_app_origin(&dev_server, &bundled));
        assert!(matches_app_origin(&external, &bundled));
    }

    #[test]
    fn frame_guard_lists_app_origins() {
        let script = frame_guard_script(&[Url::parse("http://localhost:1420/a").unwrap()]);
        assert!(script.contains(r#"["http://localhost:1420"]"#));
        assert!(script.contains("window.top === window"));
        assert!(frame_guard_script(&[]).contains("var own = [];"));
    }

    #[test]
    fn frame_guard_blanks_only_app_subframes() {
        let script = frame_guard_script(&[Url::parse("http://localhost:1420/a").unwrap()]);
        let driver = r#"
          const script = JSON.parse(process.argv[1]);
          const urls = [
            "tauri://localhost/index.html", "http://ipc.localhost/cmd",
            "https://asset.localhost/file", "http://localhost:1420/a",
            "https://example.com/", "http://localhost:3000/"
          ];
          const results = [];
          for (const top of [false, true]) {
            for (const url of urls) {
              let stopped = false;
              globalThis.window = { stop() { stopped = true; } };
              window.top = top ? window : {};
              globalThis.location = new URL(url);
              globalThis.document = { documentElement: { textContent: "page" } };
              let replaced = null;
              location.replace = (to) => { replaced = to; };
              (0, eval)(script);
              const blank = document.documentElement.textContent === "";
              // Document start: no root yet; must not throw or skip the reload.
              globalThis.document = { documentElement: null };
              replaced = null;
              let threw = false;
              try { (0, eval)(script); } catch (e) { threw = true; }
              results.push({ stopped, blank, early: !threw && replaced === "about:blank" });
            }
          }
          console.log(JSON.stringify(results));
        "#;
        let Some(out) = run_node(driver, &serde_json::to_string(&script).unwrap()) else {
            return;
        };
        let results: Vec<Value> = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(results.len(), 12);
        for (index, result) in results.iter().enumerate() {
            let blocked = index < 4;
            assert_eq!(result["stopped"], Value::Bool(blocked), "case {index}");
            assert_eq!(result["blank"], Value::Bool(blocked), "case {index}");
            assert_eq!(result["early"], Value::Bool(blocked), "case {index}");
        }
    }

    #[test]
    fn navigations_admit_web_pages_and_frame_documents_only() {
        let own = [Url::parse("http://localhost:1420/").unwrap()];
        let allowed = |s: &str| navigation_allowed(&Url::parse(s).unwrap(), &own);
        for ok in [
            "https://example.com/a",
            "http://localhost:3000/",
            "about:blank",
            "about:blank#frame",
            "about:blank?x=1",
            "about:srcdoc",
            "about:srcdoc#top",
            "blob:https://example.com/6f1c-uuid",
        ] {
            assert!(allowed(ok), "{ok}");
        }
        for blocked in [
            "file:///etc/passwd",
            "javascript:1",
            "data:text/html,x",
            "about:config",
            "blob:null/6f1c-uuid",
            "blob:file:///6f1c-uuid",
            "blob:http://localhost:1420/6f1c-uuid",
            "blob:tauri://localhost/6f1c-uuid",
            "http://localhost:1420/",
            "tauri://localhost/",
        ] {
            assert!(!allowed(blocked), "{blocked}");
        }
        // Typed and agent URLs stay limited to web pages.
        assert!(parse_url("blob:https://a/b").is_err());
        assert!(parse_url("about:srcdoc").is_err());
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
