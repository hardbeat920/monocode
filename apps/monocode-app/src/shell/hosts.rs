//! Install the shared engine hosts for the focused window.

use gpui::{App, Entity};
use monocode_app::bridge::peers::{AppHistoryHost, AppProjectsHooks};
use monocode_engine::history::{History, HistoryPackage};
use monocode_engine::projects::ProjectsGlobal;
use std::rc::Rc;

pub fn install(history: Option<Entity<History>>, cx: &mut App) {
    let host = Rc::new(AppHistoryHost);
    if let Some(history) = history {
        history.update(cx, |history, _| history.set_host(host.clone()));
    }
    if let Some(package) = HistoryPackage::try_global(cx) {
        let history = package.history.clone();
        history.update(cx, |history, _| history.set_host(host));
    }
    if ProjectsGlobal::try_global(cx).is_some() {
        ProjectsGlobal::set_hooks(cx, Rc::new(AppProjectsHooks));
    }
}
