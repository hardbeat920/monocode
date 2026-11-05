//! The shelf: every simulator and emulator as a small phone, grouped by
//! platform, with a tile for making a new one. A ⋯ menu (or a right click)
//! starts, shuts down or deletes a device.
use super::*;
use crate::{
    input::{self, Input},
    modal,
    popover::{self, Anchor, AnchorSlot},
};
use gpui::{
    Animation, AnimationExt, Entity, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point,
    PromptLevel, Role, Stateful, Subscription, linear_color_stop, linear_gradient, rgb,
};
use monocode_core::device::hub::NewDeviceOptions;
use std::time::Duration;

const PHONE_WIDTH: f32 = 34.;
const PHONE_HEIGHT: f32 = 72.;
const SHEET_IN: Duration = Duration::from_millis(220);

/// The New device sheet.
pub(super) struct NewDevice {
    platform: Platform,
    runtime: usize,
    model: usize,
    name: Entity<Input>,
    /// The name filled in for the model, followed until the person edits it.
    suggested: String,
    open: Option<Choice>,
    model_anchor: AnchorSlot,
    runtime_anchor: AnchorSlot,
    _subscriptions: [Subscription; 2],
}

#[derive(Clone, Copy, PartialEq)]
enum Choice {
    Model,
    Runtime,
}

/// A device's ⋯ menu, at the pointer for a right click.
pub(super) struct TileMenu {
    device: Device,
    at: Option<Point<Pixels>>,
}

fn noun(platform: Platform) -> &'static str {
    match platform {
        Platform::Ios => "simulator",
        Platform::Android => "emulator",
    }
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

