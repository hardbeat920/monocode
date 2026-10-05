//! Port of src/shared/ui/PrivateEmail.tsx and
//! src/features/inbox/ui/InboxProviderMark.tsx.

use std::sync::Arc;

use gpui::{
    App, ElementId, Image, ImageFormat, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, img,
};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, icon, u};

/// With email masking on, keeps an account email private in screenshots
/// until it is revealed; otherwise shows it as plain text.
///
/// CSS blurred the hidden text. GPUI cannot blur text, so a hidden email
/// shows one dot per character instead.
pub struct PrivateEmailState {
    pub revealed: bool,
}

#[derive(IntoElement)]
pub struct PrivateEmail {
    id: ElementId,
    email: SharedString,
    masked: bool,
}

/// `PrivateEmail`. `masked` is the `useMaskEmails` value the caller read.
pub fn private_email(
    id: impl Into<ElementId>,
    email: impl Into<SharedString>,
    masked: bool,
) -> PrivateEmail {
    PrivateEmail {
        id: id.into(),
        email: email.into(),
        masked,
    }
}

/// What a hidden email shows.
pub fn masked_email(email: &str) -> String {
    email.chars().map(|_| '•').collect()
}

impl RenderOnce for PrivateEmail {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        // A new email starts hidden (`key={status.email}` remounted it).
        let key = ElementId::from(SharedString::from(format!("{:?}-{}", self.id, self.email)));
        let state = window.use_keyed_state(key, cx, |_, _| PrivateEmailState { revealed: false });
        if !self.masked {
            // Turning masking back on hides an email revealed before it was
            // turned off.
            if state.read(cx).revealed {
                state.update(cx, |state, _| state.revealed = false);
            }
            let email = self.email.clone();
            let selector = format!("email-text:{email}");
            return div()
                .id(self.id)
                .min_w_0()
                .truncate()
                .tooltip(tooltip(email))
                .debug_selector(move || selector)
                .child(self.email)
                .into_any_element();
        }
        let revealed = state.read(cx).revealed;
        let action = if revealed {
            "Hide email"
        } else {
            "Reveal email"
        };
        let text = if revealed {
            self.email.to_string()
        } else {
            masked_email(&self.email)
        };
        div()
            .id(self.id)
            .min_w_0()
            .truncate()
            .rounded(u(theme.radius.sm))
            .text_color(if revealed {
                theme.content(0.65)
            } else {
                theme.content(0.40)
            })
            .debug_selector(move || format!("email:{action}"))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                state.update(cx, |state, cx| {
                    state.revealed = !state.revealed;
                    cx.notify();
                });
            })
            .child(text)
            .into_any_element()
    }
}

/// `InboxProvider`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InboxProvider {
    Github,
    Gitlab,
    AzureDevOps,
    Jira,
    Linear,
}

const GITLAB: &str = r##"<svg viewBox="0 0 50 48" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="m49.014 19-.067-.18-6.784-17.696a1.792 1.792 0 0 0-3.389.182l-4.579 14.02H15.651l-4.58-14.02a1.795 1.795 0 0 0-3.388-.182l-6.78 17.7-.071.175A12.595 12.595 0 0 0 5.01 33.556l.026.02.057.044 10.32 7.734 5.12 3.87 3.11 2.351a2.102 2.102 0 0 0 2.535 0l3.11-2.352 5.12-3.869 10.394-7.779.029-.022a12.595 12.595 0 0 0 4.182-14.554Z" fill="#E24329"/><path d="m49.014 19-.067-.18a22.88 22.88 0 0 0-9.12 4.103L24.931 34.187l9.485 7.167 10.393-7.779.03-.022a12.595 12.595 0 0 0 4.175-14.554Z" fill="#FC6D26"/><path d="m15.414 41.354 5.12 3.87 3.11 2.351a2.102 2.102 0 0 0 2.535 0l3.11-2.352 5.12-3.869-9.484-7.167-9.51 7.167Z" fill="#FCA326"/><path d="M10.019 22.923a22.86 22.86 0 0 0-9.117-4.1L.832 19A12.595 12.595 0 0 0 5.01 33.556l.026.02.057.044 10.32 7.734 9.491-7.167L10.02 22.923Z" fill="#FC6D26"/></svg>"##;

