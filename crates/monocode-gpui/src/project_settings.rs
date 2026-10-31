//! TabGroupMenu's project appearance controls and ProjectBackgroundDialog.
use crate::{
    input::{self, Input},
    popover::{Align, Anchor, AnchorSlot, popover},
    project_background::ProjectBackground,
    theme::*,
};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton,
    MouseDownEvent, MouseMoveEvent, ObjectFit, PathPromptOptions, Role, SharedString, Subscription,
    Window, canvas, div, prelude::*, px, rgb,
};
use monocode_core::project_appearance::{
    Background, COLORS, Effect, MASCOTS, ProjectAppearance, Scope, import_image,
};
use std::path::PathBuf;

pub(crate) enum Event {
    Changed(ProjectAppearance),
    Close,
}

// The original 8x8 project sprites from projectMascots.ts.
const SPRITES: [[&str; 8]; 10] = [
    [
        "..#..#..", ".######.", "##.##.##", "########", ".######.", ".#.##.#.", "#.#..#.#",
        "........",
    ],
    [
        "..####..", ".######.", "##.##.##", "########", "########", "########", "########",
        "#.##.##.",
    ],
    [
        "...#....", ".######.", ".#.##.#.", ".######.", ".#....#.", ".######.", "..#..#..",
        "........",
    ],
    [
        ".#....#.", ".##..##.", "########", "#.####.#", "########", "###..###", ".######.",
        "..#..#..",
    ],
    [
        ".######.", "########", "##.##.##", "########", ".##..##.", ".######.", ".#.##.#.",
        "........",
    ],
    [
        "#......#", ".#....#.", ".######.", "##.##.##", "########", "#.####.#", "#......#",
        "........",
    ],
    [
        "..####..", ".######.", "########", "##.##.##", "########", "...##...", "...##...",
        "..####..",
    ],
    [
        "...##...", "..####..", "..#..#..", "..####..", ".######.", ".######.", "##....##",
        "..####..",
    ],
    [
        "...#####", "...##.##", "...#####", ".#######", "########", "#####...", ".##.##..",
        "..#..#..",
    ],
    [
        "........", "##....##", "#.####.#", "########", "########", ".######.", "##....##",
        "........",
    ],
];
fn mascot(index: usize, side: f32, color: gpui::Hsla) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let unit = bounds.size.width / 8.;
            for (y, row) in SPRITES[index].iter().enumerate() {
                for (x, pixel) in row.bytes().enumerate() {
                    if pixel == b'#' {
                        window.paint_quad(gpui::fill(
                            gpui::Bounds::new(
                                bounds.origin + gpui::point(unit * x as f32, unit * y as f32),
                                gpui::size(unit, unit),
                            ),
                            color,
                        ));
                    }
                }
            }
        },
    )
    .size(px(side))
    .flex_shrink_0()
    .into_any_element()
}
pub(crate) fn project_icon(appearance: &ProjectAppearance, project: &str, side: f32) -> AnyElement {
    if project.starts_with("remote://") && appearance.logo.is_none() {
        return icon("internet", side)
            .text_color(content(0.7))
            .into_any_element();
    }
    match &appearance.logo {
        Some(path) => crate::image_memory::thumbnail(PathBuf::from(path))
            .size(px(side))
            .object_fit(ObjectFit::Contain)
            .flex_shrink_0()
            .into_any_element(),
        None => mascot(
            appearance.mascot(project),
            side,
            rgb(appearance.color(project)).into(),
        ),
    }
}
/// Sidebar.tsx's CurrentProjectIcon: a 10px mascot inside a 20px tile.
pub(crate) fn header_project_icon(appearance: &ProjectAppearance, project: &str) -> AnyElement {
    if project.starts_with("remote://") && appearance.logo.is_none() {
        return div()
            .size(px(20.))
            .flex()
            .items_center()
            .justify_center()
            .child(icon("internet", 14.).text_color(content(0.7)))
            .into_any_element();
    }
    match &appearance.logo {
        Some(path) => crate::image_memory::thumbnail(PathBuf::from(path))
            .relative()
            .left(px(-1.))
            .size(px(20.))
            .rounded(px(5.))
            .object_fit(ObjectFit::Contain)
            .flex_shrink_0()
            .into_any_element(),
        None => div()
            .relative()
            .left(px(-1.))
            .size(px(20.))
            .flex_shrink_0()
            .rounded(px(5.))
            .bg(content(0.08))
            .flex()
            .items_center()
            .justify_center()
            .child(project_icon(appearance, project, 10.))
            .into_any_element(),
    }
}
pub(crate) fn button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
) -> gpui::Stateful<gpui::Div> {
    let label = label.into();
    let selector = label.clone();
    div()
        .id(id)
        .debug_selector(move || format!("project-settings-{selector}"))
        .role(Role::Button)
        .aria_label(label.clone())
        .h(px(32.))
        .px(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .gap_1p5()
        .rounded_md()
        .border_1()
        .border_color(content(0.1))
        .text_size(px(12.))
        .line_height(px(18.))
        .text_color(content(0.7))
        .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
        .child(label)
}

pub(crate) struct AppearanceControls {
    project: String,
    appearance: ProjectAppearance,
    name: Entity<Input>,
    custom_color: Entity<Input>,
    custom_open: bool,
    allow_logo: bool,
    busy: bool,
    error: Option<String>,
    _events: Vec<Subscription>,
}
impl EventEmitter<Event> for AppearanceControls {}
impl AppearanceControls {
    pub(crate) fn new(
        project: String,
        appearance: ProjectAppearance,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            let mut field = Input::field("Project name", "Project name", content(0.35), cx);
            field.set_text(appearance.label(&project), cx);
            field.select_range(0..field.content.len(), cx);
            field
        });
        let custom_color = cx.new(|cx| {
            let mut field = Input::field("Custom project color", "#RRGGBB", content(0.35), cx);
            field.set_text(format!("#{:06x}", appearance.color(&project)), cx);
            field
        });
        let events = vec![
            cx.subscribe(&name, |this, _, _: &input::Changed, cx| {
                this.appearance.name = this.name.read(cx).content.to_string();
                this.save(cx);
            }),
            cx.subscribe(&name, |_, _, _: &input::Submitted, cx| {
                cx.emit(Event::Close)
            }),
            cx.subscribe(&custom_color, |this, _, _: &input::Changed, cx| {
                let text = this
                    .custom_color
                    .read(cx)
                    .content
                    .trim()
                    .trim_start_matches('#')
                    .to_owned();
                if text.len() == 6
                    && let Ok(color) = u32::from_str_radix(&text, 16)
                {
                    this.appearance.color = Some(color);
                    this.save(cx);
                }
            }),
        ];
        Self {
            project,
            appearance,
            name,
            custom_color,
            custom_open: false,
            allow_logo: true,
            busy: false,
            error: None,
            _events: events,
        }
    }
    pub(crate) fn for_project_group(mut self) -> Self {
        self.allow_logo = false;
        self
    }
    pub(crate) fn focus(&self, cx: &App) -> FocusHandle {
        self.name.focus_handle(cx)
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        self.appearance.normalize();
        cx.emit(Event::Changed(self.appearance.clone()));
        cx.notify();
    }
    fn choose_logo(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose project logo".into()),
        });
        let project = self.project.clone();
        cx.spawn(async move |this, cx| {
            let result = match picker.await {
                Ok(Ok(Some(paths))) if !paths.is_empty() => {
                    let source = paths[0].clone();
                    cx.background_executor()
                        .spawn(async move {
                            import_image(
                                &source,
                                &crate::data_dir().join("project-images"),
                                &project,
                                true,
                            )
                            .map(Some)
                        })
                        .await
                }
                Ok(Ok(_)) => Ok(None),
                _ => Err("Could not open the image picker.".into()),
            };
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(Some(path)) => {
                        this.appearance.logo = Some(path);
                        this.save(cx);
                    }
                    Ok(None) => {}
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
impl Render for AppearanceControls {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.appearance.color(&self.project);
        let shown = self.appearance.mascot(&self.project);
        div()
            .id("project-appearance-controls")
            .debug_selector(|| "project-appearance-controls".into())
            .role(Role::Group)
            .aria_label("Project appearance")
            .flex()
            .flex_col()
            .gap_2()
            .pb_2()
            .child(
                div()
                    .id("project-appearance-name")
                    .debug_selector(|| "project-appearance-name".into())
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .px_2p5()
                    .rounded_lg()
                    .border_1()
                    .border_color(content(0.1))
                    .bg(content(0.05))
                    .child(self.name.clone()),
            )
            .when(self.allow_logo, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px(px(2.))
                        .child(
                            div()
                                .id("choose-project-logo")
                                .debug_selector(|| "choose-project-logo".into())
                                .role(Role::Button)
                                .aria_label("Choose project logo")
                                .size(px(36.))
                                .flex_shrink_0()
                                .rounded_lg()
                                .border_1()
                                .border_color(content(0.1))
                                .bg(content(0.05))
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(content(0.1)))
                                .child(project_icon(&self.appearance, &self.project, 20.))
                                .on_click(cx.listener(|this, _, _, cx| this.choose_logo(cx))),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(11.))
                                .line_height(px(16.5))
                                .text_color(content(0.5))
                                .child("Project logo")
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.))
                                        .line_height(px(18.))
                                        .text_color(content(0.7))
                                        .child(if self.busy {
                                            "Saving image…"
                                        } else {
                                            "Shown in tabs and composer"
                                        }),
                                ),
                        )
                        .when(self.appearance.logo.is_some(), |d| {
                            d.child(
                                icon_button("remove-project-logo", "Remove project logo", "trash")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.appearance.logo = None;
                                        this.save(cx);
                                    })),
                            )
                        }),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .px(px(2.))
                    .children(COLORS.into_iter().enumerate().map(|(ix, color)| {
                        div()
                            .id(("project-color", ix))
                            .debug_selector(move || format!("project-color-{ix}"))
                            .role(Role::Button)
                            .aria_label(format!("Project color #{color:06x}"))
                            .aria_selected(current == color)
                            .size(px(20.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .when(current == color, |d| {
                                d.border_2().border_color(content(0.7))
                            })
                            .child(div().size(px(14.)).rounded_full().bg(rgb(color)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.appearance.color = Some(color);
                                this.custom_open = false;
                                this.save(cx);
                            }))
                    }))
                    .child(
                        div()
                            .id("custom-project-color")
                            .role(Role::Button)
                            .aria_label("Custom project color")
                            .size(px(20.))
                            .rounded_full()
                            .border_2()
                            .border_color(content(0.4))
                            .bg(rgb(current))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.custom_open = !this.custom_open;
                                if this.custom_open {
                                    let color =
                                        format!("#{:06x}", this.appearance.color(&this.project));
                                    this.custom_color.update(cx, |input, cx| {
                                        input.set_text(color, cx);
                                        input.select_range(0..input.content.len(), cx);
                                    });
                                    window.focus(&this.custom_color.focus_handle(cx), cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .when(self.custom_open, |d| {
                d.child(
                    div()
                        .h(px(30.))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded_md()
                        .border_1()
                        .border_color(content(0.1))
                        .child(self.custom_color.clone()),
                )
            })
            .child(
                div()
                    .px(px(2.))
                    .text_size(px(11.))
                    .line_height(px(16.5))
                    .text_color(content(0.5))
                    .child("Mascot")
                    .child(
                        div()
                            .mt_1()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_1()
                            .children(MASCOTS.into_iter().enumerate().map(|(ix, name)| {
                                div()
                                    .id(("project-mascot", ix))
                                    .debug_selector(move || format!("project-mascot-{ix}"))
                                    .role(Role::Button)
                                    .aria_label(format!("Mascot {name}"))
                                    .aria_selected(ix == shown)
                                    .size(px(20.))
                                    .rounded_md()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(ix == shown, |d| {
                                        d.bg(content(0.1)).border_1().border_color(content(0.3))
                                    })
                                    .hover(|s| s.bg(content(0.08)))
                                    .child(mascot(ix, 12., content(0.75)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.appearance.mascot = Some(name.into());
                                        this.save(cx);
                                    }))
                            })),
                    ),
            )
            .when_some(self.error.clone(), |d, error| {
                d.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(0xff6467))
                        .child(error),
                )
            })
    }
}

