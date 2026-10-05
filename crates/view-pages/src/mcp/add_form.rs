//! McpSettings.tsx `AddServerModal`'s form: provider, scope, an optional
//! name, and the pasted JSON configuration. The page wraps it in
//! monocode-ui's modal.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::input::{InputState, TextareaState};
use monocode_ui::{ProviderLogo, Theme, UiStyled as _, u};

use super::data::{McpData, McpProvider, McpScope};
use super::picker::{McpPicker, McpPickerOption};
use crate::widgets::plain_input;

type DoneFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// The placeholder JSON.
const CONFIG_PLACEHOLDER: &str =
    r#"{"mcpServers":{"my-server":{"command":"npx","args":["-y","example-mcp"]}}}"#;

pub struct AddServerForm {
    data: Rc<dyn McpData>,
    cwd: String,
    provider: McpProvider,
    scope: McpScope,
    provider_picker: Entity<McpPicker>,
    scope_picker: Entity<McpPicker>,
    name: Entity<InputState>,
    config: Entity<TextareaState>,
    busy: bool,
    error: String,
    on_close: Option<DoneFn>,
    /// `onAdded`: the page's forced refresh.
    on_added: Option<DoneFn>,
    _subscriptions: Vec<Subscription>,
}

fn provider_options() -> Vec<McpPickerOption> {
    McpProvider::ALL
        .iter()
        .map(|provider| McpPickerOption {
            value: provider.as_str().into(),
            label: provider.label().into(),
            logo: ProviderLogo::from_id(provider.harness_id()),
        })
        .collect()
}

fn scope_options(provider: McpProvider) -> Vec<McpPickerOption> {
    provider
        .scopes()
        .iter()
        .map(|scope| McpPickerOption {
            value: scope.as_str().into(),
            label: scope.label().into(),
            logo: None,
        })
        .collect()
}

fn parse_provider(value: &str) -> Option<McpProvider> {
    McpProvider::ALL
        .into_iter()
        .find(|provider| provider.as_str() == value)
}

fn parse_scope(value: &str) -> Option<McpScope> {
    [McpScope::Local, McpScope::Project, McpScope::User]
        .into_iter()
        .find(|scope| scope.as_str() == value)
}