const AZURE_DEVOPS: &str = r##"<svg viewBox="0 0 16 16" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M15 3.62172V12.1336L11.5 15L6.075 13.025V14.9825L3.00375 10.9713L11.955 11.6704V4.00624L15 3.62172ZM12.0163 4.04994L6.99375 1V3.00125L2.3825 4.35581L1 6.12984V10.1586L2.9775 11.0325V5.86767L12.0163 4.04994Z"/></svg>"##;

const LINEAR: &str = r##"<svg viewBox="0 0 100 100" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M1.22541 61.5228c-.2225-.9485.90748-1.5459 1.59638-.857L39.3342 97.1782c.6889.6889.0915 1.8189-.857 1.5964C20.0515 94.4522 5.54779 79.9485 1.22541 61.5228ZM.00189135 46.8891c-.01764375.2833.08887215.5599.28957165.7606L52.3503 99.7085c.2007.2007.4773.3075.7606.2896 2.3692-.1476 4.6938-.46 6.9624-.9259.7645-.157 1.0301-1.0963.4782-1.6481L2.57595 39.4485c-.55186-.5519-1.49117-.2863-1.648174.4782-.465915 2.2686-.77832 4.5932-.92588465 6.9624ZM4.21093 29.7054c-.16649.3738-.08169.8106.20765 1.1l64.77602 64.776c.2894.2894.7262.3742 1.1.2077 1.7861-.7956 3.5171-1.6927 5.1855-2.684.5521-.328.6373-1.0867.1832-1.5407L8.43566 24.3367c-.45409-.4541-1.21271-.3689-1.54074.1832-.99132 1.6684-1.88843 3.3994-2.68399 5.1855ZM12.6587 18.074c-.3701-.3701-.393-.9637-.0443-1.3541C21.7795 6.45931 35.1114 0 49.9519 0 77.5927 0 100 22.4073 100 50.0481c0 14.8405-6.4593 28.1724-16.7199 37.3375-.3903.3487-.984.3258-1.3542-.0443L12.6587 18.074Z"/></svg>"##;

const JIRA: &str = r##"<svg viewBox="0 0 24 24" fill="#2684FF" xmlns="http://www.w3.org/2000/svg"><path d="M11.571 11.513H0a5.218 5.218 0 0 0 5.232 5.215h2.13v2.057A5.215 5.215 0 0 0 12.575 24V12.518a1.005 1.005 0 0 0-1.005-1.005Zm5.723-5.756H5.736a5.215 5.215 0 0 0 5.215 5.214h2.129v2.058a5.218 5.218 0 0 0 5.215 5.214V6.758a1.001 1.001 0 0 0-1.001-1.001ZM23.013 0H11.455a5.215 5.215 0 0 0 5.215 5.215h2.129v2.057A5.215 5.215 0 0 0 24 12.483V1.005A1.001 1.001 0 0 0 23.013 0Z"/></svg>"##;

const GITHUB: &str = r##"<svg viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12"/></svg>"##;

/// `InboxProviderMark`: the provider's logo, 16px, in its own colors or the
/// content color.
pub fn inbox_provider_mark(provider: InboxProvider, cx: &App) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    let svg = match provider {
        InboxProvider::Github => GITHUB,
        InboxProvider::Gitlab => GITLAB,
        InboxProvider::AzureDevOps => AZURE_DEVOPS,
        InboxProvider::Jira => JIRA,
        InboxProvider::Linear => LINEAR,
    };
    if svg.is_empty() {
        return icon(IconName::Inbox).size(u(16.)).into_any_element();
    }
    let rgb = theme.colors.content.to_rgb();
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let ink = format!("#{:02x}{:02x}{:02x}", byte(rgb.r), byte(rgb.g), byte(rgb.b));
    let source = svg.replace("currentColor", &ink);
    let image = Arc::new(Image::from_bytes(ImageFormat::Svg, source.into_bytes()));
    img(image).size(u(16.)).flex_none().into_any_element()
}
