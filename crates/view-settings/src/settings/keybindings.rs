//! Port of `KeybindingsPage`, `KeybindingShortcutEditor`, and
//! `QuickComposerShortcutEditor` in SettingsView.tsx.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_core::Platform;
use monocode_core::settings::{
    KEYBINDING_OVERRIDES_KEY, KeybindingOverride, KeybindingOverrides, QUICK_COMPOSER_COMMAND,
    QUICK_COMPOSER_ENABLED_KEY, QUICK_COMPOSER_SHORTCUT_KEY, filter_keybindings,
};
use monocode_core::shortcut::{
    QUICK_COMPOSER_DEFAULT_SHORTCUT, is_global_shortcut, quick_composer_shortcut_label,
};
use monocode_settings::Kv;
use monocode_settings::settings_store as ss;
use monocode_ui::{Theme, UiStyled as _, u};

use super::chrome::group;
use super::controls::{search_field, watch_keys};
use super::host::{HostTask, KeybindingsHost};
use super::section::SectionContext;
use super::shortcut_editor::{ShortcutAction, ShortcutEditor, ShortcutRequest};

/// `String(error)` for a thrown `Error`.
// TODO(port): the TypeScript showed thrown errors through `String(reason)`,
// which prefixes "Error: ". Kept as written.
fn thrown(message: impl std::fmt::Display) -> String {
    format!("Error: {message}")
}

/// `KeybindingsPage`'s `save`: store the override, then tell the macOS
/// menus.
pub fn save_keybinding(
    kv: &Kv,
    platform: Platform,
    host: &Rc<dyn KeybindingsHost>,
    command: &str,
    override_: &KeybindingOverride,
    cx: &mut gpui::App,
) -> HostTask<()> {
    match ss::save_keybinding_override(kv, command, override_, platform) {
        Err(message) => Task::ready(Err(thrown(message))),
        Ok(next) if platform.is_mac() => host.set_keybinding_overrides(&next, cx),
        Ok(_) => Task::ready(Ok(())),
    }
}

/// `QuickComposerShortcutEditor`'s apply, disable, and reset.
pub fn quick_composer_action(
    kv: &Kv,
    platform: Platform,
    host: &Rc<dyn KeybindingsHost>,
    request: ShortcutRequest,
    cx: &mut gpui::App,
) -> HostTask<()> {
    let kv = kv.clone();
    match request {
        ShortcutRequest::Apply(next) => {
            if !is_global_shortcut(&next) {
                return Task::ready(Err(thrown(
                    "Quick Composer needs ⌘ or Ctrl as a global hotkey",
                )));
            }
            // Validate before the native call: a rejected chord must not leave
            // the OS holding a global hotkey that settings does not know about.
            if let Err(message) =
                ss::validate_keybinding_shortcut(&kv, QUICK_COMPOSER_COMMAND, &next, platform)
            {
                return Task::ready(Err(thrown(message)));
            }
            // Recording while the feature is off must not switch it back on.
            let registered = if ss::load_quick_composer_enabled(&kv) {
                host.set_quick_composer_shortcut(true, Some(&next), cx)
            } else {
                Task::ready(Ok(()))
            };
            cx.spawn(async move |_| {
                registered.await?;
                ss::save_quick_composer_shortcut(&kv, &next, platform).map_err(thrown)
            })
        }
        ShortcutRequest::Reset => {
            // Reset restores the whole default state, including the switch.
            if let Err(message) = ss::validate_keybinding_shortcut(
                &kv,
                QUICK_COMPOSER_COMMAND,
                QUICK_COMPOSER_DEFAULT_SHORTCUT,
                platform,
            ) {
                return Task::ready(Err(thrown(message)));
            }
            let registered =
                host.set_quick_composer_shortcut(true, Some(QUICK_COMPOSER_DEFAULT_SHORTCUT), cx);
            cx.spawn(async move |_| {
                registered.await?;
                ss::save_quick_composer_enabled(&kv, true);
                ss::save_quick_composer_shortcut(&kv, QUICK_COMPOSER_DEFAULT_SHORTCUT, platform)
                    .map_err(thrown)
            })
        }
        ShortcutRequest::Disable => {
            let dropped = host.set_quick_composer_shortcut(false, None, cx);
            cx.spawn(async move |_| {
                dropped.await?;
                ss::save_quick_composer_enabled(&kv, false);
                Ok(())
            })
        }
    }
}

pub struct KeybindingsSection {
    ctx: SectionContext,
    query: String,
    filter: Entity<InputState>,
    overrides: KeybindingOverrides,
    editors: HashMap<String, Entity<ShortcutEditor>>,
    _subscriptions: Vec<Subscription>,
    _watch: (Vec<monocode_settings::Subscription>, Task<()>),
}