/// `pattern="[A-Za-z0-9_-]*"`.
fn valid_name(name: &str) -> bool {
    name.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The check `add` runs before calling the backend: the browser's form
/// validation, then `JSON.parse` and the object check. Errors read the way
/// `String(cause)` printed them.
pub fn validate(provider: McpProvider, name: &str, config: &str) -> Result<(), String> {
    if provider != McpProvider::Opencode && !valid_name(name) {
        return Err("Please match the requested format.".into());
    }
    if config.is_empty() {
        return Err("Please fill out this field.".into());
    }
    let parsed: serde_json::Value =
        serde_json::from_str(config).map_err(|error| format!("SyntaxError: {error}"))?;
    if !parsed.is_object() {
        return Err("Error: Configuration must be a JSON object".into());
    }
    Ok(())
}

impl AddServerForm {
    pub fn new(
        data: Rc<dyn McpData>,
        cwd: &str,
        initial: McpProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let scope = initial.scopes()[0];
        let weak = cx.weak_entity();
        let provider_picker = cx.new(|cx| {
            McpPicker::new("Provider", initial.as_str(), provider_options(), cx).on_change(
                move |value, _, cx| {
                    let Some(provider) = parse_provider(value) else {
                        return;
                    };
                    weak.update(cx, |this, cx| this.set_provider(provider, cx))
                        .ok();
                },
            )
        });
        let weak = cx.weak_entity();
        let scope_picker = cx.new(|cx| {
            McpPicker::new("Scope", scope.as_str(), scope_options(initial), cx).on_change(
                move |value, _, cx| {
                    let Some(scope) = parse_scope(value) else {
                        return;
                    };
                    weak.update(cx, |this, cx| {
                        this.scope = scope;
                        cx.notify();
                    })
                    .ok();
                },
            )
        });
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("my-server"));
        let config = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(7)
                .placeholder(CONFIG_PLACEHOLDER)
        });
        Self {
            data,
            cwd: cwd.to_string(),
            provider: initial,
            scope,
            provider_picker,
            scope_picker,
            name,
            config,
            busy: false,
            error: String::new(),
            on_close: None,
            on_added: None,
            _subscriptions: Vec::new(),
        }
    }

    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    pub fn on_added(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_added = Some(Rc::new(f));
        self
    }

    pub fn provider(&self) -> McpProvider {
        self.provider
    }

    pub fn scope(&self) -> McpScope {
        self.scope
    }

    pub fn provider_picker(&self) -> &Entity<McpPicker> {
        &self.provider_picker
    }

    pub fn name_input(&self) -> &Entity<InputState> {
        &self.name
    }

    pub fn config_input(&self) -> &Entity<TextareaState> {
        &self.config
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    /// Picking a provider resets the scope to its first.
    fn set_provider(&mut self, provider: McpProvider, cx: &mut Context<Self>) {
        self.provider = provider;
        self.scope = provider.scopes()[0];
        let scope = self.scope;
        self.scope_picker.update(cx, |picker, cx| {
            picker.set_options(scope_options(provider), cx);
            picker.set_value(scope.as_str(), cx);
        });
        cx.notify();
    }

    /// `add`: validate, call `mcp_add`, refresh, and close.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let name = self.name.read(cx).value().to_string();
        let config = self.config.read(cx).value().to_string();
        if let Err(error) = validate(self.provider, &name, &config) {
            self.error = error;
            cx.notify();
            return;
        }
        self.busy = true;
        self.error.clear();
        cx.notify();
        let task = self.data.add(
            &self.cwd,
            self.provider,
            self.scope,
            monocode_core::js::trim(&name),
            &config,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        if let Some(added) = this.on_added.clone() {
                            added(window, cx);
                        }
                        if let Some(close) = this.on_close.clone() {
                            close(window, cx);
                        }
                    }
                    Err(error) => this.error = error,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

fn outline_button(
    id: &'static str,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let hover = theme.content(0.05);
    div()
        .id(id)
        .debug_selector(move || id.to_string())
        .px(u(12.))
        .py(u(6.))
        .rounded(u(theme.radius.md))
        .border_1()
        .border_color(theme.colors.stroke)
        .text_px(theme.text.label)
        .text_color(theme.colors.content)
        .hover(move |s| s.bg(hover))
        .child(label.into())
}

impl Render for AddServerForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let field = |theme: &Theme| {
            div()
                .mt(u(4.))
                .w_full()
                .px(u(8.))
                .py(u(6.))
                .rounded(u(theme.radius.md))
                .border_1()
                .border_color(theme.colors.stroke)
                .bg(theme.colors.background_base)
                .text_color(theme.colors.content)
        };
        let name_hint = div().flex().gap(u(4.)).child("Name").child(
            div()
                .text_color(theme.content(0.40))
                .child("(optional for an mcpServers block)"),
        );
        let busy = self.busy;
        div()
            .flex()
            .flex_col()
            .gap(u(16.))
            .p(u(16.))
            .text_color(theme.colors.content)
            .child(
                div()
                    .flex()
                    .gap(u(12.))
                    .child(div().flex_1().min_w_0().child(self.provider_picker.clone()))
                    .child(div().flex_1().min_w_0().child(self.scope_picker.clone())),
            )
            .child(
                div()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.65))
                    .child(name_hint)
                    .child(
                        field(&theme)
                            .h(u(34.))
                            .flex()
                            .items_center()
                            .text_px(theme.text.ui)
                            .debug_selector(|| "mcp-name".into())
                            .child(plain_input(&self.name, None, cx)),
                    ),
            )
            .child(
                div()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.65))
                    .child("JSON configuration")
                    .child(
                        field(&theme)
                            .h(u(126.))
                            .font_family(theme.fonts.mono.clone())
                            .text_px(theme.text.label)
                            .debug_selector(|| "mcp-config".into())
                            .child(plain_input(&self.config, None, cx)),
                    ),
            )
            .child(
                div()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child(
                        "Paste one entry from an mcpServers block, or a single server object with a name above.",
                    ),
            )
            .when(!self.error.is_empty(), |form| {
                form.child(
                    div()
                        .p(u(8.))
                        .rounded(u(theme.radius.md))
                        .border_1()
                        .border_color(monocode_ui::color::with_alpha(theme.colors.danger_fill, 0.3))
                        .bg(monocode_ui::color::with_alpha(theme.colors.danger_fill, 0.1))
                        .text_px(theme.text.label)
                        .text_color(theme.colors.danger)
                        .debug_selector(|| "mcp-add-error".into())
                        .child(self.error.clone()),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(u(8.))
                    .child(outline_button("mcp-add-cancel", "Cancel", &theme).on_click(
                        cx.listener(|this, _, window, cx| {
                            if let Some(close) = this.on_close.clone() {
                                close(window, cx);
                            }
                        }),
                    ))
                    .child(
                        outline_button(
                            "mcp-add-submit",
                            if busy { "Adding…" } else { "Add server" },
                            &theme,
                        )
                        .when(busy, |button| button.opacity(0.5))
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_like_the_form() {
        let ok = r#"{"mcpServers":{"a":{"command":"npx"}}}"#;
        assert_eq!(validate(McpProvider::Claude, "", ok), Ok(()));
        assert_eq!(validate(McpProvider::Claude, "my-server_1", ok), Ok(()));
        assert!(validate(McpProvider::Claude, "bad name", ok).is_err());
        assert_eq!(validate(McpProvider::Opencode, "any name", ok), Ok(()));
        assert_eq!(
            validate(McpProvider::Cursor, "", "[1]"),
            Err("Error: Configuration must be a JSON object".into())
        );
        assert!(
            validate(McpProvider::Cursor, "", "{")
                .unwrap_err()
                .starts_with("SyntaxError")
        );
        assert_eq!(
            validate(McpProvider::Cursor, "", ""),
            Err("Please fill out this field.".into())
        );
    }
}
