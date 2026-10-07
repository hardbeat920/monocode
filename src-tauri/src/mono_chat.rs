//! The menu bar and floating chat are views of a workspace-owned Mono.
//! Requests remain queued until that workspace is ready; only its owner can
//! publish or acknowledge them. Closing either view never stops the runtime.

use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::AssertUnwindSafe;
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::menu::{IconMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, EventTarget, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window,
};

use crate::window::{is_workspace_window, workspace_windows, MONO_CHAT_PREFIX};

mod menu_bar;

const TRAY: &str = "mono-menu-bar";
const SELECT: &str = "mono-chat:";
const CHANGED: &str = "mono_chat_changed";
const REQUEST: &str = "mono_chat_request";
/// A 464pt conversation plus the 56pt Mono rail beside it.
const WIDTH: f64 = 520.0;

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MonoEntry {
    id: String,
    name: String,
    mascot: String,
    color: String,
    session_id: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostedMono {
    mono_id: String,
    busy: bool,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    monos: Vec<MonoEntry>,
    mono_id: Option<String>,
    session: Option<Value>,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    id: u32,
    mono_id: String,
    action: Value,
}

struct Pending {
    owner: String,
    request: Request,
    reply: Option<mpsc::Sender<Result<(), String>>>,
    accepted: bool,
}

#[derive(Default)]
struct Inner {
    monos: Vec<MonoEntry>,
    menu_mascots: Vec<menu_bar::MenuMascot>,
    views: HashMap<String, View>,
    hosts: HashMap<String, Vec<HostedMono>>,
    owners: HashMap<String, String>,
    /// Floating windows switched by their rail to a Mono other than their own.
    shown: HashMap<String, String>,
    /// Floating windows whose loading UI has mounted, so they may be shown.
    ready: HashSet<String>,
    pending: VecDeque<Pending>,
    counter: u32,
}

impl Inner {
    /// The Mono a floating window shows; it starts as the one it was built for.
    fn shown<'a>(&'a self, label: &'a str) -> Option<&'a str> {
        self.shown.get(label).map(String::as_str).or_else(|| {
            label
                .strip_prefix(MONO_CHAT_PREFIX)
                .filter(|id| !id.is_empty())
        })
    }

    fn view(&mut self, mono_id: &str) -> &mut View {
        self.views
            .entry(mono_id.to_owned())
            .or_insert_with(|| View {
                monos: self.monos.clone(),
                mono_id: Some(mono_id.to_owned()),
                ..View::default()
            })
    }

    fn publish(&mut self, owner: &str, mono_id: &str, session: Value) -> bool {
        if self.owners.get(mono_id).map(String::as_str) != Some(owner) {
            return false;
        }
        let Some(view) = self.views.get_mut(mono_id) else {
            return false;
        };
        view.session = Some(session);
        view.error = None;
        true
    }

    fn enqueue(
        &mut self,
        owner: String,
        mono_id: String,
        action: Value,
        reply: Option<mpsc::Sender<Result<(), String>>>,
    ) -> u32 {
        self.counter = self.counter.wrapping_add(1);
        let id = self.counter;
        self.pending.push_back(Pending {
            owner,
            request: Request {
                id,
                mono_id,
                action,
            },
            reply,
            accepted: false,
        });
        id
    }

    fn accept(&mut self, id: u32, owner: &str) -> bool {
        let Some(pending) = self
            .pending
            .iter_mut()
            .find(|p| p.request.id == id && p.owner == owner)
        else {
            return false;
        };
        if pending.accepted {
            return false;
        }
        pending.accepted = true;
        true
    }

    fn finish(&mut self, id: u32, owner: &str, result: Result<(), String>) {
        if let Some(index) = self
            .pending
            .iter()
            .position(|p| p.request.id == id && p.owner == owner)
        {
            if let Some(reply) = self.pending.remove(index).and_then(|p| p.reply) {
                let _ = reply.send(result);
            }
        }
    }
}

#[derive(Default)]
struct MonoChatState(Mutex<Inner>);

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    app.manage(MonoChatState::default());
    let mut tray = TrayIconBuilder::with_id(TRAY)
        .tooltip("Chat with a Mono")
        .menu(&menu(app, &[], &[])?)
        .icon_as_template(true)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            if let Some(mono_id) = id.strip_prefix(SELECT) {
                let _ = open(app, mono_id.to_owned());
            } else if id == "mono-chat-composer" {
                crate::quick_composer::open(app);
            } else if id == "mono-chat-quit" {
                crate::window::request_quit(app);
            }
        });
    tray = tray.icon(menu_bar_icon().map_err(std::io::Error::other)?);
    let tray = tray.build(app)?;
    menu_bar::decorate(&tray)?;
    // Read natively so a hidden icon never flashes in before a window loads.
    if menu_bar_hidden_marker(app).is_some_and(|marker| marker.exists()) {
        tray.set_visible(false)?;
    }
    Ok(())
}

