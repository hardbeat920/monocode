//! React's command names, defaults, scopes, filter and shortcut recorder.
use crate::{
    settings::{Preferences, SettingsView},
    theme::*,
};
use gpui::{AnyElement, Context, FontWeight, Role, div, prelude::*, px};
#[derive(Clone, Copy)]
pub(crate) struct Binding {
    pub command: &'static str,
    pub keys: &'static str,
    pub when: &'static str,
    pub fixed: bool,
}
pub(crate) const BINDINGS: &[Binding] = &[
    Binding {
        command: "App: Settings",
        keys: "⌘,",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "App: Search",
        keys: "⌘K",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "App: Go to File",
        keys: "⌘P",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "App: Command Palette",
        keys: "⌘⇧P",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "App: Find in Files",
        keys: "⌘⇧F",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "App: Open Project",
        keys: "⌘O",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "App: New Window",
        keys: "⌘⇧N",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "App: Quick Composer",
        keys: "⌘⇧Space",
        when: "Anywhere",
        fixed: false,
    },
    Binding {
        command: "App: Toggle Sidebar",
        keys: "⌘B",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "App: Toggle Session Sidebar",
        keys: "⌘⇧B",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "App: Switch Model",
        keys: "⌘.",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "App: Toggle Mono",
        keys: "⌘I",
        when: "Project with a mono",
        fixed: false,
    },
    Binding {
        command: "Composer: Toggle Workspace",
        keys: "⌘⇧G",
        when: "Draft session composer",
        fixed: false,
    },
    Binding {
        command: "View: Reload",
        keys: "⌘⇧R",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "View: Zoom In",
        keys: "⌘+",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "View: Zoom Out",
        keys: "⌘-",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "View: Reset Zoom",
        keys: "⌘0",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Tab: New",
        keys: "⌘T",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Tab: Close Others",
        keys: "⌘⌥T",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Tab: Close All",
        keys: "⌘⇧W",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Tab: Next",
        keys: "⌘⇧]",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Tab: Previous",
        keys: "⌘⇧[",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Tab: Cycle Next",
        keys: "⌃Tab",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Tab: Cycle Previous",
        keys: "⌃⇧Tab",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Tab: Back",
        keys: "⌘[",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Tab: Forward",
        keys: "⌘]",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Tab: Activate 1–8",
        keys: "⌘1 … ⌘8",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Tab: Activate Last",
        keys: "⌘9",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Session: Archive",
        keys: "⌘⇧A",
        when: "sessionFocus && !overlay",
        fixed: false,
    },
    Binding {
        command: "Session: Stop",
        keys: "Esc",
        when: "sessionBusy && !terminalFocus",
        fixed: true,
    },
    Binding {
        command: "Session: Previous",
        keys: "⌘⇧↑",
        when: "!overlay && (!textFocus || emptyComposer)",
        fixed: false,
    },
    Binding {
        command: "Session: Next",
        keys: "⌘⇧↓",
        when: "!overlay && (!textFocus || emptyComposer)",
        fixed: false,
    },
    Binding {
        command: "Session: Previous in Current Tab",
        keys: "⌘↑",
        when: "!overlay && (!textFocus || emptyComposer)",
        fixed: false,
    },
    Binding {
        command: "Session: Next in Current Tab",
        keys: "⌘↓",
        when: "!overlay && (!textFocus || emptyComposer)",
        fixed: false,
    },
    Binding {
        command: "Project: Previous",
        keys: "⌘⇧←",
        when: "!overlay && (!textFocus || emptyComposer)",
        fixed: false,
    },
    Binding {
        command: "Project: Next",
        keys: "⌘⇧→",
        when: "!overlay && (!textFocus || emptyComposer)",
        fixed: false,
    },
    Binding {
        command: "Project: Move Back",
        keys: "⌘⌥⇧←",
        when: "!overlay && (!textFocus || emptyComposer)",
        fixed: false,
    },
    Binding {
        command: "Project: Move Forward",
        keys: "⌘⌥⇧→",
        when: "!overlay && (!textFocus || emptyComposer)",
        fixed: false,
    },
    Binding {
        command: "Pane: Close",
        keys: "⌘W",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Pane: Split Right",
        keys: "⌘D",
        when: "!editorFocus",
        fixed: false,
    },
    Binding {
        command: "Pane: Split Down",
        keys: "⌘⇧D",
        when: "!editorFocus",
        fixed: false,
    },
    Binding {
        command: "Pane: Focus Left",
        keys: "⌘⌥←",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Pane: Focus Right",
        keys: "⌘⌥→",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Pane: Focus Up",
        keys: "⌘⌥↑",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Pane: Focus Down",
        keys: "⌘⌥↓",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Terminal: New",
        keys: "⌘`",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Terminal: New Tab",
        keys: "⌘⇧`",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Terminal: Toggle Dock",
        keys: "⌘J",
        when: "Always",
        fixed: false,
    },
    Binding {
        command: "Editor: Find",
        keys: "⌘F",
        when: "editorFocus",
        fixed: false,
    },
    Binding {
        command: "Editor: Replace",
        keys: "⌘⌥F",
        when: "editorFocus",
        fixed: false,
    },
];
pub(super) fn quick_composer_label(p: &Preferences) -> String {
    p.keybindings
        .get("App: Quick Composer")
        .and_then(|s| s.clone())
        .map(|s| shortcut_label(&s))
        .unwrap_or_else(|| "⌘⇧Space".into())
}
/// A command's shortcut as shown, honoring the user's rebinding; `None`
/// once they have disabled it.
pub(crate) fn command_shortcut(p: &Preferences, command: &str) -> Option<String> {
    match p.keybindings.get(command) {
        Some(Some(s)) => Some(shortcut_label(s)),
        Some(None) => None,
        None => {
            let keys = BINDINGS.iter().find(|r| r.command == command)?.keys;
            Some(if cfg!(target_os = "macos") {
                keys.into()
            } else {
                keys.replace('⌘', "Ctrl+")
                    .replace('⇧', "Shift+")
                    .replace('⌥', "Alt+")
                    .replace('⌃', "Ctrl+")
            })
        }
    }
}
pub(crate) fn shortcut_label(shortcut: &str) -> String {
    let parts: Vec<_> = shortcut.split('-').collect();
    let mut label = String::new();
    for part in parts {
        label.push_str(&match part {
            "cmd" => "⌘".into(),
            "ctrl" => if cfg!(target_os = "macos") {
                "⌃"
            } else {
                "Ctrl+"
            }
            .into(),
            "alt" => if cfg!(target_os = "macos") {
                "⌥"
            } else {
                "Alt+"
            }
            .into(),
            "shift" => if cfg!(target_os = "macos") {
                "⇧"
            } else {
                "Shift+"
            }
            .into(),
            "space" => "Space".into(),
            "up" => "↑".into(),
            "down" => "↓".into(),
            "left" => "←".into(),
            "right" => "→".into(),
            "tab" => "Tab".into(),
            key => key.to_uppercase(),
        });
    }
    label
}
impl SettingsView {
    pub(super) fn keybindings_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let query = self.keybinding_query.trim().to_lowercase();
        let rows: Vec<_> = BINDINGS
            .iter()
            .filter(|r| cfg!(target_os = "macos") || r.command != "App: Quick Composer")
            .filter(|r| {
                format!("{} {} {}", r.command, self.binding_label(r), r.when)
                    .to_lowercase()
                    .contains(&query)
            })
            .copied()
            .collect();
        let action = div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(content(0.4))
                    .child(format!(
                        "{} {}",
                        rows.len(),
                        if rows.len() == 1 {
                            "binding"
                        } else {
                            "bindings"
                        }
                    )),
            )
            .child(
                div()
                    .h(px(28.))
                    .w(px(176.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(content(0.1))
                    .px_2()
                    .child(icon("search-01", 14.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(self.keybinding_filter.clone()),
                    ),
            )
            .into_any_element();
        let mut children = vec![
            div()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(content(0.1))
                .bg(content(0.05))
                .px_4()
                .py_2()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(content(0.4))
                .child(div().min_w_0().flex_1().child("COMMAND"))
                .child(div().w(px(160.)).flex_shrink_0().child("KEYBINDING"))
                .child(div().w(px(112.)).flex_shrink_0().child("WHEN"))
                .into_any_element(),
        ];
        if rows.is_empty() {
            children.push(super::super::widgets::note("No matching bindings"));
        }
        for row in rows {
            let command = row.command;
            let recording = self.recording.as_deref() == Some(command);
            let modified = self.prefs.keybindings.contains_key(command);
            let disabled = self.prefs.keybindings.get(command) == Some(&None);
            let label = if recording {
                if self.recording_preview.is_empty() {
                    "Record…".into()
                } else {
                    self.recording_preview.clone()
                }
            } else {
                self.binding_label(&row)
            };
            children.push(
                div()
                    .h(px(44.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(content(0.05))
                    .px_4()
                    .text_size(px(12.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_color(content(if disabled { 0.45 } else { 1. }))
                            .child(command),
                    )
                    .child(
                        div()
                            .w(px(160.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(
                                div()
                                    .id(gpui::SharedString::from(format!("shortcut-{command}")))
                                    .debug_selector(move || format!("shortcut-{command}"))
                                    .role(if row.fixed { Role::Label } else { Role::Button })
                                    .aria_label(format!("Change {command} shortcut"))
                                    .h(px(24.))
                                    .w(px(112.))
                                    .flex()
                                    .items_center()
                                    .truncate()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(if recording {
                                        accent_color()
                                    } else {
                                        content(0.15)
                                    })
                                    .px(px(6.))
                                    .font_family(crate::fonts::mono())
                                    .text_size(px(11.))
                                    .text_color(content(if disabled {
                                        0.35
                                    } else if row.fixed {
                                        0.6
                                    } else {
                                        0.8
                                    }))
                                    .child(label)
                                    .when(!row.fixed, |d| {
                                        d.cursor_pointer().hover(|s| s.bg(content(0.1))).on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.recording = Some(command.into());
                                                this.recording_preview.clear();
                                                this.recording_error = None;
                                                window.focus(&this.recorder_focus, cx);
                                                cx.notify();
                                            }),
                                        )
                                    }),
                            )
                            .when(modified, |d| {
                                d.child(
                                    div()
                                        .id(gpui::SharedString::from(format!("reset-{command}")))
                                        .debug_selector(move || format!("reset-shortcut-{command}"))
                                        .role(Role::Button)
                                        .aria_label(format!("Reset {command} shortcut"))
                                        .p_1()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .hover(|s| s.bg(content(0.1)))
                                        .child(icon("rotate-ccw", 14.))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.update(cx, |p| {
                                                p.keybindings.remove(command);
                                            })
                                        })),
                                )
                            }),
                    )
                    .child(
                        div()
                            .w(px(112.))
                            .flex_shrink_0()
                            .truncate()
                            .font_family(crate::fonts::mono())
                            .text_size(px(11.))
                            .text_color(content(0.4))
                            .child(row.when),
                    )
                    .into_any_element(),
            );
        }
        if let Some(error) = &self.recording_error {
            children.push(
                div()
                    .px_4()
                    .py_2()
                    .text_size(px(11.))
                    .text_color(gpui::rgb(0xf87171))
                    .child(error.clone())
                    .into_any_element(),
            );
        }
        vec![self.group(None,true,"Shortcuts",Some("Click a shortcut to record new keys. Press Delete while recording to disable it.".into()),Some(action),children)]
    }
    fn binding_label(&self, row: &Binding) -> String {
        command_shortcut(&self.prefs, row.command).unwrap_or_else(|| "Disabled".into())
    }
    pub(in crate::settings) fn record_key(
        &mut self,
        key: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) {
        let Some(command) = self.recording.clone() else {
            return;
        };
        cx.stop_propagation();
        if key.key == "escape" || key.key == "tab" {
            self.recording = None;
            cx.notify();
            return;
        }
        if matches!(
            key.key.as_str(),
            "platform" | "control" | "alt" | "shift" | "function"
        ) {
            return;
        }
        let bare = !key.modifiers.platform
            && !key.modifiers.control
            && !key.modifiers.alt
            && !key.modifiers.shift;
        if bare && matches!(key.key.as_str(), "backspace" | "delete") {
            self.recording = None;
            self.update(cx, |p| {
                p.keybindings.insert(command, None);
            });
            return;
        }
        let shortcut = key.unparse();
        self.recording_preview = shortcut_label(&shortcut);
        if bare || (!key.modifiers.platform && !key.modifiers.control && !key.modifiers.alt) {
            self.recording_error = Some("Use Command, Control, or Option with a key.".into());
            cx.notify();
            return;
        }
        if command == "Tab: Activate 1–8"
            && !matches!(
                key.key.as_str(),
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8"
            )
        {
            self.recording_error = Some("Tab: Activate 1–8 needs a number key from 1 to 8".into());
            cx.notify();
            return;
        }
        if let Some(other) = BINDINGS.iter().find(|r| {
            r.command != command
                && (default_shortcuts(r).contains(&shortcut)
                    || self
                        .prefs
                        .keybindings
                        .get(r.command)
                        .and_then(|s| s.as_ref())
                        == Some(&shortcut))
        }) {
            self.recording_error = Some(format!("Already used by {}", other.command));
            cx.notify();
            return;
        }
        self.recording = None;
        self.recording_error = None;
        self.update(cx, |p| {
            p.keybindings.insert(command, Some(shortcut));
        });
    }
}

fn default_shortcuts(row: &Binding) -> Vec<String> {
    let shortcut = row
        .keys
        .replace(
            '⌘',
            if cfg!(target_os = "macos") {
                "cmd-"
            } else {
                "ctrl-"
            },
        )
        .replace('⌃', "ctrl-")
        .replace('⌥', "alt-")
        .replace('⇧', "shift-")
        .replace('↑', "up")
        .replace('↓', "down")
        .replace('←', "left")
        .replace('→', "right")
        .replace('+', "=")
        .to_lowercase();
    let chords = if shortcut.contains('…') {
        let prefix = shortcut.split('1').next().unwrap_or("");
        (1..=8).map(|i| format!("{prefix}{i}")).collect()
    } else {
        vec![shortcut]
    };
    chords
        .into_iter()
        .filter_map(|s| gpui::Keystroke::parse(&s).ok().map(|s| s.unparse()))
        .collect()
}
