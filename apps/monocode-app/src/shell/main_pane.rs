//! The main pane area. The workspace panes, transcript, and composer are
//! ported elsewhere; this draws one mock session pane so the shell reads like
//! the real app: a pane header, an empty transcript, and a composer frame
//! styled after `data-composer-box` in src/features/sessions/ui/Composer.tsx.

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _, div,
};
use monocode_ui::widgets::{button, icon_button};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, provider_logo, u};

use super::Shell;

impl Shell {
    pub(super) fn render_main_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let session = self
            .active_session
            .and_then(|index| self.data.sessions.get(index));
        let title = session.map(|s| s.title).unwrap_or("New session");

        let header = div()
            .flex()
            .flex_none()
            .h(u(theme.metrics.toolbar_height))
            .items_center()
            .gap(u(8.))
            .px(u(8.))
            .border_b_1()
            .border_color(c.stroke)
            .child(
                icon(IconName::GripVertical)
                    .size(u(14.))
                    .text_color(theme.content(0.35)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_px(theme.text.body)
                    .medium()
                    .child(title),
            )
            .child(
                icon_button("pane-close", IconName::X)
                    .size(24.)
                    .tooltip("Close Pane"),
            );

        let transcript = div()
            .flex()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .text_px(theme.text.body)
            .text_color(theme.content(0.35))
            .child("The transcript renders here.");

        let chip = |id: &'static str, lead: AnyElement, label: &'static str| {
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .gap(u(6.))
                .h(u(26.))
                .px(u(8.))
                .rounded(u(theme.radius.md))
                .border_1()
                .border_color(theme.content(0.08))
                .text_px(theme.text.label)
                .text_color(theme.content(0.80))
                .hover({
                    let fill = theme.content(0.08);
                    move |s| s.bg(fill)
                })
                .child(lead)
                .child(label)
        };
        let provider = session
            .map(|s| s.provider)
            .unwrap_or(monocode_ui::ProviderLogo::Claude);
        let model = session.map(|s| s.model).unwrap_or("Claude Opus 5");
        let toolbar = div()
            .flex()
            .items_center()
            .gap(u(6.))
            .px(u(8.))
            .pb(u(8.))
            .child(icon_button("composer-add", IconName::Plus).size(26.))
            .child(chip(
                "composer-model",
                provider_logo(provider).size(14.).into_any_element(),
                model,
            ))
            .child(chip(
                "composer-effort",
                icon(IconName::Gauge)
                    .size(u(14.))
                    .text_color(theme.content(0.60))
                    .into_any_element(),
                "High",
            ))
            .child(chip(
                "composer-access",
                icon(IconName::Lock)
                    .size(u(14.))
                    .text_color(theme.content(0.60))
                    .into_any_element(),
                "Supervised",
            ))
            .child(div().flex_1())
            .child(
                button("composer-send", "Send")
                    .primary()
                    .icon(IconName::ArrowUp)
                    .disabled(true),
            );

        let composer = div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.03))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(10.))
                    .px(u(12.))
                    .pt(u(10.))
                    .font_family(theme.fonts.mono.clone())
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.50))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .child(
                                icon(IconName::Folder)
                                    .size(u(13.))
                                    .text_color(theme.content(0.50)),
                            )
                            .child(self.data.cwd),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(4.))
                            .child(
                                icon(IconName::GitBranch)
                                    .size(u(13.))
                                    .text_color(theme.content(0.50)),
                            )
                            .child(self.data.branch),
                    ),
            )
            .child(
                div()
                    .px(u(12.))
                    .py(u(12.))
                    .text_px(theme.text.ui)
                    .text_color(theme.content(0.35))
                    .child("Ask, build, / for commands, @ for references..."),
            )
            .child(toolbar);

        div()
            .id("main-pane")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(header)
            .child(transcript)
            .child(div().flex_none().px(u(12.)).pb(u(12.)).child(composer))
    }
}
