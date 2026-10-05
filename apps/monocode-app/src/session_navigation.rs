//! Conversation search and prompt navigation shared by local and remote panes.

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, IntoElement, Keystroke, ParentElement as _,
    Render, Styled as _, Subscription, Window, canvas, div,
};
use monocode_core::{Session, inbox::LinkedWorkItemUpdateStatus, settings::KeybindingOverride};
use monocode_view_transcript::{
    cards::{
        FindSide, PromptOutline, PromptOutlineEvent, TranscriptFind, TranscriptFindEvent,
        jump_to_prompt, prompt_outline::transcript_anchors,
    },
    transcript::{TranscriptView, model::plan::BlockStore},
};

pub struct SessionNavigation {
    transcript: Entity<TranscriptView>,
    find: Entity<TranscriptFind>,
    outline: Entity<PromptOutline>,
    blocks: BlockStore,
    session_id: Option<String>,
    visible: bool,
    focused: bool,
    find_enabled: bool,
    find_side: FindSide,
    return_focus: Option<FocusHandle>,
    _subscriptions: Vec<Subscription>,
}

impl SessionNavigation {
    pub fn new(
        transcript: Entity<TranscriptView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let find = cx.new(|cx| TranscriptFind::new(window, cx));
        let outline = cx.new(PromptOutline::new);
        let target = transcript.clone();
        let find_event = cx.subscribe_in(&find, window, move |this, find, event, window, cx| {
            let TranscriptFindEvent::Navigate { block_id, query } = event;
            target.update(cx, |transcript, cx| {
                transcript.navigate_to_block(block_id.as_deref(), query, cx);
            });
            if !find.read(cx).is_open()
                && this.visible
                && this.focused
                && let Some(focus) = this.return_focus.take()
            {
                window.focus(&focus, cx);
            }
        });
        let target = transcript.clone();
        let outline_event = cx.subscribe(&outline, move |_, _, event, cx| {
            let PromptOutlineEvent::Jump { block_id } = event;
            target.update(cx, |transcript, cx| {
                transcript.navigate_to_block(Some(block_id), "", cx);
                jump_to_prompt(transcript, block_id, cx);
            });
        });
        let transcript_changed = cx.observe(&transcript, |_, _, cx| cx.notify());
        Self {
            transcript,
            find,
            outline,
            blocks: BlockStore::default(),
            session_id: None,
            visible: false,
            focused: false,
            find_enabled: true,
            find_side: FindSide::Right,
            return_focus: None,
            _subscriptions: vec![find_event, outline_event, transcript_changed],
        }
    }

    pub fn set_session(
        &mut self,
        session: Option<&Session>,
        visible: bool,
        focused: bool,
        cx: &mut Context<Self>,
    ) {
        let id = session.map(|session| session.id.clone());
        let switched = self.session_id != id;
        if switched {
            self.session_id = id;
            self.blocks = BlockStore::default();
            self.return_focus = None;
            if self.find.read(cx).is_open() {
                self.find.update(cx, |find, cx| find.close_find(cx));
            }
        }
        let changed = self.blocks.update(
            session
                .map(|session| session.blocks.as_slice())
                .unwrap_or_default(),
        );
        if switched || changed {
            let blocks = self.blocks.blocks().to_vec();
            self.find
                .update(cx, |find, cx| find.set_blocks(blocks.clone(), cx));
            self.outline
                .update(cx, |outline, cx| outline.set_blocks(blocks, cx));
        }
        let find_enabled = session.is_some_and(|session| session.inbox_ask.is_none());
        let side = if session
            .and_then(|session| session.linked_work_item_update_card.as_ref())
            .is_some_and(|card| card.status != LinkedWorkItemUpdateStatus::Loading)
        {
            FindSide::Left
        } else {
            FindSide::Right
        };
        if self.find_side != side {
            self.find_side = side;
            self.find.update(cx, |find, cx| find.set_side(side, cx));
        }
        self.find
            .update(cx, |find, cx| find.set_visible(visible && find_enabled, cx));
        if self.visible != visible || self.focused != focused || self.find_enabled != find_enabled {
            self.visible = visible;
            self.focused = focused;
            self.find_enabled = find_enabled;
            cx.notify();
        }
    }

    /// Called by the pane's capture-phase key handler.
    pub fn handle_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.visible || !self.focused || !self.find_enabled {
            return false;
        }
        let override_ = monocode_app::boot::AppServices::try_global(cx).and_then(|services| {
            monocode_settings::load_app_settings(
                &services.kv,
                monocode_core::platform::Platform::current(),
            )
            .settings
            .keybinding_overrides
            .get("Editor: Find")
            .cloned()
        });
        self.handle_find_key(keystroke, override_.as_ref(), window, cx)
    }

    fn handle_find_key(
        &mut self,
        keystroke: &Keystroke,
        override_: Option<&KeybindingOverride>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = keystroke.modifiers;
        let default_find = (modifiers.platform || modifiers.control)
            && !modifiers.alt
            && !modifiers.shift
            && keystroke.key.eq_ignore_ascii_case("f");
        if override_.is_some_and(|value| value.is_disabled()) && default_find {
            return true;
        }
        let binding = override_
            .filter(|value| !value.is_disabled())
            .and_then(|value| value.shortcut.as_deref())
            .and_then(|shortcut| {
                Keystroke::parse(&crate::shell::keymap::native_chord(shortcut)).ok()
            });
        let was_open = self.find.read(cx).is_open();
        let previous_focus = (!was_open).then(|| window.focused(cx)).flatten();
        let consumed = self.find.update(cx, |find, cx| {
            find.handle_key(keystroke, self.focused, binding.as_ref(), window, cx)
        });
        if !was_open && self.find.read(cx).is_open() {
            self.return_focus = previous_focus;
        }
        consumed
    }
}

