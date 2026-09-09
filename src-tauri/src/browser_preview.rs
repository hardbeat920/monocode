//! A plain Wry child has no Tauri initialization scripts, custom protocols or
//! command dispatcher. Keeping it outside Tauri's webview registry also keeps
//! existing WebviewWindow arguments and window/quit handling valid.
use std::{cell::RefCell, collections::HashMap};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, Url, WebviewWindow};
use wry::{dpi::PhysicalPosition, dpi::PhysicalSize, Rect, WebView, WebViewBuilder};

const EVENT: &str = "browser-preview";

thread_local! {
    // Wry views must be created, used and dropped on the UI thread.
    static VIEWS: RefCell<HashMap<(String, String), WebView>> = RefCell::default();
    #[cfg(target_os = "linux")]
    static CONTAINERS: RefCell<HashMap<String, gtk::Fixed>> = RefCell::default();
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewEvent {
    id: String,
    kind: &'static str,
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    viewport_width: f64,
}

impl PreviewBounds {
    fn physical(&self, window: &WebviewWindow) -> Result<Rect, String> {
        if ![self.x, self.y, self.width, self.height, self.viewport_width]
            .iter()
            .all(|n| n.is_finite())
            || self.x < 0.0
            || self.y < 0.0
            || self.width <= 0.0
            || self.height <= 0.0
            || self.viewport_width <= 0.0
        {
            return Err("Invalid preview bounds".into());
        }
        let size = window.inner_size().map_err(|e| e.to_string())?;
        // CSS pixels also include the app's webview zoom, unlike OS scale alone.
        let scale = f64::from(size.width) / self.viewport_width;
        let x = (self.x * scale).min(f64::from(size.width));
        let y = (self.y * scale).min(f64::from(size.height));
        Ok(Rect {
            position: PhysicalPosition::new(x, y).into(),
            size: PhysicalSize::new(
                (self.width * scale).min(f64::from(size.width) - x),
                (self.height * scale).min(f64::from(size.height) - y),
            )
            .into(),
        })
    }
}

fn allowed_url(value: &str, app_origin: Option<&Url>) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "Enter an HTTP or HTTPS address")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url
            .host_str()
            .is_some_and(|host| host.ends_with(".localhost"))
        || app_origin.is_some_and(|origin| origin.origin() == url.origin())
    {
        return Err("Only web addresses outside the application are allowed".into());
    }
    Ok(url)
}

async fn on_ui<T: Send + 'static>(
    window: WebviewWindow,
    task: impl FnOnce(WebviewWindow) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let owner = window.clone();
    window
        .run_on_main_thread(move || {
            let _ = tx.send(task(owner));
        })
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn browser_preview_open(
    window: WebviewWindow,
    id: String,
    url: String,
    bounds: PreviewBounds,
) -> Result<(), String> {
    on_ui(window, move |window| {
        let origin = window.config().build.dev_url.clone();
        let url = allowed_url(&url, origin.as_ref())?;
        let rect = bounds.physical(&window)?;
        let key = (window.label().to_string(), id.clone());
        VIEWS.with(|views| {
            if views.borrow().contains_key(&key) {
                return Err("Preview already exists".into());
            }
            let navigation_window = window.clone();
            let navigation_id = id.clone();
            let load_window = window.clone();
            let load_id = id.clone();
            let popup_window = window.clone();
            let popup_id = id.clone();
            let download_window = window.clone();
            let download_id = id.clone();
            let focus_window = window.clone();
            let builder = WebViewBuilder::new()
                .with_url(url.as_str())
                .with_bounds(rect)
                .with_incognito(true)
                .with_focused(false)
                .with_navigation_handler(move |value| {
                    let allowed = allowed_url(&value, origin.as_ref()).is_ok();
                    if !allowed {
                        let _ = navigation_window.emit(EVENT, PreviewEvent {
                            id: navigation_id.clone(), kind: "blocked", url: String::new(),
                        });
                    }
                    allowed
                })
                .with_on_page_load_handler(move |event, url| {
                    let kind = match event {
                        wry::PageLoadEvent::Started => "loading",
                        wry::PageLoadEvent::Finished => "loaded",
                    };
                    let _ = load_window.emit(EVENT, PreviewEvent { id: load_id.clone(), kind, url });
                })
                .with_new_window_req_handler(move |_, _| {
                    let _ = popup_window.emit(EVENT, PreviewEvent {
                        id: popup_id.clone(), kind: "popup", url: String::new(),
                    });
                    wry::NewWindowResponse::Deny
                })
                .with_download_started_handler(move |_, _| {
                    let _ = download_window.emit(EVENT, PreviewEvent {
                        id: download_id.clone(), kind: "download", url: String::new(),
                    });
                    false
                })
                // One notification-only message preserves pane focus when a
                // native child consumes a click. No command names/args, eval or
                // filesystem operations can be dispatched through this channel.
                .with_initialization_script(
                    "document.addEventListener('pointerdown',()=>window.ipc.postMessage('focus'),true);",
                )
                .with_ipc_handler(move |request| {
                    if request.body() == "focus" {
                        let _ = focus_window.emit(EVENT, PreviewEvent {
                            id: id.clone(), kind: "focus", url: String::new(),
                        });
                    }
                });
            #[cfg(not(target_os = "linux"))]
            let view = builder.build_as_child(&window).map_err(|e| e.to_string())?;
            #[cfg(target_os = "linux")]
            let view = {
                use wry::WebViewBuilderExtUnix;
                let fixed = preview_container(&window)?;
                builder.build_gtk(&fixed).map_err(|e| e.to_string())?
            };
            views.borrow_mut().insert(key, view);
            Ok(())
        })
    }).await
}

