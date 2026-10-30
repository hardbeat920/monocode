//! The APNs key that keeps a phone's Live Activity current with the app
//! closed. Without one the phone redraws it only while MonoCode is open.
use crate::{
    input::{self, Input},
    settings::{
        SettingsEvent, SettingsView,
        widgets::{red, secondary_button},
    },
    theme::*,
};
use gpui::{AnyElement, Context, Entity, Role, SharedString, Subscription, div, prelude::*, px};
use monocode_core::mobile::{ApnsKey, Status};

#[derive(Default)]
pub(in crate::settings) struct PushKeyState {
    open: bool,
    /// Team ID, key ID and the .p8 file's path.
    fields: Option<[Entity<Input>; 3]>,
    _events: Vec<Subscription>,
}

const LABELS: [(&str, &str); 3] = [
    ("Team ID", "10 characters, from Membership details"),
    ("Key ID", "10 characters, from Keys"),
    ("Key file", "Path to AuthKey_XXXXXXXXXX.p8"),
];

impl SettingsView {
    fn push_key_fields(&mut self, status: &Status, cx: &mut Context<Self>) -> [Entity<Input>; 3] {
        if let Some(fields) = &self.mobile.push_key.fields {
            return fields.clone();
        }
        let key = status.apns.clone().unwrap_or_default();
        let initial = [key.team_id, key.key_id, key.key_path.display().to_string()];
        let fields: [Entity<Input>; 3] = std::array::from_fn(|ix| {
            let (label, placeholder) = LABELS[ix];
            let text = initial[ix].clone();
            cx.new(|cx| {
                let mut input = Input::field(label, placeholder, content(0.35), cx);
                input.set_font_size(12.);
                input.set_text(text, cx);
                input
            })
        });
        self.mobile.push_key._events = fields
            .iter()
            .flat_map(|field| {
                [
                    cx.subscribe(field, |_, _, _: &input::Changed, cx| cx.notify()),
                    cx.subscribe(field, |this, _, _: &input::Submitted, cx| {
                        this.save_push_key(cx)
                    }),
                ]
            })
            .collect();
        self.mobile.push_key.fields = Some(fields.clone());
        fields
    }

    fn typed_push_key(&self, cx: &Context<Self>) -> Option<ApnsKey> {
        let fields = self.mobile.push_key.fields.as_ref()?;
        let [team, key, path] = fields
            .each_ref()
            .map(|f| f.read(cx).content.trim().to_owned());
        Some(ApnsKey {
            team_id: team.to_ascii_uppercase(),
            key_id: key.to_ascii_uppercase(),
            key_path: path.into(),
        })
    }

    fn save_push_key(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.typed_push_key(cx) {
            cx.emit(SettingsEvent::MobilePushKey(Some(key)));
        }
    }

    fn remove_push_key(&mut self, cx: &mut Context<Self>) {
        if let Some(fields) = self.mobile.push_key.fields.take() {
            for field in fields {
                field.update(cx, |input, cx| input.set_text(String::new(), cx));
            }
        }
        cx.emit(SettingsEvent::MobilePushKey(None));
    }

    pub(super) fn mobile_push_key(
        &mut self,
        status: &Status,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let open = self.mobile.push_key.open;
        let mut page = vec![
            div()
                .id("mobile-push-key")
                .role(Role::Button)
                .aria_label("Live Activity updates")
                .aria_expanded(open)
                .mt_5()
                .text_size(px(12.))
                .text_color(content(0.45))
                .cursor_pointer()
                .child(if open {
                    "▾ Live Activity updates"
                } else {
                    "▸ Live Activity updates"
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.mobile.push_key.open = !this.mobile.push_key.open;
                    cx.notify();
                }))
                .into_any_element(),
        ];
        if !open {
            return page;
        }
        let fields = self.push_key_fields(status, cx);
        let typed = self.typed_push_key(cx);
        let unsaved = typed.as_ref() != status.apns.as_ref()
            && typed.as_ref().is_some_and(|key| !key.team_id.is_empty());
        let paragraph = |text: SharedString| {
            div()
                .text_size(px(12.))
                .line_height(px(19.5))
                .text_color(content(0.45))
                .child(text)
        };
        let state: SharedString = match (&status.apns, &status.apns_error) {
            (_, Some(error)) => error.clone().into(),
            (Some(key), None) => {
                format!("On: Apple delivers updates with key {}.", key.key_id).into()
            }
            (None, None) => {
                "Off: the activity updates only while MonoCode is open on the phone.".into()
            }
        };
        page.push(
            div()
                .mt_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(paragraph(
                    "A phone’s Live Activity follows your sessions with the app closed when this desktop sends its updates through Apple. That takes an APNs key from your Apple Developer account (Certificates, Identifiers & Profiles → Keys) with Apple Push Notifications enabled.".into(),
                ))
                .children(fields.into_iter().zip(LABELS).map(|(field, (label, _))| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .text_size(px(12.))
                        .text_color(content(0.65))
                        .child(label)
                        .child(
                            div()
                                .h(px(32.))
                                .rounded_md()
                                .border_1()
                                .border_color(content(0.1))
                                .px_2()
                                .flex()
                                .items_center()
                                .child(field),
                        )
                }))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            secondary_button("mobile-save-push-key", "Save", false, !unsaved)
                                .child("Save")
                                .when(unsaved, |d| {
                                    d.on_click(cx.listener(|this, _, _, cx| this.save_push_key(cx)))
                                }),
                        )
                        .when(status.apns.is_some(), |d| {
                            d.child(
                                secondary_button("mobile-remove-push-key", "Remove key", true, false)
                                    .child("Remove key")
                                    .on_click(cx.listener(|this, _, _, cx| this.remove_push_key(cx))),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(19.5))
                        .text_color(if status.apns_error.is_some() {
                            red(1.)
                        } else {
                            content(0.45)
                        })
                        .child(state),
                )
                .into_any_element(),
        );
        page
    }
}