fn menu_bar_hidden_marker(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("menu-bar-icon-hidden"))
}

/// Show or hide the menu bar icon, remembering the choice for next launch.
#[tauri::command]
pub fn mono_menu_bar_set_visible(app: AppHandle, visible: bool) -> Result<(), String> {
    let marker = menu_bar_hidden_marker(&app).ok_or("No app data directory.")?;
    if visible {
        match std::fs::remove_file(&marker) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.to_string())
            }
            _ => {}
        }
    } else {
        if let Some(dir) = marker.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&marker, b"").map_err(|e| e.to_string())?;
    }
    if let Some(tray) = app.tray_by_id(TRAY) {
        tray.set_visible(visible).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn menu_bar_icon() -> Result<tauri::image::Image<'static>, png::DecodingError> {
    // Use the original monochrome mark, not the padded/shaded Dock tile.
    // tray-icon sizes images to 18pt on macOS. Remove the Dock padding, then
    // leave a 1pt margin so the mark itself is a slightly quieter 16pt.
    let mut reader = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../macos/AppIcon.icon/Assets/icon.png"
    )))
    .read_info()?;
    let mut mask = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut mask)?;
    let width = frame.width as usize;
    let height = frame.height as usize;
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
    for (index, pixel) in mask.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let light = u32::from(pixel[0].max(pixel[1]).max(pixel[2]));
        pixel[3] = (light * u32::from(pixel[3]) / 255) as u8;
        pixel[..3].fill(0);
        if pixel[3] > 0 {
            let (x, y) = (index % width, index / width);
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }
    let padding = (bottom - top).div_ceil(16);
    let image_width = right - left + padding * 2;
    let image_height = bottom - top + padding * 2;
    let mut rgba = vec![0; image_width * image_height * 4];
    for y in top..bottom {
        let start = ((y - top + padding) * image_width + padding) * 4;
        rgba[start..start + (right - left) * 4]
            .copy_from_slice(&mask[(y * width + left) * 4..(y * width + right) * 4]);
    }
    Ok(tauri::image::Image::new_owned(
        rgba,
        image_width as u32,
        image_height as u32,
    ))
}

fn menu(
    app: &AppHandle,
    monos: &[MonoEntry],
    mascots: &[menu_bar::MenuMascot],
) -> tauri::Result<Menu<tauri::Wry>> {
    let header = MenuItemBuilder::with_id("mono-chat-header", "Monos")
        .enabled(false)
        .build(app)?;
    let mut builder = MenuBuilder::new(app).item(&header).separator();
    if monos.is_empty() {
        let empty =
            MenuItemBuilder::with_id("mono-chat-empty", "Create a Mono in MonoCode to chat here")
                .enabled(false)
                .build(app)?;
        builder = builder.item(&empty);
    } else {
        for mono in monos {
            let mut item = IconMenuItemBuilder::with_id(format!("{SELECT}{}", mono.id), &mono.name);
            if let Some(icon) = menu_bar::icon_for(&mono.id, mascots) {
                item = item.icon(icon);
            } else {
                eprintln!("monocode: missing menu portrait for Mono {}", mono.id);
            }
            let item = item.build(app)?;
            builder = builder.item(&item);
        }
    }
    for action in &menu_bar::ACTIONS {
        builder = builder
            .separator()
            .item(&MenuItemBuilder::with_id(action.id, action.title).build(app)?);
    }
    builder.build()
}

