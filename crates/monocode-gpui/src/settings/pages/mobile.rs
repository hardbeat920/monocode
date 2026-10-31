//! Port of src/features/mobile/ui/MobileSettings.tsx and PairPhoneDialog.tsx:
//! the phones paired with this desktop, and the one-use code a new one scans.
use crate::tooltip::Tip;
use crate::{
    input::{self, Input},
    settings::{
        Section, SettingsEvent, SettingsView,
        widgets::{icon_action, note, red, secondary_button},
    },
    theme::*,
};
use gpui::{
    Animation, AnimationExt, AnyElement, ClipboardItem, Context, Div, Entity, FontWeight,
    HighlightStyle, MouseButton, Role, SharedString, Stateful, StyledText, Subscription, Task,
    Window, canvas, div, prelude::*, px, relative, rgb,
};
use monocode_core::mobile::Status;
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Tailwind's amber-500 and emerald-500.
const AMBER: u32 = 0xf59e0b;
const EMERALD: u32 = 0x10b981;
/// The public TestFlight invite for the iPhone beta.
const TESTFLIGHT_URL: &str = "https://testflight.apple.com/join/xdwX38bB";
/// How long Pair phone waits for the desktop to listen and make a code.
const PAIRING_WAIT: Duration = Duration::from_secs(15);
/// PairPhoneDialog.tsx's LINEUP: mascots fanning out from the middle,
/// nearest first, each side in turn, in TAB_GROUP_COLORS after the wash.
const LINEUP: [(&str, u32); 9] = [
    ("robot", 0x459bf7),
    ("cat", 0xea603e),
    ("ghost", 0xf4c025),
    ("frog", 0x39c66d),
    ("invader", 0xe25a9e),
    ("crab", 0xb069d3),
    ("dino", 0x37beb3),
    ("mushroom", 0xef8539),
    ("rocket", 0x459bf7),
];
/// Modal.tsx's medium, fitViewport panel and PairPhoneDialog.tsx's QR tile.
const DIALOG_WIDTH: f32 = 560.;
const DIALOG_RADIUS: f32 = 16.;
const QR_SIDE: f32 = 384.;
const QR_PADDING: f32 = 14.;

/// What the Mobile page remembers between frames.
#[derive(Default)]
pub(in crate::settings) struct MobileState {
    /// Device ids already paired when the visible code was made.
    known: Option<HashSet<String>>,
    /// Since Pair phone was pressed, until its code arrives.
    pairing: Option<Instant>,
    notice: Option<String>,
    /// The device whose removal is waiting for confirmation.
    removing: Option<String>,
    copied: bool,
    copied_reset: Option<Task<()>>,
    /// "Connect away from home" is open.
    away: bool,
    remote: Option<Entity<Input>>,
    qr: Option<(String, Arc<Qr>)>,
    poll: Option<Task<()>>,
    _remote_events: Vec<Subscription>,
}

/// A QR code's modules, row by row, without the quiet zone.
#[derive(Debug, PartialEq)]
pub(super) struct Qr {
    width: usize,
    dark: Vec<bool>,
}

/// The QR code phones scan; low error correction keeps the modules large.
pub(super) fn qr(text: &str) -> Option<Qr> {
    let code =
        qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::L).ok()?;
    Some(Qr {
        width: code.width(),
        dark: code
            .to_colors()
            .into_iter()
            .map(|color| color == qrcode::Color::Dark)
            .collect(),
    })
}

/// PairPhoneDialog.tsx's status line.
pub(super) fn countdown(left: u64) -> String {
    if left == 0 {
        "Code expired".into()
    } else {
        format!("Expires in {}:{:02}", left / 60, left % 60)
    }
}