/// A phone's outline; lit when it is on.
fn phone(platform: Platform, on: bool) -> gpui::Div {
    let radius = if platform == Platform::Ios { 9. } else { 6. };
    div()
        .relative()
        .flex_shrink_0()
        .w(px(PHONE_WIDTH))
        .h(px(PHONE_HEIGHT))
        .rounded(px(radius))
        .border_1()
        .border_color(content(if on { 0.2 } else { 0.14 }))
        .map(|d| {
            if on {
                d.bg(linear_gradient(
                    160.,
                    linear_color_stop(rgb(0x4b358a), 0.),
                    linear_color_stop(rgb(0x14806e), 1.),
                ))
            } else {
                d.bg(content(0.045))
            }
        })
        .child(
            div()
                .absolute()
                .top(px(4.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .rounded_full()
                        .bg(black(if on { 0.7 } else { 0.25 }))
                        .map(|d| match platform {
                            Platform::Ios => d.w(px(10.)).h(px(3.)),
                            Platform::Android => d.size(px(3.)),
                        }),
                ),
        )
}

fn menu_row(
    id: &'static str,
    path: &'static str,
    label: &'static str,
    danger: bool,
) -> Stateful<gpui::Div> {
    let color = if danger {
        modal::danger(1.)
    } else {
        content(0.85)
    };
    div()
        .id(id)
        .role(Role::MenuItem)
        .aria_label(label)
        .h(px(28.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(6.))
        .text_size(px(12.5))
        .text_color(color)
        .cursor_pointer()
        .hover(move |s| {
            s.bg(if danger {
                modal::danger(0.1)
            } else {
                content(0.06)
            })
        })
        .child(icon(path, 14.).text_color(color))
        .child(label)
}

fn caption(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(11.5))
        .line_height(px(16.))
        .text_color(content(0.5))
        .child(text.into())
}

impl DevicePanel {
    pub(super) fn load_options(&mut self, cx: &mut Context<Self>) {
        if self.loading_options || !device::enabled() {
            return;
        }
        self.loading_options = true;
        let work = cx
            .background_executor()
            .spawn(async { hub::new_device_options() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.loading_options = false;
                this.options = Some(result.unwrap_or_else(|error| NewDeviceOptions {
                    errors: vec![error],
                    ..Default::default()
                }));
                this.suggest_name(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn open_new(&mut self, platform: Platform, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| {
            let mut input = Input::field("Device name", "Name", content(0.35), cx);
            input.set_max_chars(64);
            input
        });
        let subscriptions = [
            cx.subscribe(&name, |_, _, _: &input::Changed, cx| cx.notify()),
            cx.subscribe(&name, |this, _, _: &input::Submitted, cx| this.create(cx)),
        ];
        window.focus(&name.focus_handle(cx), cx);
        self.menu = None;
        self.sheet = Some(NewDevice {
            platform,
            runtime: 0,
            model: 0,
            name,
            suggested: String::new(),
            open: None,
            model_anchor: AnchorSlot::default(),
            runtime_anchor: AnchorSlot::default(),
            _subscriptions: subscriptions,
        });
        if self.options.is_none() {
            self.load_options(cx);
        }
        self.suggest_name(cx);
        cx.notify();
    }

    fn close_new(&mut self, cx: &mut Context<Self>) {
        self.sheet = None;
        cx.notify();
    }

    /// Fills the name in from the model, unless the person has typed one.
    fn suggest_name(&mut self, cx: &mut Context<Self>) {
        let Some(sheet) = self.sheet.as_mut() else {
            return;
        };
        let Some(model) = self
            .options
            .as_ref()
            .and_then(|o| o.runtimes(sheet.platform).get(sheet.runtime))
            .and_then(|r| r.models.get(sheet.model))
        else {
            return;
        };
        let current = sheet.name.read(cx).content.to_string();
        if !current.is_empty() && current != sheet.suggested {
            return;
        }
        let mut taken = self.list.clone().unwrap_or_default();
        taken
            .devices
            .extend(self.creating.iter().map(|(platform, name)| Device {
                id: name.clone(),
                name: name.clone(),
                version: String::new(),
                platform: *platform,
                booted: false,
                physical: false,
                supported: true,
            }));
        let suggested = hub::unique_name(sheet.platform, &model.name, &taken);
        sheet.suggested = suggested.clone();
        sheet
            .name
            .update(cx, |input, cx| input.set_text(suggested, cx));
    }

    fn choose(&mut self, choice: Choice, ix: usize, cx: &mut Context<Self>) {
        let Some(sheet) = self.sheet.as_mut() else {
            return;
        };
        match choice {
            Choice::Model => sheet.model = ix,
            Choice::Runtime => {
                sheet.runtime = ix;
                sheet.model = 0;
            }
        }
        sheet.open = None;
        self.suggest_name(cx);
        cx.notify();
    }

    /// What the sheet would create: the runtime, model id and final name.
    fn new_device_choice(
        &self,
        cx: &gpui::App,
    ) -> Option<(Platform, hub::RuntimeOption, String, String)> {
        let sheet = self.sheet.as_ref()?;
        let runtime = self
            .options
            .as_ref()?
            .runtimes(sheet.platform)
            .get(sheet.runtime)?;
        let model = runtime.models.get(sheet.model)?;
        let name = hub::device_name(sheet.platform, &sheet.name.read(cx).content);
        (!name.is_empty()).then(|| (sheet.platform, runtime.clone(), model.id.clone(), name))
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let Some((platform, runtime, model, name)) = self.new_device_choice(cx) else {
            return;
        };
        self.sheet = None;
        self.notice = None;
        self.picking = true;
        self.creating.push((platform, name.clone()));
        cx.notify();
        let work = cx.background_executor().spawn({
            let name = name.clone();
            async move { hub::create(platform, &name, &runtime, &model) }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.creating
                    .retain(|(p, n)| !(*p == platform && *n == name));
                match result {
                    Ok(device) => {
                        this.show(device, cx);
                        this.refresh(cx);
                    }
                    Err(error) => {
                        this.notice = Some(if error.starts_with("The emulator was made") {
                            format!("{name}: {error}")
                        } else {
                            format!("Could not create {name}. {error}")
                        });
                        // A device can be made yet fail to start; list it.
                        this.refresh(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn confirm_delete(&mut self, device: Device, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Delete {}?", device.name),
            Some("Its apps and data are erased. This can't be undone."),
            &["Delete", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| this.delete(device, cx));
        })
        .detach();
    }

    fn delete(&mut self, device: Device, cx: &mut Context<Self>) {
        let shown = self.selected.as_ref().is_some_and(|d| {
            d.id == device.id || (d.platform == device.platform && d.name == device.name)
        });
        if shown {
            self.disconnect();
            #[cfg(target_os = "macos")]
            if let Some(screen) = &self.screen {
                screen.clear();
            }
            self.selected = None;
            self.picking = true;
        }
        self.deleting.push(device.id.clone());
        self.notice = None;
        cx.notify();
        let work = cx.background_executor().spawn({
            let device = device.clone();
            async move { hub::remove(&device) }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.deleting.retain(|id| *id != device.id);
                match result {
                    Ok(()) => {
                        if let Some(list) = this.list.as_mut() {
                            list.devices.retain(|d| d.id != device.id);
                        }
                    }
                    Err(error) => {
                        this.notice = Some(format!("Could not delete {}. {error}", device.name))
                    }
                }
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn shut_down_device(&mut self, device: Device, cx: &mut Context<Self>) {
        self.menu = None;
        if self.selected.as_ref().is_some_and(|d| d.id == device.id) {
            self.shutdown(cx);
            return;
        }
        let work = cx.background_executor().spawn({
            let device = device.clone();
            async move { hub::shutdown(&device) }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.notice = Some(format!("Could not shut down {}. {error}", device.name));
                }
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn shelf(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(list) = self.list.clone() else {
            return self.message("Looking for devices…", None, None, cx);
        };
        if self.options.is_none() {
            self.load_options(cx);
        }
        let empty = list.devices.is_empty() && self.creating.is_empty();
        let body = div()
            .id("device-shelf")
            .debug_selector(|| "device-shelf".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb_3()
            .flex()
            .flex_col()
            .children(
                self.notice
                    .clone()
                    .map(|notice| self.notice_row(notice, cx)),
            )
            .when(empty, |d| {
                d.child(div().px_4().pt_3().child(caption(
                    "No simulators or emulators yet. Make one to see and use it here.",
                )))
            })
            .children(self.phones(&list, cx))
            .child(self.section(Platform::Ios, &list, cx))
            .child(self.section(Platform::Android, &list, cx))
            .children(
                list.errors
                    .iter()
                    .map(|error| div().px_4().pt_2().child(caption(error.clone()))),
            );
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(body)
            .children(self.menu_popover(cx))
            .children(self.sheet_overlay(cx))
            .into_any_element()
    }

    fn notice_row(&self, notice: String, cx: &mut Context<Self>) -> AnyElement {
        div()
            .mx_3()
            .mt_3()
            .p(px(10.))
            .rounded(px(8.))
            .bg(modal::danger(0.08))
            .flex()
            .items_start()
            .gap_2()
            .child(
                icon("alert-circle", 14.)
                    .text_color(modal::danger(1.))
                    .mt(px(1.)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .text_color(content(0.8))
                    .child(notice),
            )
            .child(
                icon_action("device-notice-close", "Dismiss", "x", true).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    },
                )),
            )
            .into_any_element()
    }

    fn section(&self, platform: Platform, list: &DeviceList, cx: &mut Context<Self>) -> AnyElement {
        let devices: Vec<(usize, &Device)> = list
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| d.platform == platform && !d.physical)
            .collect();
        let creating: Vec<&String> = self
            .creating
            .iter()
            .filter(|(p, _)| *p == platform)
            .map(|(_, name)| name)
            .collect();
        let label = match platform {
            Platform::Ios => "iOS Simulators",
            Platform::Android => "Android Emulators",
        };
        div()
            .pt_2()
            .flex()
            .flex_col()
            .child(section_label(label).px_4().pt_2())
            .child(
                div()
                    .px_2()
                    .grid()
                    .grid_cols(3)
                    .gap(px(2.))
                    .children(
                        devices
                            .into_iter()
                            .map(|(ix, device)| self.tile(ix, device, cx)),
                    )
                    .children(
                        creating
                            .into_iter()
                            .enumerate()
                            .map(|(ix, name)| self.creating_tile(platform, ix, name.clone(), cx)),
                    )
                    .child(self.new_tile(platform, cx)),
            )
            .into_any_element()
    }

    /// Android phones on USB, while any are plugged in. They have no ⋯ menu:
    /// there is nothing to start, shut down or delete.
    fn phones(&self, list: &DeviceList, cx: &mut Context<Self>) -> Option<AnyElement> {
        let phones: Vec<(usize, &Device)> = list
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| d.physical)
            .collect();
        if phones.is_empty() {
            return None;
        }
        Some(
            div()
                .pt_2()
                .flex()
                .flex_col()
                .child(section_label("Connected Phones").px_4().pt_2())
                .child(
                    div().px_2().grid().grid_cols(3).gap(px(2.)).children(
                        phones
                            .into_iter()
                            .map(|(ix, device)| self.tile(ix, device, cx)),
                    ),
                )
                .into_any_element(),
        )
    }

    fn tile_frame(id: impl Into<gpui::ElementId>) -> Stateful<gpui::Div> {
        div()
            .id(id)
            .group("device-tile")
            .relative()
            .min_w_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .pt(px(12.))
            .pb(px(9.))
            .px_1()
            .rounded(px(10.))
    }

    fn tile_text(name: SharedString, detail: SharedString, dim: bool) -> [gpui::Div; 2] {
        [
            div()
                .max_w_full()
                .truncate()
                .text_size(px(12.))
                .line_height(px(15.))
                .text_color(content(if dim { 0.55 } else { 0.88 }))
                .child(name),
            div()
                .max_w_full()
                .truncate()
                .text_size(px(11.))
                .line_height(px(14.))
                .text_color(content(0.45))
                .child(detail),
        ]
    }

    fn tile(&self, ix: usize, device: &Device, cx: &mut Context<Self>) -> AnyElement {
        let shown = !self.picking && self.selected.as_ref().is_some_and(|d| d.id == device.id);
        let deleting = self.deleting.contains(&device.id);
        let menu_open = self.menu.as_ref().is_some_and(|m| m.device.id == device.id);
        let detail: SharedString = if deleting {
            "Deleting…".into()
        } else {
            let version = if device.version.is_empty() {
                device.platform.label()
            } else {
                device.version.as_str()
            };
            if device.physical {
                format!("{version} · USB").into()
            } else {
                version.to_owned().into()
            }
        };
        let entry = device.clone();
        let for_menu = device.clone();
        let for_right = device.clone();
        let more = div()
            .id(("device-tile-more", ix))
            .debug_selector(move || format!("device-tile-more-{ix}"))
            .role(Role::Button)
            .aria_label(SharedString::from(format!("More for {}", device.name)))
            .absolute()
            .top(px(4.))
            .right(px(4.))
            .size(px(22.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .bg(content(0.07))
            .hover(|s| s.bg(content(0.12)))
            .opacity(if menu_open { 1. } else { 0. })
            .group_hover("device-tile", |s| s.opacity(1.))
            .child(icon("more-horizontal", 14.).text_color(content(0.75)))
            .when(menu_open, |d| d.child(self.menu_anchor.probe()))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.menu = Some(TileMenu {
                    device: for_menu.clone(),
                    at: None,
                });
                cx.notify();
            }));
        Self::tile_frame(("device-tile", ix))
            .debug_selector(move || format!("device-tile-{ix}"))
            .role(Role::Button)
            .aria_label(SharedString::from(format!("{}, {}", device.name, detail)))
            .when(shown, |d| d.bg(content(0.07)))
            .when(deleting, |d| d.opacity(0.45))
            .when(!deleting, |d| {
                d.cursor_pointer()
                    .hover(|s| s.bg(content(0.05)))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(entry.clone(), cx)))
            })
            .when(!deleting && !device.physical, |d| {
                d.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.menu = Some(TileMenu {
                            device: for_right.clone(),
                            at: Some(event.position),
                        });
                        cx.notify();
                    }),
                )
                .child(more)
            })
            .child(
                phone(device.platform, device.booted).when(device.booted, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(-3.))
                            .right(px(-3.))
                            .size(px(9.))
                            .rounded_full()
                            .border_2()
                            .border_color(pane_fill())
                            .bg(rgb(crate::right_panel::DIFF_ADD)),
                    )
                }),
            )
            .children(Self::tile_text(
                device.name.clone().into(),
                detail,
                deleting,
            ))
            .into_any_element()
    }

    fn creating_tile(
        &self,
        platform: Platform,
        ix: usize,
        name: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let outline = phone(platform, false);
        let outline = if cx.reduce_motion() {
            outline.opacity(0.6).into_any_element()
        } else {
            outline
                .with_animation(
                    SharedString::from(format!("device-creating-{platform:?}-{ix}")),
                    Animation::new(Duration::from_millis(1400)).repeat(),
                    |d, t| d.opacity(0.35 + 0.45 * (t * std::f32::consts::PI).sin()),
                )
                .into_any_element()
        };
        Self::tile_frame(SharedString::from(format!(
            "device-creating-tile-{platform:?}-{ix}"
        )))
        .aria_label(SharedString::from(format!("Creating {name}")))
        .child(outline)
        .children(Self::tile_text(name.into(), "Creating…".into(), true))
        .into_any_element()
    }

    fn new_tile(&self, platform: Platform, cx: &mut Context<Self>) -> AnyElement {
        let label: SharedString = format!("New {}", noun(platform)).into();
        let id = match platform {
            Platform::Ios => "device-new-ios",
            Platform::Android => "device-new-android",
        };
        Self::tile_frame(id)
            .debug_selector(move || id.into())
            .role(Role::Button)
            .aria_label(label.clone())
            .cursor_pointer()
            .hover(|s| s.bg(content(0.05)))
            .child(
                div()
                    .w(px(PHONE_WIDTH))
                    .h(px(PHONE_HEIGHT))
                    .rounded(px(if platform == Platform::Ios { 9. } else { 6. }))
                    .border_1()
                    .border_dashed()
                    .border_color(content(0.25))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("plus", 15.).text_color(content(0.6))),
            )
            .child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_size(px(12.))
                    .line_height(px(15.))
                    .text_color(content(0.65))
                    .child(label),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.open_new(platform, window, cx)))
            .into_any_element()
    }

    fn menu_popover(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let device = menu.device.clone();
        let anchor = match menu.at {
            Some(at) => Anchor::Point(at),
            None => Anchor::Slot(self.menu_anchor.clone()),
        };
        let this = cx.entity().downgrade();
        let showing = !self.picking && self.selected.as_ref().is_some_and(|d| d.id == device.id);
        let (open_path, open_label) = if device.booted {
            ("smart-phone", "Show")
        } else {
            ("play", "Start")
        };
        let (start, shut, delete) = (device.clone(), device.clone(), device.clone());
        Some(
            popover::popover("device-tile-menu", anchor)
                .width(176.)
                .padding(4.)
                .gap(4.)
                .on_dismiss(move |_, cx| {
                    let _ = this.update(cx, |panel, cx| {
                        panel.menu = None;
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .when(!showing, |d| {
                            d.child(
                                menu_row("device-menu-open", open_path, open_label, false)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.menu = None;
                                        this.select(start.clone(), cx);
                                    })),
                            )
                        })
                        .when(device.booted, |d| {
                            d.child(
                                menu_row("device-menu-shutdown", "power", "Shut down", false)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.shut_down_device(shut.clone(), cx)
                                    })),
                            )
                        })
                        .when(!showing || device.booted, |d| {
                            d.child(div().h(px(1.)).mx(px(6.)).my(px(4.)).bg(content(0.08)))
                        })
                        .child(
                            menu_row("device-menu-delete", "trash", "Delete…", true).on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.confirm_delete(delete.clone(), window, cx)
                                }),
                            ),
                        ),
                )
                .into_any_element(),
        )
    }

    fn sheet_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sheet = self.sheet.as_ref()?;
        let platform = sheet.platform;
        let title = match platform {
            Platform::Ios => "New iOS Simulator",
            Platform::Android => "New Android Emulator",
        };
        let runtimes = self.options.as_ref().map(|o| o.runtimes(platform));
        let runtime = runtimes.and_then(|r| r.get(sheet.runtime));
        let model = runtime.and_then(|r| r.models.get(sheet.model));
        let typed = sheet.name.read(cx).content.to_string();
        let saved_as = hub::device_name(platform, &typed);
        let ready = model.is_some() && !saved_as.is_empty();
        let body: AnyElement = match runtimes {
            None => div()
                .py_6()
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .child(spinner("device-options-spinner", "loader", 16.))
                .child(caption("Looking for what this Mac can run…"))
                .into_any_element(),
            Some([]) => {
                let how = match platform {
                    Platform::Ios => {
                        "No iOS versions are installed. Add one in Xcode › Settings › Components, then try again."
                    }
                    Platform::Android => {
                        "No Android system images are installed. Add one in Android Studio's SDK Manager, then try again."
                    }
                };
                let errors = self
                    .options
                    .as_ref()
                    .map(|o| o.errors.join("\n"))
                    .unwrap_or_default();
                div()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(modal::body(how))
                    .when(!errors.is_empty(), |d| d.child(caption(errors)))
                    .into_any_element()
            }
            Some(_) => div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(self.field(
                    "Model",
                    self.choice_button(
                        "device-new-model",
                        Choice::Model,
                        model.map(|m| m.name.clone()).unwrap_or_default(),
                        &sheet.model_anchor,
                        cx,
                    ),
                    None,
                ))
                .child(self.field(
                    match platform {
                        Platform::Ios => "iOS version",
                        Platform::Android => "System image",
                    },
                    self.choice_button(
                        "device-new-runtime",
                        Choice::Runtime,
                        runtime.map(|r| r.name.clone()).unwrap_or_default(),
                        &sheet.runtime_anchor,
                        cx,
                    ),
                    None,
                ))
                .child(
                    self.field(
                        "Name",
                        div()
                            .id("device-new-name")
                            .h(px(32.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(content(0.12))
                            .bg(content(0.04))
                            .child(sheet.name.clone())
                            .into_any_element(),
                        (platform == Platform::Android
                            && !saved_as.is_empty()
                            && saved_as != typed.trim())
                        .then(|| format!("Saved as {saved_as}")),
                    ),
                )
                .into_any_element(),
        };
        let backdrop = div()
            .id("device-sheet-scrim")
            .absolute()
            .inset_0()
            .occlude()
            .bg(modal::scrim_color())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_new(cx)),
            );
        let card = modal::card()
            .w_full()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                modal::card_body()
                    .p(px(16.))
                    .gap(px(14.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(modal::title(title).flex_1())
                            .child(
                                icon_action("device-new-close", "Close", "x", true)
                                    .on_click(cx.listener(|this, _, _, cx| this.close_new(cx))),
                            ),
                    )
                    .child(body)
                    .child(
                        div()
                            .pt(px(2.))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                modal::ghost_button("device-new-cancel", "Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| this.close_new(cx))),
                            )
                            .child(
                                modal::disabled(
                                    modal::primary_button("device-new-create", "Create and Start"),
                                    !ready,
                                )
                                .when(ready, |b| {
                                    b.on_click(cx.listener(|this, _, _, cx| this.create(cx)))
                                }),
                            ),
                    ),
            );
        let reduced = cx.reduce_motion();
        let sheet_card = div()
            .id("device-sheet")
            .debug_selector(|| "device-sheet".into())
            .role(Role::Dialog)
            .aria_label(title)
            .absolute()
            .left(px(8.))
            .right(px(8.))
            .bottom(px(8.))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    let open = this.sheet.as_ref().is_some_and(|s| s.open.is_some());
                    if open {
                        if let Some(sheet) = this.sheet.as_mut() {
                            sheet.open = None;
                        }
                        cx.notify();
                    } else {
                        this.close_new(cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .child(card);
        let sheet_card = if reduced {
            sheet_card.into_any_element()
        } else {
            sheet_card
                .with_animation(
                    "device-sheet-in",
                    Animation::new(SHEET_IN).with_easing(ease_out),
                    |d, t| d.bottom(px(8. - 16. * (1. - t))).opacity(t),
                )
                .into_any_element()
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .child(backdrop)
                .child(sheet_card)
                .children(self.choice_popover(cx))
                .into_any_element(),
        )
    }

    fn field(&self, label: &'static str, control: AnyElement, note: Option<String>) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(content(0.6))
                    .child(label),
            )
            .child(control)
            .children(note.map(caption))
    }

    fn choice_button(
        &self,
        id: &'static str,
        choice: Choice,
        value: String,
        anchor: &AnchorSlot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.sheet.as_ref().is_some_and(|s| s.open == Some(choice));
        div()
            .id(id)
            .debug_selector(move || id.into())
            .role(Role::ComboBox)
            .aria_label(SharedString::from(value.clone()))
            .h(px(32.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(8.))
            .border_1()
            .border_color(content(if open { 0.25 } else { 0.12 }))
            .bg(content(if open { 0.07 } else { 0.04 }))
            .cursor_pointer()
            .hover(|s| s.bg(content(0.07)))
            .child(anchor.probe())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .text_color(content(0.9))
                    .child(value),
            )
            .child(icon("chevrons-up-down", 13.))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(sheet) = this.sheet.as_mut() {
                    sheet.open = if sheet.open == Some(choice) {
                        None
                    } else {
                        Some(choice)
                    };
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn choice_popover(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sheet = self.sheet.as_ref()?;
        let choice = sheet.open?;
        let runtimes = self.options.as_ref()?.runtimes(sheet.platform);
        let (anchor, current, items): (_, usize, Vec<String>) = match choice {
            Choice::Model => (
                sheet.model_anchor.clone(),
                sheet.model,
                runtimes
                    .get(sheet.runtime)?
                    .models
                    .iter()
                    .map(|m| m.name.clone())
                    .collect(),
            ),
            Choice::Runtime => (
                sheet.runtime_anchor.clone(),
                sheet.runtime,
                runtimes.iter().map(|r| r.name.clone()).collect(),
            ),
        };
        let width = anchor
            .get()
            .map(|b| f32::from(b.size.width))
            .unwrap_or(280.);
        let this = cx.entity().downgrade();
        let rows = items.into_iter().enumerate().map(|(ix, label)| {
            div()
                .id(("device-choice", ix))
                .role(Role::ListBoxOption)
                .aria_label(SharedString::from(label.clone()))
                .h(px(28.))
                .px(px(8.))
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(8.))
                .rounded(px(6.))
                .text_size(px(12.5))
                .text_color(content(0.85))
                .cursor_pointer()
                .hover(|s| s.bg(content(0.06)))
                .child(div().size(px(14.)).when(ix == current, |d| {
                    d.child(icon("check", 14.).text_color(content(0.85)))
                }))
                .child(div().flex_1().min_w_0().truncate().child(label))
                .on_click(cx.listener(move |this, _, _, cx| this.choose(choice, ix, cx)))
        });
        Some(
            popover::popover("device-choice-list", Anchor::Slot(anchor))
                .width(width.max(200.))
                .max_height(300.)
                .padding(4.)
                .gap(4.)
                .priority(60)
                .on_dismiss(move |_, cx| {
                    let _ = this.update(cx, |panel, cx| {
                        if let Some(sheet) = panel.sheet.as_mut() {
                            sheet.open = None;
                        }
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .id("device-choice-scroll")
                        .max_h(px(292.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .children(rows),
                )
                .into_any_element(),
        )
    }
}

/// Renders the shelf, its menu and the New device sheet with the Metal
/// renderer: `DEVICE_SHOTS=/tmp/device-shots cargo test device_shelf_shots -- --ignored`.
#[cfg(all(test, target_os = "macos"))]
mod shots {
    use super::*;
    use gpui::{HeadlessAppContext, size};
    use monocode_core::device::hub::{ModelOption, RuntimeOption};
    use std::sync::Arc;

    struct Root(Entity<DevicePanel>);
    impl Render for Root {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .bg(pane_fill())
                .text_color(content(1.))
                .child(self.0.clone())
        }
    }

    fn device(platform: Platform, name: &str, version: &str, booted: bool) -> Device {
        Device {
            id: name.into(),
            name: name.into(),
            version: version.into(),
            platform,
            booted,
            physical: false,
            supported: true,
        }
    }

    fn options() -> NewDeviceOptions {
        let models = |names: &[&str]| {
            names
                .iter()
                .map(|n| ModelOption {
                    id: n.to_lowercase().replace(' ', "_"),
                    name: (*n).into(),
                })
                .collect()
        };
        NewDeviceOptions {
            ios: vec![RuntimeOption {
                id: "rt.iOS-27-0".into(),
                name: "iOS 27.0".into(),
                models: models(&[
                    "iPhone 18 Pro",
                    "iPhone 18 Pro Max",
                    "iPhone 17 Pro",
                    "iPhone 17 Pro Max",
                    "iPhone 17e",
                    "iPhone Air",
                    "iPhone 17",
                    "iPhone 16 Pro",
                    "iPhone 16e",
                    "iPhone 16",
                    "iPhone SE (3rd generation)",
                ]),
            }],
            android: vec![RuntimeOption {
                id: "system-images;android-36;google_apis;arm64-v8a".into(),
                name: "Android 16 · Google APIs".into(),
                models: models(&[
                    "Pixel 10 Pro XL",
                    "Pixel 10 Pro",
                    "Pixel 10",
                    "Pixel 9 Pro",
                    "Pixel 9",
                ]),
            }],
            errors: vec![],
        }
    }

    #[test]
    #[ignore = "Requires a Metal device; writes screenshots"]
    fn device_shelf_shots() {
        let dir = std::env::var("DEVICE_SHOTS").unwrap_or_else(|_| "/tmp/device-shots".into());
        std::fs::create_dir_all(&dir).unwrap();
        device::set_enabled(true, false);
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(gpui_apple::AppleTextSystem::new()),
            Arc::new(crate::assets::Assets),
            || {
                Ok(Some(Box::new(
                    gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
                )))
            },
        );
        cx.update(crate::popover::bind);
        cx.update(crate::input::bind);
        let (width, height) = (400u32, 720u32);
        let mut panel = None;
        let window = cx
            .open_window(size(px(width as f32), px(height as f32)), |_, cx| {
                let entity = cx.new(DevicePanel::new);
                panel = Some(entity.clone());
                cx.new(|_| Root(entity))
            })
            .unwrap();
        let panel = panel.unwrap();
        let shot = |cx: &mut HeadlessAppContext, name: &str| {
            for _ in 0..4 {
                std::thread::sleep(std::time::Duration::from_millis(150));
                cx.run_until_parked();
                cx.update_window(window.into(), |_, w, _| w.refresh())
                    .unwrap();
                cx.run_until_parked();
            }
            cx.capture_screenshot(window.into())
                .unwrap()
                .save(format!("{dir}/{name}.png"))
                .unwrap();
        };
        let stage =
            |cx: &mut HeadlessAppContext,
             f: &dyn Fn(&mut DevicePanel, &mut Window, &mut Context<DevicePanel>)| {
                window
                    .update(cx, |_, window, cx| {
                        panel.update(cx, |p, cx| {
                            f(p, window, cx);
                            cx.notify();
                        })
                    })
                    .unwrap();
            };
        let list = DeviceList {
            devices: vec![
                device(Platform::Ios, "iPhone 18 Pro", "iOS 27.0", true),
                device(Platform::Ios, "iPhone 17", "iOS 27.0", false),
                device(Platform::Ios, "iPhone 17 Pro", "iOS 27.0", false),
                device(Platform::Ios, "iPhone Air", "iOS 27.0", false),
                device(Platform::Ios, "iPad Air", "iOS 27.0", false),
                device(Platform::Android, "Pixel_9_API_36", "Android 16.0", false),
                Device {
                    id: "R5CT20ABCDE".into(),
                    physical: true,
                    ..device(Platform::Android, "samsung SM-S928B", "Android 15.0", true)
                },
            ],
            errors: vec![],
        };
        for (theme, light) in [("dark", false), ("light", true)] {
            let prefs = crate::settings::Preferences {
                theme: theme.into(),
                ..Default::default()
            };
            crate::theme::configure(&prefs, light);
            let list = list.clone();
            stage(&mut cx, &move |p, _, _| {
                p.hub = HubState::Ready;
                p.list = Some(list.clone());
                p.options = Some(options());
                p.picking = true;
                p.selected = None;
                p.sheet = None;
                p.menu = None;
                p.creating.clear();
                p.notice = None;
            });
            shot(&mut cx, &format!("{theme}-1-shelf"));
            stage(&mut cx, &|p, _, _| {
                let target = p.list.as_ref().unwrap().devices[0].clone();
                p.menu = Some(TileMenu {
                    device: target,
                    at: None,
                });
            });
            shot(&mut cx, &format!("{theme}-2-menu"));
            stage(&mut cx, &|p, _, _| {
                p.menu = None;
                p.creating.push((Platform::Android, "Pixel_10_Pro".into()));
                p.notice = Some("Could not delete iPhone 17 Pro. Unable to delete a device in the current state: Booted.".into());
            });
            shot(&mut cx, &format!("{theme}-3-creating"));
            stage(&mut cx, &|p, window, cx| {
                p.creating.clear();
                p.notice = None;
                p.open_new(Platform::Ios, window, cx);
            });
            shot(&mut cx, &format!("{theme}-4-new-ios"));
            stage(&mut cx, &|p, _, _| {
                p.sheet.as_mut().unwrap().open = Some(Choice::Model);
            });
            shot(&mut cx, &format!("{theme}-5-models"));
            stage(&mut cx, &|p, window, cx| {
                p.open_new(Platform::Android, window, cx);
                p.sheet
                    .as_ref()
                    .unwrap()
                    .name
                    .update(cx, |i, cx| i.set_text("Pixel 10 Pro (work)".into(), cx));
            });
            shot(&mut cx, &format!("{theme}-6-new-android"));
            stage(&mut cx, &|p, _, _| {
                p.sheet = None;
                p.list = Some(DeviceList::default());
            });
            shot(&mut cx, &format!("{theme}-7-empty"));
        }
    }
}