fn label(mono_id: &str) -> String {
    format!("{MONO_CHAT_PREFIX}{mono_id}")
}

/// Every floating window currently showing this Mono.
fn panels(app: &AppHandle, mono_id: &str) -> Vec<WebviewWindow> {
    let windows: Vec<_> = app
        .webview_windows()
        .into_values()
        .filter(|w| w.label().starts_with(MONO_CHAT_PREFIX))
        .collect();
    let state = app.state::<MonoChatState>();
    let inner = state.0.lock().unwrap();
    windows
        .into_iter()
        .filter(|w| inner.shown(w.label()) == Some(mono_id))
        .collect()
}

fn changed(app: &AppHandle, mono_id: &str) {
    let view = app
        .state::<MonoChatState>()
        .0
        .lock()
        .unwrap()
        .views
        .get(mono_id)
        .cloned();
    if let Some(view) = view {
        for panel in panels(app, mono_id) {
            let _ = app.emit_to(
                EventTarget::webview_window(panel.label()),
                CHANGED,
                view.clone(),
            );
        }
    }
}

fn workspace(window: &Window) -> Result<(), String> {
    if is_workspace_window(window.label()) {
        Ok(())
    } else {
        Err("This action belongs to a workspace.".into())
    }
}

fn panel(app: &AppHandle, label: &str) -> Result<String, String> {
    app.state::<MonoChatState>()
        .0
        .lock()
        .unwrap()
        .shown(label)
        .map(str::to_owned)
        .ok_or_else(|| "This action belongs to the floating chat.".into())
}

#[tauri::command]
pub fn mono_chat_sync(
    app: AppHandle,
    window: Window,
    monos: Vec<MonoEntry>,
    hosted: Vec<HostedMono>,
    mascots: Vec<menu_bar::MenuMascot>,
) -> Result<(), String> {
    workspace(&window)?;
    if monos
        .iter()
        .any(|m| m.id.is_empty() || !m.id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'))
    {
        return Err("Invalid Mono identity.".into());
    }
    let roster_changed = {
        let state = app.state::<MonoChatState>();
        let mut inner = state.0.lock().unwrap();
        inner.hosts.insert(window.label().to_owned(), hosted);
        let updated = inner.monos != monos || inner.menu_mascots != mascots;
        inner.menu_mascots = mascots;
        inner
            .owners
            .retain(|id, _| monos.iter().any(|m| m.id == *id));
        for (id, view) in &mut inner.views {
            view.monos = monos.clone();
            if !monos.iter().any(|m| m.id == *id) {
                view.session = None;
                view.error = Some("That Mono is no longer available.".into());
            }
        }
        inner.monos = monos;
        updated
    };
    if roster_changed {
        let (roster, mascots) = {
            let state = app.state::<MonoChatState>();
            let inner = state.0.lock().unwrap();
            (inner.monos.clone(), inner.menu_mascots.clone())
        };
        if let Some(tray) = app.tray_by_id(TRAY) {
            tray.set_menu(Some(
                menu(&app, &roster, &mascots).map_err(|e| e.to_string())?,
            ))
            .map_err(|e| e.to_string())?;
            menu_bar::decorate(&tray).map_err(|e| e.to_string())?;
        }
        let ids: Vec<_> = app
            .state::<MonoChatState>()
            .0
            .lock()
            .unwrap()
            .views
            .keys()
            .cloned()
            .collect();
        for id in ids {
            changed(&app, &id);
        }
    }
    Ok(())
}

fn owner(app: &AppHandle, mono_id: &str) -> Result<String, String> {
    let state = app.state::<MonoChatState>();
    let inner = state.0.lock().unwrap();
    if !inner.monos.iter().any(|m| m.id == mono_id) {
        return Err("That Mono is no longer available.".into());
    }
    if let Some(label) = inner
        .owners
        .get(mono_id)
        // A workspace window that hosts browser tabs is no longer a
        // WebviewWindow, so look it up as a plain window.
        .filter(|label| app.get_window(label).is_some())
    {
        return Ok(label.clone());
    }
    let mut windows = workspace_windows(app);
    // A loaded conversation, especially a running one, owns its runtime.
    windows.sort_by_key(|w| {
        let hosted = inner
            .hosts
            .get(w.label())
            .and_then(|monos| monos.iter().find(|m| m.mono_id == mono_id));
        (
            hosted.is_none(),
            !hosted.is_some_and(|m| m.busy),
            !w.is_focused().unwrap_or(false),
            w.label().to_owned(),
        )
    });
    drop(inner);
    let window = match windows.first() {
        Some(window) => window.clone(),
        None => crate::window::open_session_window(app, false)?
            .as_ref()
            .window(),
    };
    let label = window.label().to_owned();
    state
        .0
        .lock()
        .unwrap()
        .owners
        .insert(mono_id.to_owned(), label.clone());
    Ok(label)
}

pub fn open(app: &AppHandle, mono_id: String) -> Result<(), String> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Err(error) = select(&handle, &mono_id) {
            eprintln!("monocode: open floating Mono: {error}");
            handle
                .state::<MonoChatState>()
                .0
                .lock()
                .unwrap()
                .view(&mono_id)
                .error = Some(error);
            changed(&handle, &mono_id);
        }
    })
    .map_err(|e| e.to_string())
}