/// Enough of a fingerprint to compare by eye.
pub(super) fn short_fingerprint(fingerprint: &str) -> String {
    let short: String = fingerprint.chars().take(16).collect();
    if short.len() < fingerprint.len() {
        format!("{short}…")
    } else {
        short
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl SettingsView {
    /// Status and the router change on their own, and the code counts down:
    /// read them every second while the page is open.
    pub(in crate::settings) fn watch_mobile(&mut self, cx: &mut Context<Self>) {
        if self.section != Section::Connections {
            self.mobile.poll = None;
            return;
        }
        cx.emit(SettingsEvent::MobileRefresh);
        self.mobile.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let alive = this.update(cx, |this, cx| {
                    if this
                        .mobile
                        .pairing
                        .is_some_and(|since| since.elapsed() > PAIRING_WAIT)
                    {
                        this.mobile.pairing = None;
                        this.mobile.known = None;
                        cx.notify();
                    }
                    if this.ctx.mobile_invite.is_some() {
                        cx.notify();
                    }
                    cx.emit(SettingsEvent::MobileRefresh);
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }

    /// Leaving the page closes its code, as unmounting MobileSettings did.
    pub(in crate::settings) fn leave_mobile(&mut self, cx: &mut Context<Self>) {
        if self.section != Section::Connections {
            return;
        }
        if self.ctx.mobile_invite.is_some() {
            cx.emit(SettingsEvent::MobileCancelInvite);
        }
        self.mobile.known = None;
        self.mobile.pairing = None;
        self.mobile.removing = None;
        self.mobile.notice = None;
    }

    /// Escape closes the pairing code, then a pending removal.
    pub(in crate::settings) fn dismiss_mobile(&mut self, cx: &mut Context<Self>) -> bool {
        if self.section != Section::Connections {
            return false;
        }
        if self.ctx.mobile_invite.is_some() {
            self.close_invite(cx);
            return true;
        }
        if self.mobile.removing.take().is_some() {
            cx.notify();
            return true;
        }
        false
    }

    /// A phone that appears while the code is showing just paired with it.
    pub(in crate::settings) fn mobile_context_changed(&mut self, cx: &mut Context<Self>) {
        let showing = self.ctx.mobile_invite.is_some();
        if showing {
            self.mobile.pairing = None;
        } else if self.mobile.pairing.is_none() {
            self.mobile.known = None;
        }
        if self
            .ctx
            .mobile
            .as_ref()
            .is_some_and(|status| status.error.is_some())
        {
            self.mobile.pairing = None;
        }
        let paired = showing
            .then(|| {
                let known = self.mobile.known.as_ref()?;
                self.ctx
                    .mobile
                    .as_ref()?
                    .devices
                    .iter()
                    .find(|device| !known.contains(&device.id))
            })
            .flatten();
        if let Some(device) = paired {
            self.mobile.notice = Some(format!("{} is paired.", device.name));
            self.mobile.known = None;
            cx.emit(SettingsEvent::MobileCancelInvite);
        }
    }

    fn pair_phone(&mut self, cx: &mut Context<Self>) {
        let Some(status) = &self.ctx.mobile else {
            return;
        };
        self.mobile.known = Some(status.devices.iter().map(|d| d.id.clone()).collect());
        self.mobile.notice = None;
        self.mobile.pairing = Some(Instant::now());
        cx.emit(SettingsEvent::MobilePair);
        cx.notify();
    }

    /// The desktop only listens while a phone is paired or being paired.
    fn close_invite(&mut self, cx: &mut Context<Self>) {
        self.mobile.known = None;
        self.mobile.pairing = None;
        cx.emit(SettingsEvent::MobileCancelInvite);
        if let Some(status) = &self.ctx.mobile
            && status.devices.is_empty()
        {
            cx.emit(SettingsEvent::MobileConfigure {
                enabled: false,
                open_router: status.open_router,
                remote_address: status.remote_address.clone(),
            });
        }
        cx.notify();
    }

    fn configure_mobile(
        &mut self,
        enabled: bool,
        open_router: bool,
        remote_address: String,
        cx: &mut Context<Self>,
    ) {
        self.mobile.notice = None;
        cx.emit(SettingsEvent::MobileConfigure {
            enabled,
            open_router,
            remote_address,
        });
        cx.notify();
    }

    fn save_remote_address(&mut self, cx: &mut Context<Self>) {
        let (Some(status), Some(field)) = (&self.ctx.mobile, &self.mobile.remote) else {
            return;
        };
        let remote = field.read(cx).content.trim().to_owned();
        if remote == status.remote_address {
            return;
        }
        let (enabled, open_router) = (status.enabled, status.open_router);
        self.configure_mobile(enabled, open_router, remote, cx);
    }

    fn remote_field(&mut self, status: &Status, cx: &mut Context<Self>) -> Entity<Input> {
        if let Some(field) = &self.mobile.remote {
            return field.clone();
        }
        let initial = status.remote_address.clone();
        let field = cx.new(|cx| {
            let mut input = Input::field(
                "Remote address",
                "Public hostname or VPN IP, optionally :port",
                content(0.35),
                cx,
            );
            input.set_font_size(12.);
            input.set_text(initial, cx);
            input
        });
        self.mobile._remote_events = vec![
            cx.subscribe(&field, |_, _, _: &input::Changed, cx| cx.notify()),
            cx.subscribe(&field, |this, _, _: &input::Submitted, cx| {
                this.save_remote_address(cx)
            }),
        ];
        self.mobile.remote = Some(field.clone());
        field
    }

    fn copy_invite(&mut self, url: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(url));
        self.mobile.copied = true;
        self.mobile.copied_reset = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.mobile.copied = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn invite_qr(&mut self, url: &str) -> Option<Arc<Qr>> {
        if let Some((cached, qr)) = &self.mobile.qr
            && cached == url
        {
            return Some(qr.clone());
        }
        let qr = Arc::new(qr(url)?);
        self.mobile.qr = Some((url.to_owned(), qr.clone()));
        Some(qr)
    }

    pub(super) fn mobile_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let status = self.ctx.mobile.clone();
        let showing = self.ctx.mobile_invite.is_some();
        let busy = self.mobile.pairing.is_some();
        let mut page = vec![self.mobile_header(status.is_some() && !busy && !showing, cx)];
        let Some(status) = status else {
            page.push(
                div()
                    .mt_5()
                    .rounded_xl()
                    .border_1()
                    .border_color(content(0.1))
                    .bg(content(0.03))
                    .child(note(
                        "Mobile access isn’t available on this desktop right now.",
                    ))
                    .into_any_element(),
            );
            return page;
        };
        if !status.devices.is_empty() {
            page.push(self.mobile_access(&status, cx));
        }
        page.push(self.mobile_devices(&status, showing, cx));
        if let Some(error) = &status.error {
            page.push(
                div()
                    .mt_5()
                    .rounded_lg()
                    .bg(red(0.05))
                    .p_3()
                    .text_size(px(12.))
                    .line_height(px(19.5))
                    .text_color(red(1.))
                    .child(error.clone())
                    .into_any_element(),
            );
        }
        if let Some(notice) = &self.mobile.notice {
            page.push(
                div()
                    .mt_5()
                    .text_size(px(13.))
                    .text_color(rgb(EMERALD))
                    .child(notice.clone())
                    .into_any_element(),
            );
        }
        page.extend(self.mobile_away(&status, cx));
        page
    }

    fn mobile_header(&self, can_pair: bool, cx: &mut Context<Self>) -> AnyElement {
        let paragraph = |text: &'static str| {
            div()
                .mt_1()
                .text_size(px(12.))
                .line_height(px(19.5))
                .text_color(content(0.45))
                .child(text)
        };
        // Below Your machines, set off as MobileSettings.tsx was.
        div()
            .relative()
            .mt_10()
            .pt_8()
            .border_t_1()
            .border_color(content(0.1))
            .child(self.reveal_probe("mobile-access"))
            .flex()
            .items_end()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Your phones"),
                            )
                            .child(
                                div()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(content(0.1))
                                    .bg(content(0.05))
                                    .px(px(6.))
                                    .py(px(2.))
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(content(0.6))
                                    .child("Invite-only beta"),
                            ),
                    )
                    .child(paragraph(
                        "Check on projects and conversations from the MonoCode mobile app. Your phone connects straight to this desktop.",
                    ))
                    .child(
                        paragraph("The iPhone app is in beta on TestFlight.")
                            .flex()
                            .flex_wrap()
                            .gap_x_1()
                            .child(
                                div()
                                    .id("mobile-testflight")
                                    .debug_selector(|| "mobile-testflight".into())
                                    .role(Role::Link)
                                    .aria_label("Get the iPhone app on TestFlight")
                                    .text_color(content(0.7))
                                    .underline()
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(content(1.)))
                                    .child("Get the iPhone app")
                                    .on_click(|_, _, cx| cx.open_url(TESTFLIGHT_URL)),
                            ),
                    ),
            )
            .child(
                secondary_button("pair-phone", "Pair phone", false, !can_pair)
                    .child(icon("plus", 16.))
                    .child("Pair phone")
                    .when(can_pair, |d| {
                        d.on_click(cx.listener(|this, _, _, cx| this.pair_phone(cx)))
                    }),
            )
            .into_any_element()
    }

    /// Whether paired phones can reach this desktop, and the switch for it.
    fn mobile_access(&self, status: &Status, cx: &mut Context<Self>) -> AnyElement {
        let stuck = status.enabled && !status.listening;
        let description: SharedString = if status.listening {
            "Your phones can reach this desktop.".into()
        } else {
            "Your phones can’t connect because this desktop isn’t accepting connections right now."
                .into()
        };
        let (open_router, remote) = (status.open_router, status.remote_address.clone());
        let mut controls = Vec::new();
        if stuck {
            let remote = remote.clone();
            controls.push(
                secondary_button("mobile-reconnect", "Reconnect", false, false)
                    .child("Reconnect")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.configure_mobile(true, open_router, remote.clone(), cx)
                    }))
                    .into_any_element(),
            );
        }
        controls.push(self.toggle(
            "mobile-enabled",
            "Accept connections",
            status.enabled,
            false,
            cx,
            move |this, on, cx| this.configure_mobile(on, open_router, remote.clone(), cx),
        ));
        div()
            .mt_5()
            .overflow_hidden()
            .rounded_xl()
            .border_1()
            .border_color(if stuck {
                let mut amber: gpui::Hsla = rgb(AMBER).into();
                amber.a = 0.3;
                amber
            } else {
                content(0.1)
            })
            .bg(content(0.03))
            .child(self.row(
                None,
                true,
                "Accept connections",
                Some(description),
                controls,
            ))
            .into_any_element()
    }

    fn mobile_devices(&self, status: &Status, showing: bool, cx: &mut Context<Self>) -> AnyElement {
        if status.devices.is_empty() {
            return div()
                .mt_5()
                .rounded_xl()
                .border_1()
                .border_color(content(0.15))
                .px_5()
                .py_8()
                .text_center()
                .text_size(px(13.))
                .text_color(content(0.45))
                .child("Pair your iPhone or Android phone to get started.")
                .into_any_element();
        }
        let last = status.devices.len() == 1;
        let (open_router, remote) = (status.open_router, status.remote_address.clone());
        div()
            .mt_5()
            .overflow_hidden()
            .rounded_xl()
            .border_1()
            .border_color(content(0.1))
            .flex()
            .flex_col()
            .children(status.devices.iter().enumerate().map(|(ix, device)| {
                let id = device.id.clone();
                let name = device.name.clone();
                let confirming = self.mobile.removing.as_deref() == Some(id.as_str());
                div()
                    .when(ix > 0, |d| d.border_t_1().border_color(content(0.1)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_4()
                            .py_4()
                            .child(icon("smart-phone", 20.).text_color(content(0.45)))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(name.clone()),
                                    )
                                    .child(
                                        div()
                                            .mt_1()
                                            .text_size(px(12.))
                                            .text_color(content(0.5))
                                            .child("Paired"),
                                    ),
                            )
                            .child(
                                icon_action(
                                    SharedString::from(format!("mobile-remove-{id}")),
                                    format!("Remove {name}"),
                                    "trash",
                                    32.,
                                    16.,
                                    0.4,
                                    false,
                                )
                                .on_click({
                                    let id = id.clone();
                                    cx.listener(move |this, _, _, cx| {
                                        this.mobile.removing = Some(id.clone());
                                        cx.notify();
                                    })
                                }),
                            ),
                    )
                    .when(confirming, |d| {
                        let remote = remote.clone();
                        d.child(
                            div()
                                .id(SharedString::from(format!("mobile-confirm-{id}")))
                                .role(Role::Group)
                                .aria_label(format!("Confirm removing {name}"))
                                .flex()
                                .flex_col()
                                .gap_3()
                                .border_t_1()
                                .border_color(content(0.1))
                                .bg(content(0.03))
                                .px_4()
                                .py_4()
                                .text_size(px(12.))
                                .line_height(px(19.5))
                                .text_color(content(0.6))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(content(1.))
                                        .child(format!("Remove {name}?")),
                                )
                                .child(
                                    "It disconnects right away and can’t reach this desktop again until you pair it with a new code.",
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap_2()
                                        .child(
                                            secondary_button(
                                                "mobile-confirm-remove",
                                                "Remove phone",
                                                true,
                                                false,
                                            )
                                            .child("Remove phone")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.emit(SettingsEvent::MobileRevoke(id.clone()));
                                                if last && !showing {
                                                    cx.emit(SettingsEvent::MobileConfigure {
                                                        enabled: false,
                                                        open_router,
                                                        remote_address: remote.clone(),
                                                    });
                                                }
                                                this.mobile.removing = None;
                                                this.mobile.notice =
                                                    Some(format!("{name} was removed."));
                                                cx.notify();
                                            })),
                                        )
                                        .child(
                                            secondary_button(
                                                "mobile-cancel-remove",
                                                "Cancel",
                                                false,
                                                false,
                                            )
                                            .border_0()
                                            .child("Cancel")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.mobile.removing = None;
                                                cx.notify();
                                            })),
                                        ),
                                ),
                        )
                    })
            }))
            .into_any_element()
    }

    /// MobileSettings.tsx's "Connect away from home" details.
    fn mobile_away(&mut self, status: &Status, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let open = self.mobile.away;
        let mut page = vec![
            div()
                .id("mobile-away")
                .relative()
                .child(self.reveal_probe("mobile-remote"))
                .role(Role::Button)
                .aria_label("Connect away from home")
                .aria_expanded(open)
                .mt_8()
                .text_size(px(12.))
                .text_color(content(0.45))
                .cursor_pointer()
                .child(if open {
                    "▾ Connect away from home"
                } else {
                    "▸ Connect away from home"
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.mobile.away = !this.mobile.away;
                    cx.notify();
                }))
                .into_any_element(),
        ];
        if !open {
            return page;
        }
        let field = self.remote_field(status, cx);
        let unsaved = field.read(cx).content.trim() != status.remote_address;
        let (enabled, remote) = (status.enabled, status.remote_address.clone());
        let paragraph = |text: SharedString| {
            div()
                .text_size(px(12.))
                .line_height(px(19.5))
                .text_color(content(0.45))
                .child(text)
        };
        let addresses = status
            .addresses
            .iter()
            .map(|a| format!("{}:{}", a.host, a.port))
            .collect::<Vec<_>>()
            .join(", ");
        page.push(
            div()
                .mt_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(paragraph(
                    "On the same Wi-Fi, phones connect without any setup. To reach this desktop from anywhere else, it needs a public address. Keep the desktop awake.".into(),
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(13.))
                        .text_color(content(0.7))
                        .child(self.toggle(
                            "mobile-open-router",
                            "Open the router port automatically",
                            status.open_router,
                            false,
                            cx,
                            move |this, on, cx| {
                                this.configure_mobile(enabled, on, remote.clone(), cx)
                            },
                        ))
                        .child("Open the router port automatically"),
                )
                .when(!status.router_status.is_empty(), |d| {
                    d.child(paragraph(status.router_status.clone().into()))
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .text_size(px(12.))
                        .text_color(content(0.65))
                        .child("Remote address")
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .h(px(32.))
                                        .min_w_0()
                                        .flex_1()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(content(0.1))
                                        .px_2()
                                        .flex()
                                        .items_center()
                                        .child(field),
                                )
                                .child(
                                    secondary_button(
                                        "mobile-save-remote",
                                        "Save",
                                        false,
                                        !unsaved,
                                    )
                                    .child("Save")
                                    .when(unsaved, |d| {
                                        d.on_click(cx.listener(|this, _, _, cx| {
                                            this.save_remote_address(cx)
                                        }))
                                    }),
                                ),
                        ),
                )
                .child(paragraph(
                    "Automatic mapping needs a public IP. Otherwise, forward UDP port 38416 to this desktop or use a VPN address. Set the address before making a pairing code.".into(),
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_size(px(12.))
                        .line_height(px(19.5))
                        .text_color(content(0.45))
                        .child(format!("This desktop: {}", status.name))
                        .when_some(status.fingerprint.as_deref(), |d, fingerprint| {
                            d.child(format!("Fingerprint: {}", short_fingerprint(fingerprint)))
                        })
                        .child(if addresses.is_empty() {
                            "Not reachable on any network yet.".to_owned()
                        } else {
                            format!("Reachable at {addresses}")
                        }),
                )
                .into_any_element(),
        );
        page
    }

    /// PairPhoneDialog.tsx, presented by Shell alongside the other window
    /// overlays so its scrim and centering cover the entire viewport.
    pub(crate) fn mobile_overlay_open(&self) -> bool {
        self.section == Section::Connections && self.ctx.mobile_invite.is_some()
    }

    pub(crate) fn mobile_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.mobile_overlay_open() {
            return None;
        }
        let invite = self.ctx.mobile_invite.clone()?;
        let left = invite.expires.saturating_sub(now());
        let expired = left == 0;
        let busy = self.mobile.pairing.is_some();
        let qr = self.invite_qr(&invite.url);
        let viewport = window.viewport_size();
        let width = DIALOG_WIDTH.min((viewport.width.as_f32() - 24.).max(0.));
        let max_height = (viewport.height.as_f32() - 32.).max(0.);
        // Keep the QR square on narrow windows, without growing past size-96.
        let qr_side = QR_SIDE.min((width - 50.).max(0.));
        let lead = "Open MonoCode on your phone, tap ";
        let action = "Connect to desktop";
        let instructions = format!("{lead}{action}, and scan the code.");
        let bold = lead.len()..lead.len() + action.len();
        let url = invite.url.clone();
        let code = div()
            .id("mobile-pair-code")
            .debug_selector(|| "mobile-pair-code".into())
            .role(Role::Image)
            .aria_label("Scan this one-use pairing code in the MonoCode mobile app")
            .relative()
            .size(px(qr_side))
            .flex_shrink_0()
            .overflow_hidden()
            .rounded_2xl()
            .bg(rgb(0xffffff))
            .p(px(QR_PADDING))
            .when_some(qr, |d, qr| {
                d.child(
                    div()
                        .size_full()
                        .when(expired, |d| d.opacity(0.15))
                        .child(qr_canvas(qr)),
                )
            })
            .when(expired, |d| {
                d.child(expired_qr_blur()).child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .id("mobile-new-code-expired")
                                .debug_selector(|| "mobile-new-code-expired".into())
                                .role(Role::Button)
                                .aria_label("New code")
                                .rounded_lg()
                                .bg(rgb(0x000000))
                                .px_3()
                                .py_2()
                                .text_size(px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(0xffffff))
                                .child("New code")
                                .map(|d| {
                                    if busy {
                                        d.opacity(0.4)
                                    } else {
                                        d.cursor_pointer().on_click(
                                            cx.listener(|this, _, _, cx| this.pair_phone(cx)),
                                        )
                                    }
                                }),
                        ),
                )
            });
        let status = div()
            .mt(px(6.))
            .h(px(40.))
            .flex_shrink_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .pl_1()
            .text_size(px(12.))
            .text_color(content(0.5))
            .child(icon("clock", 14.))
            .child(
                div()
                    .debug_selector(|| "mobile-countdown".into())
                    .flex_1()
                    .child(countdown(left)),
            )
            .child(
                pairing_icon_action(
                    "mobile-copy-link",
                    if self.mobile.copied {
                        "Copied pairing link"
                    } else {
                        "Copy pairing link"
                    },
                    if self.mobile.copied {
                        "check"
                    } else {
                        "response-copy"
                    },
                    self.mobile.copied,
                    expired,
                )
                .when(!expired, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| this.copy_invite(url.clone(), cx)))
                }),
            )
            .child(
                pairing_icon_action(
                    "mobile-new-code",
                    "Make a new code",
                    "refresh-cw",
                    false,
                    busy,
                )
                .when(!busy, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| this.pair_phone(cx)))
                }),
            );
        let body = div()
            .flex()
            .flex_col()
            .items_center()
            .px_6()
            .pb_6()
            .child(
                div()
                    .text_size(px(22.))
                    .line_height(px(33.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(content(1.))
                    .child("Connect a phone to this desktop"),
            )
            .child(
                div()
                    .mt(px(6.))
                    .text_center()
                    .text_size(px(12.))
                    .line_height(px(19.5))
                    .text_color(content(0.5))
                    .child(StyledText::new(instructions).with_highlights([(
                        bold,
                        HighlightStyle {
                            color: Some(content(0.8)),
                            font_weight: Some(FontWeight::MEDIUM),
                            ..Default::default()
                        },
                    )])),
            )
            .child(
                div()
                    .mt_5()
                    .w(px(qr_side))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(code)
                    .child(status),
            )
            .child(
                div()
                    .mt_4()
                    .text_center()
                    .text_size(px(12.))
                    .line_height(px(19.5))
                    .text_color(content(0.45))
                    .child("Can’t scan? Copy the link and open it on your phone. Each code works once."),
            );
        Some(
            div()
                .id("mobile-pair-overlay")
                .debug_selector(|| "mobile-pair-overlay".into())
                .absolute()
                .inset_0()
                .occlude()
                .child(crate::modal::scrim("mobile-pair-scrim"))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.close_invite(cx);
                    }),
                )
                .child(
                    div()
                        .id("mobile-pair-dialog")
                        .debug_selector(|| "mobile-pair-dialog".into())
                        .role(Role::Dialog)
                        .aria_label("Pair a phone")
                        .relative()
                        .w(px(width))
                        .max_h(px(max_height))
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .rounded(px(DIALOG_RADIUS))
                        .shadow_2xl()
                        .border_1()
                        .border_color(content(0.07))
                        .child(pairing_backdrop(
                            self.ctx.classic || self.ctx.new_layout_boxy,
                        ))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .id("mobile-pair-scroll")
                                .debug_selector(|| "mobile-pair-scroll".into())
                                .relative()
                                .min_h_0()
                                .overflow_y_scroll()
                                .child(mascot_lineup())
                                .child(body),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// PairPhoneDialog's compact controls: an emerald copied check stays green
/// while hovered; the remaining icons brighten with the button.
fn pairing_icon_action(
    id: &'static str,
    label: &'static str,
    path: &'static str,
    copied: bool,
    disabled: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Button)
        .aria_label(label)
        .tip(label)
        .group("pairing-icon-action")
        .size(px(28.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .when(disabled, |d| d.opacity(0.4))
        .when(!disabled, |d| {
            d.cursor_pointer().hover(|s| s.bg(content(0.08)))
        })
        .child(if copied {
            // The tick grows in as the copy lands, like the Inbox's Copy link.
            icon(path, 14.)
                .text_color(rgb(EMERALD))
                .with_animation(
                    "mobile-copied-tick",
                    Animation::new(Duration::from_millis(260))
                        .with_easing(|t| 1. - (1. - t).powi(3)),
                    |svg, t| svg.size(px(14. * (0.5 + 0.5 * t))).opacity(t),
                )
                .into_any_element()
        } else {
            icon(path, 14.)
                .text_color(content(0.45))
                .when(!disabled, |i| {
                    i.group_hover("pairing-icon-action", |s| s.text_color(content(1.)))
                })
                .into_any_element()
        })
}

/// Modal.tsx's stationary GlassBackdrop, below the scrolling content.
fn pairing_backdrop(classic: bool) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let light = content(1.).l < 0.5;
            let fill = if classic || light {
                let mut fill = shell_fill();
                fill.a = if light { 1. } else { 0.55 };
                fill
            } else {
                content(0.02)
            };
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let fill = {
                let mut fill = shell_fill();
                fill.a = 1.;
                fill
            };
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let quad = gpui::fill(bounds, fill)
                .border_color(gpui::backdrop_blur_marker(24. * window.scale_factor()));
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let quad = gpui::fill(bounds, fill);
            window.paint_quad(quad.corner_radii(px(DIALOG_RADIUS)));
        },
    )
    .absolute()
    .size_full()
}

