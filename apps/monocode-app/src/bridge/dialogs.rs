//! Message and confirm dialogs for engine hooks, drawn by the platform
//! (`ask` and `message` from the Tauri dialog plugin, and
//! `window.confirm`). They attach to the active window, or the first open
//! window when none is active. Without a window, `confirm` declines and
//! `alert` logs.

use gpui::{AnyWindowHandle, App, PromptLevel, Task};

fn some_window(cx: &App) -> Option<AnyWindowHandle> {
    cx.active_window()
        .or_else(|| cx.windows().into_iter().next())
}

/// A warning dialog with `ok_label` and Cancel. Resolves to whether the
/// user picked `ok_label`.
pub fn confirm(message: &str, ok_label: &str, cx: &mut App) -> Task<bool> {
    let Some(window) = some_window(cx) else {
        return Task::ready(false);
    };
    let answers = [ok_label, "Cancel"];
    let prompt = window.update(cx, |_, window, cx| {
        window.prompt(PromptLevel::Warning, message, None, &answers, cx)
    });
    match prompt {
        Ok(answer) => cx.spawn(async move |_| matches!(answer.await, Ok(0))),
        Err(_) => Task::ready(false),
    }
}

/// An error or warning message with an OK button.
pub fn alert(message: &str, error: bool, cx: &mut App) {
    let Some(window) = some_window(cx) else {
        log::warn!("[monocode] {message}");
        return;
    };
    let level = if error {
        PromptLevel::Critical
    } else {
        PromptLevel::Warning
    };
    let answer = window.update(cx, |_, window, cx| {
        window.prompt(level, message, None, &["OK"], cx)
    });
    if let Ok(answer) = answer {
        cx.spawn(async move |_| {
            let _ = answer.await;
        })
        .detach();
    }
}
