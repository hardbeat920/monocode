//! Port of `ProviderBinaryControl` and `binaryInspectionError` in
//! SettingsView.tsx: the folder button beside each Agent CLI that shows the
//! resolved CLI path and version and lets the user point at another binary.

use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Task, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::input::InputState;
use monocode_core::harness::HarnessId;
use monocode_settings::Kv;
use monocode_ui::widgets::popover_frame;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::controls::{
    Leading, TriggerBounds, anchored_popover, opens_above, plain_input, secondary_button,
};
use super::host::{BinaryInspection, ProvidersHost};
use super::store;

/// `MINIMUM_OPENCODE_VERSION` in opencodeProtocol.ts.
pub const MINIMUM_OPENCODE_VERSION: &str = "1.14.19";

/// `parseOpenCodeVersion`: the first `x.y.z` in the output.
// TODO(port): duplicates the OpenCode adapter's helper, which this crate
// cannot depend on.
pub fn parse_open_code_version(output: &str) -> Option<String> {
    let bytes = output.as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut start = 0;
    while start < bytes.len() {
        let major = digits(start);
        if major > 0 {
            let dot1 = start + major;
            if bytes.get(dot1) == Some(&b'.') {
                let minor = digits(dot1 + 1);
                let dot2 = dot1 + 1 + minor;
                if minor > 0 && bytes.get(dot2) == Some(&b'.') {
                    let patch = digits(dot2 + 1);
                    if patch > 0 {
                        return Some(output[start..dot2 + 1 + patch].to_string());
                    }
                }
            }
        }
        start += 1;
    }
    None
}

/// `compareSemver`.
pub fn compare_semver(left: &str, right: &str) -> i64 {
    let parts = |value: &str| -> Vec<i64> {
        value
            .split('.')
            .map(|part| {
                let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().unwrap_or(0)
            })
            .collect()
    };
    let (a, b) = (parts(left), parts(right));
    for i in 0..3 {
        let delta = a.get(i).copied().unwrap_or(0) - b.get(i).copied().unwrap_or(0);
        if delta != 0 {
            return delta;
        }
    }
    0
}

/// `/^codex-cli\s+\d+\.\d+\.\d+/`.
fn is_codex_version(version: &str) -> bool {
    let Some(rest) = version.strip_prefix("codex-cli") else {
        return false;
    };
    let trimmed = rest.trim_start();
    if trimmed.len() == rest.len() {
        return false;
    }
    parse_open_code_version(trimmed).is_some_and(|found| trimmed.starts_with(&found))
}

/// `binaryInspectionError`.
pub fn binary_inspection_error(
    provider: HarnessId,
    inspection: &BinaryInspection,
) -> Option<String> {
    if let Some(error) = &inspection.error {
        return Some(error.clone());
    }
    let version = inspection.version.as_deref().unwrap_or("");
    if provider == HarnessId::Codex && !is_codex_version(version) {
        return Some("Codex CLI returned an invalid version.".into());
    }
    if provider == HarnessId::Opencode {
        let Some(version) = parse_open_code_version(version) else {
            return Some("OpenCode CLI returned an invalid version.".into());
        };
        if compare_semver(&version, MINIMUM_OPENCODE_VERSION) < 0 {
            return Some(format!(
                "OpenCode v{version} is too old. Upgrade to v{MINIMUM_OPENCODE_VERSION} or newer."
            ));
        }
    }
    None
}

pub struct BinaryControl {
    provider: HarnessId,
    kv: Kv,
    host: Rc<dyn ProvidersHost>,
    open: bool,
    editing: bool,
    draft: Entity<InputState>,
    overridden: bool,
    inspection: Option<BinaryInspection>,
    working: bool,
    error: Option<String>,
    reveal_error: Option<String>,
    trigger: FocusHandle,
    popover: FocusHandle,
    trigger_bounds: TriggerBounds,
    job: Option<Task<()>>,
}

impl BinaryControl {
    pub fn new(
        provider: HarnessId,
        kv: Kv,
        host: Rc<dyn ProvidersHost>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let stored = store::load_provider_binary_path(&kv, provider);
        let draft = cx.new(|cx| {
            InputState::new(window, cx).default_value(stored.clone().unwrap_or_default())
        });
        Self {
            provider,
            overridden: stored.is_some(),
            kv,
            host,
            open: false,
            editing: false,
            draft,
            inspection: None,
            working: false,
            error: None,
            reveal_error: None,
            trigger: cx.focus_handle(),
            popover: cx.focus_handle(),
            trigger_bounds: TriggerBounds::default(),
            job: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn reveal_error(&self) -> Option<&str> {
        self.reveal_error.as_deref()
    }

    pub fn inspection(&self) -> Option<&BinaryInspection> {
        self.inspection.as_ref()
    }

    pub fn trigger_focus(&self) -> &FocusHandle {
        &self.trigger
    }

    pub fn draft(&self) -> &Entity<InputState> {
        &self.draft
    }

    fn title(&self) -> &'static str {
        self.provider.title()
    }

    /// `providerBinaryPathChangePending`.
    pub fn restart_required(&self) -> bool {
        self.host.runtime_binary_path(self.provider)
            != store::load_provider_binary_path(&self.kv, self.provider)
    }

    /// The status pill.
    pub fn status(&self) -> &'static str {
        if self.error.is_some() {
            "Needs attention"
        } else if self.restart_required() {
            "Restart required"
        } else if self.overridden {
            "Configured"
        } else {
            "Auto-detected"
        }
    }

