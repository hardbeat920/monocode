//! Native's side of the browser module: the state every view follows, and
//! the user's commands, which run off the UI thread because most of them
//! wait for the browser.
use gpui::{App, AppContext, Entity, Global};
use monocode_core::browser::{self as core, BrowserState};
use std::sync::Arc;

pub(crate) use core::{WINDOW, session_key, turn};

/// The newest browser state, and the last command's error.
pub(crate) struct Browser {
    pub(crate) state: Option<BrowserState>,
    pub(crate) error: Option<String>,
}

struct Shared(Entity<Browser>);
impl Global for Shared {}

pub(crate) fn get(cx: &App) -> Option<Entity<Browser>> {
    cx.try_global::<Shared>().map(|shared| shared.0.clone())
}

/// The state now, when the module runs.
pub(crate) fn state(cx: &App) -> Option<BrowserState> {
    get(cx).and_then(|browser| browser.read(cx).state.clone())
}

/// Follow the browser module's state. After `monocode_core::browser::start`.
pub(crate) fn install(cx: &mut App) {
    if get(cx).is_some() {
        return;
    }
    let receive = core::subscribe();
    let browser = cx.new(|cx| {
        cx.spawn(async move |this, cx| {
            while let Ok(mut state) = receive.recv().await {
                // A burst of changes draws once, with the newest.
                while let Ok(newer) = receive.try_recv() {
                    state = newer;
                }
                let updated = this.update(cx, |browser: &mut Browser, cx| {
                    browser.state = Some(state);
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
        Browser {
            state: core::state(),
            error: None,
        }
    });
    cx.set_global(Shared(browser));
}

/// Run one of the user's commands on the host, off the UI thread and one
/// at a time; an error shows in Settings → Browser.
pub(crate) fn command(
    cx: &mut App,
    work: impl FnOnce(&Arc<core::BrowserHost>) -> Result<(), String> + Send + 'static,
) {
    let (Some(host), Some(browser)) = (core::host(), get(cx)) else {
        return;
    };
    let task = cx
        .background_executor()
        .spawn(async move { host.serialized(|| work(&host)) });
    cx.spawn(async move |cx| {
        let result = task.await;
        browser.update(cx, |browser, cx| {
            browser.error = result.err();
            cx.notify();
        });
    })
    .detach();
}

/// Ask the host something off the UI thread, one at a time with the
/// user's commands; `None` when the module does not run.
pub(crate) fn query<T: Send + 'static>(
    cx: &App,
    work: impl FnOnce(&Arc<core::BrowserHost>) -> Result<T, String> + Send + 'static,
) -> Option<gpui::Task<Result<T, String>>> {
    let host = core::host()?;
    Some(
        cx.background_executor()
            .spawn(async move { host.serialized(|| work(&host)) }),
    )
}
