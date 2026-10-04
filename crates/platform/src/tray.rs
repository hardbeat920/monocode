//! The tray icon: the way back to a window that closing hid. Port of
//! src-tauri/src/tray.rs over the `tray-icon` crate, which Tauri wrapped.
//!
//! Windows only. Closing a window hides it so the harness children keep
//! running, and a hidden window drops off the taskbar, so without the tray
//! the windows would be unreachable. On macOS and Linux [`install`] does
//! nothing and returns `None`, as the Tauri app did.
//!
//! Create the tray on the thread that runs the window message loop. Commands
//! arrive on that thread; send them into a channel rather than touching UI
//! state from the callback.

/// The "Show MonoCode" menu item id.
pub const SHOW_ID: &str = "tray_show";
/// The "Quit MonoCode" menu item id.
pub const QUIT_ID: &str = "tray_quit";
/// The tray icon id.
pub const TRAY_ID: &str = "main";

/// What the user asked the tray for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    /// `show_hidden_or_open_new`.
    Show,
    /// `request_quit`.
    Quit,
}

/// A mouse button on the tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayButton {
    Left,
    Right,
    Middle,
}

/// Whether the button went down or up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayButtonState {
    Down,
    Up,
}

/// RGBA pixels for the icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl TrayImage {
    /// Decode the packaged app icon into the pixels the tray accepts.
    pub fn from_png(bytes: &[u8]) -> Result<Self, String> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
        let mut pixels = vec![
            0;
            reader
                .output_buffer_size()
                .ok_or("The tray icon is too large.")?
        ];
        let frame = reader
            .next_frame(&mut pixels)
            .map_err(|error| error.to_string())?;
        let pixels = &pixels[..frame.buffer_size()];
        let rgba = match frame.color_type {
            png::ColorType::Rgba => pixels.to_vec(),
            png::ColorType::Rgb => pixels
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|&[r, g, b]| [r, g, b, 255])
                .collect(),
            png::ColorType::Grayscale => pixels
                .iter()
                .flat_map(|&gray| [gray, gray, gray, 255])
                .collect(),
            png::ColorType::GrayscaleAlpha => pixels
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|&[gray, alpha]| [gray, gray, gray, alpha])
                .collect(),
            png::ColorType::Indexed => {
                return Err("The tray icon has an unexpanded palette.".into());
            }
        };
        Ok(Self {
            rgba,
            width: frame.width,
            height: frame.height,
        })
    }
}

/// The tray's text and icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayOptions {
    pub tooltip: String,
    pub show_label: String,
    pub quit_label: String,
    /// The app icon. The Tauri app used the default window icon.
    pub icon: Option<TrayImage>,
}

impl Default for TrayOptions {
    fn default() -> Self {
        Self {
            tooltip: "MonoCode".into(),
            show_label: "Show MonoCode".into(),
            quit_label: "Quit MonoCode".into(),
            icon: None,
        }
    }
}

/// The tray exists on this platform.
pub fn supported() -> bool {
    cfg!(windows)
}

/// `on_menu_event`.
pub fn command_for_menu(id: &str) -> Option<TrayCommand> {
    match id {
        SHOW_ID => Some(TrayCommand::Show),
        QUIT_ID => Some(TrayCommand::Quit),
        _ => None,
    }
}

/// `on_tray_icon_event`: a left click reopens when the button comes up. The
/// menu stays on the right button.
pub fn command_for_click(button: TrayButton, state: TrayButtonState) -> Option<TrayCommand> {
    (button == TrayButton::Left && state == TrayButtonState::Up).then_some(TrayCommand::Show)
}

/// The installed tray. Dropping it removes the icon.
pub struct Tray {
    #[cfg(windows)]
    _icon: tray_icon::TrayIcon,
}

/// `install`: add the tray icon with Show and Quit. Returns `None` where the
/// platform has no tray.
#[cfg(not(windows))]
pub fn install(
    _options: TrayOptions,
    _on_command: impl Fn(TrayCommand) + Send + Sync + 'static,
) -> Result<Option<Tray>, String> {
    Ok(None)
}

/// `install`: add the tray icon with Show and Quit. `on_command` replaces
/// any earlier handler only on the first call, because the crate keeps one
/// process-wide handler.
#[cfg(windows)]
pub fn install(
    options: TrayOptions,
    on_command: impl Fn(TrayCommand) + Send + Sync + 'static,
) -> Result<Option<Tray>, String> {
    use std::sync::Arc;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(SHOW_ID, &options.show_label, true, None);
    let quit = MenuItem::with_id(QUIT_ID, &options.quit_label, true, None);
    let menu = Menu::with_items(&[&show, &quit]).map_err(|err| err.to_string())?;

    let on_command: Arc<dyn Fn(TrayCommand) + Send + Sync> = Arc::new(on_command);
    let menu_command = on_command.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some(command) = command_for_menu(event.id.as_ref()) {
            menu_command(command);
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button,
            button_state,
            ..
        } = event
        {
            let button = match button {
                MouseButton::Left => TrayButton::Left,
                MouseButton::Right => TrayButton::Right,
                MouseButton::Middle => TrayButton::Middle,
            };
            let state = match button_state {
                MouseButtonState::Up => TrayButtonState::Up,
                MouseButtonState::Down => TrayButtonState::Down,
            };
            if let Some(command) = command_for_click(button, state) {
                on_command(command);
            }
        }
    }));

    let mut builder = TrayIconBuilder::new()
        .with_id(TRAY_ID)
        .with_tooltip(&options.tooltip)
        .with_menu(Box::new(menu))
        // Left click reopens; the menu stays on the right button.
        .with_menu_on_left_click(false);
    if let Some(image) = options.icon {
        let icon = Icon::from_rgba(image.rgba, image.width, image.height)
            .map_err(|err| err.to_string())?;
        builder = builder.with_icon(icon);
    }
    let icon = builder.build().map_err(|err| err.to_string())?;
    Ok(Some(Tray { _icon: icon }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_items_map_to_show_and_quit() {
        assert_eq!(command_for_menu(SHOW_ID), Some(TrayCommand::Show));
        assert_eq!(command_for_menu(QUIT_ID), Some(TrayCommand::Quit));
        assert_eq!(command_for_menu("other"), None);
    }

    #[test]
    fn only_a_left_button_release_reopens() {
        assert_eq!(
            command_for_click(TrayButton::Left, TrayButtonState::Up),
            Some(TrayCommand::Show)
        );
        assert_eq!(
            command_for_click(TrayButton::Left, TrayButtonState::Down),
            None
        );
        assert_eq!(
            command_for_click(TrayButton::Right, TrayButtonState::Up),
            None
        );
    }

    #[test]
    fn the_tray_is_a_no_op_off_windows() {
        let options = TrayOptions::default();
        assert_eq!(options.show_label, "Show MonoCode");
        assert_eq!(options.quit_label, "Quit MonoCode");
        if !supported() {
            assert!(install(options, |_| {}).unwrap().is_none());
        }
    }

    #[test]
    fn packaged_app_icon_decodes_for_the_tray() {
        let image = TrayImage::from_png(include_bytes!("../../../packaging/assets/icon.png"))
            .expect("the packaged icon must decode");
        assert_eq!((image.width, image.height), (256, 256));
        assert_eq!(image.rgba.len(), 256 * 256 * 4);
        assert!(
            image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 0)
        );
    }
}
