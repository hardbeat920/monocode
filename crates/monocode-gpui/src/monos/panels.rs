use super::*;
use crate::{
    pickers::{ModelList, ModelPicked},
    popover::{MenuAction, MenuItem, MenuStyle, Side},
    project_settings::{BackgroundDialog, Event as BackgroundEvent},
};
use gpui::{AnyView, Div, FontWeight, Stateful};
use monocode_core::mono::heartbeat;
use monocode_core::project_appearance::{MASCOTS, ProjectAppearance};
#[derive(Clone, Copy, PartialEq)]
pub(super) enum OverviewTab {
    Overview,
    Sessions,
    Habits,
    Documents,
    Changes,
    Activity,
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum SettingsSection {
    General,
    Soul,
    Memory,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Route {
    Review,
    Session(i64),
    Document(String),
    Habit(String),
    NewHabit,
}
impl Route {
    pub(super) fn key(&self) -> String {
        match self {
            Self::Review => "review".into(),
            Self::Session(id) => format!("session:{id}"),
            Self::Document(id) => format!("document:{id}"),
            Self::Habit(id) => format!("habit:{id}"),
            Self::NewHabit => "new-habit".into(),
        }
    }
}
pub(super) struct OpenPicker {
    pub(super) menu: Entity<Menu>,
    pub(super) anchor: Anchor,
    pub(super) width: f32,
    pub(super) align: crate::popover::Align,
    pub(super) _events: Vec<Subscription>,
}
pub(super) fn button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .cursor_pointer()
        .flex()
        .items_center()
        .gap_1p5()
        .px_2p5()
        .py_1p5()
        .rounded_md()
        .text_size(px(12.))
        .text_color(content(0.8))
        .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
        .child(label)
}
fn setting(label: &str, description: Option<&str>, control: AnyElement) -> Div {
    div()
        .min_h(px(48.))
        .flex()
        .items_center()
        .gap_4()
        .px_3()
        .py_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .text_color(content(0.9))
                        .child(label.to_string()),
                )
                .when_some(description, |d, text| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(20.))
                            .text_color(content(0.4))
                            .child(text.to_string()),
                    )
                }),
        )
        .child(
            div()
                .w(px(224.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_end()
                .pr_1p5()
                .text_size(px(12.))
                .text_color(content(0.85))
                .child(control),
        )
}
fn switch_setting(label: &str, description: &str, disabled: bool, control: AnyElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .when(disabled, |d| d.opacity(0.4))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .text_color(content(0.9))
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(20.))
                        .text_color(content(0.4))
                        .truncate()
                        .child(description.to_string()),
                ),
        )
        .child(control)
}
fn heading(title: &str, description: Option<&str>) -> Div {
    div()
        .pt_7()
        .pb_2()
        .px_1()
        .text_size(px(12.))
        .text_color(content(0.7))
        .font_weight(FontWeight::MEDIUM)
        .child(title.to_string())
        .when_some(description, |d, text| {
            d.child(
                div()
                    .line_height(px(20.))
                    .text_color(content(0.4))
                    .font_weight(FontWeight::NORMAL)
                    .child(text.to_string()),
            )
        })
}
/// The outlined, rounded card each Sub-agents, Documents and Habits row sits in.
pub(super) fn outlined<E: Styled + InteractiveElement>(element: E) -> E {
    element
        .rounded_xl()
        .border_1()
        .border_color(content(0.07))
        .bg(content(0.025))
        .hover(|s| s.bg(content(0.05)).border_color(content(0.14)))
}
enum AgentState {
    Working,
    NeedsInput,
    Idle(u64),
    Unavailable,
}
struct SubAgent<'a> {
    id: i64,
    title: String,
    harness: &'a str,
    /// The live session's model label, when it is loaded.
    model: Option<SharedString>,
    model_id: Option<&'a str>,
    place: String,
    branch: bool,
    state: AgentState,
}
/// A sub-agent the Mono started: who it is (model and provider) first, then
/// what it was asked to do and where.
fn sub_agent_card(agent: SubAgent) -> Stateful<Div> {
    let kind = Kind::from_id(agent.harness);
    let model = agent
        .model
        .unwrap_or_else(|| crate::pickers::cached_model_name(kind, agent.model_id));
    // Claude models already say who made them.
    let model: SharedString = model
        .strip_prefix("Claude ")
        .map_or(model.clone(), |short| short.to_owned().into());
    let amber: gpui::Hsla = rgb(0xfbbf24).into();
    let (label, color, live): (String, gpui::Hsla, bool) = match agent.state {
        AgentState::Working => ("Working".into(), accent_color(), true),
        AgentState::NeedsInput => ("Needs approval".into(), amber, true),
        AgentState::Idle(at) => (
            monocode_core::time::relative(at, monocode_core::time::now_ms()),
            content(0.45),
            false,
        ),
        AgentState::Unavailable => ("Unavailable".into(), content(0.45), false),
    };
    let needs_input = matches!(agent.state, AgentState::NeedsInput);
    let id = agent.id;
    outlined(
        div()
            .id(("mono-launch", id as usize))
            .debug_selector(move || format!("mono-launch-{id}"))
            .role(Role::Button)
            .aria_label(format!("{}, {model}", agent.title)),
    )
    .when(needs_input, |d| {
        let mut c = amber;
        c.a = 0.45;
        d.border_color(c)
    })
    .when(matches!(agent.state, AgentState::Unavailable), |d| {
        d.opacity(0.5)
    })
    .flex_shrink_0()
    .px(px(10.))
    .py_2()
    .flex()
    .items_center()
    .gap_2p5()
    .child(
        div()
            .size(px(28.))
            .flex_shrink_0()
            .rounded_md()
            .bg(content(0.06))
            .flex()
            .items_center()
            .justify_center()
            .child(provider_icon(agent.harness, 16.)),
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            // What it was asked to do, and how it is doing.
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(content(1.))
                            .child(agent.title),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_size(px(11.))
                            .text_color(color)
                            .when(live, |d| {
                                d.child(div().size(px(6.)).rounded_full().bg(color))
                            })
                            .child(label),
                    ),
            )
            // Which model runs it, and where.
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(content(0.5))
                    .child(div().min_w_0().truncate().child(model))
                    .child(
                        div()
                            .ml_auto()
                            .min_w_0()
                            .max_w(px(160.))
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_color(content(0.4))
                            .child(
                                icon(
                                    if agent.branch {
                                        "git-branch"
                                    } else {
                                        "folder-01"
                                    },
                                    11.,
                                )
                                .text_color(content(0.4)),
                            )
                            .child(div().min_w_0().truncate().child(agent.place)),
                    ),
            ),
    )
}
fn card() -> Div {
    div()
        .rounded_xl()
        .border_1()
        .border_color(content(0.04))
        .overflow_hidden()
        .bg(content(0.02))
        .flex()
        .flex_col()
}
/// MonoOverview's search row above the Sessions, Habits and Documents lists.
pub(super) fn search_bar(field: Entity<Input>) -> Div {
    div()
        .h(px(36.))
        .flex_shrink_0()
        .px_2()
        .border_b_1()
        .border_color(crate::right_panel::stroke())
        .flex()
        .items_center()
        .gap_2()
        .child(icon("search", 12.))
        .child(div().flex_1().min_w_0().child(field))
}
/// Whether `haystack` contains `query`, ignoring case and surrounding spaces.
pub(super) fn matches_query(query: &str, haystack: &str) -> bool {
    haystack
        .to_lowercase()
        .contains(&query.trim().to_lowercase())
}
/// monoPanelParts' EmptyState: an icon over a short, centred note.
pub(super) fn empty_state(glyph: &'static str, message: &str) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .px_6()
        .py_10()
        .text_center()
        .child(icon(glyph, 18.).text_color(content(0.55)))
        .child(
            div()
                .max_w(px(288.))
                .text_size(px(12.))
                .line_height(px(18.))
                .text_color(content(0.55))
                .child(message.to_string()),
        )
        .into_any_element()
}
fn empty(message: &str) -> AnyElement {
    div()
        .px_5()
        .py_8()
        .text_size(px(12.))
        .line_height(px(20.))
        .text_color(content(0.45))
        .child(message.to_string())
        .into_any_element()
}
struct LookChanged {
    mono: Mono,
}
/// Enter in the name field: MonoMenu commits the name and closes.
struct LookSubmitted;
struct LookEditor {
    mono: Mono,
    name: Entity<Input>,
    color: Entity<Input>,
    settings: bool,
    custom_open: bool,
    custom: Entity<super::color_picker::ColorPicker>,
    focus: gpui::FocusHandle,
    _events: Vec<Subscription>,
}
impl EventEmitter<LookChanged> for LookEditor {}
impl EventEmitter<LookSubmitted> for LookEditor {}
impl LookEditor {
    fn new(mono: Mono, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| {
            let mut f = Input::field("Mono name", mono.label(), content(0.45), cx);
            f.set_text(mono.name.clone(), cx);
            f
        });
        let color = cx.new(|cx| {
            let mut f = Input::field("Custom Mono color", "#rrggbb", content(0.4), cx);
            f.set_text(format!("#{:06x}", mono.color), cx);
            f
        });
        let custom = cx.new(|cx| super::color_picker::ColorPicker::new(mono.color, cx));
        let events = vec![
            cx.subscribe(
                &custom,
                |this, _, event: &super::color_picker::Picked, cx| {
                    this.mono.color = event.0;
                    this.color.update(cx, |field, cx| {
                        field.set_text(format!("#{:06x}", event.0), cx)
                    });
                    cx.emit(LookChanged {
                        mono: this.mono.clone(),
                    });
                    cx.notify();
                },
            ),
            cx.subscribe(&name, |this, input, _: &input::Changed, cx| {
                this.mono.name = input.read(cx).content.chars().take(40).collect();
                cx.emit(LookChanged {
                    mono: this.mono.clone(),
                });
            }),
            cx.subscribe(&name, |_, _, _: &input::Submitted, cx| {
                cx.emit(LookSubmitted)
            }),
            cx.subscribe(&color, |this, input, _: &input::Changed, cx| {
                let text = input.read(cx).content.trim();
                if text.len() == 7
                    && let Ok(color) = u32::from_str_radix(text.trim_start_matches('#'), 16)
                {
                    this.mono.color = color;
                    this.custom
                        .update(cx, |picker, cx| picker.set_color(color, cx));
                    cx.emit(LookChanged {
                        mono: this.mono.clone(),
                    });
                    cx.notify();
                }
            }),
        ];
        Self {
            mono,
            name,
            color,
            settings: false,
            custom_open: false,
            custom,
            focus: cx.focus_handle(),
            _events: events,
        }
    }
}
impl gpui::Render for LookEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.settings {
            return self.settings_controls(cx);
        }
        // MonoMenu: the name, MascotPicker and the inline ColorPicker, each
        // ten columns wide.
        const COLOR_NAMES: [&str; 9] = [
            "Blue", "Coral", "Yellow", "Green", "Pink", "Purple", "Teal", "Orange", "Indigo",
        ];
        let preset = mono::COLORS_MONO.contains(&self.mono.color);
        let swatch = |id: gpui::ElementId, label: SharedString, ring: bool, dot: AnyElement| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .aria_selected(ring)
                .flex_1()
                .min_w_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .size(px(20.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(ring, |d| d.border_2().border_color(content(0.8)))
                        .child(dot),
                )
        };
        div()
            .w_full()
            .child(
                div()
                    .debug_selector(|| "mono-options-name".into())
                    .mb_2()
                    .px_2p5()
                    .py_1p5()
                    .rounded_lg()
                    .border_1()
                    .border_color(content(0.1))
                    .bg(content(0.05))
                    .text_size(px(13.))
                    .child(self.name.clone()),
            )
            .child(
                div()
                    .mb_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .id("mono-mascot-options")
                            .role(Role::RadioGroup)
                            .aria_label("Mascot")
                            .px_1()
                            .flex()
                            .gap(px(2.))
                            .children(MASCOTS.iter().enumerate().map(|(ix, name)| {
                                let selected = self.mono.mascot == *name;
                                div()
                                    .id(("mono-mascot", ix))
                                    .debug_selector(move || format!("mono-mascot-{ix}"))
                                    .role(Role::RadioButton)
                                    .aria_label(*name)
                                    .aria_selected(selected)
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(32.))
                                    .rounded_md()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(selected, |d| {
                                        d.bg(content(0.1)).border_1().border_color(content(0.2))
                                    })
                                    .when(!selected, |d| d.hover(|s| s.bg(content(0.06))))
                                    .child(avatar::avatar(
                                        name,
                                        self.mono.color,
                                        "idle",
                                        20.,
                                        !selected,
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.mono.mascot = (*name).into();
                                        cx.emit(LookChanged {
                                            mono: this.mono.clone(),
                                        });
                                        cx.notify();
                                    }))
                            })),
                    )
                    .child(
                        div()
                            .id("mono-color-options")
                            .role(Role::Group)
                            .aria_label("Color")
                            .child(
                                div()
                                    .px_1()
                                    .flex()
                                    .gap(px(2.))
                                    .children(mono::COLORS_MONO.into_iter().enumerate().map(
                                        |(ix, color)| {
                                            swatch(
                                                ("mono-color", ix).into(),
                                                COLOR_NAMES[ix].into(),
                                                self.mono.color == color,
                                                div()
                                                    .size(px(14.))
                                                    .rounded_full()
                                                    .bg(rgb(color))
                                                    .into_any_element(),
                                            )
                                            .debug_selector(move || format!("mono-color-{ix}"))
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    this.mono.color = color;
                                                    this.custom_open = false;
                                                    this.custom.update(cx, |picker, cx| {
                                                        picker.set_color(color, cx)
                                                    });
                                                    cx.emit(LookChanged {
                                                        mono: this.mono.clone(),
                                                    });
                                                    cx.notify();
                                                }),
                                            )
                                        },
                                    ))
                                    .child(
                                        swatch(
                                            "mono-custom-color".into(),
                                            "Custom color".into(),
                                            !preset || self.custom_open,
                                            div()
                                                .relative()
                                                .size(px(14.))
                                                .rounded_full()
                                                .overflow_hidden()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .map(|d| {
                                                    if preset {
                                                        d.child(
                                                            div().absolute().inset_0().child(
                                                                super::color_picker::spectrum(),
                                                            ),
                                                        )
                                                        .child(
                                                            icon("pipette", 8.)
                                                                .relative()
                                                                .text_color(gpui::white()),
                                                        )
                                                    } else {
                                                        d.bg(rgb(self.mono.color))
                                                    }
                                                })
                                                .into_any_element(),
                                        )
                                        .debug_selector(|| "mono-custom-color".into())
                                        .aria_expanded(self.custom_open)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.custom_open = !this.custom_open;
                                                cx.notify();
                                            }),
                                        ),
                                    ),
                            )
                            .when(self.custom_open, |d| {
                                d.child(div().px_1().pt_2().child(self.custom.clone()))
                            }),
                    ),
            )
            .into_any_element()
    }
}
impl LookEditor {
    fn settings_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        const COLOR_NAMES: [&str; 9] = [
            "Blue", "Coral", "Yellow", "Green", "Pink", "Purple", "Teal", "Orange", "Indigo",
        ];
        div()
            .id("mono-settings-look-picker")
            .debug_selector(|| "mono-settings-look-picker".into())
            .role(Role::Dialog)
            .aria_label("Mascot and color")
            .track_focus(&self.focus)
            .p_3()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .id("mono-mascot-options")
                    .role(Role::RadioGroup)
                    .aria_label("Mascot")
                    .px_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(MASCOTS.chunks(5).enumerate().map(|(row, names)| {
                        div().flex().gap_1().children(names.iter().enumerate().map(
                            |(col, name)| {
                                let selected = self.mono.mascot == *name;
                                div()
                                    .id(("settings-mascot", row * 5 + col))
                                    .role(Role::RadioButton)
                                    .aria_label(*name)
                                    .aria_selected(selected)
                                    .h(px(48.))
                                    .flex_1()
                                    .min_w_0()
                                    .rounded_lg()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(selected, |d| {
                                        d.bg(content(0.1)).border_1().border_color(content(0.2))
                                    })
                                    .hover(|d| d.bg(content(0.06)))
                                    .child(avatar::avatar(
                                        name,
                                        self.mono.color,
                                        "idle",
                                        32.,
                                        !selected,
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.mono.mascot = (*name).into();
                                        cx.emit(LookChanged {
                                            mono: this.mono.clone(),
                                        });
                                        cx.notify();
                                    }))
                            },
                        ))
                    })),
            )
            .child(div().h(px(1.)).bg(content(0.1)))
            .child(
                div()
                    .id("mono-color-options")
                    .role(Role::Group)
                    .aria_label("Color")
                    .px_1()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children((0..2).map(|row| {
                        div().flex().justify_between().children((0..5).map(|col| {
                            let index = row * 5 + col;
                            if index < 9 {
                                let color = mono::COLORS_MONO[index];
                                let selected = self.mono.color == color;
                                div()
                                    .id(("settings-color", index))
                                    .role(Role::Button)
                                    .aria_label(COLOR_NAMES[index])
                                    .aria_selected(selected)
                                    .size(px(36.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .child(
                                        div()
                                            .size(px(32.))
                                            .rounded_full()
                                            .border_2()
                                            .border_color(if selected {
                                                content(0.8)
                                            } else {
                                                content(0.)
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                div().size(px(28.)).rounded_full().bg(rgb(color)),
                                            ),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.mono.color = color;
                                        this.custom_open = false;
                                        this.custom
                                            .update(cx, |picker, cx| picker.set_color(color, cx));
                                        this.color.update(cx, |f, cx| {
                                            f.set_text(format!("#{color:06x}"), cx)
                                        });
                                        cx.emit(LookChanged {
                                            mono: this.mono.clone(),
                                        });
                                        cx.notify();
                                    }))
                                    .into_any_element()
                            } else {
                                div()
                                    .id("settings-custom-color")
                                    .role(Role::Button)
                                    .aria_label("Custom color")
                                    .size(px(36.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .child(
                                        div()
                                            .size(px(28.))
                                            .rounded_full()
                                            .when(
                                                !mono::COLORS_MONO.contains(&self.mono.color),
                                                |d| d.bg(rgb(self.mono.color)),
                                            )
                                            .when(
                                                mono::COLORS_MONO.contains(&self.mono.color),
                                                |d| {
                                                    d.child(
                                                        div()
                                                            .absolute()
                                                            .inset_0()
                                                            .child(super::color_picker::spectrum()),
                                                    )
                                                },
                                            )
                                            .relative()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                icon("pipette", 14.)
                                                    .relative()
                                                    .text_color(gpui::white()),
                                            ),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.custom_open = !this.custom_open;
                                        cx.notify();
                                    }))
                                    .into_any_element()
                            }
                        }))
                    })),
            )
            .when(self.custom_open, |d| {
                d.child(div().px_1().pt_2().child(self.custom.clone()))
            })
            .into_any_element()
    }
}
impl Monos {
    fn open_settings_look(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mono) = self.roster.get(id).cloned() else {
            return;
        };
        let editor = cx.new(|cx| {
            let mut editor = LookEditor::new(mono, cx);
            editor.settings = true;
            editor
        });
        let focus = editor.read(cx).focus.clone();
        let changed = cx.subscribe(&editor, |this, _, event: &LookChanged, cx| {
            if let Some(mono) = this.roster.get_mut(&event.mono.id) {
                mono.mascot = event.mono.mascot.clone();
                mono.color = event.mono.color;
            }
            this.configure_chat(&event.mono.id, cx);
            this.save(cx);
        });
        let menu = cx.new(|cx| {
            Menu::new(
                "Mascot and color",
                vec![MenuItem::Custom(editor.into(), focus)],
                window,
                cx,
            )
            .width(272.)
        });
        let closed = cx.subscribe(&menu, |this, _, _: &MenuEvent, cx| {
            this.picker = None;
            cx.notify();
        });
        let anchor = Anchor::Slot(self.settings_anchor.clone());
        self.picker = Some(OpenPicker {
            menu,
            anchor,
            width: 272.,
            align: crate::popover::Align::Center,
            _events: vec![changed, closed],
        });
        cx.notify();
    }
    pub(super) fn field(
        &mut self,
        key: &str,
        label: &str,
        text: String,
        multi: bool,
        cx: &mut Context<Self>,
    ) -> Entity<Input> {
        if let Some(field) = self.fields.get(key) {
            return field.clone();
        }
        let label = label.to_string();
        let field = cx.new(|cx| {
            let mut f = if multi {
                Input::markdown_editor(&label, cx)
            } else if key == "memory-fact" || key.starts_with("memory-edit:") {
                Input::memory_fact(&label, cx)
            } else {
                Input::field(label.clone(), label.clone(), content(0.35), cx)
            };
            f.set_font_size(if key == "memory-fact" || key.starts_with("memory-edit:") {
                12.
            } else {
                13.
            });
            f.set_placeholder(label, cx);
            f.set_text(text, cx);
            f
        });
        let key_owned = key.to_string();
        self.observations.push(
            cx.subscribe(&field, move |this, input, _: &input::Changed, cx| {
                if key_owned == "mono-name" {
                    let name = input.read(cx).content.trim().chars().take(40).collect();
                    if let Some(id) = this.active.clone() {
                        if let Some(mono) = this.roster.get_mut(&id) {
                            mono.name = name;
                        }
                        this.configure_chat(&id, cx);
                        this.save(cx);
                    }
                }
                cx.notify();
            }),
        );
        self.fields.insert(key.into(), field.clone());
        field
    }
    pub(super) fn value(&self, key: &str, cx: &App) -> String {
        self.fields
            .get(key)
            .map(|f| f.read(cx).content.to_string())
            .unwrap_or_default()
    }
    pub(super) fn topbar(&self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let id = mono.id.clone();
        div()
            .absolute()
            .top_0()
            .right(px(10.))
            .h(px(40.))
            .flex()
            .items_center()
            .child(
                icon_button("mono-settings", "Mono settings", "settings")
                    .debug_selector(|| "mono-settings".into())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.settings = Some(SettingsSection::General);
                        window.focus(&this.settings_focus, cx);
                        this.reload_files(&id, cx);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
    pub(super) fn select_overview(&mut self, tab: OverviewTab, cx: &mut Context<Self>) {
        if tab != OverviewTab::Changes {
            self.pause_changes(cx);
        }
        if self.route.is_some() {
            self.save_open_file(cx);
            self.route = None;
            cx.notify();
        }
        if self.overview_tab != tab {
            self.overview_reveal =
                Some(crate::project_tabs::TabReveal::new(tab, self.overview_tab));
            if tab != OverviewTab::Activity {
                self.overview_previous = tab;
            }
            self.overview_tab = tab;
            cx.notify();
        }
    }
    /// MonoSidebarHeader lives in the base page, so routed pages replace its
    /// tabs and close control together with the overview body.
    pub(super) fn original_overview_header(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = self.owner.clone();
        div()
            .id("original-mono-overview-header")
            .debug_selector(|| "original-mono-overview-header".into())
            .h(px(40.))
            .flex_shrink_0()
            // MonoSidebarHeader's pl-4 and OverviewTabs' -ml-2 combine
            // into an 8px inset, keeping the tab icons aligned with actions.
            .pl(px(8.))
            // The window's top-right corner: clear of client-drawn controls.
            .pr(px(12. + crate::shell::corner_controls_width(window)))
            .border_b_1()
            .border_color(content(0.07))
            .flex()
            .items_center()
            .gap(px(2.))
            .child(self.overview_controls(true, window, cx))
            .child(
                div()
                    .id("original-hide-mono-overview")
                    .debug_selector(|| "original-hide-mono-overview".into())
                    .role(Role::Button)
                    .aria_label("Hide overview")
                    .aria_toggled(gpui::Toggled::True)
                    .size(px(24.))
                    .flex_shrink_0()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(content(1.))
                    .hover(|s| s.bg(content(0.1)))
                    .child(icon("sidebar-right", 14.).text_color(content(1.)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let owner = owner.clone();
                        cx.spawn(async move |_, cx| {
                            let _ = owner.update_in(cx, |shell, window, cx| {
                                shell.close_mono_overview(window, cx)
                            });
                        })
                        .detach();
                    })),
            )
            .into_any_element()
    }
    pub(crate) fn overview_controls(
        &self,
        expand_label: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(mono) = self.current() else {
            return div().into_any_element();
        };
        let progress = self
            .overview_reveal
            .as_ref()
            .and_then(|r| r.progress(self.overview_tab, window, cx));
        let tabs = [
            (OverviewTab::Overview, "Overview", "dashboard", 0),
            (
                OverviewTab::Sessions,
                "Sub-agents",
                "message-multiple",
                mono.launches.len(),
            ),
            (
                OverviewTab::Documents,
                "Documents",
                "file",
                mono.documents.len(),
            ),
            (OverviewTab::Habits, "Habits", "clock", self.habits.len()),
            (
                OverviewTab::Changes,
                "Changes",
                "file-diff",
                self.project_changes.count(&mono.id),
            ),
        ];
        div()
            .id("mono-overview-tabs")
            .debug_selector(|| "mono-overview-tabs".into())
            .role(Role::TabList)
            .aria_label("Mono details")
            .h(px(TITLE_BAR_HEIGHT))
            .flex_1()
            .min_w_0()
            .mr(px(2.))
            .flex()
            .items_center()
            .gap(px(2.))
            .children(tabs.into_iter().map(|(tab, label, glyph, count)| {
                let selected = self.overview_tab == tab;
                let unfolded = match progress {
                    Some((_, p)) if selected => p,
                    Some((closing, p)) if tab == closing => 1. - p,
                    _ if selected => 1.,
                    _ => 0.,
                };
                let id = match tab {
                    OverviewTab::Overview => "mono-overview-Overview",
                    OverviewTab::Sessions => "mono-overview-Sessions",
                    OverviewTab::Documents => "mono-overview-Documents",
                    OverviewTab::Habits => "mono-overview-Habits",
                    OverviewTab::Changes => "mono-overview-Changes",
                    OverviewTab::Activity => unreachable!(),
                };
                crate::project_tabs::tab_button(
                    id,
                    (label, count),
                    glyph,
                    selected,
                    tab == OverviewTab::Changes && count > 0 && !selected,
                    if expand_label { unfolded } else { 0. },
                    window,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.select_overview(tab, cx)))
            }))
            .child(crate::shell::titlebar_drag_area().flex_1().min_w_0())
            .when(self.overview_tab == OverviewTab::Habits, |d| {
                d.child(
                    icon_button("mono-new-habit", "New habit", "plus")
                        .debug_selector(|| "mono-new-habit".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.route = Some(Route::NewHabit);
                            this.clear_habit_fields();
                            cx.notify();
                        })),
                )
            })
            .child(
                icon_button("mono-activity", "Show activity", "list-bullet")
                    .debug_selector(|| "mono-activity".into())
                    .when(self.overview_tab == OverviewTab::Activity, |d| {
                        d.bg(content(0.1))
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        let tab = if this.overview_tab == OverviewTab::Activity {
                            this.overview_previous
                        } else {
                            OverviewTab::Activity
                        };
                        this.select_overview(tab, cx);
                    })),
            )
            .into_any_element()
    }
    pub(super) fn overview_panel(
        &mut self,
        mono: &Mono,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_changes_summary(mono, cx);
        if self.overview_tab == OverviewTab::Changes {
            self.ensure_changes(mono, window, cx);
        } else {
            self.pause_changes(cx);
        }
        let body = match self.overview_tab {
            OverviewTab::Overview => self.dashboard(mono, window, cx),
            OverviewTab::Sessions => self.session_list(mono, cx),
            OverviewTab::Habits => self.habits_list(mono, cx),
            OverviewTab::Documents => self.document_list(mono, cx),
            OverviewTab::Changes => self.changes_list(mono, cx),
            OverviewTab::Activity => self.activity_view(mono, cx),
        };
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(body)
            .into_any_element()
    }
    fn session_list(&mut self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let filter = self.field(
            "mono-session-search",
            "Search sub-agents…",
            String::new(),
            false,
            cx,
        );
        let query = filter.read(cx).content.to_lowercase();
        let views = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).mono_session_views(cx))
            .unwrap_or_default();
        let mut launches: Vec<_> = mono
            .launches
            .iter()
            .rev()
            .filter(|l| {
                self.session_filters
                    .matches(l, views.iter().find(|v| v.id == l.session_id))
                    && format!(
                        "{} {} {}",
                        views
                            .iter()
                            .find(|v| v.id == l.session_id)
                            .map(|v| v.title.as_str())
                            .unwrap_or(&l.title),
                        views
                            .iter()
                            .find(|v| v.id == l.session_id)
                            .map(|v| v.cwd.as_str())
                            .unwrap_or(&l.cwd),
                        l.harness
                    )
                    .to_lowercase()
                    .contains(&query)
            })
            .cloned()
            .collect();
        // Most recent activity first: running or waiting sessions, then by
        // last update. Launch order alone puts re-attached old sessions on top.
        launches.sort_by_cached_key(|l| {
            std::cmp::Reverse(
                views
                    .iter()
                    .find(|v| v.id == l.session_id)
                    .map(|v| (v.busy || v.needs_input, v.updated_at)),
            )
        });
        let search_bar = search_bar(filter).child(
            icon_button("mono-session-filter", "Filter sub-agents", "filter")
                .relative()
                .child(self.session_filter_anchor.probe())
                .on_click(cx.listener(|this, _, window, cx| this.filter_menu(window, cx))),
        );
        let no_matches = launches.is_empty() && !mono.launches.is_empty();
        div().size_full().flex().flex_col()
            .when(!mono.launches.is_empty(),|d|d.child(search_bar))
            .child(div().id("mono-session-list").debug_selector(|| "mono-session-list".into()).flex_1().min_h_0().overflow_y_scroll().p_2().flex().flex_col().gap_1p5()
                .when(mono.launches.is_empty(),|d|d.child(empty_state("message-multiple",&format!("Ask {} to hand work to a sub-agent on one of your projects, like a bug fix or a review, and it shows up here.",mono.label()))))
                .when(no_matches,|d|d.child(empty_state("search-01",if query.trim().is_empty(){"No sub-agents match these filters."}else{"No sub-agents match this search."})))
                .children(launches.into_iter().map(|launch|{
                    let id = launch.session_id;
                    let state = views.iter().find(|v| v.id == id);
                    let title = state.map(|s| s.title.clone()).unwrap_or(launch.title.clone());
                    let cwd = state.map(|s| &s.cwd).unwrap_or(&launch.cwd);
                    let project = mono.projects.iter().find(|p| monocode_core::files::path_key(p) == monocode_core::files::path_key(cwd)).unwrap_or(cwd);
                    let project = std::path::Path::new(project).file_name().map_or(project.clone(), |n| n.to_string_lossy().to_string());
                    let branch = state.and_then(|s| s.branch.clone());
                    sub_agent_card(SubAgent {
                        id,
                        title,
                        harness: &launch.harness,
                        model: state.map(|s| s.model.clone()).filter(|m| !m.is_empty()),
                        model_id: launch.model.as_deref(),
                        place: match &branch { Some(b) => format!("{project} · {b}"), None => project },
                        branch: branch.is_some(),
                        state: match state {
                            Some(s) if s.needs_input => AgentState::NeedsInput,
                            Some(s) if s.busy => AgentState::Working,
                            Some(s) => AgentState::Idle(s.updated_at),
                            None => AgentState::Unavailable,
                        },
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.route = Some(Route::Session(id));
                        cx.notify();
                    }))
}))).into_any_element()
    }
    fn document_list(&mut self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let filter = self.field(
            "mono-document-search",
            "Search documents…",
            String::new(),
            false,
            cx,
        );
        let query = filter.read(cx).content.to_string();
        let documents: Vec<_> = mono
            .documents
            .iter()
            .rev()
            .filter(|d| matches_query(&query, &format!("{} {}", d.title, d.summary)))
            .collect();
        let list = div()
            .id("mono-document-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_1p5()
            .when(mono.documents.is_empty(), |d| {
                d.child(empty_state(
                    "file",
                    &format!(
                        "Ask {} to write something up, like a plan or a summary, and it shows up here.",
                        mono.label()
                    ),
                ))
            })
            .when(documents.is_empty() && !mono.documents.is_empty(), |d| {
                d.child(empty_state("search-01", "No documents match this search."))
            })
            .children(documents.into_iter().map(|document| {
                // MonoOverview's DocumentRow: laid out like a habit.
                let id = document.id.clone();
                let visual = matches!(document.kind.as_str(), "html" | "visual");
                let label = if visual { "Visual" } else { "Document" };
                outlined(div().id(SharedString::from(format!("mono-document-{id}"))))
                    .debug_selector({
                        let id = id.clone();
                        move || format!("mono-document-{id}")
                    })
                    .role(Role::Button)
                    .aria_label(format!("Open {} {}", label.to_lowercase(), document.title))
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .flex_shrink_0()
                    .px(px(10.))
                    .py_2()
                    .child(
                        div()
                            .size(px(28.))
                            .flex_shrink_0()
                            .rounded_md()
                            .bg(content(0.06))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                icon(if visual { "chart-square" } else { "file" }, 14.)
                                .text_color(content(0.6)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .line_height(px(20.))
                                    .text_color(content(0.85))
                                    .child(document.title.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.))
                                    .line_height(px(16.))
                                    .text_color(content(0.4))
                                    .child(if document.summary.trim().is_empty() {
                                        label.to_string()
                                    } else {
                                        document.summary.clone()
                                    }),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_document(&id, cx);
                        this.route = Some(Route::Document(id.clone()));
                        cx.notify();
                    }))
            }));
        div()
            .size_full()
            .flex()
            .flex_col()
            .when(!mono.documents.is_empty(), |d| d.child(search_bar(filter)))
            .child(list)
            .into_any_element()
    }
    pub(super) fn page_header(&self, title: &str, cx: &mut Context<Self>) -> Div {
        div()
            .h(px(40.))
            .flex_shrink_0()
            .min_w_0()
            .overflow_hidden()
            .border_b_1()
            .border_color(content(0.1))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .child(
                icon_button("mono-page-back", "Back", "chevron-left")
                    .debug_selector(|| "mono-page-back".into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.route = None;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(title.to_string()),
            )
    }
    pub(super) fn route_page(
        &mut self,
        mono: &Mono,
        route: Route,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let interactive = self.route.as_ref() == Some(&route);
        match route {
            Route::Review => div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.page_header("Changes", cx))
                .when_some(self.review.clone(), |d, review| {
                    d.child(div().flex_1().min_h_0().child(review))
                })
                .into_any_element(),
            Route::Session(id) => {
                let view = self.owner.upgrade().and_then(|owner| {
                    owner.update(cx, |owner, cx| owner.mono_ensure_session(id, cx))
                });
                if let Some(view) = &view {
                    view.update(cx, |v, cx| v.load(cx));
                }
                let title = view
                    .as_ref()
                    .map(|v| v.read(cx).summary.title.clone())
                    .unwrap_or_else(|| "Session unavailable".into());
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        self.page_header(&title, cx).child(
                            icon_button(
                                "mono-open-session",
                                "Open session in workspace",
                                "external-link",
                            )
                            .on_click(
                                cx.listener(move |this, _, w, cx| this.open_workspace(id, w, cx)),
                            ),
                        ),
                    )
                    .when_some(view, |d, view| {
                        d.child(div().flex_1().min_h_0().child(view))
                    })
                    .into_any_element()
            }
            Route::Document(id) => {
                let Some(document) = self
                    .work
                    .documents
                    .get(&id)
                    .or_else(|| {
                        mono.documents.iter().find(|d| {
                            d.id == id
                                && (d.kind == "visual" || !mono.desktop_document_ids.contains(&id))
                        })
                    })
                    .cloned()
                else {
                    return empty(if self.work.loading_documents.contains(&id) {
                        "Loading document…"
                    } else {
                        "Document unavailable"
                    });
                };
                let view: AnyElement = if document.kind == "html"
                    && (self.settings.is_some()
                        || self.menu.is_some()
                        || self.picker.is_some()
                        || self.background.is_some())
                {
                    div().into_any_element()
                } else if document.kind == "html" {
                    let html = self
                        .html_documents
                        .entry(id.clone())
                        .or_insert_with(|| {
                            cx.new(|_| super::html::Html::new(document.body.clone()))
                        })
                        .clone();
                    html.update(cx, |html, _| html.set_interactive(interactive));
                    html.into_any_element()
                } else if document.kind == "visual" {
                    // The same native dither visual the chat shows, on its own page.
                    let visual = match self.visual_documents.get(&id) {
                        Some(view) => Some(view.clone()),
                        None => serde_json::from_str(&document.body).ok().map(|visual| {
                            // The page header already titles it.
                            let view = cx.new(|cx| {
                                let mut view = crate::visuals::VisualView::new(visual, cx);
                                view.set_inline(Some(String::new()), cx);
                                view
                            });
                            self.visual_documents.insert(id.clone(), view.clone());
                            view
                        }),
                    };
                    match visual {
                        Some(view) => div()
                            .id("mono-visual-document")
                            .size_full()
                            .overflow_y_scroll()
                            .px_4()
                            .py_3()
                            .child(view)
                            .into_any_element(),
                        None => empty("Visual unavailable"),
                    }
                } else {
                    self.documents
                        .entry(id.clone())
                        .or_insert_with(|| {
                            cx.new(|cx| {
                                crate::transcript::TranscriptView::document(
                                    document.body.clone(),
                                    cx,
                                )
                            })
                        })
                        .clone()
                        .into_any_element()
                };
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(self.page_header(&document.title, cx).when(
                        document.kind != "visual",
                        |header| {
                            header.child(
                                icon_button("mono-document-copy", "Copy document", "response-copy")
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            document.body.clone(),
                                        ))
                                    }),
                            )
                        },
                    ))
                    .child(div().flex_1().min_h_0().child(view))
                    .into_any_element()
            }
            Route::Habit(id) => self.habit_page(mono, Some(&id), window, cx),
            Route::NewHabit => self.habit_page(mono, None, window, cx),
        }
    }
    fn activity_view(&mut self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let Some(chat) = self.chat() else {
            return empty("Loading…");
        };
        // Response actions number turns across every loaded page.
        let source = chat.read(cx).transcript.read(cx).history_rows();
        let Some(start) = self.activity_turn.filter(|i| *i < source.len()) else {
            return empty_state(
                "list-bullet",
                &format!(
                    "Select the activity on one of {}’s messages to see each step it took.",
                    mono.label()
                ),
            );
        };
        let end = source
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, r)| r.is_prompt() && r.first)
            .map(|(i, _)| i)
            .unwrap_or(source.len());
        let rows = source[start..end]
            .iter()
            .filter(|r| matches!(r.role.as_str(), "tool" | "reasoning"))
            .cloned()
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return empty_state("list-bullet", "No activity in this turn yet.");
        }
        let key = mono::files::content_hash(&format!("{rows:?}"));
        if self.activity.as_ref().is_none_or(|(old, _)| old != &key) {
            self.activity = Some((
                key,
                cx.new(|cx| crate::transcript::TranscriptView::activity(rows, cx)),
            ));
        }
        div()
            .size_full()
            .child(self.activity.as_ref().unwrap().1.clone())
            .into_any_element()
    }
    pub(super) fn settings_modal(
        &mut self,
        mono: &Mono,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let section = self.settings?;
        let size = window.viewport_size();
        let width = 880f32.min(size.width.as_f32() - 24.);
        let height = 680f32.min(size.height.as_f32() - 32.);
        let body = match section {
            SettingsSection::General => self.general_settings(mono, window, cx),
            SettingsSection::Soul => self.soul_page(mono, cx),
            SettingsSection::Memory => self.memory_page(mono, window, cx),
        };
        self.bind_settings_blurs(window, cx);
        let weak = cx.weak_entity();
        Some(
            div()
                .absolute()
                .size_full()
                .child(
                    gpui::deferred(
                        gpui::anchored()
                            .position(gpui::point(px(0.), px(0.)))
                            .child(
                                div()
                                    .w(size.width)
                                    .h(size.height)
                                    .bg(gpui::black().opacity(0.4))
                                    .occlude(),
                            ),
                    )
                    .with_priority(69),
                )
                .child(
                    popover(
                        "mono-settings-modal",
                        Anchor::Point(gpui::point(
                            px((size.width.as_f32() - width) / 2.),
                            px((size.height.as_f32() - height) / 2.),
                        )),
                    )
                    .ignore(self.notice_region.clone())
                    .modal_surface(true)
                    .without_animation()
                    .priority(70)
                    .width(width)
                    .height(height)
                    .gap(0.)
                    .on_dismiss(move |_, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.save_open_file(cx);
                            this.settings = None;
                            this.picker = None;
                            cx.notify();
                        });
                    })
                    .child(
                        div()
                            .id("mono-settings-dialog")
                            .debug_selector(|| "mono-settings-dialog".into())
                            .role(Role::Dialog)
                            .track_focus(&self.settings_focus)
                            .aria_label(format!("{} settings", mono.label()))
                            .h_full()
                            .w_full()
                            .flex()
                            .relative()
                            .child(
                                div()
                                    .id("mono-settings-navigation")
                                    .debug_selector(|| "mono-settings-navigation".into())
                                    .role(Role::Navigation)
                                    .aria_label("Settings sections")
                                    .w(px(192.))
                                    .flex_shrink_0()
                                    .h_full()
                                    .border_r_1()
                                    .border_color(content(0.04))
                                    .p_2()
                                    .flex()
                                    .flex_col()
                                    .gap_px()
                                    .child(
                                        div()
                                            .px_2p5()
                                            .pt_1p5()
                                            .pb_2()
                                            .text_size(px(11.))
                                            .text_color(content(0.45))
                                            .child("Settings"),
                                    )
                                    .children(
                                        [
                                            (SettingsSection::General, "General", "settings"),
                                            (SettingsSection::Soul, "Soul", "sparkles"),
                                            (SettingsSection::Memory, "Memory", "sticky-note"),
                                        ]
                                        .into_iter()
                                        .map(
                                            |(tab, label, glyph)| {
                                                let row_id = format!("mono-settings-{label}");
                                                div()
                                                    .id(SharedString::from(row_id.clone()))
                                                    .debug_selector(move || row_id.clone())
                                                    .role(Role::Button)
                                                    .aria_label(label)
                                                    .w_full()
                                                    .h(px(32.))
                                                    .flex_shrink_0()
                                                    .flex()
                                                    .flex_row()
                                                    .items_center()
                                                    .justify_start()
                                                    .text_align(gpui::TextAlign::Left)
                                                    .gap(px(10.))
                                                    .px(px(10.))
                                                    .py(px(6.))
                                                    .rounded_lg()
                                                    .text_size(px(13.))
                                                    .line_height(px(20.))
                                                    .text_color(content(if section == tab {
                                                        1.
                                                    } else {
                                                        0.65
                                                    }))
                                                    .when(section == tab, |d| d.bg(content(0.08)))
                                                    .when(section != tab, |d| {
                                                        d.hover(|d| {
                                                            d.bg(content(0.05))
                                                                .text_color(content(1.))
                                                        })
                                                    })
                                                    .child(
                                                        div()
                                                            .id(SharedString::from(format!(
                                                                "mono-settings-icon-{label}"
                                                            )))
                                                            .debug_selector(move || {
                                                                format!(
                                                                    "mono-settings-icon-{label}"
                                                                )
                                                            })
                                                            .size(px(14.))
                                                            .flex_shrink_0()
                                                            .child(icon(glyph, 14.).text_color(
                                                                content(if section == tab {
                                                                    1.
                                                                } else {
                                                                    0.65
                                                                }),
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .id(SharedString::from(format!(
                                                                "mono-settings-label-{label}"
                                                            )))
                                                            .debug_selector(move || {
                                                                format!(
                                                                    "mono-settings-label-{label}"
                                                                )
                                                            })
                                                            .child(label),
                                                    )
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.save_open_file(cx);
                                                            this.settings = Some(tab);
                                                            window.focus(&this.settings_focus, cx);
                                                            cx.notify();
                                                        },
                                                    ))
                                            },
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .flex()
                                    .flex_col()
                                    .child(body),
                            )
                            .child(
                                div().absolute().right_3().top_1p5().child(
                                    icon_button(
                                        "mono-settings-close",
                                        "Close Mono settings",
                                        "close",
                                    )
                                    .size(px(28.))
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.save_open_file(cx);
                                            this.settings = None;
                                            cx.notify();
                                        },
                                    )),
                                ),
                            ),
                    ),
                )
                .into_any_element(),
        )
    }
    fn general_settings(
        &mut self,
        mono: &Mono,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = self.field("mono-name", "Name", mono.name.clone(), false, cx);
        name.update(cx, |f, cx| {
            f.set_font_size(15.);
            f.set_line_height(20.);
            f.set_centered(true);
            f.set_placeholder(mono.label(), cx);
        });
        let look = mono.id.clone();
        let add = mono.id.clone();
        let reset = mono.id.clone();
        let chat = self.chat();
        let model = chat
            .as_ref()
            .map(|v| v.read(cx).mono_model_label())
            .unwrap_or_else(|| "Loading…".into());
        let provider = chat
            .as_ref()
            .map(|v| v.read(cx).summary.provider.clone())
            .unwrap_or_else(|| "codex".into());
        let mode = chat
            .as_ref()
            .map(|v| v.read(cx).settings.access)
            .unwrap_or(AccessMode::Auto);
        let model_options = chat
            .as_ref()
            .map(|v| v.read(cx).mono_model_options())
            .unwrap_or_default();
        let access_icon = match mode {
            AccessMode::FullAccess => "shield-alert",
            AccessMode::Auto => "sparkles",
            AccessMode::AutoAcceptEdits => "pencil-edit",
            _ => "square-lock",
        };
        let plain = |id, label: SharedString| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label.clone())
                .h(px(28.))
                .px_1p5()
                .mx(px(-6.))
                .rounded_md()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(12.))
                .text_color(content(0.85))
                .hover(|s| s.bg(content(0.06)))
                .child(label)
        };
        div().id("mono-general-settings").debug_selector(||"mono-general-settings".into())
            .flex_1().min_h_0().overflow_y_scroll().px_8().pt_6().pb_8()
            .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child("General"))
            .child(div().flex().flex_col().items_center().pt_4().pb_6()
                .child(div().id("mono-look").debug_selector(||"mono-look".into()).role(Role::Button)
                    .aria_label("Change mascot and color").relative().size(px(80.)).flex_shrink_0()
                    .flex().items_center().justify_center().rounded_2xl().border_1().border_color(content(0.04))
                    .bg(content(0.04)).hover(|s|s.bg(content(0.08)))
                    .child(self.settings_anchor.probe())
                    .child(avatar::avatar(&mono.mascot, mono.color, "idle", 48., false))
                    .on_click(cx.listener(move |this, _, w, cx|this.open_settings_look(&look,w,cx))))
                .child(div().id("mono-name-field").debug_selector(||"mono-name-field".into()).mt_3().h(px(32.)).w(px(256.)).max_w_full().px_2().py_1p5()
                    .rounded_md().font_weight(FontWeight::MEDIUM).hover(|s|s.bg(content(0.05))).child(name)))
            .child(heading("Model", None))
            .child(card()
                .child(setting("Model",Some("What it thinks and replies with."),
                    div().id("mono-settings-model").role(Role::Button).aria_label(format!("Choose model, {model}"))
                        .h(px(28.)).px_1p5().mx(px(-6.)).rounded_md().flex().items_center().gap_2()
                        .text_size(px(12.)).text_color(content(0.85)).hover(|s|s.bg(content(0.06)))
                        .child(self.model_anchor.probe()).child(provider_icon(&provider,14.)).child(model)
                        .on_click(cx.listener(|this,_,w,cx|this.open_model_picker(w,cx))).into_any_element()))
                .children(model_options.into_iter().filter_map(|item| {
                    let MenuItem::Action(option)=item else { return None; };
                    let label=option.label.clone();
                    let value=option.value.clone().unwrap_or_else(||if option.checked {"On"}else{"Off"}.into());
                    let anchor=AnchorSlot::default(); let probe=anchor.probe();
                    Some(setting(&label,None,
                        plain(SharedString::from(format!("mono-option-{}",option.id)),value).child(probe)
                            .on_click(cx.listener(move |this,_,w,cx| {
                                if option.submenu.is_empty() {
                                    if let Some(chat)=this.chat() && let Err(e)=chat.update(cx,|v,cx|v.mono_pick_model_option(&option.id,cx)) { this.error=Some(e); }
                                    this.changed(cx);
                                } else { this.open_model_option(option.label.clone(),option.submenu.clone(),anchor.clone(),w,cx); }
                            })).into_any_element()).border_t_1().border_color(content(0.04)))
                }))
                .child(setting("Permissions",Some("What it may do without asking."),
                    div().id("mono-settings-access").role(Role::Button).aria_label(format!("Permissions, {}",mode.label()))
                        .h(px(28.)).max_w(px(208.)).px_1p5().mx(px(-6.)).rounded_md().flex().items_center().gap_2()
                        .text_size(px(12.)).text_color(content(0.85)).hover(|s|s.bg(content(0.06)))
                        .child(self.access_anchor.probe()).child(icon(access_icon,14.).when(mode==AccessMode::FullAccess,|i|i.text_color(rgb(0xffb900))))
                        .child(mode.label()).child(icon("chevron-down",12.))
                        .on_click(cx.listener(|this,_,w,cx|this.open_access_picker(w,cx))).into_any_element())
                    .border_t_1().border_color(content(0.04))))
            .child(heading("Projects",Some("It can see every session, note and folder in these.")))
            .child(card()
                .children(mono.projects.iter().enumerate().map(|(ix,project)| {
                    let remove=project.clone();
                    let appearance=self.owner.upgrade().and_then(|o|o.read(cx).workspace.project_appearance.get(project).cloned()).unwrap_or_default();
                    let label=appearance.label(project);
                    let pretty=std::env::var("HOME").ok().and_then(|home|project.strip_prefix(&home).map(|rest|format!("~{rest}"))).unwrap_or_else(||project.clone());
                    let group:SharedString=format!("mono-project-{ix}").into();
                    div().id(("mono-project",ix)).group(group.clone()).min_h(px(48.)).flex().items_center().gap_3().px_3().py_2()
                        .when(ix>0,|d|d.border_t_1().border_color(content(0.04)))
                        .child(div().size(px(32.)).flex_shrink_0().rounded_lg().bg(content(0.05)).flex().items_center().justify_center()
                            .child(crate::project_settings::project_icon(&appearance,project,16.)))
                        .child(div().flex_1().min_w_0()
                            .child(div().text_size(px(13.)).line_height(px(20.)).text_color(content(0.9)).truncate().child(label))
                            .child(div().text_size(px(11.)).line_height(px(16.)).text_color(content(0.4)).truncate().child(pretty)))
                        .child(icon_button("mono-remove-project","Remove project","close").opacity(0.).group_hover(group,|s|s.opacity(1.))
                            .on_click(cx.listener(move |this,_,_,cx| { if let Some(id)=this.active.clone() {
                                if let Some(m)=this.roster.get_mut(&id) {m.projects.retain(|p|p!=&remove);}
                                this.configure_chat(&id,cx);this.save(cx);
                            }})))
                }))
                .child(div().id("mono-add-project").role(Role::Button).aria_label("Add project").h(px(44.)).px_3().flex().items_center().gap_3()
                    .text_size(px(13.)).text_color(content(0.55)).hover(|s|s.bg(content(0.04)).text_color(content(1.)))
                    .when(!mono.projects.is_empty(),|d|d.border_t_1().border_color(content(0.04)))
                    .child(self.projects_anchor.probe())
                    .child(div().size(px(32.)).rounded_lg().border_1().border_color(content(0.15)).flex().items_center().justify_center().child(icon("plus",14.)))
                    .child("Add project").on_click(cx.listener(move |this,_,w,cx|this.pick_project(&add,w,cx)))))
            .child(heading("Sessions it starts",None))
            .child(card()
                .child(switch_setting("Show sessions in sidebar","Sessions it starts appear in your project sidebar. Hidden ones stay saved and open from its overview.",false,
                    self.switch("mono-show-started",mono.show_started_sessions_in_sidebar,false,|m,on|m.show_started_sessions_in_sidebar=on,cx)))
                .child(switch_setting("Group in a folder",&format!("Sessions it starts go in a “{}” folder in your project sidebar.",mono.label()),!mono.show_started_sessions_in_sidebar,
                    self.switch("mono-group-started",mono.show_started_sessions_in_sidebar&&mono.use_sidebar_folders,!mono.show_started_sessions_in_sidebar,|m,on|m.use_sidebar_folders=on,cx))
                    .border_t_1().border_color(content(0.04))))
            .child(heading("Heartbeat",Some("When it has been quiet for a while, it looks over its recent work, sessions you left, your notes and inbox, and suggests what to do next. It never acts without asking.")))
            .child(card()
                .child(switch_setting("Check in on its own","Quiet check-ins leave nothing behind; you only hear from it when it has an idea.",false,
                    self.switch("mono-heartbeat",mono.heartbeat_minutes>0,false,|m,on|m.heartbeat_minutes=if on {heartbeat::DEFAULT_MINUTES} else {0},cx)))
                .child(setting("After it has been quiet for",None,self.heartbeat_intervals(mono,cx))
                    .when(mono.heartbeat_minutes==0,|d|d.opacity(0.4)).border_t_1().border_color(content(0.04))))
            .child(heading("Conversation",None))
            .child(card().child(setting("Reset conversation",Some("Clear all messages and start fresh. Its soul, memory and habits stay."),
                div().id("mono-reset").role(Role::Button).aria_label("Reset").flex().items_center().px_3().py_1p5().rounded_md().text_size(px(12.)).border_1().border_color(content(0.04)).text_color(rgb(0xff6467)).hover(|s|s.bg(rgb(0xfb2c36).opacity(0.1))).child("Reset")
                    .on_click(cx.listener(move |this,_,w,cx|this.reset_conversation(&reset,w,cx))).into_any_element())))
            .into_any_element()
    }
    fn heartbeat_intervals(&self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let enabled = mono.heartbeat_minutes > 0;
        div()
            .flex()
            .gap_1()
            .children(heartbeat::INTERVALS.iter().map(|&minutes| {
                let selected = enabled && mono.heartbeat_minutes == minutes;
                let label = if minutes < 60 {
                    format!("{minutes}m")
                } else {
                    format!("{}h", minutes / 60)
                };
                div()
                    .id(("mono-heartbeat-interval", minutes as usize))
                    .role(Role::Button)
                    .aria_label(label.clone())
                    .aria_selected(selected)
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .text_size(px(12.))
                    .border_1()
                    .border_color(content(if selected { 0.2 } else { 0.06 }))
                    .when(selected, |d| d.bg(content(0.08)).text_color(content(1.)))
                    .when(!selected, |d| d.text_color(content(0.55)))
                    .when(enabled, |d| d.hover(|s| s.bg(content(0.05))))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(id) = this.active.clone()
                            && let Some(m) = this.roster.get_mut(&id)
                            && m.heartbeat_minutes > 0
                        {
                            m.heartbeat_minutes = minutes;
                            this.save(cx);
                            cx.notify();
                        }
                    }))
            }))
            .into_any_element()
    }
    fn switch(
        &self,
        id: &'static str,
        on: bool,
        disabled: bool,
        change: fn(&mut Mono, bool),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .role(Role::Switch)
            .aria_label(id)
            .aria_selected(on)
            .w(px(36.))
            .h(px(20.))
            .flex_shrink_0()
            .rounded_full()
            .bg(if on { accent_color() } else { content(0.2) })
            .child(
                div()
                    .size(px(16.))
                    .mt(px(2.))
                    .ml(px(if on { 18. } else { 2. }))
                    .rounded_full()
                    .bg(gpui::white()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if disabled {
                    return;
                }
                if let Some(id) = this.active.clone() {
                    if let Some(m) = this.roster.get_mut(&id) {
                        change(m, !on);
                    }
                    this.configure_chat(&id, cx);
                    this.save(cx);
                }
            }))
            .into_any_element()
    }
    /// MonoMenu, beside the row's options button or at a right-click.
    pub(super) fn open_options(
        &mut self,
        id: &str,
        at: Option<gpui::Point<gpui::Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mono) = self.roster.get(id).cloned() else {
            return;
        };
        let editor = cx.new(|cx| LookEditor::new(mono, cx));
        let focus = editor.read(cx).name.focus_handle(cx);
        let changed = cx.subscribe(&editor, |this, _, event: &LookChanged, cx| {
            if let Some(mono) = this.roster.get_mut(&event.mono.id) {
                mono.name = event.mono.name.clone();
                mono.mascot = event.mono.mascot.clone();
                mono.color = event.mono.color;
            }
            this.configure_chat(&event.mono.id, cx);
            this.save(cx);
        });
        let submitted = cx.subscribe(&editor, |this, _, _: &LookSubmitted, cx| {
            this.menu = None;
            cx.notify();
        });
        let menu = cx.new(|cx| {
            Menu::new(
                "Mono options",
                vec![
                    MenuItem::Custom(AnyView::from(editor), focus.clone()),
                    MenuItem::Separator,
                    MenuAction::new("background", "Background image")
                        .icon("image-add")
                        .into(),
                    MenuAction::new("delete", "Delete mono…")
                        .icon("trash")
                        .danger()
                        .into(),
                ],
                window,
                cx,
            )
            .width(280.)
            .style(MenuStyle::Group)
        });
        let target = id.to_string();
        let picked = cx.subscribe_in(&menu, window, move |this, _, event: &MenuEvent, w, cx| {
            this.menu = None;
            if let MenuEvent::Picked(action, _) = event {
                match action.as_ref() {
                    "background" => this.open_background(&target, w, cx),
                    "delete" => this.delete_mono(&target, w, cx),
                    _ => {}
                }
            }
            cx.notify();
        });
        self.menu_anchor = match at {
            Some(at) => Anchor::Point(at),
            None => self
                .rail_bounds
                .get(id)
                .cloned()
                .map(Anchor::Slot)
                .unwrap_or_else(|| Anchor::Slot(self.settings_anchor.clone())),
        };
        self.menu = Some((id.into(), menu, vec![changed, submitted, picked]));
        window.focus(&focus, cx);
        cx.notify();
    }
    pub(super) fn options_surface(&self, cx: &App) -> Option<AnyElement> {
        self.menu.as_ref().map(|(_, menu, _)| {
            menu_popover(menu, self.menu_anchor.clone(), cx)
                .without_animation()
                .side(Side::Right)
                .priority(90)
                .gap(0.)
                .into_any_element()
        })
    }
    pub(super) fn picker_surface(&self, cx: &App) -> Option<AnyElement> {
        self.picker.as_ref().map(|p| {
            menu_popover(&p.menu, p.anchor.clone(), cx)
                .align(p.align)
                .without_animation()
                .priority(90)
                .width(p.width)
                .into_any_element()
        })
    }
    fn open_model_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat() else {
            return;
        };
        let view = chat.read(cx);
        let provider = Kind::from_id(&view.summary.provider);
        let model = view.mono_model();
        let project = view.summary.cwd.clone();
        let account = view.settings.account.clone();
        let list = cx.new(|cx| ModelList::new_for(provider, model, &project, account, cx));
        let focus = list.read(cx).search_focus(cx);
        let picked = cx.subscribe(&list, |this, _, picked: &ModelPicked, cx| {
            if let Some(view) = this.chat()
                && let Err(e) = view.update(cx, |v, cx| {
                    v.mono_config(picked.0, picked.1.clone(), HashMap::new(), cx)
                })
            {
                this.error = Some(e);
            }
            this.picker = None;
            this.changed(cx);
        });
        let menu = cx.new(|cx| {
            Menu::new(
                "Model",
                vec![MenuItem::Custom(list.into(), focus)],
                window,
                cx,
            )
            .width(400.)
        });
        let closed = cx.subscribe(&menu, |this, _, _: &MenuEvent, cx| {
            this.picker = None;
            cx.notify();
        });
        self.picker = Some(OpenPicker {
            menu,
            anchor: Anchor::Slot(self.model_anchor.clone()),
            width: 400.,
            align: crate::popover::Align::Start,
            _events: vec![picked, closed],
        });
        cx.notify();
    }
    fn open_model_option(
        &mut self,
        label: SharedString,
        choices: Vec<MenuAction>,
        anchor: AnchorSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = cx.new(|cx| {
            Menu::new(
                label,
                choices.into_iter().map(Into::into).collect(),
                window,
                cx,
            )
        });
        let picked = cx.subscribe(&menu, |this, _, event: &MenuEvent, cx| {
            if let MenuEvent::Picked(id, _) = event
                && let Some(chat) = this.chat()
                && let Err(e) = chat.update(cx, |v, cx| v.mono_pick_model_option(id, cx))
            {
                this.error = Some(e);
            }
            this.picker = None;
            this.changed(cx);
        });
        self.picker = Some(OpenPicker {
            menu,
            anchor: Anchor::Slot(anchor),
            width: 228.,
            align: crate::popover::Align::Start,
            _events: vec![picked],
        });
        cx.notify();
    }
    fn open_access_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let options = self
            .chat()
            .map(|chat| chat.read(cx).mono_access_options())
            .unwrap_or_default();
        let menu = cx.new(|cx| {
            Menu::new("Permissions", options, window, cx)
                .style(MenuStyle::Access)
                .width(288.)
        });
        let picked = cx.subscribe(&menu, |this, _, event: &MenuEvent, cx| {
            if let MenuEvent::Picked(id, _) = event
                && let Some(view) = this.chat()
            {
                view.update(cx, |v, cx| {
                    v.mono_access(
                        AccessMode::from_id(id.strip_prefix("access:").unwrap_or(id)),
                        cx,
                    )
                });
            }
            this.picker = None;
            cx.notify();
        });
        self.picker = Some(OpenPicker {
            menu,
            anchor: Anchor::Slot(self.access_anchor.clone()),
            width: 288.,
            align: crate::popover::Align::Start,
            _events: vec![picked],
        });
        cx.notify();
    }
    fn pick_project(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let projects = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).workspace.projects.clone())
            .unwrap_or_default();
        let mono = id.to_string();
        let paths = projects.clone();
        let mut items: Vec<MenuItem> = projects
            .into_iter()
            .enumerate()
            .map(|(ix, path)| MenuAction::new(format!("project:{ix}"), path).into())
            .collect();
        items.push(MenuItem::Separator);
        items.push(MenuAction::new("browse", "Open project…").into());
        let menu = cx.new(|cx| Menu::new("Add project", items, window, cx).width(300.));
        let picked = cx.subscribe(&menu, move |this, _, event: &MenuEvent, cx| {
            this.picker = None;
            if let MenuEvent::Picked(action, _) = event {
                if action == "browse" {
                    this.browse_project(&mono, cx);
                } else if let Some(path) = action
                    .strip_prefix("project:")
                    .and_then(|n| n.parse::<usize>().ok())
                    .and_then(|ix| paths.get(ix))
                {
                    this.add_project(&mono, path.clone(), cx);
                }
            }
            cx.notify();
        });
        self.picker = Some(OpenPicker {
            menu,
            anchor: Anchor::Slot(self.projects_anchor.clone()),
            width: 300.,
            align: crate::popover::Align::Start,
            _events: vec![picked],
        });
        cx.notify();
    }
    fn add_project(&mut self, id: &str, path: String, cx: &mut Context<Self>) {
        if let Some(m) = self.roster.get_mut(id) {
            m.projects.push(path);
            m.normalize();
        }
        self.configure_chat(id, cx);
        self.save(cx);
    }
    fn browse_project(&mut self, id: &str, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add project".into()),
        });
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    this.add_project(&id, path.to_string_lossy().into(), cx)
                });
            }
        })
        .detach();
    }
    fn open_background(&mut self, id: &str, window: &Window, cx: &mut Context<Self>) {
        let Some(mono) = self.roster.get(id).cloned() else {
            return;
        };
        let view = cx.new(|cx| {
            BackgroundDialog::new(
                format!("mono:{id}"),
                ProjectAppearance {
                    name: mono.label(),
                    color: Some(mono.color),
                    mascot: Some(mono.mascot),
                    background: mono.background,
                    ..Default::default()
                },
                cx,
            )
        });
        let id = id.to_string();
        let events = cx.subscribe(
            &view,
            move |this, _, event: &BackgroundEvent, cx| match event {
                BackgroundEvent::Changed(appearance) => {
                    if let Some(mono) = this.roster.get_mut(&id) {
                        mono.background = appearance.background.clone();
                    }
                    this.save(cx);
                }
                BackgroundEvent::Close => {
                    this.background = None;
                    cx.notify();
                }
            },
        );
        let width = window.viewport_size().width.as_f32();
        let at = gpui::point(px((width - 640.) / 2.), px(50.));
        self.background = Some((view, events, at));
        cx.notify();
    }
    pub(super) fn background_modal(&self) -> Option<AnyElement> {
        let (view, _, at) = self.background.as_ref()?;
        Some(
            popover("mono-background", Anchor::Point(*at))
                .modal_surface(false)
                .without_animation()
                .priority(100)
                .width(640.)
                .child(view.clone())
                .into_any_element(),
        )
    }
    pub(crate) fn finish_reorder(&mut self, cx: &mut Context<Self>) {
        if let Some((id, _, Some(over))) = self.drag.take()
            && self.skip_click
        {
            let mut ids = self.rail_ids.clone();
            if let Some((from, to)) = ids
                .iter()
                .position(|s| s == &id)
                .zip(ids.iter().position(|s| s == &over))
            {
                let id = ids.remove(from);
                ids.insert(to, id);
                self.roster.reorder(&ids);
                self.save(cx);
            }
        }
    }
}
