//! The row control that chooses the model writing a kind of text, which
//! opens the same model picker as a conversation's composer.
use super::{ModelChoice, Preferences, SettingsView};
use crate::{
    pickers::{ModelList, ModelPicked, cached_model_name},
    popover::{Align, Anchor, Side, popover},
    theme::*,
};
use gpui::{AnyElement, Context, Role, SharedString, Window, div, prelude::*, px};
use monocode_core::provider::Kind;

impl SettingsView {
    /// A trigger showing `current` (else `none`) that opens the model picker,
    /// with a reset beside it while a model is chosen.
    pub(super) fn text_model_picker(
        &self,
        id: &'static str,
        label: &'static str,
        current: &Option<ModelChoice>,
        none: &'static str,
        cx: &mut Context<Self>,
        set: fn(&mut Preferences, Option<ModelChoice>),
    ) -> AnyElement {
        let open = self.select_open == Some(id) && self.text_picker.is_some();
        let slot = self
            .select_anchors
            .borrow_mut()
            .entry(id)
            .or_default()
            .clone();
        let (kind, text) = match current {
            Some(choice) => {
                let kind = Kind::from_id(&choice.provider);
                (Some(kind), cached_model_name(kind, choice.model.as_deref()))
            }
            None => (None, SharedString::from(none)),
        };
        let weak = cx.weak_entity();
        let chosen = current.clone();
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .relative()
                    .w(px(240.))
                    .child(
                        div()
                            .id(SharedString::from(format!("text-model-{id}")))
                            .debug_selector(move || format!("settings-text-model-{id}"))
                            .role(Role::ComboBox)
                            .aria_label(format!("{label}: {text}"))
                            .aria_expanded(open)
                            .relative()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .rounded_md()
                            .border_1()
                            .border_color(content(0.1))
                            .bg(content(0.05))
                            .px_2()
                            .py_1()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(content(1.))
                            .cursor_pointer()
                            .hover(|s| s.border_color(content(0.2)))
                            .child(slot.probe())
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .when_some(kind, |d, kind| {
                                        d.child(provider_icon(kind.id(), 14.))
                                    })
                                    .child(div().min_w_0().truncate().child(text)),
                            )
                            .child(icon("chevron-down-hi", 14.).text_color(content(0.5)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.select_open == Some(id) {
                                    this.select_open = None;
                                    this.text_picker = None;
                                } else {
                                    this.open_text_picker(id, chosen.clone(), set, window, cx);
                                }
                                cx.notify();
                            })),
                    )
                    .when(open, |d| {
                        let Some((list, _)) = &self.text_picker else {
                            return d;
                        };
                        d.child(
                            popover(
                                SharedString::from(format!("text-model-popover-{id}")),
                                Anchor::Slot(slot),
                            )
                            .side(Side::Bottom)
                            .align(Align::End)
                            .width(310.)
                            .height(442.)
                            .padding(0.)
                            .on_dismiss(move |window, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.select_open = None;
                                    this.text_picker = None;
                                    window.focus(&this.focus, cx);
                                    cx.notify();
                                });
                            })
                            .child(list.clone()),
                        )
                    }),
            )
            .when(current.is_some(), |d| {
                d.child(
                    div()
                        .id(SharedString::from(format!("text-model-reset-{id}")))
                        .role(Role::Button)
                        .aria_label(format!("Reset {label}"))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|s| s.bg(content(0.1)))
                        .child(icon("x", 12.))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.update(cx, |p| set(p, None))),
                        ),
                )
            })
            .into_any_element()
    }

    fn open_text_picker(
        &mut self,
        id: &'static str,
        current: Option<ModelChoice>,
        set: fn(&mut Preferences, Option<ModelChoice>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Titles only run on providers session_title can drive.
        let titles = id == "title-model";
        let fallback = self
            .prefs
            .default_provider
            .as_deref()
            .and_then(Kind::parse)
            .filter(|kind| !titles || monocode_core::session_title::supports(*kind))
            .unwrap_or(Kind::Claude);
        let unsupported = Kind::ALL
            .into_iter()
            .filter(move |kind| titles && !monocode_core::session_title::supports(*kind))
            .map(|kind| kind.id().to_string());
        let cwd = self.ctx.cwd.clone();
        let list = cx.new(|cx| {
            ModelList::for_choice(current.as_ref(), fallback, &cwd, cx)
                .also_hide_providers(unsupported)
        });
        let subscription = cx.subscribe_in(
            &list,
            window,
            move |this, _, picked: &ModelPicked, window, cx| {
                let choice = ModelChoice {
                    provider: picked.0.id().into(),
                    model: picked.1.clone(),
                };
                this.select_open = None;
                this.text_picker = None;
                window.focus(&this.focus, cx);
                this.update(cx, |p| set(p, Some(choice)));
            },
        );
        window.focus(&list.read(cx).search_focus(cx), cx);
        self.select_open = Some(id);
        self.text_picker = Some((list, subscription));
    }
}