impl KeybindingsSection {
    pub fn new(ctx: SectionContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter"));
        let subscriptions = vec![
            cx.subscribe_in(&filter, window, |this, filter, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.query = filter.read(cx).value().to_string();
                    cx.notify();
                }
            }),
        ];
        let watch = watch_keys(
            &ctx.kv,
            &[
                KEYBINDING_OVERRIDES_KEY,
                QUICK_COMPOSER_SHORTCUT_KEY,
                QUICK_COMPOSER_ENABLED_KEY,
            ],
            |this: &mut Self, cx| {
                this.overrides = ss::load_keybinding_overrides(&this.ctx.kv, this.ctx.platform);
                cx.notify();
            },
            cx,
        );
        Self {
            overrides: ss::load_keybinding_overrides(&ctx.kv, ctx.platform),
            ctx,
            query: String::new(),
            filter,
            editors: HashMap::new(),
            _subscriptions: subscriptions,
            _watch: watch,
        }
    }

    pub fn editor(&self, command: &str) -> Option<&Entity<ShortcutEditor>> {
        self.editors.get(command)
    }

    pub fn filter_input(&self) -> &Entity<InputState> {
        &self.filter
    }

    fn editor_for(
        &mut self,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ShortcutEditor> {
        if let Some(editor) = self.editors.get(command) {
            return editor.clone();
        }
        let (kv, platform) = (self.ctx.kv.clone(), self.ctx.platform);
        let host = self.ctx.hosts.keybindings.clone();
        let action: ShortcutAction = if command == QUICK_COMPOSER_COMMAND {
            Rc::new(move |request, _, cx| quick_composer_action(&kv, platform, &host, request, cx))
        } else {
            let command = command.to_string();
            Rc::new(move |request, _, cx| {
                let override_ = match request {
                    ShortcutRequest::Apply(shortcut) => KeybindingOverride {
                        disabled: None,
                        shortcut: Some(shortcut),
                    },
                    ShortcutRequest::Disable => KeybindingOverride {
                        disabled: Some(true),
                        shortcut: None,
                    },
                    ShortcutRequest::Reset => KeybindingOverride::default(),
                };
                save_keybinding(&kv, platform, &host, &command, &override_, cx)
            })
        };
        let name = if command == QUICK_COMPOSER_COMMAND {
            "quick composer".to_string()
        } else {
            command.to_string()
        };
        let editor = cx.new(|cx| ShortcutEditor::new(name, platform, action, window, cx));
        self.editors.insert(command.to_string(), editor.clone());
        editor
    }
}

impl Render for KeybindingsSection {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let reveal = self.ctx.reveal(cx);
        let (kv, platform) = (self.ctx.kv.clone(), self.ctx.platform);
        let rows = filter_keybindings(&ss::current_keybindings(&kv, platform), &self.query);
        let count = rows.len();

        let action = div()
            .flex()
            .items_center()
            .gap(u(12.))
            .child(
                div()
                    .flex_none()
                    .text_px(theme.text.label)
                    .tabular()
                    .text_color(theme.content(0.40))
                    .child(format!(
                        "{count} {}",
                        if count == 1 { "binding" } else { "bindings" }
                    )),
            )
            .child(search_field(
                "filter-keybindings",
                &self.filter,
                176.,
                None,
                window,
                cx,
            ));

        let header = div()
            .flex()
            .items_center()
            .px(u(16.))
            .py(u(8.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .bg(theme.content(0.05))
            .text_px(theme.text.caption)
            .semibold()
            .text_color(theme.content(0.40))
            .child(div().min_w_0().flex_1().child("COMMAND"))
            .child(div().w(u(160.)).flex_none().child("KEYBINDING"))
            .child(div().w(u(112.)).flex_none().child("WHEN"));

        let mut card = group(&reveal, "Shortcuts")
            .first(true)
            .description(
                "Click a shortcut to record new keys. Press Delete while recording to disable it.",
            )
            .action(action)
            .child(header);
        if rows.is_empty() {
            card = card.child(
                div()
                    .px(u(16.))
                    .py(u(12.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child("No matching bindings"),
            );
        }
        let quick_enabled = ss::load_quick_composer_enabled(&kv);
        let quick_shortcut = ss::load_quick_composer_shortcut(&kv);
        for row in rows {
            let override_ = self.overrides.get(&row.command).cloned();
            let disabled = override_
                .as_ref()
                .is_some_and(KeybindingOverride::is_disabled);
            let editor = self.editor_for(&row.command, window, cx);
            let (display, reset_visible) = if row.command == QUICK_COMPOSER_COMMAND {
                (
                    quick_enabled
                        .then(|| quick_composer_shortcut_label(&quick_shortcut, platform).into()),
                    !quick_enabled || quick_shortcut != QUICK_COMPOSER_DEFAULT_SHORTCUT,
                )
            } else {
                (
                    (!disabled).then(|| SharedString::from(row.keys.clone())),
                    override_.is_some(),
                )
            };
            editor.update(cx, |editor, _| editor.set_state(display, reset_visible));
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .h(u(44.))
                    .px(u(16.))
                    .border_b_1()
                    .border_color(theme.content(0.05))
                    .text_px(theme.text.label)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .when(disabled, |el| el.text_color(theme.content(0.45)))
                            .child(row.command.clone()),
                    )
                    .child(editor)
                    .child(
                        div()
                            .w(u(112.))
                            .flex_none()
                            .font_family(theme.fonts.mono.clone())
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.40))
                            .child(row.when.clone()),
                    ),
            );
        }
        card
    }
}
