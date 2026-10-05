//! Drawing the quick composer card: the drag handle, close button, working
//! copy controls, attachment chips, the prompt, the toolbar, and the list
//! that opens under it.

use std::time::Instant;

use base64::Engine as _;
use gpui::{
    AnyElement, Context, ExternalPaths, ImageFormat, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, canvas, div, img, prelude::*, px,
};
use monocode_core::{Attachment, AttachmentKind, HarnessId};
use monocode_layout::paths::project_name;
use monocode_ui::widgets::kbd;
use monocode_ui::{IconName, Theme, UiStyled as _, file_type_icon, icon, u};
use monocode_view_composer::composer::model::commands::DRAFT;
use monocode_view_composer::composer::model::mode_commands::Mode;
use monocode_view_composer::composer::prompt_input;

use super::{OpenModels, OpenProjects, PROMPT_CONTEXT, QuickComposer};
use crate::colors;
use crate::model::motion::Picker;
use crate::model::prompt::command_label;
use crate::model::selector::pretty_parent;
use crate::permissions::permission_icon_element;
use crate::project_icon::quick_project_icon;
use crate::selector::harness_icon;

/// The card's tallest height (`max-h-[520px]`).
const CARD_MAX_HEIGHT: f32 = 520.0;
const CARD_RADIUS: f32 = 16.0;
const FRAME_GROUP: &str = "quick-composer-frame";

impl QuickComposer {
    fn option_row(
        &self,
        id: SharedString,
        index: usize,
        stacked: bool,
        theme: &Theme,
    ) -> gpui::Stateful<gpui::Div> {
        let highlighted = index == self.highlight;
        let mut row = div()
            .id(id)
            .flex()
            .flex_none()
            .w_full()
            .rounded(u(theme.radius.lg))
            .px(u(8.))
            .py(u(6.))
            .text_px(13.)
            .text_color(if highlighted {
                theme.colors.content
            } else {
                theme.content(0.75)
            });
        row = if stacked {
            row.flex_col().items_start().gap(u(2.))
        } else {
            row.items_center().gap(u(10.))
        };
        if highlighted {
            row = row.bg(theme.colors.selection_emphasis);
        }
        row
    }