/// A module edge in physical pixels. Snapping each edge independently fills
/// the image area, like React's pixelated QR image, without shaving an entire
/// logical pixel from every module on Retina displays.
fn qr_edge(index: usize, modules: usize, side: f32, scale: f32) -> f32 {
    (index as f32 * side * scale / modules as f32).round() / scale
}

/// React's expired image has opacity-15 and blur-[2px]. The QR ink is already
/// dimmed; blur that image rectangle before painting the New code button.
fn expired_qr_blur() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |bounds, _, window, _| {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                // A fully transparent fill is optimized out by GPUI before
                // reaching Metal. This invisible white tint retains the
                // marker without changing the dimmed QR's appearance.
                let mut tint: gpui::Hsla = rgb(0xffffff).into();
                tint.a = 0.000001;
                window.paint_quad(
                    gpui::fill(bounds, tint)
                        .border_color(gpui::backdrop_blur_marker(2. * window.scale_factor())),
                );
            });
            // Other renderers keep the dimmed expired image and the usable
            // New code control; they have no backdrop-blur primitive.
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let _ = (bounds, window);
        },
    )
    .absolute()
    .top(px(QR_PADDING))
    .left(px(QR_PADDING))
    .right(px(QR_PADDING))
    .bottom(px(QR_PADDING))
}

