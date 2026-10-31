//! FloatingMonoChat.tsx: the menu bar's chat panel. It floats over other
//! apps without activating MonoCode, and its rail moves between Monos without
//! touching the workspace window's selection.
use super::*;
use crate::tooltip::Tip;
use gpui::{Bounds, WindowBounds, WindowKind, WindowOptions, point, size};
/// A 384pt conversation plus the 56pt Mono rail beside it.
const WIDTH: f32 = 440.;
const HEIGHT: f32 = 560.;
const MARGIN: f32 = 12.;
struct Floating {
    owner: Entity<Monos>,
    selected: String,
    /// Hidden panels are kept for reuse; only a shown one owns its chat.
    shown: bool,
    /// The Mono whose composer last received focus here.
    focused: Option<String>,
    _observe: Subscription,
}
fn hide(window: &mut Window) {
    #[cfg(all(target_os = "macos", not(test)))]
    crate::traffic_lights::hide(window);
    #[cfg(any(not(target_os = "macos"), test))]
    window.remove_window();
}
/// Blank chrome moves the panel, as `data-tauri-drag-region` does.
fn drag(window: &mut Window) {
    #[cfg(target_os = "macos")]
    window.start_window_move();
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}
/// A header button: 28pt, and it never starts a window move.
fn header_button(
    id: &'static str,
    label: &'static str,
    path: &'static str,
) -> gpui::Stateful<gpui::Div> {
    icon_button_hover(id, label, path, 0.08)
        .size(px(28.))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}