    fn render_commands(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let options = self.command_options();
        let mut list = div()
            .id("quick-composer-commands")
            .track_scroll(&self.list_scroll)
            .flex()
            .flex_col()
            .flex_none()
            .border_t_1()
            .border_color(theme.colors.stroke)
            .p(u(8.));
        if options.is_empty() {
            return list
                .child(
                    div()
                        .px(u(8.))
                        .py(u(8.))
                        .text_px(12.)
                        .text_color(theme.content(0.45))
                        .child("No matching commands"),
                )
                .into_any_element();
        }
        for (index, command) in options.into_iter().enumerate() {
            let mode = Mode::from_name(&command.name);
            let mut label = div().flex().items_center().gap(u(6.));
            if let Some(mode) = mode {
                label = label.child(
                    icon(mode.icon())
                        .size(u(14.))
                        .text_color(colors::mode_menu_icon(mode, theme)),
                );
            }
            label = label.child(command_label(&command.name));
            let row = self
                .option_row(
                    SharedString::from(format!("quick-command-{}", command.invocation)),
                    index,
                    true,
                    theme,
                )
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.highlight != index {
                        this.highlight = index;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.choose_at(index, window, cx)))
                .child(label)
                .child(
                    div()
                        .text_px(11.)
                        .line_height(u(16.))
                        .text_color(theme.content(0.50))
                        .child(command.description.clone()),
                );
            list = list.child(row);
        }
        list.into_any_element()
    }

    fn render_projects(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let options = self.project_options(cx);
        let search = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(8.))
            .px(u(16.))
            .py(u(8.))
            .child(
                icon(IconName::Search)
                    .size(u(14.))
                    .text_color(theme.content(0.45)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_px(13.)
                    .line_height(u(20.))
                    .text_color(theme.colors.content)
                    .capture_action(cx.listener(|this, _: &prompt_input::Escape, window, cx| {
                        cx.stop_propagation();
                        this.close_picker(window, cx);
                    }))
                    .capture_action(cx.listener(|this, _: &prompt_input::MoveDown, _, cx| {
                        cx.stop_propagation();
                        this.step_highlight(true, cx);
                    }))
                    .capture_action(cx.listener(|this, _: &prompt_input::MoveUp, _, cx| {
                        cx.stop_propagation();
                        this.step_highlight(false, cx);
                    }))
                    .capture_action(cx.listener(|this, _: &prompt_input::Enter, window, cx| {
                        cx.stop_propagation();
                        if this.query.read(cx).is_composing() {
                            return;
                        }
                        let count = this.project_options(cx).len();
                        if count > 0 {
                            this.choose_at(this.highlight.min(count - 1), window, cx);
                        }
                    }))
                    .capture_action(cx.listener(|_, _: &prompt_input::Newline, _, cx| {
                        cx.stop_propagation();
                    }))
                    .child(self.query.clone()),
            );
        let mut list = div()
            .id("quick-composer-projects")
            .track_scroll(&self.list_scroll)
            .flex()
            .flex_col()
            .min_h_0()
            .max_h(u(256.))
            .overflow_y_scroll()
            .px(u(8.))
            .pb(u(8.));
        if options.is_empty() {
            list = list.child(
                div()
                    .px(u(8.))
                    .py(u(8.))
                    .text_px(12.)
                    .text_color(theme.content(0.45))
                    .child("No matches"),
            );
        }
        for (index, path) in options.into_iter().enumerate() {
            let row = self
                .option_row(
                    SharedString::from(format!("quick-project-{path}")),
                    index,
                    false,
                    theme,
                )
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.highlight != index {
                        this.highlight = index;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.choose_at(index, window, cx)))
                .child(quick_project_icon(
                    &path,
                    &self.appearance,
                    12.,
                    theme.colors.content,
                ))
                .child(div().min_w_0().truncate().child(project_name(&path)))
                .child(
                    div()
                        .ml_auto()
                        .min_w_0()
                        .truncate()
                        .pl(u(12.))
                        .text_px(11.)
                        .text_color(theme.content(0.40))
                        .child(pretty_parent(&path)),
                );
            list = list.child(row);
        }
        div()
            .flex()
            .flex_col()
            .min_h_0()
            .border_t_1()
            .border_color(theme.colors.stroke)
            .child(search)
            .child(list)
            .into_any_element()
    }

    /// `AttachmentChip`: images show a 36px thumbnail, other files their
    /// icon and name.
    fn render_chip(&self, file: &Attachment, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let preview = attachment_image(file);
        let image = file.kind == AttachmentKind::Image && preview.is_some();
        let mut chip = div()
            .relative()
            .flex()
            .flex_none()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .rounded(u(theme.radius.md));
        chip = match preview.filter(|_| image) {
            Some(source) => chip.child(
                div()
                    .size(u(36.))
                    .flex_none()
                    .rounded(u(theme.radius.lg))
                    .overflow_hidden()
                    .child(img(source).size_full().object_fit(gpui::ObjectFit::Cover)),
            ),
            None => chip
                .bg(theme.content(0.10))
                .py(u(2.))
                .px(u(4.))
                .child(
                    div()
                        .size(u(20.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .child(file_type_icon(file.name.clone()).size(16.)),
                )
                .child(
                    div()
                        .min_w_0()
                        .max_w(u(140.))
                        .truncate()
                        .text_px(11.)
                        .line_height(gpui::relative(1.))
                        .text_color(theme.content(0.80))
                        .child(file.name.clone()),
                ),
        };
        if !self.busy {
            let id = file.id.clone();
            let hover = theme.content(0.15);
            let hover_ink = theme.colors.content;
            let mut remove = div()
                .id(SharedString::from(format!("quick-attachment-remove-{id}")))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded_full()
                .hover(move |style| style.bg(hover).text_color(hover_ink))
                .tooltip(monocode_ui::widgets::tooltip("Remove"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.remove_attachment(&id, cx);
                }));
            remove = if image {
                remove
                    .absolute()
                    .top(u(-4.))
                    .right(u(-4.))
                    .size(u(20.))
                    .bg(theme.content(0.20))
                    .shadow_sm()
                    .child(
                        icon(IconName::X)
                            .size(u(12.))
                            .text_color(theme.content(0.70)),
                    )
            } else {
                remove.size(u(16.)).child(
                    icon(IconName::X)
                        .size(u(12.))
                        .text_color(theme.content(0.40)),
                )
            };
            chip = chip.child(remove);
        }
        chip.into_any_element()
    }

    fn toolbar_button(
        &self,
        id: &'static str,
        active: bool,
        theme: &Theme,
    ) -> gpui::Stateful<gpui::Div> {
        self.picker_button(id, active, 0.70, theme)
    }

    /// A button that opens a picker. `rest` is its ink while closed.
    fn picker_button(
        &self,
        id: &'static str,
        active: bool,
        rest: f32,
        theme: &Theme,
    ) -> gpui::Stateful<gpui::Div> {
        let hover_bg = theme.colors.selection_hover;
        let hover_ink = theme.colors.content;
        let button = div()
            .id(id)
            .flex()
            .min_w_0()
            .items_center()
            .rounded(u(theme.radius.md))
            .text_px(12.);
        if active {
            button
                .bg(theme.colors.selection_emphasis)
                .text_color(theme.colors.content)
        } else {
            button
                .text_color(theme.content(rest))
                .hover(move |style| style.bg(hover_bg).text_color(hover_ink))
        }
    }

    /// The project picker's button, at the right end of the header.
    fn render_project_button(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let active = self.picker == Some(Picker::Project);
        let mut project = self
            .picker_button("quick-project", active, 0.55, theme)
            .ml_auto()
            .max_w(gpui::relative(0.4))
            .h(u(24.))
            .gap(u(6.))
            .px(u(6.))
            .tooltip(monocode_ui::widgets::tooltip("Project (\u{2318}P)"));
        if let Some(cwd) = &self.cwd {
            project = project.child(quick_project_icon(
                cwd,
                &self.appearance,
                12.,
                theme.colors.content,
            ));
        }
        let alpha = if active { 1.0 } else { 0.55 };
        project = project
            .child(
                div().min_w_0().truncate().child(
                    self.cwd
                        .as_deref()
                        .map(project_name)
                        .unwrap_or_else(|| "No project".into()),
                ),
            )
            .child(
                icon(IconName::ChevronDown)
                    .size(u(12.))
                    .text_color(theme.content(0.60 * alpha)),
            );
        if self.projects.is_empty() {
            project = project.opacity(0.5);
        } else {
            project = project.on_click(
                cx.listener(|this, _, window, cx| this.open_picker(Picker::Project, window, cx)),
            );
        }
        project.into_any_element()
    }

    fn render_toolbar(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let supported = self.attachments_supported();
        let loading = self.attachments.loading;
        let picker = self.picker;
        let mut plus = self
            .toolbar_button(
                "quick-add-attachment",
                picker == Some(Picker::Attachments),
                theme,
            )
            .size(u(26.))
            .flex_none()
            .justify_center()
            .tooltip(monocode_ui::widgets::tooltip(if supported {
                "Attach files or take a screenshot"
            } else {
                "This provider does not support attachments"
            }))
            .child(icon(IconName::Plus).size(u(14.)).text_color(
                if picker == Some(Picker::Attachments) {
                    theme.colors.content
                } else {
                    theme.content(0.70)
                },
            ));
        if !supported || loading || self.busy {
            plus = plus.opacity(0.4);
        } else {
            plus =
                plus.on_click(cx.listener(|this, _, window, cx| {
                    this.open_picker(Picker::Attachments, window, cx)
                }));
        }

        let model = self.model();
        let model_button = self
            .toolbar_button("quick-model", picker == Some(Picker::Model), theme)
            .max_w(gpui::relative(0.4))
            .gap(u(6.))
            .px(u(8.))
            .py(u(4.))
            .tooltip(monocode_ui::widgets::tooltip("Model (\u{2318}.)"))
            .on_click(
                cx.listener(|this, _, window, cx| this.open_picker(Picker::Model, window, cx)),
            )
            .child(harness_icon(model.harness, 14., theme.colors.content))
            .child(div().min_w_0().truncate().child(model.name.clone()))
            .child(
                icon(IconName::ChevronDown)
                    .size(u(12.))
                    .text_color(theme.content(0.60 * 0.70)),
            );

        // Fx sessions run without the permission modes.
        let permissions = (model.harness != HarnessId::Fx).then(|| {
            let active = picker == Some(Picker::Permissions);
            let ink = if active {
                theme.colors.content
            } else {
                theme.content(0.70)
            };
            let mode = self.runtime_mode;
            self.toolbar_button("quick-permissions", active, theme)
                .gap(u(6.))
                .px(u(8.))
                .py(u(4.))
                .tooltip(monocode_ui::widgets::tooltip("Permissions"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_picker(Picker::Permissions, window, cx)
                }))
                .child(permission_icon_element(mode, 14., ink, theme))
                .child(div().min_w_0().truncate().child(mode.label()))
                .child(
                    icon(IconName::ChevronDown)
                        .size(u(12.))
                        .text_color(theme.content(0.60 * 0.70)),
                )
        });

        let status: AnyElement = if loading {
            div().child("Adding attachment\u{2026}").into_any_element()
        } else if let Some(error) = &self.error {
            div()
                .max_w(u(288.))
                .truncate()
                .text_color(colors::red(theme, 1.0))
                .child(error.clone())
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap(u(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(u(4.))
                        .child(kbd("\u{21b5}"))
                        .child("start"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(u(4.))
                        .child(kbd("\u{2318}\u{21b5}"))
                        .child("start and open"),
                )
                .into_any_element()
        };
        let draft = self
            .leading_mode(cx)
            .is_some_and(|token| token.mode.name() == DRAFT);
        let can_submit = self.can_submit(cx);
        let mut start = div()
            .id("quick-start")
            .rounded(u(theme.radius.md))
            .bg(theme.colors.accent)
            .px(u(10.))
            .py(u(4.))
            .text_px(12.)
            .medium()
            .text_color(colors::white())
            .child(if draft { "Save draft" } else { "Start" });
        if can_submit {
            start =
                start.on_click(cx.listener(|this, _, window, cx| this.submit(false, window, cx)));
        } else {
            start = start.opacity(0.4);
        }
        div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(6.))
            .border_t_1()
            .border_color(theme.colors.stroke)
            .px(u(12.))
            .py(u(8.))
            .child(plus)
            .child(model_button)
            .children(permissions)
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(12.))
                    .text_px(11.)
                    .text_color(theme.content(0.45))
                    .child(status)
                    .child(start),
            )
            .into_any_element()
    }

    /// The + menu: a small popover above the plus button.
    fn render_attachment_menu(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let enabled = !self.attachments.loading && self.attachments_supported();
        let row = |id: &'static str, glyph: IconName, label: &'static str| {
            let hover = theme.colors.selection_hover;
            let mut row = div()
                .id(id)
                .flex()
                .w_full()
                .items_center()
                .gap(u(8.))
                .rounded(u(theme.radius.md))
                .px(u(8.))
                .py(u(6.))
                .text_px(12.)
                .text_color(theme.colors.content)
                .child(icon(glyph).size(u(14.)).text_color(theme.colors.content))
                .child(label);
            if enabled {
                row = row.hover(move |style| style.bg(hover));
            } else {
                row = row.opacity(0.4);
            }
            row
        };
        let mut choose = row(
            "quick-choose-files",
            IconName::ImagePlus,
            "Choose files\u{2026}",
        );
        let mut capture = row(
            "quick-take-screenshot",
            IconName::Maximize2,
            "Take screenshot\u{2026}",
        );
        if enabled {
            choose = choose.on_click(cx.listener(|this, _, window, cx| {
                this.set_picker(None, cx);
                this.choose_files(window, cx);
                this.focus_prompt(window, cx);
            }));
            capture = capture.on_click(cx.listener(|this, _, window, cx| {
                this.set_picker(None, cx);
                this.take_screenshot(window, cx);
                this.focus_prompt(window, cx);
            }));
        }
        div()
            .absolute()
            .left(u(12.))
            .bottom(u(4.))
            .child(
                monocode_ui::widgets::popover_frame("quick-attachment-menu")
                    .side(monocode_ui::widgets::PopoverSide::Top)
                    .width(220.)
                    .child(div().p(u(4.)).child(choose).child(capture)),
            )
            .into_any_element()
    }

    fn render_prompt(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        div()
            .relative()
            .flex_none()
            // `rows={2}` at 24px plus the padding.
            .min_h(u(16. + 48. + 8.))
            .text_px(16.)
            .line_height(u(24.))
            .text_color(theme.colors.content)
            .key_context(PROMPT_CONTEXT)
            .on_action(cx.listener(|this, _: &OpenProjects, window, cx| {
                this.open_picker(Picker::Project, window, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenModels, window, cx| {
                this.open_picker(Picker::Model, window, cx)
            }))
            .capture_action(cx.listener(|this, _: &prompt_input::Escape, window, cx| {
                if this.prompt.read(cx).is_composing() {
                    return;
                }
                cx.stop_propagation();
                this.escape(window, cx);
            }))
            .capture_action(cx.listener(|this, _: &prompt_input::MoveDown, _, cx| {
                if this.picker == Some(Picker::Commands) && !this.prompt.read(cx).is_composing() {
                    cx.stop_propagation();
                    this.step_highlight(true, cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &prompt_input::MoveUp, _, cx| {
                if this.picker == Some(Picker::Commands) && !this.prompt.read(cx).is_composing() {
                    cx.stop_propagation();
                    this.step_highlight(false, cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &prompt_input::Tab, window, cx| {
                if this.complete_command(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &prompt_input::Enter, window, cx| {
                if this.prompt.read(cx).is_composing() {
                    return;
                }
                let modifiers = window.modifiers();
                if !modifiers.shift && !modifiers.alt && this.complete_command(window, cx) {
                    cx.stop_propagation();
                    return;
                }
                if modifiers.alt {
                    // Option+Return adds a line, as the textarea did.
                    return;
                }
                cx.stop_propagation();
                this.submit(modifiers.platform, window, cx);
            }))
            .capture_action(cx.listener(|this, _: &prompt_input::Paste, window, cx| {
                if this.cwd.is_none() || this.prompt.read(cx).is_composing() {
                    return;
                }
                if this.paste(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.prompt
                        .update(cx, |prompt, cx| prompt.focus(window, cx))
                }),
            )
            .child(self.prompt.clone())
            .into_any_element()
    }

    /// Enter or Tab over the command list picks the highlighted command.
    fn complete_command(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.picker != Some(Picker::Commands) {
            return false;
        }
        let modifiers = window.modifiers();
        if modifiers.shift || modifiers.alt || modifiers.platform || modifiers.control {
            return false;
        }
        let count = self.command_options().len();
        if count == 0 {
            return false;
        }
        self.choose_at(self.highlight.min(count - 1), window, cx);
        true
    }
}

/// `attachmentPreviewSrc` as an image source: inline bytes, else the file.
fn attachment_image(file: &Attachment) -> Option<gpui::ImageSource> {
    if let Some(data) = file.data.as_deref().filter(|data| !data.is_empty())
        && file.kind == AttachmentKind::Image
        && let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data)
    {
        let format = ImageFormat::from_mime_type(&file.mime_type).unwrap_or(ImageFormat::Png);
        return Some(gpui::ImageSource::Image(std::sync::Arc::new(
            gpui::Image::from_bytes(format, bytes),
        )));
    }
    file.path
        .as_deref()
        .filter(|path| !path.is_empty() && file.kind == AttachmentKind::Image)
        .map(|path| gpui::ImageSource::from(std::path::PathBuf::from(path)))
}

impl Render for QuickComposer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let now = Instant::now();
        // `prefers-reduced-motion`: pickers open at their full height at once.
        self.motion.set_reduced_motion(cx.reduce_motion());

        // After a picker change the card holds its last height for one
        // frame, while the probe measures the new layout; the resize then
        // runs from there.
        let hold = if self.motion_pending {
            self.motion.last_height()
        } else {
            None
        };
        let animated_height = hold.or_else(|| self.motion.height_at(now));
        let picker_opacity = self.motion.opacity_at(now);
        if self.motion.animating() {
            window.request_animation_frame();
        }

        let mut frame = div()
            .id("quick-composer")
            .key_context("QuickComposer")
            .track_focus(&self.focus_handle)
            .group(FRAME_GROUP)
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .max_h(u(CARD_MAX_HEIGHT))
            .overflow_hidden()
            .rounded(u(CARD_RADIUS))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.colors.background_base.opacity(0.45))
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && !event.keystroke.modifiers.modified() {
                    cx.stop_propagation();
                    this.escape(window, cx);
                }
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.on_drop_paths(paths, window, cx)
            }));
        if let Some(height) = animated_height {
            frame = frame.h(px(height));
        }

        // The body keeps its natural height; the frame clips it while it
        // animates. A probe measures it each frame.
        let natural = self.natural_height.clone();
        let entity = cx.entity().downgrade();
        let probe = canvas(
            move |bounds, window, cx| {
                let height = f32::from(bounds.size.height);
                if (natural.get() - height).abs() > 0.25 {
                    natural.set(height);
                    window.request_animation_frame();
                }
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        if this.motion_pending {
                            this.motion_pending = false;
                            this.motion
                                .picker_changed(this.picker, height, Instant::now());
                            window.request_animation_frame();
                        } else {
                            this.motion.retarget(height);
                        }
                        let shown = this.motion.height_at(Instant::now()).unwrap_or(height);
                        if let Some(fit) = this.motion.measured(shown.min(CARD_MAX_HEIGHT)) {
                            cx.emit(super::QuickComposerEvent::Fit(fit));
                        }
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        let mut body = div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w_full()
            .child(probe);

        // Drag to move.
        let handle_hover = theme.content(0.35);
        body = body.child(
            div()
                .id("quick-drag")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(u(12.))
                .flex()
                .justify_center()
                .pt(u(4.))
                .cursor_grab()
                .group("quick-drag")
                .tooltip(monocode_ui::widgets::tooltip("Drag to move"))
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    cx.stop_propagation();
                    window.start_window_move();
                })
                .child(
                    div()
                        .h(u(2.))
                        .w(u(24.))
                        .rounded_full()
                        .bg(theme.content(0.15))
                        .group_hover("quick-drag", move |style| style.bg(handle_hover)),
                ),
        );

        let close_hover_bg = theme.colors.selection_hover;
        let close_hover_ink = theme.colors.content;
        body = body.child(
            div()
                .id("quick-close")
                .absolute()
                .right(u(8.))
                .top(u(14.))
                .size(u(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(u(theme.radius.sm))
                .text_color(theme.content(0.35))
                .hover(move |style| style.bg(close_hover_bg).text_color(close_hover_ink))
                .tooltip(monocode_ui::widgets::tooltip("Close (Esc)"))
                .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx)))
                .child(
                    icon(IconName::X)
                        .size(u(12.))
                        .text_color(theme.content(0.35)),
                ),
        );

        let project_button = self.render_project_button(&theme, cx);
        body = body.child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(u(8.))
                .px(u(20.))
                .pt(u(12.))
                .pr(u(32.))
                .child(self.render_workspace_controls(cx))
                .child(project_button),
        );

        if !self.attachments.files.is_empty() {
            let files = self.attachments.files.clone();
            let mut chips = div()
                .id("quick-attachments")
                .flex()
                .flex_none()
                .flex_wrap()
                .gap(u(6.))
                .max_h(u(112.))
                .overflow_y_scroll()
                .px(u(20.))
                .pt(u(16.))
                .pb(u(4.));
            for file in &files {
                chips = chips.child(self.render_chip(file, &theme, cx));
            }
            body = body.child(chips);
        }

        body = body.child(self.render_prompt(&theme, cx));

        if !self.attachments.files.is_empty() && !self.attachments_supported() {
            body = body.child(
                div()
                    .px(u(20.))
                    .pb(u(8.))
                    .text_px(12.)
                    .text_color(colors::amber(&theme, 1.0))
                    .child(
                        "Choose a provider that supports attachments, or remove the attached files.",
                    ),
            );
        }

        body = body.child(self.render_toolbar(&theme, cx));

        if let Some(picker) = self.picker.filter(|picker| *picker != Picker::Attachments) {
            let panel: AnyElement = match picker {
                Picker::Commands => self.render_commands(&theme, cx),
                Picker::Project => self.render_projects(&theme, cx),
                Picker::Model => match self.selector() {
                    Some(selector) => selector.clone().into_any_element(),
                    None => div().into_any_element(),
                },
                Picker::Permissions => match self.permissions() {
                    Some(permissions) => permissions.clone().into_any_element(),
                    None => div().into_any_element(),
                },
                Picker::Attachments => div().into_any_element(),
            };
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .min_h_0()
                    .opacity(picker_opacity)
                    .child(panel),
            );
        }

        frame = frame.child(body);

        if self.picker == Some(Picker::Attachments) {
            // Above the toolbar, so it never moves the prompt.
            let menu = self.render_attachment_menu(&theme, cx);
            frame = frame.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(u(43.))
                    .h(u(0.))
                    .child(menu),
            );
        }

        if self.can_collect() {
            frame = frame.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(u(CARD_RADIUS))
                    .border_1()
                    .border_dashed()
                    .border_color(theme.accent(0.60))
                    .bg(theme.colors.background_base.opacity(0.90))
                    .text_px(14.)
                    .text_color(theme.colors.accent)
                    .invisible()
                    .group_drag_over::<ExternalPaths>(FRAME_GROUP, |style| style.visible())
                    .child("Drop to attach"),
            );
        }
        frame
    }
}
