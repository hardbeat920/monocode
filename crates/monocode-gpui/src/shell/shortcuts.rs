//! Workspace fallback for orphaned GPUI focus and embedded AppKit previews.
//! Normal focused inputs keep the keymap's context ordering. Resolve from the
//! live keymap, including user overrides, rather than maintaining another list
//! of physical shortcuts.
use super::*;
use gpui::{Action, KeyContext, Keystroke, WeakEntity};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
mod macos;

#[derive(Default)]
struct Forwarding {
    native: Cell<bool>,
    handled: Cell<bool>,
    chord_deadline: Cell<Option<Instant>>,
}
pub(super) struct Shortcuts {
    _keys: Subscription,
    #[cfg(target_os = "macos")]
    _native: Option<macos::NativeKeys>,
}

fn workspace_action(action: &dyn Action) -> bool {
    let name = action.name();
    (name.starts_with("workspace::")
        && !matches!(
            name,
            "workspace::FocusNext" | "workspace::FocusPrevious" | "workspace::DismissMenu"
        ))
        || name == "application::Quit"
}

/// Keymap owns precedence, disabled bindings, physical-key matching and chords.
fn candidates(keys: &[Keystroke], cx: &App) -> Vec<KeyBinding> {
    let context = KeyContext::parse("Workspace").expect("static workspace key context");
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let contexts = [context];
    let (complete, _) = keymap.bindings_for_input(keys, &contexts);
    complete
        .into_iter()
        .chain(keymap.possible_next_bindings_for_input(keys, &contexts))
        .filter(|binding| workspace_action(binding.action()))
        .collect()
}

pub(super) fn install(owner: WeakEntity<Shell>, window: &Window, cx: &mut App) -> Shortcuts {
    let window_id = window.window_handle().window_id();
    let forwarding = Rc::new(Forwarding::default());
    #[cfg(target_os = "macos")]
    let native = macos::NativeKeys::new(window, forwarding.clone());
    let pending = RefCell::new(Vec::<Keystroke>::new());
    let keys = cx.intercept_keystrokes(move |event, window, cx| {
        if window.window_handle().window_id() != window_id {
            return;
        }
        let Some(owner) = owner.upgrade() else {
            return;
        };
        let root = owner.read(cx).focus.clone();
        let native = forwarding.native.get();
        let now = Instant::now();
        if forwarding
            .chord_deadline
            .get()
            .is_some_and(|until| now >= until)
        {
            pending.borrow_mut().clear();
            forwarding.chord_deadline.set(None);
        }
        if !native && root.contains_focused(window, cx) && pending.borrow().is_empty() {
            return;
        }
        // Modifier release observations are not strokes in a shortcut chord.
        if matches!(
            event.keystroke.key.as_str(),
            "control" | "platform" | "alt" | "shift" | "function"
        ) {
            return;
        }
        let mut sequence = pending.borrow_mut();
        sequence.push(event.keystroke.clone());
        let mut matches = candidates(&sequence, cx);
        if matches.is_empty() && sequence.len() > 1 {
            sequence.clear();
            sequence.push(event.keystroke.clone());
            matches = candidates(&sequence, cx);
        }
        let action = matches
            .iter()
            .find(|binding| binding.match_keystrokes(sequence.as_slice()) == Some(false))
            .map(|binding| binding.action().boxed_clone());
        let partial = action.is_none() && !matches.is_empty();
        if !partial {
            sequence.clear();
        }
        drop(sequence);
        forwarding
            .chord_deadline
            .set(partial.then(|| now + Duration::from_secs(1)));
        if let Some(action) = action {
            if !native && !root.contains_focused(window, cx) {
                // Repair lost GPUI focus without disturbing an embedded native
                // view's selection or its first-responder ownership.
                owner.update(cx, |shell, cx| shell.focus_workspace(window, cx));
            }
            root.dispatch_action(action.as_ref(), window, cx);
            forwarding.handled.set(true);
            cx.stop_propagation();
        } else if partial {
            forwarding.handled.set(true);
            cx.stop_propagation();
        } else if native {
            // The monitor re-delivers the original event to AppKit. Prevent
            // the stale GPUI input from processing Copy, typing, IME, etc.
            cx.stop_propagation();
        }
    });
    Shortcuts {
        _keys: keys,
        #[cfg(target_os = "macos")]
        _native: native,
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    #[gpui::test]
    fn lost_focus_shortcuts_follow_live_remaps_and_disabled_bindings(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::input::bind(cx);
            super::super::bind_keys(cx);
            cx.set_reduce_motion(true);
        });
        let (window, _, _) = super::super::tests::window(cx);
        cx.refresh().unwrap();
        let orphan = window
            .update(cx, |_, window, cx| {
                let orphan = cx.focus_handle();
                window.focus(&orphan, cx);
                orphan
            })
            .unwrap();
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.simulate_keystrokes(window.into(), &format!("{modifier}-b"));
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(!shell.workspace.sidebar_visible))
            .unwrap();
        for (binding, stroke, expected) in [
            (None, format!("{modifier}-b"), false),
            (
                Some(format!("{modifier}-alt-b")),
                format!("{modifier}-b"),
                false,
            ),
            (
                Some(format!("{modifier}-alt-b")),
                format!("{modifier}-alt-b"),
                true,
            ),
            (
                Some(format!("{modifier}-alt-y {modifier}-alt-b")),
                format!("{modifier}-alt-y {modifier}-alt-b"),
                false,
            ),
        ] {
            window
                .update(cx, |shell, window, cx| {
                    shell
                        .workspace
                        .preferences
                        .keybindings
                        .insert("App: Toggle Sidebar".into(), binding);
                    crate::settings::apply_keybindings(&shell.workspace.preferences, cx);
                    window.focus(&orphan, cx);
                })
                .unwrap();
            cx.simulate_keystrokes(window.into(), &stroke);
            cx.run_until_parked();
            window
                .update(cx, |shell, _, _| {
                    assert_eq!(shell.workspace.sidebar_visible, expected)
                })
                .unwrap();
        }
    }
}