    /// `inspect`.
    fn inspect(
        &mut self,
        path: Option<String>,
        cx: &mut Context<Self>,
    ) -> Task<Option<BinaryInspection>> {
        self.working = true;
        self.inspection = None;
        self.error = None;
        self.reveal_error = None;
        cx.notify();
        let provider = self.provider;
        let path = path.filter(|path| !path.trim().is_empty());
        let task = self.host.inspect_binary(provider, path.as_deref(), cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.working = false;
                cx.notify();
                match result {
                    Ok(next) => {
                        this.error = binary_inspection_error(provider, &next);
                        this.inspection = Some(next.clone());
                        Some(next)
                    }
                    Err(message) => {
                        this.error = Some(message);
                        None
                    }
                }
            })
            .ok()
            .flatten()
        })
    }

    fn dismiss(&mut self, restore_focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.editing = false;
        if restore_focus {
            self.trigger.focus(window, cx);
        }
        cx.notify();
    }

    /// The trigger: inspect on first open, then toggle.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open && self.inspection.is_none() && !self.working && self.error.is_none() {
            let stored = store::load_provider_binary_path(&self.kv, self.provider);
            let task = self.inspect(stored, cx);
            self.job = Some(cx.spawn(async move |_, _| {
                task.await;
            }));
        }
        self.open = !self.open;
        self.editing = false;
        if self.open {
            self.popover.focus(window, cx);
        }
        cx.notify();
    }

    pub fn start_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = true;
        self.draft.update(cx, |draft, cx| draft.focus(window, cx));
        cx.notify();
    }

    pub fn cancel_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = store::load_provider_binary_path(&self.kv, self.provider).unwrap_or_default();
        self.draft
            .update(cx, |draft, cx| draft.set_value(stored, window, cx));
        self.editing = false;
        self.trigger.focus(window, cx);
        cx.notify();
    }

    /// `submit`.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working {
            return;
        }
        let value = self.draft.read(cx).value().trim().to_string();
        if value.is_empty() {
            self.use_auto(window, cx);
            return;
        }
        let provider = self.provider;
        let inspected = self.inspect(Some(value.clone()), cx);
        self.job = Some(cx.spawn_in(window, async move |this, cx| {
            let Some(next) = inspected.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                if let Some(error) = binary_inspection_error(provider, &next) {
                    this.error = Some(error);
                    cx.notify();
                    return;
                }
                if !store::save_provider_binary_path(&this.kv, provider, Some(&value)) {
                    this.inspection = None;
                    this.error = Some("Could not save the binary path.".into());
                    cx.notify();
                    return;
                }
                this.overridden = true;
                this.dismiss(true, window, cx);
            })
            .ok();
        }));
    }

    /// `useAuto`.
    pub fn use_auto(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working {
            return;
        }
        let provider = self.provider;
        let inspected = self.inspect(None, cx);
        self.job = Some(cx.spawn_in(window, async move |this, cx| {
            let Some(next) = inspected.await else {
                return;
            };
            if binary_inspection_error(provider, &next).is_some() {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                if !store::save_provider_binary_path(&this.kv, provider, None) {
                    this.inspection = None;
                    this.error = Some("Could not save the binary path.".into());
                    cx.notify();
                    return;
                }
                this.draft
                    .update(cx, |draft, cx| draft.set_value("", window, cx));
                this.overridden = false;
                this.dismiss(true, window, cx);
            })
            .ok();
        }));
    }

    /// The retry button: the configured path, or auto-detect.
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        let path = if self.overridden {
            let draft = self.draft.read(cx).value().trim().to_string();
            (!draft.is_empty()).then_some(draft)
        } else {
            None
        };
        let task = self.inspect(path, cx);
        self.job = Some(cx.spawn(async move |_, _| {
            task.await;
        }));
    }

    pub fn open_location(&mut self, cx: &mut Context<Self>) {
        let Some(inspection) = self.inspection.clone() else {
            return;
        };
        let revealed = self.host.reveal_path(&inspection.path, cx);
        self.job = Some(cx.spawn(async move |this, cx| {
            if let Err(error) = revealed.await {
                this.update(cx, |this, cx| {
                    this.reveal_error = Some(error);
                    cx.notify();
                })
                .ok();
            }
        }));
    }

    fn on_popover_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "escape" => {
                self.dismiss(true, window, cx);
                cx.stop_propagation();
            }
            "enter" if self.editing => {
                self.submit(window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn pill(text: &'static str, cx: &App) -> AnyElement {
        let theme = Theme::of(cx);
        div()
            .rounded_full()
            .bg(theme.content(0.10))
            .px(u(6.))
            .py(u(2.))
            .text_px(theme.text.micro)
            .text_color(theme.content(0.50))
            .child(text)
            .into_any_element()
    }

    fn alert(text: String, cx: &App) -> AnyElement {
        let theme = Theme::of(cx);
        div()
            .mt(u(6.))
            .max_h(u(80.))
            .overflow_hidden()
            .text_px(theme.text.micro)
            .leading(1.6)
            .text_color(theme.colors.danger)
            .child(text)
            .into_any_element()
    }

    fn render_popover(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let id = self.provider.as_str();
        let title = self.title();
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(u(12.))
            .child(
                div()
                    .text_px(theme.text.label)
                    .medium()
                    .text_color(theme.colors.content)
                    .child(format!("{title} CLI")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .child(Self::pill("Global path", cx))
                    .child(Self::pill(self.status(), cx)),
            );
        let body: AnyElement = if self.editing {
            let input = plain_input(&self.draft, cx);
            let mut form = div()
                .mt(u(8.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.50))
                        .child("CLI path"),
                )
                .child(
                    div()
                        .mt(u(6.))
                        .flex()
                        .items_center()
                        .h(u(32.))
                        .w_full()
                        .px(u(8.))
                        .rounded(u(theme.radius.md))
                        .border_1()
                        .border_color(theme.content(0.10))
                        .bg(theme.content(0.04))
                        .font_family(theme.fonts.mono.clone())
                        .text_px(theme.text.caption)
                        .when(self.working, |el| el.opacity(0.5))
                        .debug_selector(move || format!("binary-path-{id}"))
                        .child(div().flex_1().min_w_0().child(input)),
                )
                .child(
                    div()
                        .mt(u(6.))
                        .text_px(theme.text.micro)
                        .text_color(theme.content(0.40))
                        .child("Enter the absolute path to the CLI executable. Changes apply after restarting MonoCode."),
                );
            if let Some(error) = self.error.clone() {
                form = form.child(Self::alert(error, cx));
            }
            let mut buttons = div().mt(u(12.)).flex().justify_end().gap(u(8.)).child(
                secondary_button(format!("cancel-path-{id}"), "Cancel")
                    .disabled(self.working)
                    .on_click(cx.listener(|this, _, window, cx| this.cancel_editing(window, cx))),
            );
            if self.overridden {
                buttons = buttons.child(
                    secondary_button(format!("use-auto-{id}"), "Use auto-detected path")
                        .disabled(self.working)
                        .on_click(cx.listener(|this, _, window, cx| this.use_auto(window, cx))),
                );
            }
            buttons = buttons.child(
                secondary_button(format!("save-path-{id}"), "Save path")
                    .disabled(self.working)
                    .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            );
            form.child(buttons).into_any_element()
        } else {
            let path_text = match (&self.inspection, &self.error) {
                (Some(inspection), _) => inspection.path.clone(),
                (None, Some(_)) => "CLI could not be resolved".into(),
                (None, None) => "Checking the selected CLI…".into(),
            };
            let version_text = match (&self.inspection, &self.error) {
                (Some(inspection), _) => inspection.version.clone().unwrap_or_default(),
                (None, Some(_)) => "Retry to check this CLI".into(),
                (None, None) => "Checking version…".into(),
            };
            let mut details = div().flex().flex_col().child(
                div()
                    .mt(u(8.))
                    .rounded(u(theme.radius.md))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .bg(theme.content(0.03))
                    .px(u(10.))
                    .py(u(8.))
                    .child(
                        div()
                            .font_family(theme.fonts.mono.clone())
                            .text_px(theme.text.micro)
                            .text_color(theme.content(0.65))
                            .debug_selector(move || format!("binary-details-path-{id}"))
                            .child(path_text),
                    )
                    .child(
                        div()
                            .mt(u(4.))
                            .text_px(theme.text.micro)
                            .text_color(theme.content(0.40))
                            .child(version_text),
                    ),
            );
            if let Some(error) = self.error.clone() {
                details = details.child(Self::alert(error, cx));
            }
            if let Some(error) = self.reveal_error.clone() {
                details = details.child(Self::alert(
                    format!("Could not open the CLI location: {error}"),
                    cx,
                ));
            }
            let mut buttons = div().mt(u(12.)).flex().justify_end().gap(u(8.));
            if self.error.is_some() {
                let label = if self.overridden {
                    "Retry configured path"
                } else {
                    "Retry auto-detect"
                };
                buttons = buttons.child(
                    secondary_button(format!("retry-{id}"), label)
                        .leading(Leading::Icon(IconName::RefreshCw))
                        .disabled(self.working)
                        .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                );
            }
            buttons = buttons
                .child(
                    secondary_button(format!("open-location-{id}"), "Open location")
                        .leading(Leading::Icon(IconName::ExternalLink))
                        .disabled(self.inspection.is_none())
                        .on_click(cx.listener(|this, _, _, cx| this.open_location(cx))),
                )
                .child(
                    secondary_button(format!("edit-path-{id}"), "Edit path")
                        .leading(Leading::Icon(IconName::Pencil))
                        .disabled(self.working)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.start_editing(window, cx)),
                        ),
                );
            details.child(buttons).into_any_element()
        };
        let above = opens_above(self.trigger_bounds.get(), 200.0, window);
        anchored_popover(
            above,
            false,
            theme.layer.popover,
            window,
            div()
                .id("binary-popover")
                .occlude()
                .track_focus(&self.popover)
                .on_key_down(cx.listener(Self::on_popover_key))
                .on_mouse_down_out(
                    cx.listener(|this, _, window, cx| this.dismiss(false, window, cx)),
                )
                .debug_selector(move || format!("binary-popover-{id}"))
                .child(
                    popover_frame(format!("binary-popover-frame-{id}"))
                        .width(440.)
                        .child(div().p(u(12.)).child(header).child(body)),
                ),
        )
    }
}

