//! Release notes in the native modal, including the first updated launch.

#[cfg(test)]
#[path = "whats_new_tests.rs"]
mod tests;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, WindowId, div,
};
use monocode_markdown::MarkdownView;
use monocode_ui::widgets::modal;
use monocode_ui::{Theme, u};
use monocode_updater::release_notes::{
    BUNDLED_CHANGELOG, format_release_date, present_release_notes,
};
use monocode_view_transcript::transcript::view::style::{MarkdownVariant, markdown_style};

#[derive(Default)]
struct Dialogs(std::collections::HashMap<WindowId, Entity<WhatsNew>>);
impl Global for Dialogs {}

struct WhatsNew {
    version: Option<String>,
    description: String,
    markdown: Entity<MarkdownView>,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    _theme: Subscription,
}

fn ensure(window: &Window, cx: &mut App) -> Entity<WhatsNew> {
    let id = window.window_handle().window_id();
    if let Some(dialog) = cx
        .try_global::<Dialogs>()
        .and_then(|dialogs| dialogs.0.get(&id))
    {
        return dialog.clone();
    }
    let dialog = cx.new(|cx| {
        let markdown = cx.new(MarkdownView::new);
        let theme = cx.observe_global::<Theme>(|this: &mut WhatsNew, cx| {
            this.markdown.update(cx, |view, cx| {
                view.set_style(markdown_style(Theme::of(cx), MarkdownVariant::Normal), cx)
            });
            cx.notify();
        });
        WhatsNew {
            version: None,
            description: String::new(),
            markdown,
            focus: cx.focus_handle(),
            previous_focus: None,
            _theme: theme,
        }
    });
    if !cx.has_global::<Dialogs>() {
        cx.default_global::<Dialogs>();
        cx.on_window_closed(|cx, id| {
            cx.global_mut::<Dialogs>().0.remove(&id);
        })
        .detach();
    }
    cx.global_mut::<Dialogs>().0.insert(id, dialog.clone());
    dialog
}

pub fn open(version: &str, window: &mut Window, cx: &mut App) {
    ensure(window, cx).update(cx, |dialog, cx| {
        let notes = present_release_notes(version, BUNDLED_CHANGELOG);
        dialog.description = format!("MonoCode {version}");
        if let Some(date) = notes.as_ref().and_then(|notes| notes.date.as_deref()) {
            dialog
                .description
                .push_str(&format!(" · {}", format_release_date(date)));
        }
        if dialog.version.is_none() {
            dialog.previous_focus = window.focused(cx);
        }
        dialog.version = Some(version.to_owned());
        dialog.markdown.update(cx, |view, cx| {
            view.set_style(markdown_style(Theme::of(cx), MarkdownVariant::Normal), cx);
            view.set_streaming(false, cx);
            view.set_text(
                notes
                    .as_ref()
                    .map(|notes| notes.markdown.as_str())
                    .filter(|text| !text.is_empty())
                    .unwrap_or("Release notes for this version are not available in this build."),
                cx,
            );
        });
        dialog.focus.focus(window, cx);
        cx.notify();
    });
}

pub fn layer(window: &mut Window, cx: &mut App) -> AnyElement {
    let first = cx.try_global::<Dialogs>().is_none();
    let dialog = ensure(window, cx);
    if first && let Some(version) = super::sidebar_update::installed(cx) {
        open(&version, window, cx);
    }
    dialog.into_any_element()
}

impl WhatsNew {
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.version = None;
        if let Some(focus) = self.previous_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }
}

impl Render for WhatsNew {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.version.is_none() {
            return div().into_any_element();
        }
        let weak = cx.weak_entity();
        div()
            .id("whats-new-focus")
            .debug_selector(|| "whats-new-overlay".into())
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|dialog, event: &gpui::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        dialog.close(window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .child(
                modal("whats-new-modal", "What's new")
                    .description(self.description.clone())
                    .on_close(move |window, cx| {
                        weak.update(cx, |dialog, cx| dialog.close(window, cx)).ok();
                    })
                    .child(
                        div()
                            .id("whats-new-scroll")
                            .h(gpui::px(f32::from(window.viewport_size().height) * 0.6))
                            .max_h(u(560.))
                            .overflow_y_scroll()
                            .p(u(20.))
                            .child(self.markdown.clone()),
                    ),
            )
            .into_any_element()
    }
}