/// Dark modules on white with the same two-module quiet zone as React's
/// QRCode.toDataURL. All edges land on device pixels so cameras see crisp ink.
fn qr_canvas(qr: Arc<Qr>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let modules = qr.width + 4;
            let side = bounds.size.width.min(bounds.size.height).as_f32();
            let scale = window.scale_factor();
            let origin = gpui::point(
                px((bounds.origin.x.as_f32() * scale).round() / scale),
                px((bounds.origin.y.as_f32() * scale).round() / scale),
            );
            for (y, row) in qr.dark.chunks(qr.width).enumerate() {
                let top = qr_edge(y + 2, modules, side, scale);
                let bottom = qr_edge(y + 3, modules, side, scale);
                let mut x = 0;
                while x < row.len() {
                    if !row[x] {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x < row.len() && row[x] {
                        x += 1;
                    }
                    let left = qr_edge(start + 2, modules, side, scale);
                    let right = qr_edge(x + 2, modules, side, scale);
                    window.paint_quad(gpui::fill(
                        gpui::Bounds::new(
                            origin + gpui::point(px(left), px(top)),
                            gpui::size(px(right - left), px(bottom - top)),
                        ),
                        rgb(0x000000),
                    ));
                }
            }
        },
    )
    .size_full()
}

/// The mascot crew, gathered round a glow, standing in for an illustration.
fn mascot_lineup() -> AnyElement {
    div()
        .relative()
        .h(px(96.))
        .flex_shrink_0()
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .bottom(px(14.))
                .left(relative(0.5))
                .ml(px(-112.))
                .h(px(12.))
                .w(px(224.))
                .child(mascot_glow()),
        )
        // The farthest first, so the nearest paint over them.
        .children(LINEUP.iter().enumerate().rev().map(|(i, &(name, color))| {
            let step = i.div_ceil(2) as f32;
            let side = if i % 2 == 1 { -1. } else { 1. };
            let size = 40. - step * 6.;
            div()
                .absolute()
                .left(relative(0.5))
                .ml(px(side * step * 42. - size / 2.))
                .bottom(px(18. + step * 2.))
                .size(px(size))
                .opacity(1. - step * 0.17)
                .child(crate::monos::avatar::avatar(
                    name, color, "idle", size, true,
                ))
        }))
        .into_any_element()
}

