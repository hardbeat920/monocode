//! OS bridge. Call initialization and Dock updates on the UI thread. All other
//! results are channels so authorization/dispatch never block the UI thread.

use std::sync::mpsc::{self, Receiver};

use super::delivery::{Permission, Target};

pub struct Notification {
    pub tag: String,
    pub subtitle: String,
    pub body: String,
    pub sound: bool,
}

#[cfg(target_os = "macos")]
#[path = "platform_macos.rs"]
mod implementation;

#[cfg(target_os = "macos")]
pub use implementation::*;

#[cfg(not(target_os = "macos"))]
pub fn initialize() -> bool {
    false
}
#[cfg(not(target_os = "macos"))]
pub fn permission() -> Receiver<Permission> {
    resolved(Permission::Unsupported)
}
#[cfg(not(target_os = "macos"))]
pub fn request_permission() -> Receiver<Permission> {
    permission()
}
#[cfg(not(target_os = "macos"))]
pub fn show(_: Notification) -> Receiver<Result<(), String>> {
    resolved(Err(
        "Native notifications are unavailable on this platform".into()
    ))
}
#[cfg(not(target_os = "macos"))]
pub fn drain_clicks() -> Vec<Target> {
    Vec::new()
}
#[cfg(not(target_os = "macos"))]
pub fn set_dock_badge(_: usize) {}
#[cfg(not(target_os = "macos"))]
pub fn dismiss(_: &str) {}
#[cfg(not(target_os = "macos"))]
pub fn open_settings() -> Result<(), String> {
    Err("Notification settings are unavailable on this platform".into())
}

fn resolved<T>(value: T) -> Receiver<T> {
    let (sender, receiver) = mpsc::channel();
    let _ = sender.send(value);
    receiver
}
