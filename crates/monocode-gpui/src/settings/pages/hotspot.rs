//! Settings › General › "If Wi-Fi drops": the hotspot keep-awake joins when
//! the internet goes while agents work. The name is a preference; the
//! password lives in the Keychain.
use crate::{
    input::{self, Input},
    settings::{SettingsView, widgets::secondary_button},
    theme::*,
};
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use monocode_core::keep_awake::hotspot;

const NAME: &str = "Hotspot name";
const PASSWORD: &str = "Hotspot password";
/// The network picker's value for typing a name it doesn't list.
const OTHER: &str = "\u{0}other";

#[derive(Default)]
pub(in crate::settings) struct HotspotState {
    initialized: bool,
    /// Networks this Mac remembers; `None` until read.
    networks: Option<Vec<String>>,
    loading: bool,
    /// Typing a name rather than picking one.
    other: bool,
    /// Whether the Keychain holds a password for this name.
    password: Option<(String, bool)>,
    checking: bool,
    testing: bool,
    result: Option<Result<SharedString, SharedString>>,
}

impl SettingsView {
    fn init_hotspot(&mut self, cx: &mut Context<Self>) {
        if self.hotspot.initialized {
            return;
        }
        self.hotspot.initialized = true;
        for (key, placeholder, secret) in [
            (NAME, "Network name, e.g. My iPhone", false),
            (PASSWORD, "Password", true),
        ] {
            let input = cx.new(|cx| {
                let mut input = Input::field(key, placeholder, content(0.35), cx);
                input.set_font_size(13.);
                input.set_secret(secret);
                input
            });
            self._subscriptions
                .push(cx.subscribe(&input, |_, _, _: &input::Changed, cx| cx.notify()));
            self._subscriptions.push(cx.subscribe(
                &input,
                move |this, _, _: &input::Submitted, cx| match key {
                    NAME => this.save_hotspot_name(cx),
                    _ => this.save_hotspot_password(cx),
                },
            ));
            self.fields.insert(key, input);
        }
    }

    fn hotspot_field(&self, key: &'static str, cx: &Context<Self>) -> String {
        self.fields
            .get(key)
            .map(|input| input.read(cx).content.trim().to_string())
            .unwrap_or_default()
    }
    fn clear_hotspot_field(&self, key: &'static str, cx: &mut Context<Self>) {
        if let Some(input) = self.fields.get(key) {
            input.update(cx, |input, cx| input.set_text(String::new(), cx));
        }
    }

    fn set_hotspot_name(&mut self, name: String, cx: &mut Context<Self>) {
        self.hotspot.result = None;
        self.update(cx, |p| p.keep_awake.hotspot_ssid = name);
    }
    fn save_hotspot_name(&mut self, cx: &mut Context<Self>) {
        let name = self.hotspot_field(NAME, cx);
        if name.is_empty() {
            return;
        }
        self.hotspot.other = false;
        self.clear_hotspot_field(NAME, cx);
        self.set_hotspot_name(name, cx);
    }

