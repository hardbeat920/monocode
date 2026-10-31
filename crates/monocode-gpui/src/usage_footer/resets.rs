use super::*;
use crate::{
    accounts::resets::{ResetRow, rows},
    settings::widgets::{primary_button, secondary_button},
};

#[derive(Clone)]
enum Command {
    Confirm(String),
    Cancel,
    Apply(String),
}

impl UsageFooter {
    pub(super) fn has_banked_resets(&self) -> bool {
        self.kind == Kind::Codex
            && self.cache[index(self.kind)]
                .snapshot
                .as_ref()
                .and_then(|s| s.reset_credits.as_ref())
                .is_some_and(|credits| credits.available_count > 0)
    }

    pub(super) fn banked_resets(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.kind != Kind::Codex {
            return None;
        }
        let snapshot = self.cache[index(self.kind)].snapshot.as_ref();
        let rows = rows(snapshot);
        let center = crate::accounts::get(cx);
        let action = center
            .as_ref()
            .and_then(|c| c.read(cx).reset_actions.get(&self.key))
            .cloned();
        let action = action.as_ref();
        if rows.is_empty() && action.is_none() {
            return None;
        }
        let count = snapshot
            .and_then(|s| s.reset_credits.as_ref())
            .map_or(0, |c| c.available_count);
        let busy = action.is_some_and(|a| a.applying());
        Some(
            div()
                .id("banked-resets")
                .debug_selector(|| "banked-resets".into())
                .mt(px(10.))
                .pt(px(10.))
                .border_t_1()
                .border_color(content(0.08))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .font_weight(FontWeight::MEDIUM)
                        .child("Banked resets")
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(content(0.5))
                                .child(format!("{count} available")),
                        ),
                )
                .child(
                    div()
                        .id("banked-reset-cards")
                        .max_h(px(224.))
                        .overflow_y_scroll()
                        .mt_2()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .children(rows.iter().map(|row| {
                            let selected = action.is_some_and(|a| a.row.as_ref() == Some(&row.key));
                            let spent = selected
                                && action
                                    .is_some_and(|a| a.result.as_ref().is_some_and(Result::is_ok));
                            self.reset_card(
                                row,
                                selected && busy,
                                busy || spent || center.is_none(),
                                cx,
                            )
                        })),
                )
                .children(action.and_then(|a| a.message()).map(|message| {
                    div()
                        .id("banked-reset-result")
                        .role(Role::Status)
                        .mt_2()
                        .text_size(px(10.))
                        .line_height(px(16.))
                        .text_color(
                            if action.is_some_and(|a| a.result.as_ref().is_some_and(Result::is_err))
                            {
                                rgb(0xfcd34d).into()
                            } else {
                                content(0.65)
                            },
                        )
                        .child(message.to_owned())
                }))
                .into_any_element(),
        )
    }

    fn reset_card(
        &self,
        row: &ResetRow,
        applying: bool,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let confirming = self.confirm_reset.as_ref() == Some(&row.key) && !disabled;
        let now = usage::now_ms();
        let expiry = match row.credit.as_ref().and_then(|c| c.expires_at) {
            None => "Expiry not provided".into(),
            Some(at) if at <= now => "Expires now".into(),
            Some(at) => format!("Expires in {}", usage::duration(at - now)),
        };
        div()
            .id(gpui::SharedString::from(format!(
                "banked-reset-{}",
                row.key
            )))
            .flex_shrink_0()
            .rounded_lg()
            .bg(content(0.045))
            .border_1()
            .border_color(content(0.06))
            .px(px(10.))
            .py_2()
            .child(
                div()
                    .text_size(px(10.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(row.title.clone()),
            )
            .children(
                row.credit
                    .as_ref()
                    .and_then(|c| c.description.as_ref())
                    .map(|description| {
                        div()
                            .mt(px(2.))
                            .text_size(px(10.))
                            .line_height(px(16.))
                            .text_color(content(0.45))
                            .child(description.clone())
                    }),
            )
            .child(
                div()
                    .mt(px(6.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(10.))
                            .text_color(content(0.4))
                            .child(expiry),
                    )
                    .when(!confirming, |d| {
                        if applying {
                            d.child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(content(0.45))
                                    .child("Applying…"),
                            )
                        } else {
                            d.child(self.reset_button(
                                &row.key,
                                "Use reset",
                                Command::Confirm(row.key.clone()),
                                disabled,
                                cx,
                            ))
                        }
                    }),
            )
            .when(confirming, |d| {
                d.child(
                    div()
                        .mt_2()
                        .pt_2()
                        .border_t_1()
                        .border_color(content(0.07))
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(content(0.5))
                                .child("Spend this reset now?"),
                        )
                        .child(
                            div()
                                .mt(px(6.))
                                .flex()
                                .justify_end()
                                .gap_1()
                                .child(self.reset_button(
                                    &row.key,
                                    "Cancel",
                                    Command::Cancel,
                                    false,
                                    cx,
                                ))
                                .child(self.reset_button(
                                    &row.key,
                                    "Confirm",
                                    Command::Apply(row.key.clone()),
                                    false,
                                    cx,
                                )),
                        ),
                )
            })
            .into_any_element()
    }

    fn reset_button(
        &self,
        row: &str,
        label: &'static str,
        command: Command,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let id =
            gpui::SharedString::from(format!("banked-reset-{}-{row}", label.replace(' ', "-")));
        let selector = id.clone();
        let button = if label == "Confirm" {
            primary_button(id, label, disabled)
        } else {
            secondary_button(id, label, false, disabled)
        };
        button
            .debug_selector(move || selector.to_string())
            .h_6()
            .py_0()
            .px_2()
            .text_size(px(10.))
            .child(label)
            .when(!disabled, |d| {
                let click = command.clone();
                d.focusable()
                    .tab_index(0)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.reset_command(click.clone(), window, cx)
                    }))
                    .on_key_down(cx.listener(
                        move |this, event: &gpui::KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                cx.stop_propagation();
                                this.reset_command(command.clone(), window, cx);
                            }
                        },
                    ))
            })
    }

    fn reset_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        let Some(center) = crate::accounts::get(cx) else {
            return;
        };
        if self.kind != Kind::Codex
            || center
                .read(cx)
                .reset_actions
                .get(&self.key)
                .is_some_and(|a| a.applying())
        {
            return;
        }
        match command {
            Command::Confirm(row) => self.confirm_reset = Some(row),
            Command::Cancel => self.confirm_reset = None,
            Command::Apply(row) => {
                if self.confirm_reset.as_ref() != Some(&row) {
                    return;
                }
                self.confirm_reset = None;
                center.update(cx, |center, cx| {
                    center.consume_reset(self.key.clone(), &row, cx)
                });
            }
        }
        window.focus(&self.details, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::VisualTestContext;
    use std::{
        path::Path,
        sync::{Arc, Mutex},
    };

    #[gpui::test]
    fn banked_cards_require_confirmation_cancel_without_claiming_and_hide_when_empty(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = Arc::new(Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        cx.update(|cx| crate::accounts::install(store, cx));
        let center = cx.update(|cx| crate::accounts::get(cx).unwrap());
        let key = Key::new(Kind::Codex, Default::default(), None);
        center.update(cx, |accounts, _| {
            accounts
                .usage
                .entry(key.clone())
                .or_default()
                .complete(Ok(Snapshot {
                    reset_credits: Some(usage::ResetCredits {
                        available_count: 2,
                        credits: vec![usage::ResetCredit {
                            id: "one".into(),
                            status: "available".into(),
                            reset_type: "codexRateLimits".into(),
                            granted_at: None,
                            expires_at: Some(usage::now_ms() + 3_600_000),
                            title: Some("A bonus reset".into()),
                            description: Some("One Codex usage reset".into()),
                        }],
                    }),
                    ..Default::default()
                }));
        });
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| UsageFooter::new(Kind::Codex, cx))
            })
            .unwrap()
        });
        window
            .update(cx, |footer, window, cx| {
                assert!(footer.has_banked_resets());
                footer.toggle(window, cx);
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("banked-resets").is_some());
        assert!(
            visual
                .debug_bounds("banked-reset-Use-reset-unlisted:1")
                .is_some()
        );
        let use_reset = visual
            .debug_bounds("banked-reset-Use-reset-credit:one")
            .unwrap();
        visual.simulate_click(use_reset.center(), Default::default());
        visual.refresh().unwrap();
        assert!(
            visual
                .debug_bounds("banked-reset-Confirm-credit:one")
                .is_some()
        );
        assert!(center.read_with(visual, |accounts, _| accounts.reset_actions.is_empty()));
        let cancel = visual
            .debug_bounds("banked-reset-Cancel-credit:one")
            .unwrap();
        visual.simulate_click(cancel.center(), Default::default());
        visual.refresh().unwrap();
        window
            .update(visual, |footer, window, cx| {
                assert!(footer.confirm_reset.is_none());
                footer.reset_command(Command::Apply("credit:one".into()), window, cx);
            })
            .unwrap();
        assert!(center.read_with(visual, |accounts, _| accounts.reset_actions.is_empty()));
        center.update(visual, |accounts, cx| {
            accounts
                .usage
                .get_mut(&key)
                .unwrap()
                .complete(Ok(Snapshot::default()));
            cx.notify();
        });
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("banked-resets").is_none());
    }
}
