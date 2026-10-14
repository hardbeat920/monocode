//! Tray icon: the way back to a window that closing hid.
//!
//! Windows only. Closing a window hides it so the harness children keep
//! running, and a hidden window drops off the taskbar, so without this the
//! windows would be unreachable.

use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Wry};

use crate::i18n::{t, Text};

const TRAY: &str = "main";
const SHOW: &str = "tray_show";
const QUIT: &str = "tray_quit";

fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let show = MenuItemBuilder::with_id(SHOW, t(Text::ShowMonoCode)).build(app)?;
    let quit = MenuItemBuilder::with_id(QUIT, t(Text::QuitMonoCode)).build(app)?;
    MenuBuilder::new(app).items(&[&show, &quit]).build()
}

/// Rebuilds the tray menu in the current language.
pub fn refresh(app: &AppHandle) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id(TRAY) {
        tray.set_menu(Some(menu(app)?))?;
    }
    Ok(())
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let menu = menu(app)?;

    let mut tray = TrayIconBuilder::with_id(TRAY)
        .tooltip("MonoCode")
        .menu(&menu)
        // Left click reopens; the menu stays on the right button.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            SHOW => {
                let _ = crate::window::show_hidden_or_open_new(app);
            }
            QUIT => crate::window::request_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = crate::window::show_hidden_or_open_new(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;
    Ok(())
}