pub(crate) struct BackgroundDialog {
    project: String,
    appearance: ProjectAppearance,
    background: Background,
    preview: Entity<ProjectBackground>,
    focus: FocusHandle,
    sliders: [FocusHandle; 2],
    dragging: Option<bool>,
    busy: bool,
    error: Option<String>,
    bounds: AnchorSlot,
    effect_anchor: AnchorSlot,
    effect_region: AnchorSlot,
    effect_open: bool,
    effect_active: usize,
    effect_focus: FocusHandle,
    effect_trigger_focus: FocusHandle,
    _preview_events: Subscription,
}
impl EventEmitter<Event> for BackgroundDialog {}
impl Focusable for BackgroundDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl BackgroundDialog {
    pub(crate) fn new(
        project: String,
        appearance: ProjectAppearance,
        cx: &mut Context<Self>,
    ) -> Self {
        let background = appearance.background.clone().unwrap_or_default();
        let preview = cx.new(|_| ProjectBackground::preview());
        let preview_events = cx.observe(&preview, |_, _, cx| cx.notify());
        Self {
            project,
            appearance,
            background,
            preview,
            _preview_events: preview_events,
            bounds: AnchorSlot::default(),
            effect_anchor: AnchorSlot::default(),
            effect_region: AnchorSlot::default(),
            effect_open: false,
            effect_active: 0,
            effect_focus: cx.focus_handle(),
            effect_trigger_focus: cx.focus_handle(),
            focus: cx.focus_handle(),
            sliders: [cx.focus_handle(), cx.focus_handle()],
            dragging: None,
            busy: false,
            error: None,
        }
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        self.background.normalize();
        self.appearance.background =
            (!self.background.path.is_empty()).then(|| self.background.clone());
        cx.emit(Event::Changed(self.appearance.clone()));
        cx.notify();
    }
    fn choose(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose background image".into()),
        });
        let project = self.project.clone();
        cx.spawn(async move |this, cx| {
            let result = match picker.await {
                Ok(Ok(Some(paths))) if !paths.is_empty() => {
                    let source = paths[0].clone();
                    cx.background_executor()
                        .spawn(async move {
                            import_image(
                                &source,
                                &crate::data_dir().join("project-images"),
                                &project,
                                false,
                            )
                            .map(Some)
                        })
                        .await
                }
                Ok(Ok(_)) => Ok(None),
                _ => Err("Could not open the image picker.".into()),
            };
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(Some(path)) => {
                        this.background.path = path;
                        this.save(cx);
                    }
                    Ok(None) => {}
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn set_opacity(&mut self, empty: bool, value: f32, cx: &mut Context<Self>) {
        let value = (value * 100.).round() / 100.;
        if empty {
            self.background.empty_opacity = value;
        } else {
            self.background.session_opacity = value;
        }
        self.save(cx);
    }
    pub(crate) fn measured_height(&self) -> Option<f32> {
        self.bounds.get().map(|bounds| bounds.size.height.as_f32())
    }
    pub(crate) fn popup_region(&self) -> AnchorSlot {
        self.effect_region.clone()
    }
    fn close_effect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.effect_open = false;
        window.focus(&self.effect_trigger_focus, cx);
        cx.notify();
    }
    fn toggle_effect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.effect_open {
            self.close_effect(window, cx);
            return;
        }
        self.effect_open = true;
        self.effect_active = Effect::ALL
            .iter()
            .position(|effect| *effect == self.background.effect)
            .unwrap_or(0);
        window.focus(&self.effect_focus, cx);
        cx.notify();
    }
    fn choose_effect(&mut self, effect: Effect, window: &mut Window, cx: &mut Context<Self>) {
        self.background.effect = effect;
        self.close_effect(window, cx);
        self.save(cx);
    }
    fn effect_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.effect_open {
            return None;
        }
        let weak = cx.weak_entity();
        Some(
            popover(
                "project-background-effects",
                Anchor::Slot(self.effect_anchor.clone()),
            )
            .width(144.)
            .gap(4.)
            .align(Align::End)
            .padding(4.)
            .priority(70)
            .track(self.effect_region.clone())
            .on_dismiss(move |window, cx| {
                let _ = weak.update(cx, |this, cx| this.close_effect(window, cx));
            })
            .child(
                div()
                    .id("project-background-effect-options")
                    .role(Role::ListBox)
                    .aria_label("Project background effect options")
                    .track_focus(&self.effect_focus)
                    .flex()
                    .flex_col()
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        match event.keystroke.key.as_str() {
                            "up" => this.effect_active = this.effect_active.saturating_sub(1),
                            "down" => {
                                this.effect_active =
                                    (this.effect_active + 1).min(Effect::ALL.len() - 1)
                            }
                            "enter" | "space" => {
                                this.choose_effect(Effect::ALL[this.effect_active], window, cx)
                            }
                            "escape" => this.close_effect(window, cx),
                            _ => return,
                        }
                        window.prevent_default();
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .children(Effect::ALL.into_iter().enumerate().map(|(ix, effect)| {
                        div()
                            .id(("background-effect-option", ix))
                            .debug_selector(move || format!("project-settings-{}", effect.label()))
                            .role(Role::ListBoxOption)
                            .aria_label(effect.label())
                            .aria_selected(self.background.effect == effect)
                            .h(px(32.))
                            .flex_shrink_0()
                            .px_2()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(13.))
                            .line_height(px(13.))
                            .text_color(content(if self.effect_active == ix { 1. } else { 0.75 }))
                            .when(self.effect_active == ix, |d| d.bg(content(0.1)))
                            .hover(|s| s.bg(content(0.05)).text_color(content(1.)))
                            .child(
                                div()
                                    .size(px(14.))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(self.background.effect == effect, |d| {
                                        d.child(icon("check", 12.).text_color(content(1.)))
                                    }),
                            )
                            .child(effect.label())
                            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                                if *hovering {
                                    this.effect_active = ix;
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose_effect(effect, window, cx)
                            }))
                    })),
            )
            .into_any_element(),
        )
    }
    fn slider(&self, empty: bool, cx: &mut Context<Self>) -> AnyElement {
        let value = if empty {
            self.background.empty_opacity
        } else {
            self.background.session_opacity
        };
        let fraction = (value - 0.05) / 0.60;
        let bounds = AnchorSlot::default();
        let movement = bounds.clone();
        let probe = bounds.clone();
        let focus = self.sliders[usize::from(!empty)].clone();
        let label = if empty {
            "Empty chat visibility"
        } else {
            "Session visibility"
        };
        div()
            .debug_selector(move || {
                if empty {
                    "background-empty-row".into()
                } else {
                    "background-session-row".into()
                }
            })
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .pt_4()
            .border_t_1()
            .border_color(content(0.07))
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(19.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(label),
            )
            .child(
                div()
                    .w(px(224.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id(if empty {
                                "empty-background-opacity"
                            } else {
                                "session-background-opacity"
                            })
                            .debug_selector(move || {
                                if empty {
                                    "empty-background-opacity".into()
                                } else {
                                    "session-background-opacity".into()
                                }
                            })
                            .role(Role::Slider)
                            .aria_label(label)
                            .aria_value(format!("{}%", (value * 100.).round()))
                            .track_focus(&focus)
                            .tab_index(0)
                            .relative()
                            .w(px(172.))
                            .h(px(12.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .child(probe.probe())
                            .child(div().w_full().h(px(4.)).rounded_full().bg(content(0.15)))
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left(px(fraction * 160.))
                                    .size(px(12.))
                                    .rounded_full()
                                    .bg(content(1.)),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    window.focus(&this.sliders[usize::from(!empty)], cx);
                                    this.dragging = Some(empty);
                                    if let Some(bounds) = bounds.get() {
                                        this.set_opacity(
                                            empty,
                                            0.05 + ((event.position.x - bounds.left()).as_f32()
                                                - 6.)
                                                / (bounds.size.width.as_f32() - 12.)
                                                * 0.60,
                                            cx,
                                        );
                                    }
                                    cx.stop_propagation();
                                }),
                            )
                            .on_mouse_move(cx.listener(
                                move |this, event: &MouseMoveEvent, _, cx| {
                                    if this.dragging == Some(empty)
                                        && event.pressed_button == Some(MouseButton::Left)
                                        && let Some(bounds) = movement.get()
                                    {
                                        this.set_opacity(
                                            empty,
                                            0.05 + ((event.position.x - bounds.left()).as_f32()
                                                - 6.)
                                                / (bounds.size.width.as_f32() - 12.)
                                                * 0.60,
                                            cx,
                                        );
                                    }
                                },
                            ))
                            .on_key_down(cx.listener(
                                move |this, event: &gpui::KeyDownEvent, _, cx| {
                                    let old = if empty {
                                        this.background.empty_opacity
                                    } else {
                                        this.background.session_opacity
                                    };
                                    let next = match event.keystroke.key.as_str() {
                                        "left" | "down" => old - 0.01,
                                        "right" | "up" => old + 0.01,
                                        "home" => 0.05,
                                        "end" => 0.65,
                                        _ => return,
                                    };
                                    this.set_opacity(empty, next, cx);
                                    cx.stop_propagation();
                                },
                            )),
                    )
                    .child(
                        div()
                            .w(px(40.))
                            .text_right()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .font_features(crate::right_panel::tabular())
                            .child(format!("{}%", (value * 100.).round())),
                    ),
            )
            .into_any_element()
    }
    fn effect_row(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .debug_selector(|| "background-effect-row".into())
            .flex_shrink_0()
            .pt_4()
            .border_t_1()
            .border_color(content(0.07))
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(13.))
                            // React's inline label sits in a 16px / 24px line box.
                            .line_height(px(24.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child("Background effect"),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.))
                            .line_height(px(16.5))
                            .text_color(content(0.45))
                            .child(self.background.effect.description()),
                    ),
            )
            .child(
                div()
                    .id("project-background-effect")
                    .debug_selector(|| "project-background-effect".into())
                    .role(Role::ComboBox)
                    .aria_label("Project background effect")
                    .aria_value(self.background.effect.label())
                    .aria_expanded(self.effect_open)
                    .track_focus(&self.effect_trigger_focus)
                    .tab_index(0)
                    .w(px(144.))
                    .h(px(36.))
                    .flex_shrink_0()
                    .px(px(10.))
                    .rounded_md()
                    .border_1()
                    .border_color(content(0.1))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .text_size(px(13.))
                    .line_height(px(19.5))
                    .hover(|s| s.border_color(content(0.2)))
                    .child(self.effect_anchor.probe())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(self.background.effect.label()),
                    )
                    .child(icon("chevron-down", 14.).text_color(content(0.45)))
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_effect(window, cx)))
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space" | "down") {
                            this.toggle_effect(window, cx);
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                    })),
            )
            .into_any_element()
    }
    fn scope_row(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .debug_selector(|| "background-scope-row".into())
            .flex_shrink_0()
            .pt_4()
            .border_t_1()
            .border_color(content(0.07))
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(19.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child("Show on"),
            )
            .child(
                div()
                    .id("background-scope-control")
                    .debug_selector(|| "background-scope-control".into())
                    .w(px(176.))
                    .h(px(32.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .p(px(2.))
                    .rounded_md()
                    .border_1()
                    .border_color(content(0.1))
                    .role(Role::RadioGroup)
                    .aria_label("Show project background on")
                    .children(
                        [(Scope::Empty, "Empty only"), (Scope::All, "All sessions")]
                            .into_iter()
                            .enumerate()
                            .map(|(ix, (scope, label))| {
                                div()
                                    .id(("background-scope", ix))
                                    .debug_selector(move || format!("project-settings-{label}"))
                                    .role(Role::RadioButton)
                                    .aria_label(label)
                                    .aria_selected(self.background.scope == scope)
                                    .h_full()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .px(px(6.))
                                    .rounded(px(5.))
                                    .text_size(px(12.))
                                    .line_height(px(18.))
                                    .text_color(content(if self.background.scope == scope {
                                        1.
                                    } else {
                                        0.5
                                    }))
                                    .when(self.background.scope == scope, |d| d.bg(content(0.1)))
                                    .hover(|s| s.text_color(content(1.)))
                                    .child(label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.background.scope = scope;
                                        this.save(cx);
                                    }))
                            }),
                    ),
            )
            .into_any_element()
    }
}
impl Render for BackgroundDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_image = !self.background.path.is_empty();
        let settings = has_image.then(|| self.background.clone());
        self.preview.update(cx, |preview, cx| {
            preview.configure(settings, true, None, cx)
        });
        let error = self
            .error
            .clone()
            .or_else(|| self.preview.read(cx).error.clone());
        div().id("project-background-dialog").debug_selector(||"project-background-dialog".into())
            .role(Role::Dialog).aria_label("Background Image").track_focus(&self.focus)
            .relative().min_h_0().max_h(px(window.viewport_size().height.as_f32()-34.))
            .flex().flex_col().text_color(content(1.))
            .child(self.bounds.layout_probe())
            .on_key_down(cx.listener(|this,event:&gpui::KeyDownEvent,window,cx| {
                if event.keystroke.key=="escape" {
                    if this.effect_open {this.close_effect(window,cx);} else {cx.emit(Event::Close);}
                    window.prevent_default();cx.stop_propagation();
                }
            }))
            .on_mouse_up(MouseButton::Left,cx.listener(|this,_,_,_|this.dragging=None))
            .child(div().debug_selector(||"background-dialog-header".into()).flex_shrink_0().flex().items_start().gap_2().px_4().pt_3()
                .child(div().min_w_0().flex_1().pt(px(2.)).flex().flex_col()
                    .child(div().debug_selector(||"background-dialog-title".into()).text_size(px(16.)).line_height(px(20.)).font_weight(gpui::FontWeight::SEMIBOLD).child("Background Image"))
                    .child(div().mt(px(2.)).min_w_0().truncate().text_size(px(12.)).line_height(px(16.5)).text_color(content(0.5)).child(format!("Choose a background image for {}",self.appearance.label(&self.project)))))
                .child(icon_button("close-project-background","Close background settings","close").size(px(28.)).on_click(cx.listener(|_,_,_,cx|cx.emit(Event::Close)))))
            .child(div().id("background-dialog-body").debug_selector(||"background-dialog-body".into())
                .min_h_0().overflow_y_scroll().flex().flex_col().gap_5().p_4()
                .child(div().flex_shrink_0().flex().flex_col()
                    .child(div().debug_selector(||"background-dialog-preview".into()).h(px(162.)).flex_shrink_0().overflow_hidden().rounded(px(12.)).border_1().border_color(content(0.1))
                        .bg(if self.background.effect==Effect::GradientBlur {rgb(0x171717).into()} else {content(0.05)})
                        .child(div().relative().h(px(160.)).child(self.preview.clone()).when(!has_image,|d|d.flex().items_center().justify_center().child(div().text_size(px(12.)).text_color(content(0.4)).child("No background selected")))))
                    .child(button("choose-project-background",if self.busy{"Saving image…"}else if has_image{"Change image"}else{"Choose image"})
                        .mt_2().flex_shrink_0().on_click(cx.listener(|this,_,_,cx|this.choose(cx))))
                    .child(div().mt(px(6.)).text_size(px(11.)).line_height(px(17.875)).text_color(content(0.45))
                        .child(if has_image{"This image overrides the global background for this project."}else{"No background selected for this project."}))
                    .when_some(error,|d,error|d.child(div().mt(px(6.)).text_size(px(12.)).line_height(px(18.)).text_color(rgb(0xff6467)).child(error))))
                .when(has_image,|d|d.child(self.effect_row(cx)))
                .child(self.scope_row(cx))
                .child(self.slider(true,cx)).child(self.slider(false,cx))
                .when(has_image,|d|d.child(button("remove-project-background","Remove background image").flex_shrink_0().text_color(rgb(0xff6467))
                    .on_click(cx.listener(|this,_,_,cx|{this.background=Background::default();this.effect_open=false;this.error=None;this.save(cx);}))))
            )
            .children(self.effect_popup(cx))
    }
}