impl Render for SessionNavigation {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().into_any_element();
        }
        let transcript = self.transcript.clone();
        let outline = self.outline.clone();
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let (band, anchors, distance) =
                            transcript_anchors(transcript.read(cx), bounds);
                        outline.update(cx, |outline, cx| {
                            outline.set_viewport(band, &anchors, distance, cx)
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(self.outline.clone())
            .child(self.find.clone())
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};
    use monocode_core::{Block, BlockRole, HarnessId};
    use std::sync::Arc;

    fn mount(
        cx: &mut TestAppContext,
    ) -> (
        Entity<SessionNavigation>,
        Entity<TranscriptView>,
        &mut VisualTestContext,
    ) {
        cx.skip_drawing();
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
            monocode_view_transcript::transcript::init(cx);
        });
        let cx = cx.add_empty_window();
        let transcript = cx.new(TranscriptView::new);
        let navigation = cx.update(|window, cx| {
            cx.new(|cx| SessionNavigation::new(transcript.clone(), window, cx))
        });
        let mut session = Session::blank("conversation", HarnessId::Claude, "sonnet", "/repo");
        session.blocks = vec![
            Block::new("first", BlockRole::User, "Find the cat"),
            Block::new("reply", BlockRole::Assistant, "The cat is here"),
            Block::new("second", BlockRole::User, "Find the dog"),
        ];
        transcript.update(cx, |transcript, cx| {
            transcript.set_session(Arc::new(session.clone()), cx)
        });
        navigation.update(cx, |navigation, cx| {
            navigation.set_session(Some(&session), true, true, cx)
        });
        (navigation, transcript, cx)
    }

    fn selected(transcript: &Entity<TranscriptView>, cx: &VisualTestContext) -> Option<String> {
        transcript.read_with(cx, |transcript, _| {
            transcript
                .rows()
                .iter()
                .find(|row| row.search_current)
                .and_then(|row| row.first_block())
                .map(|block| block.id.clone())
        })
    }

    #[gpui::test]
    fn find_and_outline_navigate_the_real_transcript(cx: &mut TestAppContext) {
        let (navigation, transcript, cx) = mount(cx);
        let find = navigation.read_with(cx, |navigation, _| navigation.find.clone());
        find.update(cx, |find, cx| {
            find.open(cx);
            find.set_query("cat".into(), cx);
        });
        cx.run_until_parked();
        assert_eq!(selected(&transcript, cx).as_deref(), Some("first"));
        find.update(cx, |find, cx| find.step(1, cx));
        cx.run_until_parked();
        assert_eq!(selected(&transcript, cx).as_deref(), Some("reply"));
        find.update(cx, |find, cx| find.close_find(cx));
        cx.run_until_parked();
        assert!(selected(&transcript, cx).is_none());
        let outline = navigation.read_with(cx, |navigation, _| navigation.outline.clone());
        outline.update(cx, |outline, cx| outline.jump_to("second", cx));
        cx.run_until_parked();
        assert_eq!(selected(&transcript, cx).as_deref(), Some("second"));
    }

    #[gpui::test]
    fn find_keys_respect_disabled_custom_and_hidden_panes(cx: &mut TestAppContext) {
        let (navigation, _, cx) = mount(cx);
        let default = Keystroke::parse("secondary-f").unwrap();
        let custom = Keystroke::parse("secondary-shift-h").unwrap();
        let disabled = KeybindingOverride {
            disabled: Some(true),
            shortcut: None,
        };
        navigation.update_in(cx, |navigation, window, cx| {
            assert!(navigation.handle_find_key(&default, Some(&disabled), window, cx));
            assert!(!navigation.find.read(cx).is_open());
            let override_ = KeybindingOverride {
                disabled: None,
                shortcut: Some(
                    if cfg!(target_os = "macos") {
                        "Command+Shift+KeyH"
                    } else {
                        "Control+Shift+KeyH"
                    }
                    .into(),
                ),
            };
            assert!(navigation.handle_find_key(&default, Some(&override_), window, cx));
            assert!(!navigation.find.read(cx).is_open());
            assert!(navigation.handle_find_key(&custom, Some(&override_), window, cx));
            assert!(navigation.find.read(cx).is_open());
            navigation.visible = false;
            assert!(!navigation.handle_key(&default, window, cx));
            navigation.visible = true;
            navigation.focused = false;
            assert!(!navigation.handle_key(&default, window, cx));
        });
    }

    #[gpui::test]
    fn closing_find_restores_the_previous_focus(cx: &mut TestAppContext) {
        let (navigation, _, cx) = mount(cx);
        let previous = cx.update(|window, cx| {
            let previous = cx.focus_handle();
            window.focus(&previous, cx);
            previous
        });
        navigation.update_in(cx, |navigation, window, cx| {
            assert!(navigation.handle_key(&Keystroke::parse("secondary-f").unwrap(), window, cx));
            assert!(!previous.is_focused(window));
            assert!(navigation.handle_key(&Keystroke::parse("escape").unwrap(), window, cx));
        });
        cx.run_until_parked();
        cx.update(|window, _| assert!(previous.is_focused(window)));
    }
}
