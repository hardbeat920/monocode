use super::*;
use crate::explorer::DraggedFile;
use gpui::ExternalPaths;

impl Conversation {
    /// Like the session drop region, accepts files over the transcript as well
    /// as the input. Both sources go through the same attachment validation.
    pub(super) fn attachment_drop_target(
        &mut self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        if !cx.has_active_drag() {
            self.file_drag = false;
        }
        div()
            .id(id)
            .key_context("Session")
            .relative()
            .child(self.completion.read(cx).container_probe())
            .on_drag_move::<ExternalPaths>(cx.listener(
                |this, event: &gpui::DragMoveEvent<ExternalPaths>, _, cx| {
                    this.set_file_drag(event.bounds.contains(&event.event.position), cx);
                },
            ))
            .on_drag_move::<DraggedFile>(cx.listener(
                |this, event: &gpui::DragMoveEvent<DraggedFile>, _, cx| {
                    this.set_file_drag(event.bounds.contains(&event.event.position), cx);
                },
            ))
            .on_file_drop_exit(cx.listener(|this, _, _, cx| this.set_file_drag(false, cx)))
            .on_mouse_exit(cx.listener(|this, _, _, cx| this.set_file_drag(false, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.set_file_drag(false, cx)),
            )
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.file_drag {
                    cx.stop_active_drag(window);
                    this.set_file_drag(false, cx);
                    cx.stop_propagation();
                }
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.set_file_drag(false, cx);
                if paths.paths().is_empty() {
                    this.attachment_error = Some(
                        "This drag did not provide a file. Save the image, then drag the saved file here."
                            .into(),
                    );
                    cx.notify();
                } else {
                    this.add_attachments(paths.paths().to_vec(), cx);
                }
                window.focus(&this.input.focus_handle(cx), cx);
            }))
            .on_drop(cx.listener(|this, file: &DraggedFile, window, cx| {
                this.set_file_drag(false, cx);
                this.add_attachments(vec![file.0.clone()], cx);
                window.focus(&this.input.focus_handle(cx), cx);
            }))
    }

    fn set_file_drag(&mut self, over: bool, cx: &mut Context<Self>) {
        if self.file_drag != over {
            self.file_drag = over;
            cx.notify();
        }
    }

    pub(super) fn attachment_drop_overlay(&self, radius: f32) -> Option<AnyElement> {
        self.file_drag.then(|| {
            div()
                .absolute()
                .inset_0()
                .rounded(px(radius))
                .bg(accent_color().opacity(0.08))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.))
                .text_color(content(0.7))
                .child("Drop files to attach")
                .into_any_element()
        })
    }
}