#[tauri::command]
pub async fn browser_preview_sync(
    window: WebviewWindow,
    id: String,
    bounds: Option<PreviewBounds>,
) -> Result<(), String> {
    on_ui(window, move |window| {
        VIEWS.with(|views| {
            let views = views.borrow();
            let view = views
                .get(&(window.label().to_string(), id))
                .ok_or("Preview is closed")?;
            if let Some(bounds) = bounds {
                view.set_bounds(bounds.physical(&window)?)
                    .map_err(|e| e.to_string())?;
                view.set_visible(true).map_err(|e| e.to_string())
            } else {
                view.set_visible(false).map_err(|e| e.to_string())
            }
        })
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewAction {
    Navigate(String),
    Back,
    Forward,
    Reload,
}

#[tauri::command]
pub async fn browser_preview_action(
    window: WebviewWindow,
    id: String,
    action: PreviewAction,
) -> Result<(), String> {
    on_ui(window, move |window| {
        VIEWS.with(|views| {
            let views = views.borrow();
            let view = views
                .get(&(window.label().to_string(), id))
                .ok_or("Preview is closed")?;
            let result = match action {
                PreviewAction::Navigate(value) => {
                    let url = allowed_url(&value, window.config().build.dev_url.as_ref())?;
                    view.load_url(url.as_str())
                }
                PreviewAction::Back => view.evaluate_script("history.back()"),
                PreviewAction::Forward => view.evaluate_script("history.forward()"),
                PreviewAction::Reload => view.reload(),
            };
            result.map_err(|e| e.to_string())
        })
    })
    .await
}

#[tauri::command]
pub async fn browser_preview_url(window: WebviewWindow, id: String) -> Result<String, String> {
    on_ui(window, move |window| {
        VIEWS.with(|views| {
            views
                .borrow()
                .get(&(window.label().to_string(), id))
                .ok_or("Preview is closed")?
                .url()
                .map_err(|e| e.to_string())
        })
    })
    .await
}

#[tauri::command]
pub async fn browser_preview_close(window: WebviewWindow, id: String) -> Result<(), String> {
    on_ui(window, move |window| {
        VIEWS.with(|views| {
            views.borrow_mut().remove(&(window.label().to_string(), id));
        });
        Ok(())
    })
    .await
}

pub fn close_window(label: &str) {
    VIEWS.with(|views| views.borrow_mut().retain(|(owner, _), _| owner != label));
    #[cfg(target_os = "linux")]
    CONTAINERS.with(|containers| {
        containers.borrow_mut().remove(label);
    });
}

#[cfg(target_os = "linux")]
fn preview_container(window: &WebviewWindow) -> Result<gtk::Fixed, String> {
    use gtk::prelude::*;
    CONTAINERS.with(|containers| {
        let mut containers = containers.borrow_mut();
        if let Some(fixed) = containers.get(window.label()) {
            return Ok(fixed.clone());
        }
        let vbox = window.default_vbox().map_err(|e| e.to_string())?;
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        for child in vbox.children() {
            vbox.remove(&child);
            content.pack_start(&child, true, true, 0);
        }
        let overlay = gtk::Overlay::new();
        overlay.add(&content);
        let fixed = gtk::Fixed::new();
        overlay.add_overlay(&fixed);
        vbox.pack_start(&overlay, true, true, 0);
        overlay.show_all();
        containers.insert(window.label().to_string(), fixed.clone());
        Ok(fixed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_pages_without_credentials_or_app_origins_are_allowed() {
        let app = Url::parse("http://localhost:1420").unwrap();
        for value in [
            "https://example.com",
            "http://localhost:3000",
            "http://[::1]:5173/a",
        ] {
            assert!(allowed_url(value, Some(&app)).is_ok(), "{value}");
        }
        for value in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "tauri://localhost",
            "https://tauri.localhost",
            "http://asset.localhost/a",
            "http://localhost:1420/app",
            "https://user:pass@example.com",
            "data:text/html,hi",
            "about:blank",
        ] {
            assert!(allowed_url(value, Some(&app)).is_err(), "{value}");
        }
    }
}