    fn save_hotspot_password(&mut self, cx: &mut Context<Self>) {
        let ssid = self.prefs.keep_awake.hotspot_ssid.trim().to_string();
        let password = self.hotspot_field(PASSWORD, cx);
        if ssid.is_empty() || password.is_empty() {
            return;
        }
        self.clear_hotspot_field(PASSWORD, cx);
        self.hotspot.checking = true;
        let work = cx.background_executor().spawn({
            let ssid = ssid.clone();
            async move {
                #[cfg(target_os = "macos")]
                return hotspot::mac::set_password(&ssid, &password);
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (ssid, password);
                    Err::<(), String>("Only on macOS.".into())
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.hotspot.checking = false;
                match result {
                    Ok(()) => this.hotspot.password = Some((ssid, true)),
                    Err(error) => this.hotspot.result = Some(Err(error.into())),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn forget_hotspot_password(&mut self, cx: &mut Context<Self>) {
        let ssid = self.prefs.keep_awake.hotspot_ssid.trim().to_string();
        #[cfg(target_os = "macos")]
        hotspot::mac::forget_password(&ssid);
        self.hotspot.password = Some((ssid, false));
        cx.notify();
    }

    /// Reads the saved networks once, and the Keychain whenever the name
    /// changes.
    fn load_hotspot(&mut self, cx: &mut Context<Self>) {
        if self.hotspot.networks.is_none() && !self.hotspot.loading {
            self.hotspot.loading = true;
            let work = cx.background_executor().spawn(async {
                if cfg!(test) {
                    return vec!["Home Wi-Fi".to_string(), "My iPhone".to_string()];
                }
                #[cfg(target_os = "macos")]
                return hotspot::mac::saved_networks();
                #[cfg(not(target_os = "macos"))]
                Vec::<String>::new()
            });
            cx.spawn(async move |this, cx| {
                let networks = work.await;
                let _ = this.update(cx, |this, cx| {
                    this.hotspot.loading = false;
                    this.hotspot.networks = Some(networks);
                    cx.notify();
                });
            })
            .detach();
        }
        let ssid = self.prefs.keep_awake.hotspot_ssid.trim().to_string();
        let known = self
            .hotspot
            .password
            .as_ref()
            .is_some_and(|(name, _)| *name == ssid);
        if ssid.is_empty() || known || self.hotspot.checking {
            return;
        }
        self.hotspot.checking = true;
        let work = cx.background_executor().spawn({
            let ssid = ssid.clone();
            async move {
                #[cfg(target_os = "macos")]
                return !cfg!(test) && hotspot::mac::password(&ssid).is_some();
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = ssid;
                    false
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let saved = work.await;
            let _ = this.update(cx, |this, cx| {
                this.hotspot.checking = false;
                this.hotspot.password = Some((ssid, saved));
                cx.notify();
            });
        })
        .detach();
    }

    /// Joins the hotspot, checks the internet, then goes back.
    fn test_hotspot(&mut self, cx: &mut Context<Self>) {
        let ssid = self.prefs.keep_awake.hotspot_ssid.trim().to_string();
        if ssid.is_empty() || self.hotspot.testing {
            return;
        }
        self.hotspot.testing = true;
        self.hotspot.result = None;
        let work = cx.background_executor().spawn({
            let ssid = ssid.clone();
            async move {
                #[cfg(target_os = "macos")]
                {
                    let joined = hotspot::mac::join(&ssid);
                    let _ = hotspot::mac::leave();
                    joined
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = ssid;
                    Err::<(), String>("Only on macOS.".into())
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.hotspot.testing = false;
                this.hotspot.result = Some(match result {
                    Ok(()) => Ok(format!(
                        "Joined {ssid} and reached the internet, then went back to your usual Wi-Fi."
                    )
                    .into()),
                    Err(error) => Err(error.into()),
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn hotspot_input(&self, key: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let input = self.fields[key].clone();
        let save = match key {
            NAME => "Use",
            _ => "Save",
        };
        let empty = self.hotspot_field(key, cx).is_empty();
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(200.))
                    .h(px(28.))
                    .rounded_md()
                    .border_1()
                    .border_color(content(0.12))
                    .px_2()
                    .flex()
                    .items_center()
                    .child(input),
            )
            .child(
                secondary_button(
                    SharedString::from(format!("{key}-save")),
                    save,
                    false,
                    empty,
                )
                .child(save)
                .on_click(cx.listener(move |this, _, _, cx| match key {
                    NAME => this.save_hotspot_name(cx),
                    _ => this.save_hotspot_password(cx),
                })),
            )
            .into_any_element()
    }

    /// The group, or nothing off macOS.
    pub(super) fn hotspot_group(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !crate::keep_awake::KeepAwake::supported() {
            return None;
        }
        let k = self.prefs.keep_awake.clone();
        let mut rows = vec![self.row(
            Some("keep-awake-hotspot"),
            true,
            "Join a hotspot if Wi-Fi drops",
            Some(
                "While agents work and the internet is gone for 15 seconds. Back to your usual Wi-Fi when they finish or you open the lid."
                    .into(),
            ),
            vec![self.toggle(
                "keep-awake-hotspot",
                "Join a hotspot if Wi-Fi drops",
                k.hotspot,
                false,
                cx,
                |this, on, cx| this.update(cx, |p| p.keep_awake.hotspot = on),
            )],
        )];
        let resume = self.row(
            Some("keep-awake-continue"),
            false,
            "Continue cut-off replies",
            Some("Once the internet is back, sends \"continue\" to a reply the drop interrupted. Up to three times in a row.".into()),
            vec![self.toggle(
                "keep-awake-continue",
                "Continue cut-off replies",
                k.continue_after_drop,
                false,
                cx,
                |this, on, cx| this.update(cx, |p| p.keep_awake.continue_after_drop = on),
            )],
        );
        if !k.hotspot {
            rows.push(resume);
            return Some(self.group(None, false, "If Wi-Fi drops", None, None, rows));
        }
        self.init_hotspot(cx);
        self.load_hotspot(cx);
        let ssid = k.hotspot_ssid.trim().to_string();
        let networks = self.hotspot.networks.clone().unwrap_or_default();
        let typing = self.hotspot.other || (self.hotspot.networks.is_some() && networks.is_empty());
        let network = if typing {
            self.hotspot_input(NAME, cx)
        } else {
            let mut options: Vec<(String, SharedString, Option<AnyElement>)> = Vec::new();
            if !ssid.is_empty() && !networks.contains(&ssid) {
                options.push((ssid.clone(), ssid.clone().into(), None));
            }
            options.extend(networks.iter().map(|n| (n.clone(), n.clone().into(), None)));
            options.push((OTHER.into(), "Other name…".into(), None));
            let value = if ssid.is_empty() { "" } else { ssid.as_str() };
            self.select(
                "keep-awake-hotspot-network",
                "Hotspot network",
                value,
                options,
                Some(200.),
                cx,
                |this, value, cx| {
                    if value == OTHER {
                        this.hotspot.other = true;
                        cx.notify();
                    } else {
                        this.set_hotspot_name(value, cx);
                    }
                },
            )
        };
        rows.push(
            self.row(
                Some("keep-awake-hotspot-network"),
                false,
                "Network",
                Some(
                    if self.hotspot.loading {
                        "Reading the networks this Mac knows…"
                    } else {
                        "Your iPhone's name as it shows in the Wi-Fi menu."
                    }
                    .into(),
                ),
                vec![network],
            ),
        );
        if !ssid.is_empty() {
            let saved = self
                .hotspot
                .password
                .as_ref()
                .is_some_and(|(name, saved)| *name == ssid && *saved);
            let (description, control) = if saved {
                (
                    "Saved in your Keychain.",
                    secondary_button("keep-awake-hotspot-forget", "Remove", true, false)
                        .child("Remove")
                        .on_click(cx.listener(|this, _, _, cx| this.forget_hotspot_password(cx)))
                        .into_any_element(),
                )
            } else {
                (
                    "Only if this Mac hasn't joined it before. Kept in your Keychain.",
                    self.hotspot_input(PASSWORD, cx),
                )
            };
            rows.push(self.row(
                Some("keep-awake-hotspot-password"),
                false,
                "Password",
                Some(description.into()),
                vec![control],
            ));
            let testing = self.hotspot.testing;
            let description: SharedString = match &self.hotspot.result {
                Some(Ok(text) | Err(text)) => text.clone(),
                None if testing => "Joining the hotspot…".into(),
                None => "Joins it now, checks the internet, then goes back. Your connection drops for a few seconds.".into(),
            };
            rows.push(self.row(
                Some("keep-awake-hotspot-test"),
                false,
                "Try it",
                Some(description),
                vec![
                    secondary_button("keep-awake-hotspot-test", "Test now", false, testing)
                        .child(if testing { "Testing…" } else { "Test now" })
                        .on_click(cx.listener(|this, _, _, cx| this.test_hotspot(cx)))
                        .into_any_element(),
                ],
            ));
        }
        rows.push(resume);
        Some(self.group(
            None,
            false,
            "If Wi-Fi drops",
            Some("On the iPhone, keep Personal Hotspot › Allow Others to Join on.".into()),
            None,
            rows,
        ))
    }
}