/// PairPhoneDialog's elliptical closest-side radial gradient, fading from
/// twelve-percent content ink at its center to transparent at its edge.
fn mascot_glow() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |bounds, _, window, _| {
            const STEPS: usize = 24;
            let center = bounds.center();
            for layer in 0..STEPS {
                let radius = 1. - layer as f32 / STEPS as f32;
                let rx = bounds.size.width.as_f32() / 2. * radius;
                let ry = bounds.size.height.as_f32() / 2. * radius;
                let mut path = gpui::PathBuilder::fill();
                for segment in 0..=64 {
                    let angle = std::f32::consts::TAU * segment as f32 / 64.;
                    let point = center + gpui::point(px(rx * angle.cos()), px(ry * angle.sin()));
                    if segment == 0 {
                        path.move_to(point);
                    } else {
                        path.line_to(point);
                    }
                }
                path.close();
                if let Ok(path) = path.build() {
                    let previous_alpha = 0.12 * layer as f32 / STEPS as f32;
                    window.paint_path(path, content((0.12 / STEPS as f32) / (1. - previous_alpha)));
                }
            }
        },
    )
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Preferences, SettingsContext};
    use gpui::{
        Bounds, Render, TestAppContext, VisualTestContext, WindowBounds, WindowHandle,
        WindowOptions, point, size,
    };

    /// The same placement as Shell: the settings pane is a child, while the
    /// pairing overlay belongs to the window and observes the settings entity.
    struct Preview {
        settings: Entity<SettingsView>,
        cancellations: usize,
        _changes: Subscription,
        _events: Subscription,
    }

    impl Render for Preview {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let overlay = self
                .settings
                .update(cx, |view, cx| view.mobile_overlay(window, cx));
            div()
                .relative()
                .size_full()
                .child(self.settings.clone())
                .children(overlay)
        }
    }

    fn preview(cx: &mut TestAppContext) -> WindowHandle<Preview> {
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(1200.), px(900.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|cx| {
                        let settings = cx.new(|cx| {
                            let mut view = SettingsView::new(
                                Section::Connections,
                                Preferences {
                                    notifications: false,
                                    ..Default::default()
                                },
                                SettingsContext {
                                    mobile: Some(Status::default()),
                                    mobile_invite: Some(monocode_core::mobile::Invite {
                                        url: format!("monocode://pair?i={}", "A".repeat(600)),
                                        expires: now() + 900,
                                    }),
                                    ..Default::default()
                                },
                                cx,
                            );
                            view.discovery_task = None;
                            view.discovery_loading = false;
                            view.mobile.poll = None;
                            view
                        });
                        let changes = cx.observe(&settings, |_, _, cx| cx.notify());
                        let events = cx.subscribe(&settings, |this: &mut Preview, _, event, cx| {
                            if matches!(event, SettingsEvent::MobileCancelInvite) {
                                this.cancellations += 1;
                                this.settings.update(cx, |view, cx| {
                                    view.ctx.mobile_invite = None;
                                    cx.notify();
                                });
                                cx.notify();
                            }
                        });
                        Preview {
                            settings,
                            cancellations: 0,
                            _changes: changes,
                            _events: events,
                        }
                    })
                },
            )
            .unwrap()
        });
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
    }

    #[gpui::test]
    fn pairing_dialog_matches_react_dimensions_and_scrolls_inside_small_windows(
        cx: &mut TestAppContext,
    ) {
        let window = preview(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let overlay = visual.debug_bounds("mobile-pair-overlay").unwrap();
        let dialog = visual.debug_bounds("mobile-pair-dialog").unwrap();
        let code = visual.debug_bounds("mobile-pair-code").unwrap();
        assert_eq!(overlay.size, size(px(1200.), px(900.)));
        assert_eq!(dialog.size.width, px(560.));
        assert!(dialog.size.height > px(620.) && dialog.size.height < px(720.));
        assert_eq!(dialog.center(), overlay.center());
        assert_eq!(code.size, size(px(384.), px(384.)));
        assert_eq!(code.center().x, dialog.center().x);

        visual.simulate_resize(size(px(420.), px(480.)));
        visual.run_until_parked();
        let dialog = visual.debug_bounds("mobile-pair-dialog").unwrap();
        let scroll = visual.debug_bounds("mobile-pair-scroll").unwrap();
        let code = visual.debug_bounds("mobile-pair-code").unwrap();
        assert_eq!(dialog.size.width, px(396.));
        assert!(dialog.top() >= px(16.) && dialog.bottom() <= px(464.));
        assert!(scroll.size.height <= dialog.size.height);
        assert_eq!(code.size, size(px(346.), px(346.)));
        assert!(code.left() > dialog.left() && code.right() < dialog.right());
    }

    #[gpui::test]
    fn pairing_panel_keeps_inside_clicks_and_dismisses_on_the_window_scrim(
        cx: &mut TestAppContext,
    ) {
        let window = preview(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let code = visual.debug_bounds("mobile-pair-code").unwrap();
        visual.simulate_click(code.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |view, _, _| assert_eq!(view.cancellations, 0))
            .unwrap();
        assert!(visual.debug_bounds("mobile-pair-dialog").is_some());
        visual.simulate_click(point(px(8.), px(8.)), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |view, _, _| assert_eq!(view.cancellations, 1))
            .unwrap();
        assert!(visual.debug_bounds("mobile-pair-dialog").is_none());
    }

    #[test]
    fn dense_qr_codes_fill_the_image_with_crisp_device_pixel_edges() {
        let side = QR_SIDE - QR_PADDING * 2.;
        // A dense code used to lose almost a third of its side because each
        // module was rounded down to a whole logical pixel.
        let modules = 153;
        for scale in [1., 1.5, 2.] {
            assert_eq!(qr_edge(0, modules, side, scale), 0.);
            assert_eq!(qr_edge(modules, modules, side, scale), side);
            let left = qr_edge(2, modules, side, scale);
            let right = qr_edge(modules - 2, modules, side, scale);
            assert!(right - left > side * 0.96);
            for index in 0..=modules {
                let edge = qr_edge(index, modules, side, scale) * scale;
                assert!((edge - edge.round()).abs() < 0.001);
                if index > 0 {
                    assert!(
                        qr_edge(index, modules, side, scale)
                            > qr_edge(index - 1, modules, side, scale)
                    );
                }
            }
        }
    }

    #[test]
    fn countdown_reads_minutes_and_padded_seconds() {
        assert_eq!(countdown(0), "Code expired");
        assert_eq!(countdown(9), "Expires in 0:09");
        assert_eq!(countdown(14 * 60 + 32), "Expires in 14:32");
        assert_eq!(countdown(15 * 60), "Expires in 15:00");
    }

    #[test]
    fn pairing_links_fit_a_scannable_code() {
        let url = format!("monocode://pair?i={}", "A".repeat(600));
        let code = qr(&url).unwrap();
        assert_eq!(code.dark.len(), code.width * code.width);
        assert!(code.dark.iter().any(|&dark| dark));
        // The three finder patterns start with a dark corner.
        assert!(code.dark[0] && code.dark[code.width - 1]);
        assert!(code.dark[code.width * (code.width - 1)]);
    }

    #[test]
    fn fingerprints_shorten_only_when_long() {
        assert_eq!(short_fingerprint("abcd"), "abcd");
        assert_eq!(
            short_fingerprint("0123456789abcdef0123"),
            "0123456789abcdef…"
        );
    }
}