fn select(app: &AppHandle, mono_id: &str) -> Result<(), String> {
    let owner = owner(app, mono_id)?;
    let mut showing = panels(app, mono_id);
    showing.sort_by_key(|w| !w.is_visible().unwrap_or(false));
    // A window switched away still answers for the Mono it was built for.
    let window = match showing
        .into_iter()
        .next()
        .or_else(|| app.get_webview_window(&label(mono_id)))
    {
        Some(window) => window,
        None => build(app, mono_id).map_err(|e| e.to_string())?,
    };
    let renderer_ready = {
        let state = app.state::<MonoChatState>();
        let mut inner = state.0.lock().unwrap();
        inner
            .shown
            .insert(window.label().to_owned(), mono_id.to_owned());
        let renderer_ready = inner.ready.contains(window.label());
        inner.view(mono_id).error = None;
        inner.enqueue(
            owner.clone(),
            mono_id.to_owned(),
            json!({"kind": "open"}),
            None,
        );
        renderer_ready
    };
    changed(app, mono_id);
    if renderer_ready {
        present(&window)?;
    }
    let _ = app.emit_to(EventTarget::webview_window(owner), REQUEST, ());
    Ok(())
}

/// Point a floating window at another Mono. The window stays put; only its
/// conversation changes, and a Mono opened before shows its last snapshot.
fn switch(app: &AppHandle, window: &Window, to: &str) -> Result<(), String> {
    let owner = owner(app, to)?;
    {
        let state = app.state::<MonoChatState>();
        let mut inner = state.0.lock().unwrap();
        inner.shown.insert(window.label().to_owned(), to.to_owned());
        inner.view(to).error = None;
        inner.enqueue(owner.clone(), to.to_owned(), json!({"kind": "open"}), None);
    }
    changed(app, to);
    let _ = app.emit_to(EventTarget::webview_window(owner), REQUEST, ());
    Ok(())
}

#[tauri::command]
pub fn mono_chat_switch(
    app: AppHandle,
    window: Window,
    from: String,
    to: String,
) -> Result<(), String> {
    let target = if is_workspace_window(window.label()) {
        // A Mono added from a floating chat: switch the window that asked.
        let mut showing = panels(&app, &from);
        showing.sort_by_key(|w| !w.is_visible().unwrap_or(false));
        showing.into_iter().next().map(|w| w.as_ref().window())
    } else if panel(&app, window.label())? == from {
        Some(window)
    } else {
        return Err("This action belongs to a different Mono.".into());
    };
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = match &target {
            Some(window) => switch(&handle, window, &to),
            None => select(&handle, &to),
        };
        if let Err(error) = result {
            eprintln!("monocode: switch floating Mono: {error}");
            handle
                .state::<MonoChatState>()
                .0
                .lock()
                .unwrap()
                .view(&from)
                .error = Some(error);
            changed(&handle, &from);
        }
    })
    .map_err(|e| e.to_string())
}