impl Focusable for BinaryControl {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.trigger.clone()
    }
}

impl Render for BinaryControl {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let restart = self.restart_required();
        let title = self.title();
        let ink = if restart {
            theme.colors.warning
        } else {
            theme.content(0.35)
        };
        let hover_ink = if restart {
            theme.colors.warning
        } else {
            theme.colors.content
        };
        let hover_fill = theme.content(0.10);
        let trigger = div()
            .id("binary-trigger")
            .flex()
            .size(u(24.))
            .items_center()
            .justify_center()
            .rounded(u(theme.radius.sm))
            .track_focus(&self.trigger)
            .hover(move |s| s.bg(hover_fill).text_color(hover_ink))
            .debug_selector(move || format!("binary:{title}"))
            .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx)))
            .child(icon(IconName::FolderOpen).size(u(14.)).text_color(ink));
        let popover = self.open.then(|| self.render_popover(window, cx));
        div()
            .relative()
            .flex()
            .child(self.trigger_bounds.probe())
            .child(trigger)
            .children(popover)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inspection(version: &str) -> BinaryInspection {
        BinaryInspection {
            path: "/bin/x".into(),
            version: Some(version.into()),
            error: None,
        }
    }

    #[test]
    fn parses_and_compares_versions() {
        assert_eq!(
            parse_open_code_version("opencode 1.18.32").as_deref(),
            Some("1.18.32")
        );
        assert_eq!(parse_open_code_version("v2.0"), None);
        assert!(compare_semver("1.14.18", MINIMUM_OPENCODE_VERSION) < 0);
        assert_eq!(compare_semver("1.14.19", MINIMUM_OPENCODE_VERSION), 0);
        assert!(compare_semver("1.18.32", MINIMUM_OPENCODE_VERSION) > 0);
    }

    #[test]
    fn validates_codex_and_opencode_versions() {
        assert_eq!(
            binary_inspection_error(HarnessId::Codex, &inspection("codex-cli 0.156.1")),
            None
        );
        assert_eq!(
            binary_inspection_error(HarnessId::Codex, &inspection("codex 0.1.0")).as_deref(),
            Some("Codex CLI returned an invalid version.")
        );
        assert_eq!(
            binary_inspection_error(HarnessId::Opencode, &inspection("opencode 1.2.3")).as_deref(),
            Some("OpenCode v1.2.3 is too old. Upgrade to v1.14.19 or newer.")
        );
        assert_eq!(
            binary_inspection_error(HarnessId::Opencode, &inspection("nope")).as_deref(),
            Some("OpenCode CLI returned an invalid version.")
        );
        assert_eq!(
            binary_inspection_error(HarnessId::Pi, &inspection("anything")),
            None
        );
        let failed = BinaryInspection {
            error: Some("Codex failed to start".into()),
            ..inspection("")
        };
        assert_eq!(
            binary_inspection_error(HarnessId::Codex, &failed).as_deref(),
            Some("Codex failed to start")
        );
    }
}