fn status_label(state: &str) -> &'static str {
    match state {
        "working" => "Working",
        "needs-you" => "Needs you",
        _ => "Idle",
    }
}
/// MonoStatusDot and MonoStatus's dot: accent when it needs you, the Mono's
/// color while it works, grey at rest.
fn status_fill(state: &str, color: u32) -> gpui::Hsla {
    match state {
        "working" => rgb(color).into(),
        "needs-you" => accent_color(),
        _ => content(0.3),
    }
}
impl Floating {
    fn select(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = id.clone();
        self.focused = None;
        self.owner.update(cx, |owner, cx| {
            owner.ensure_chat(&id, cx);
            // The workspace may now show, or stop deferring to, this chat.
            cx.notify();
        });
        self.focus_chat(window, cx);
        cx.notify();
    }
    fn focus_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focused.as_deref() == Some(&self.selected) {
            return;
        }
        if let Some(chat) = self.owner.read(cx).chats.get(&self.selected) {
            window.focus(&chat.read(cx).input.focus_handle(cx), cx);
            self.focused = Some(self.selected.clone());
        }
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shown = false;
        self.focused = None;
        hide(window);
        self.owner.update(cx, |_, cx| cx.notify());
    }
    fn rail(&self, roster: Vec<Mono>, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = self.owner.read(cx);
        let states: HashMap<String, String> = roster
            .iter()
            .map(|m| {
                let state = owner
                    .chats
                    .get(&m.id)
                    .map(|c| c.read(cx).mono_status().to_owned())
                    .unwrap_or_else(|| "idle".into());
                (m.id.clone(), state)
            })
            .collect();
        div()
            .id("floating-mono-rail")
            .role(Role::Navigation)
            .aria_label("Monos")
            .w(px(56.))
            .h_full()
            .flex_shrink_0()
            .overflow_y_scroll()
            // Puts the first mascot's center level with the header's.
            .pt(px(17.))
            .pb_3()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .on_mouse_down(MouseButton::Left, |_, window, _| drag(window))
            .children(roster.into_iter().map(|mono| {
                let id = mono.id.clone();
                let selected = id == self.selected;
                let state = states[&id].clone();
                div()
                    .id(SharedString::from(format!("floating-mono-{id}")))
                    .group("floating-mono-row")
                    .relative()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .w(px(4.))
                                    .h(px(if selected { 20. } else { 8. }))
                                    .rounded_r_full()
                                    .bg(content(1.))
                                    .when(!selected, |d| {
                                        d.opacity(0.)
                                            .group_hover("floating-mono-row", |s| s.opacity(1.))
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("floating-mono-button-{id}")))
                            .role(Role::Button)
                            .aria_label(mono.label())
                            .size(px(32.))
                            .rounded_lg()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .when(!selected, |d| d.opacity(0.4).hover(|s| s.opacity(1.)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(avatar::avatar(&mono.mascot, mono.color, "idle", 28., true))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.selected != id {
                                    this.select(id.clone(), window, cx);
                                }
                            })),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(2.))
                            .right(px(10.))
                            .size(px(8.))
                            .rounded_full()
                            .border_1()
                            .border_color(shell_fill())
                            .bg(status_fill(&state, mono.color)),
                    )
            }))
            .child(
                div()
                    .id("floating-new-mono")
                    .role(Role::Button)
                    .aria_label("New mono")
                    .tip("New mono")
                    .size(px(32.))
                    .flex_shrink_0()
                    .rounded_lg()
                    .border_1()
                    .border_color(content(0.1))
                    .bg(content(0.05))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .group("floating-new-mono")
                    .hover(|s| s.bg(content(0.1)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        icon("plus", 14.)
                            .text_color(content(0.6))
                            .group_hover("floating-new-mono", |s| s.text_color(content(1.))),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        // Shown here in place of the chat that asked, as in
                        // Tauri; the workspace keeps its own selection.
                        let id = this.owner.update(cx, |owner, cx| {
                            let projects = owner
                                .roster
                                .get(&this.selected)
                                .map(|m| m.projects.clone())
                                .unwrap_or_default();
                            owner.create_floating(projects, cx)
                        });
                        this.select(id, window, cx);
                    })),
            )
    }
    fn header(
        &self,
        mono: &Mono,
        state: &str,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_3()
            .border_b_1()
            .border_color(content(0.08))
            .on_mouse_down(MouseButton::Left, |_, window, _| drag(window))
            .child(avatar::avatar(
                &mono.mascot,
                mono.color,
                state,
                28.,
                state == "idle",
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h(px(28.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.))
                            .line_height(px(16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(mono.label()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(11.))
                            .line_height(px(14.))
                            .text_color(content(0.45))
                            .child(
                                div()
                                    .size(px(8.))
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .bg(status_fill(state, mono.color)),
                            )
                            .child(status_label(state)),
                    ),
            )
            .when(busy, |d| {
                d.child(
                    header_button("floating-stop", "Stop reply", "square")
                        .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                )
            })
            .child(
                header_button("floating-reveal", "Open in MonoCode", "external-link").on_click(
                    cx.listener(|this, _, window, cx| {
                        let id = this.selected.clone();
                        let owner = this.owner.clone();
                        let parent = owner.read(cx).parent_window;
                        // As Tauri's reveal: the workspace takes the chat over,
                        // so this panel steps aside.
                        this.close(window, cx);
                        let app: &mut App = cx;
                        app.defer(move |cx| {
                            if let Some(parent) = parent {
                                let _ = parent.update(cx, |_, w, cx| {
                                    owner.update(cx, |m, cx| m.open(&id, w, cx));
                                    w.activate_window();
                                    cx.activate(true);
                                });
                            }
                        });
                    }),
                ),
            )
            .child(
                header_button("floating-close", "Hide chat", "close")
                    .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            )
    }
    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(chat) = self.owner.read(cx).chats.get(&self.selected).cloned() {
            chat.update(cx, |v, cx| v.stop_if_busy(cx));
        }
    }
}
impl gpui::Render for Floating {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shell = self.owner.read(cx).owner.clone();
        // Defer until this root releases its render lease; the shell snapshots
        // all floating selections together with its own visible panes.
        let app: &mut App = cx;
        app.defer(move |cx| {
            let _ = shell.update(cx, |shell, cx| shell.keep_visible_idle_agents(cx));
        });
        if self.focused.as_deref() != Some(&self.selected)
            && self.owner.read(cx).chats.contains_key(&self.selected)
        {
            cx.defer_in(window, |this, window, cx| this.focus_chat(window, cx));
        }
        let owner = self.owner.read(cx);
        let chat = owner.chats.get(&self.selected).cloned();
        let roster = owner.roster.monos.clone();
        let mono = owner.roster.get(&self.selected).cloned();
        let failure = owner.error.clone();
        let (state, busy) = chat.as_ref().map_or(("idle".to_owned(), false), |c| {
            let c = c.read(cx);
            (c.mono_status().to_owned(), c.busy)
        });
        let light = is_light();
        div()
            .id("floating-mono")
            .size_full()
            .bg(shell_fill())
            .text_color(content(1.))
            .font_family(crate::fonts::UI)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.stop(cx);
                }
            }))
            .child(
                // The card's spread shadow: it shades the rail and the gaps
                // around the chat, never the chat itself.
                div()
                    .size_full()
                    .bg(black(if light { 0.05 } else { 0.28 }))
                    .flex()
                    .child(self.rail(roster, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .my(px(6.))
                            .mr(px(6.))
                            .rounded(px(10.))
                            .overflow_hidden()
                            .bg(floating_fill())
                            .when_some(mono.clone(), |d, mono| {
                                d.child(self.header(&mono, &state, busy, cx))
                            })
                            .map(|d| match (chat, mono) {
                                (Some(chat), Some(_)) => {
                                    d.child(div().flex_1().min_h_0().child(chat))
                                }
                                (_, mono) => d.child(
                                    div()
                                        .flex_1()
                                        .min_h_0()
                                        .px_8()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_center()
                                        .gap_4()
                                        .when_some(mono, |d, mono| {
                                            d.child(avatar::avatar(
                                                &mono.mascot,
                                                mono.color,
                                                "idle",
                                                48.,
                                                true,
                                            ))
                                        })
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .text_color(content(0.5))
                                                .text_center()
                                                .child(failure.unwrap_or_else(|| {
                                                    "Loading conversation…".into()
                                                })),
                                        ),
                                ),
                            }),
                    ),
            )
    }
}
impl Monos {
    pub(crate) fn visible_idle_sessions(&self, cx: &App) -> Vec<i64> {
        let mut ids = self
            .chat()
            .into_iter()
            .map(|chat| chat.read(cx).summary.id)
            .collect::<Vec<_>>();
        for window in &self.floating {
            if let Some(window) = window.downcast::<Floating>()
                && let Ok(view) = window.read(cx)
                && view.shown
                && let Some(chat) = self.chats.get(&view.selected)
            {
                ids.push(chat.read(cx).summary.id);
            }
        }
        ids
    }
    /// A shown floating panel owns this Mono's chat. A conversation measures
    /// one viewport, so drawing it in two windows at once makes both relayout
    /// against each other every frame.
    pub(super) fn floating_shows(&self, id: &str, cx: &App) -> bool {
        self.floating.iter().any(|window| {
            window
                .downcast::<Floating>()
                .and_then(|w| w.read(cx).ok())
                .is_some_and(|v| v.shown && v.selected == id)
        })
    }
    /// Stands in for a chat that a floating panel is showing.
    pub(super) fn floating_placeholder(&self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let id = mono.id.clone();
        div()
            .flex_1()
            .min_h_0()
            .px_8()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(avatar::avatar(&mono.mascot, mono.color, "idle", 48., true))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(content(0.5))
                    .text_center()
                    .child(format!("{} is open in a floating chat.", mono.label())),
            )
            .child(
                crate::settings::widgets::secondary_button(
                    "floating-show-here",
                    "Show here",
                    false,
                    false,
                )
                .child("Show here")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.hide_floating(&id, cx);
                    this.focus_active(window, cx);
                })),
            )
            .into_any_element()
    }
    /// Hide every panel showing this Mono, so the workspace can draw it.
    pub(super) fn hide_floating(&mut self, id: &str, cx: &mut Context<Self>) {
        for window in self.floating.clone() {
            if let Some(window) = window.downcast::<Floating>() {
                let _ = window.update(cx, |view, window, _| {
                    if view.shown && view.selected == id {
                        view.shown = false;
                        view.focused = None;
                        hide(window);
                    }
                });
            }
        }
        cx.notify();
    }
    /// Tauri's floating `create`: add a Mono without opening it in the
    /// workspace, so the panel that asked can show it.
    pub(super) fn create_floating(
        &mut self,
        projects: Vec<String>,
        cx: &mut Context<Self>,
    ) -> String {
        let id = self.roster.create(projects).id.clone();
        self.save(cx);
        self.ensure_chat(&id, cx);
        self.changed(cx);
        id
    }
    pub(super) fn open_floating(&mut self, id: String, cx: &mut Context<Self>) {
        if self.roster.get(&id).is_none() {
            return;
        }
        self.ensure_chat(&id, cx);
        // Bring back the panel already on this Mono, else reuse a hidden one.
        let mut hidden = None;
        let mut shown = 0;
        for existing in &self.floating {
            let Some(existing) = existing.downcast::<Floating>() else {
                continue;
            };
            let Ok(view) = existing.read(cx) else {
                continue;
            };
            if view.shown {
                shown += 1;
            }
            if view.selected == id {
                hidden = Some(existing);
                break;
            }
            if !view.shown && hidden.is_none() {
                hidden = Some(existing);
            }
        }
        if let Some(existing) = hidden {
            let app: &mut App = cx;
            app.defer(move |cx| {
                let _ = existing.update(cx, |view, window, cx| {
                    view.shown = true;
                    if view.selected != id {
                        view.select(id, window, cx);
                    }
                    view.focused = None;
                    view.focus_chat(window, cx);
                    // Key without activating: a panel never raises the app.
                    window.activate_window();
                    cx.notify();
                });
            });
            cx.notify();
            return;
        }
        let owner = cx.entity();
        let display = cx.primary_display();
        let bounds = match &display {
            Some(display) => {
                let area = display.visible_bounds();
                let height = px(HEIGHT).min(area.size.height - px(2. * MARGIN));
                // Each further panel steps down and left, as Tauri's do.
                let offset = px((shown % 8) as f32 * 28.);
                Bounds {
                    origin: point(
                        area.origin.x + area.size.width - px(WIDTH + MARGIN) - offset,
                        area.origin.y + px(MARGIN) + offset,
                    ),
                    size: size(px(WIDTH), height),
                }
            }
            None => Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(370.), px(420.))),
            titlebar: None,
            kind: WindowKind::PopUp,
            is_minimizable: false,
            display_id: display.map(|d| d.id()),
            window_background: crate::theme::window_backdrop(!is_light()),
            app_id: Some("dev.usemono.mono-chat".into()),
            ..Default::default()
        };
        // Opening a window draws its root immediately. Release the manager's
        // mutable lease before Floating reads it during that first draw.
        let app: &mut App = cx;
        app.defer(move |cx| {
            let window_owner = owner.clone();
            let result = cx.open_window(options, |window, cx| {
                #[cfg(all(target_os = "macos", not(test)))]
                crate::traffic_lights::floating_panel(window, true);
                #[cfg(any(not(target_os = "macos"), test))]
                let _ = window;
                cx.new(|cx| {
                    let observation = cx.observe(&window_owner, |_, _, cx| cx.notify());
                    Floating {
                        owner: window_owner,
                        selected: id,
                        shown: true,
                        focused: None,
                        _observe: observation,
                    }
                })
            });
            owner.update(cx, |owner, cx| {
                match result {
                    Ok(handle) => owner.floating.push(handle.into()),
                    Err(e) => owner.error = Some(e.to_string()),
                }
                cx.notify();
            });
        });
    }
    pub(super) fn open_workspace(&self, id: i64, window: &Window, cx: &mut Context<Self>) {
        let owner = self.owner.clone();
        let handle = window.window_handle();
        let app: &mut App = cx;
        app.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let _ = owner.update(cx, |owner, cx| owner.open(id, window, cx));
            });
        });
    }
    pub(crate) fn move_rail_drag(&mut self, event: &gpui::MouseMoveEvent, cx: &mut Context<Self>) {
        if let Some((_, start, over)) = &mut self.drag {
            if event.pressed_button != Some(MouseButton::Left) {
                self.finish_reorder(cx);
                return;
            }
            if (event.position - *start).magnitude() > 5. {
                self.skip_click = true;
                *over = self.rail_bounds.iter().find_map(|(id, s)| {
                    s.get()
                        .filter(|b| b.contains(&event.position))
                        .map(|_| id.clone())
                });
            }
        }
    }
}
