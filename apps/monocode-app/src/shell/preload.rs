//! Port of src/app/model/preloadNavigation.ts and its App.tsx effect: once
//! the workspace has painted, load the Automations list and the notes so the
//! first visit to either page shows them at once instead of a spinner.
//!
//! The React app also loaded the Inbox, Notes, and Automations code there
//! (`lazySurface.preload`) and opened pages in a transition that kept the
//! current view until the code arrived. The native pages are compiled in and
//! build in the same frame, so only the data needs loading early.

use gpui::{Context, Window};
use monocode_engine::automations::AutomationsPackage;
use monocode_engine::history::HistoryPackage;

use super::Shell;

/// `preloadNavigationWhenIdle`: run `preload` after the window draws two
/// more frames, as the two `requestAnimationFrame`s did. GPUI has no idle
/// callback, so it runs on the next turn of the event loop after that, like
/// the `setTimeout(preload, 0)` fallback. A view dropped before then never
/// runs it.
pub(super) fn after_paint<T: 'static>(
    window: &Window,
    cx: &mut Context<T>,
    preload: impl FnOnce(&mut T, &mut Context<T>) + 'static,
) {
    let view = cx.weak_entity();
    window.on_next_frame(move |window, _| {
        window.on_next_frame(move |_, cx| {
            cx.defer(move |cx| {
                view.update(cx, preload).ok();
            });
        });
    });
}

impl Shell {
    /// The App.tsx warmup: the automations list (`listAutomations`) and,
    /// while Notes is on, the notes (`loadNotes`).
    pub(super) fn preload_navigation(&mut self, cx: &mut Context<Self>) {
        if let Some(package) = AutomationsPackage::try_global(cx) {
            let automations = package.automations.clone();
            if automations.read(cx).is_loading() {
                automations.update(cx, |automations, cx| automations.refresh(cx).detach());
            }
        }
        let notes_enabled = self
            .settings_kv(cx)
            .is_none_or(|kv| monocode_settings::settings_store::load_notes_enabled(&kv));
        if notes_enabled && let Some(package) = HistoryPackage::try_global(cx) {
            let notes = package.notes.clone();
            // The entity keeps the listing in flight, so dropping this
            // handle does not cancel it.
            notes.update(cx, |notes, cx| drop(notes.load_notes(false, cx)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{IntoElement, Render, TestAppContext, div};

    #[derive(Default)]
    struct Page {
        preloads: usize,
    }

    impl Render for Page {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn schedule(page: &gpui::WindowHandle<Page>, cx: &mut TestAppContext) {
        page.update(cx, |_, window, cx| {
            after_paint(window, cx, |page, _| page.preloads += 1)
        })
        .unwrap();
    }

    fn paint(page: &gpui::WindowHandle<Page>, cx: &mut TestAppContext) {
        page.update(cx, |_, window, cx| {
            window.simulate_next_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    fn preloads(page: &gpui::WindowHandle<Page>, cx: &mut TestAppContext) -> usize {
        page.update(cx, |page, _, _| page.preloads).unwrap()
    }

    #[gpui::test]
    fn preloads_after_two_painted_frames(cx: &mut TestAppContext) {
        let page = cx.add_window(|_, _| Page::default());
        cx.run_until_parked();
        schedule(&page, cx);
        cx.run_until_parked();
        assert_eq!(preloads(&page, cx), 0);
        paint(&page, cx);
        assert_eq!(preloads(&page, cx), 0);
        paint(&page, cx);
        assert_eq!(preloads(&page, cx), 1);
        paint(&page, cx);
        assert_eq!(preloads(&page, cx), 1);
    }

    #[gpui::test]
    fn a_closed_window_never_preloads(cx: &mut TestAppContext) {
        let ran = std::rc::Rc::new(std::cell::Cell::new(false));
        let page = cx.add_window(|_, _| Page::default());
        let flag = ran.clone();
        page.update(cx, |_, window, cx| {
            after_paint(window, cx, move |_, _| flag.set(true))
        })
        .unwrap();
        paint(&page, cx);
        page.update(cx, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        assert!(!ran.get());
    }
}