fn present(window: &WebviewWindow) -> Result<(), String> {
    if objc2::MainThreadMarker::new().is_none() {
        return Err("Floating chats must be presented on the macOS main thread.".into());
    }
    // No Mono state lock is held across AppKit. Readiness is committed only
    // after this succeeds, so failed presentation can be retried safely.
    objc2::exception::catch(AssertUnwindSafe(|| crate::quick_composer::present(window)))
        .map_err(|exception| format!("Could not show the floating chat: {exception:?}"))
}

fn build(app: &AppHandle, mono_id: &str) -> tauri::Result<WebviewWindow> {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let height = monitor
        .as_ref()
        .map(|m| (f64::from(m.work_area().size.height) / m.scale_factor() - 24.0).min(680.0))
        .unwrap_or(680.0);
    let window = WebviewWindowBuilder::new(
        app,
        label(mono_id),
        WebviewUrl::App("mono-chat.html".into()),
    )
    .title("Mono chat")
    .inner_size(WIDTH, height)
    .min_inner_size(370.0, 420.0)
    .resizable(true)
    .maximizable(false)
    .minimizable(false)
    .decorations(false)
    .transparent(true)
    .background_color(tauri::window::Color(0, 0, 0, 0))
    .shadow(true)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .visible(false)
    .focused(false)
    .build()?;
    // Catch Objective-C exceptions before crossing Tauri's Rust event
    // callback, where a foreign unwind aborts the entire application.
    // No Rust state locks are held during AppKit setup. A partial window is
    // discarded on failure, so catching this unwind cannot expose it later.
    if let Err(exception) = objc2::exception::catch(AssertUnwindSafe(|| {
        crate::quick_composer::make_panel(&window);
        crate::macos::prepare_floating_window(&window.as_ref().window());
    })) {
        let _ = window.destroy();
        return Err(std::io::Error::other(format!(
            "Could not prepare the floating chat: {exception:?}"
        ))
        .into());
    }
    if let Some(monitor) = monitor {
        let area = monitor.work_area();
        let width = (WIDTH * monitor.scale_factor()) as i32;
        let margin = (12.0 * monitor.scale_factor()) as i32;
        let count = app
            .webview_windows()
            .values()
            .filter(|w| {
                w.label() != window.label()
                    && w.label().starts_with(MONO_CHAT_PREFIX)
                    && w.is_visible().unwrap_or(false)
            })
            .count();
        let offset = ((count % 8) as f64 * 28.0 * monitor.scale_factor()) as i32;
        let _ = window.set_position(PhysicalPosition::new(
            area.position.x + (area.size.width as i32 - width - margin - offset).max(0),
            area.position.y
                + (margin + offset).min(
                    (area.size.height as i32 - (height * monitor.scale_factor()) as i32).max(0),
                ),
        ));
    }
    let hidden = window.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = hidden.hide();
        }
        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
            if let Some(native) = crate::macos::ns_window(&hidden) {
                native.invalidateShadow();
            }
        }
        _ => {}
    });
    Ok(window)
}

#[tauri::command]
pub async fn mono_chat_ready(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    panel(&app, window.label())?;
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let already_ready = handle
            .state::<MonoChatState>()
            .0
            .lock()
            .unwrap()
            .ready
            .contains(window.label());
        let result = if already_ready {
            Ok(())
        } else {
            present(&window)
        };
        if result.is_ok() {
            handle
                .state::<MonoChatState>()
                .0
                .lock()
                .unwrap()
                .ready
                .insert(window.label().to_owned());
        } else if let Err(error) = &result {
            eprintln!("monocode: {error}");
        }
        let _ = tx.try_send(result);
    })
    .map_err(|error| error.to_string())?;
    rx.recv()
        .await
        .ok_or_else(|| "The floating chat closed before it could open.".to_owned())?
}

#[tauri::command]
pub fn mono_chat_state(app: AppHandle, window: WebviewWindow) -> Result<View, String> {
    let id = panel(&app, window.label())?;
    Ok(app
        .state::<MonoChatState>()
        .0
        .lock()
        .unwrap()
        .view(&id)
        .clone())
}

#[tauri::command]
pub fn mono_chat_publish(
    app: AppHandle,
    window: Window,
    mono_id: String,
    session: Value,
) -> Result<(), String> {
    workspace(&window)?;
    let publish = {
        let state = app.state::<MonoChatState>();
        let mut inner = state.0.lock().unwrap();
        inner.publish(window.label(), &mono_id, session)
    };
    if publish {
        changed(&app, &mono_id);
    }
    Ok(())
}

#[tauri::command]
pub fn mono_chat_take(app: AppHandle, window: Window) -> Result<Vec<Request>, String> {
    workspace(&window)?;
    Ok(app
        .state::<MonoChatState>()
        .0
        .lock()
        .unwrap()
        .pending
        .iter()
        .filter(|p| p.owner == window.label() && !p.accepted)
        .map(|p| p.request.clone())
        .collect())
}

#[tauri::command]
pub fn mono_chat_accept(app: AppHandle, window: Window, id: u32) -> Result<bool, String> {
    workspace(&window)?;
    Ok(app
        .state::<MonoChatState>()
        .0
        .lock()
        .unwrap()
        .accept(id, window.label()))
}

#[tauri::command]
pub fn mono_chat_reply(
    app: AppHandle,
    window: Window,
    id: u32,
    error: Option<String>,
) -> Result<(), String> {
    workspace(&window)?;
    let result = error.map_or(Ok(()), Err);
    let (mono_id, reveal) = {
        let state = app.state::<MonoChatState>();
        let mut inner = state.0.lock().unwrap();
        let request = inner
            .pending
            .iter()
            .find(|p| p.request.id == id && p.owner == window.label())
            .map(|p| (p.request.clone(), p.reply.is_none()));
        let Some((request, opening)) = request else {
            return Ok(());
        };
        if opening {
            inner.view(&request.mono_id).error = result.as_ref().err().cloned();
        }
        let reveal = result.is_ok()
            && matches!(
                request.action.get("kind").and_then(Value::as_str),
                Some("reveal" | "openFile" | "openArtifact")
            );
        inner.finish(id, window.label(), result);
        (request.mono_id, reveal)
    };
    if reveal {
        for panel in panels(&app, &mono_id) {
            let _ = panel.hide();
        }
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    changed(&app, &mono_id);
    Ok(())
}

#[tauri::command]
pub async fn mono_chat_action(
    app: AppHandle,
    window: WebviewWindow,
    mono_id: String,
    action: Value,
) -> Result<(), String> {
    if panel(&app, window.label())? != mono_id {
        return Err("This action belongs to a different Mono.".into());
    }
    let kind = action
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(
        kind,
        "submit"
            | "create"
            | "stop"
            | "approval"
            | "question"
            | "questionInteraction"
            | "reveal"
            | "openFile"
            | "openArtifact"
            | "resume"
    ) {
        return Err("Unknown chat action.".into());
    }
    if action.to_string().len() > 32 * 1024 * 1024 {
        return Err("That message is too large.".into());
    }
    let (sender, receiver) = mpsc::channel();
    let (id, owner) = {
        let state = app.state::<MonoChatState>();
        let mut inner = state.0.lock().unwrap();
        let owner = inner
            .owners
            .get(&mono_id)
            .cloned()
            .ok_or("The Mono is still opening.")?;
        let id = inner.enqueue(owner.clone(), mono_id, action, Some(sender));
        (id, owner)
    };
    let _ = app.emit_to(EventTarget::webview_window(&owner), REQUEST, ());
    let result = tauri::async_runtime::spawn_blocking(move || {
        receiver.recv_timeout(Duration::from_secs(30))
    })
    .await
    .map_err(|e| e.to_string())?;
    match result {
        Ok(reply) => reply,
        Err(_) => {
            let state = app.state::<MonoChatState>();
            let mut inner = state.0.lock().unwrap();
            let accepted = inner
                .pending
                .iter()
                .find(|p| p.request.id == id)
                .is_some_and(|p| p.accepted);
            inner.pending.retain(|p| p.request.id != id);
            // Once accepted, retrying could send the same message twice.
            if accepted {
                Ok(())
            } else {
                Err("The Mono did not respond. Your message is still in the composer.".into())
            }
        }
    }
}

#[tauri::command]
pub fn mono_chat_keep_alive(app: AppHandle, window: Window) -> bool {
    let state = app.state::<MonoChatState>();
    let inner = state.0.lock().unwrap();
    inner.views.keys().any(|id| {
        inner
            .owners
            .get(id)
            .is_some_and(|owner| owner == window.label())
    })
}

pub fn window_closed(app: &AppHandle, label: &str) {
    let state = app.state::<MonoChatState>();
    let mut inner = state.0.lock().unwrap();
    if let Some(id) = inner.shown(label).map(str::to_owned) {
        inner.views.remove(&id);
        inner.owners.remove(&id);
    }
    inner.shown.remove(label);
    inner.ready.remove(label);
    inner.hosts.remove(label);
    inner.owners.retain(|_, owner| owner != label);
    let ids: Vec<_> = inner
        .pending
        .iter()
        .filter(|p| p.owner == label)
        .map(|p| p.request.id)
        .collect();
    for id in ids {
        inner.finish(id, label, Err("The Mono's workspace closed.".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_bar_mark_is_sixteen_points_inside_the_standard_eighteen_point_image() {
        let icon = menu_bar_icon().unwrap();
        let width = icon.width() as usize;
        let height = icon.height() as usize;
        assert!(width > 0 && height > 0);
        let occupied_rows: Vec<_> = (0..height)
            .filter(|y| (0..width).any(|x| icon.rgba()[(y * width + x) * 4 + 3] > 0))
            .collect();
        let visible_height = occupied_rows.last().unwrap() - occupied_rows.first().unwrap() + 1;
        let points = 18.0 * visible_height as f64 / height as f64;
        assert!((points - 16.0).abs() < 0.05, "mark height: {points}pt");
        assert_eq!(
            *occupied_rows.first().unwrap(),
            height - occupied_rows.last().unwrap() - 1
        );
        // The terminal cutout and space around the orbit remain transparent.
        assert!(icon
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] == 0));
    }

    #[test]
    fn a_floating_window_shows_its_own_mono_until_the_rail_switches_it() {
        let mut inner = Inner::default();
        let window = label("first");
        assert_eq!(inner.shown(&window), Some("first"));
        assert_eq!(inner.shown("main"), None);
        inner.shown.insert(window.clone(), "second".into());
        assert_eq!(inner.shown(&window), Some("second"));
    }

    #[test]
    fn each_mono_keeps_its_own_snapshot_when_another_window_opens_or_streams() {
        let mut inner = Inner::default();
        inner.owners.insert("first".into(), "main".into());
        inner
            .owners
            .insert("second".into(), "other-workspace".into());
        inner.view("first");
        assert!(inner.publish("main", "first", json!({"text": "First reply"})));
        inner.view("second");
        assert!(inner.publish("other-workspace", "second", json!({"text": "Second reply"})));
        assert!(inner.publish("main", "first", json!({"text": "First reply continues"})));
        assert!(!inner.publish("main", "second", json!({"text": "Wrong owner"})));
        // Reopening one Mono reuses its snapshot; it never selects another.
        assert_eq!(
            inner.view("first").session,
            Some(json!({"text": "First reply continues"}))
        );
        assert_eq!(
            inner.view("second").session,
            Some(json!({"text": "Second reply"}))
        );
        assert_ne!(label("first"), label("second"));
    }

    #[test]
    fn only_the_owner_can_accept_or_acknowledge_a_request() {
        let mut inner = Inner::default();
        let (sender, receiver) = mpsc::channel();
        let id = inner.enqueue(
            "main".into(),
            "mono".into(),
            json!({"kind": "submit"}),
            Some(sender),
        );
        assert!(!inner.accept(id, "other"));
        inner.finish(id, "other", Ok(()));
        assert_eq!(inner.pending.len(), 1);
        assert!(inner.accept(id, "main"));
        assert!(!inner.accept(id, "main"));
        inner.finish(id, "main", Ok(()));
        assert!(receiver.try_recv().unwrap().is_ok());
        assert!(inner.pending.is_empty());
    }

    #[test]
    fn expired_requests_cannot_be_delivered_after_a_retry() {
        let mut inner = Inner::default();
        let id = inner.enqueue(
            "main".into(),
            "mono".into(),
            json!({"kind": "submit"}),
            None,
        );
        inner.pending.retain(|p| p.request.id != id);
        assert!(!inner.accept(id, "main"));
    }
}
