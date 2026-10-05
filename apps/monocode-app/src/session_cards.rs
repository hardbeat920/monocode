//! Composer context cards and session notices over the engine's callbacks.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Subscription,
    WeakEntity, Window, div,
};
use monocode_core::{
    Session,
    handoff::HandoffComposerCard,
    inbox::InboxComposerCard,
    notes::{NoteComposerCard, note_card_meta},
};
use monocode_engine::{
    attention::{Attention, UsageLimits},
    history::HistoryPackage,
    inbox::inbox::Inbox,
    runtime::Engine,
    workspace::Workspace,
};
use monocode_view_composer::composer::Composer;
use monocode_view_inbox::list::mini_card::inbox_mini_card;
use monocode_view_inbox::pr::linked_notice::{LinkedWorkItemUpdateNotice, NoticeHandlers};
use monocode_view_pages::{ProjectsData as _, notes::note_mini_card};
use monocode_view_transcript::threads::{HandoffCard, handoff_mini_card};
use monocode_view_workbench::panes::notices::{UsageLimit, UsageLimitEvent, UsageLimitNotice};

#[derive(Default, PartialEq)]
struct ContextCards {
    inbox: Option<InboxComposerCard>,
    note: Option<NoteComposerCard>,
    handoff: Option<HandoffComposerCard>,
}

pub struct ComposerCards {
    session_id: String,
    cards: ContextCards,
}
impl ComposerCards {
    pub fn new(session_id: String, _: &mut Context<Self>) -> Self {
        Self {
            session_id,
            cards: ContextCards::default(),
        }
    }
    pub fn set_session(&mut self, session: Option<&Session>, cx: &mut Context<Self>) {
        let cards = ContextCards {
            inbox: session.and_then(|s| s.inbox_card.clone()),
            note: session.and_then(|s| s.note_card.clone()),
            handoff: session.and_then(|s| s.handoff_card.clone()),
        };
        if cards != self.cards {
            self.cards = cards;
            cx.notify();
        }
    }
}
impl Render for ComposerCards {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut cards = div();
        if let Some(card) = self.cards.inbox.clone() {
            let id = self.session_id.clone();
            cards = cards.child(
                inbox_mini_card(card)
                    .on_open(|url, _, cx| cx.open_url(url))
                    .on_dismiss(move |_, cx| {
                        Inbox::global(cx).update(cx, |inbox, cx| inbox.dismiss_inbox_card(&id, cx));
                    }),
            );
        }
        if let Some(card) = self.cards.note.as_ref() {
            let id = self.session_id.clone();
            let mark = card.source_cwd.as_deref().and_then(|cwd| {
                crate::adapters::projects_data::AppProjectsData::new(cx)
                    .map(|data| data.mark(cwd, cx))
            });
            cards = cards.child(note_mini_card(note_card_meta(card), mark).on_dismiss(
                move |_, cx| {
                    Engine::sessions(cx).update(cx, |sessions, cx| {
                        sessions.update(&id, cx, |session| session.note_card = None);
                    });
                },
            ));
        }
        if let Some(card) = self.cards.handoff.as_ref() {
            let id = self.session_id.clone();
            cards = cards.child(
                handoff_mini_card("composer-handoff-dismiss", HandoffCard::from(card)).on_dismiss(
                    move |_, _, cx| {
                        Engine::sessions(cx).update(cx, |sessions, cx| {
                            sessions.update(&id, cx, |session| session.handoff_card = None);
                        });
                    },
                ),
            );
        }
        cards
    }
}

pub struct ComposerHeaders {
    session_id: String,
    limit: Option<monocode_core::session::UsageLimit>,
    notice: Option<Entity<UsageLimitNotice>>,
    subscription: Option<Subscription>,
}
impl ComposerHeaders {
    pub fn new(session_id: String, _: &mut Context<Self>) -> Self {
        Self {
            session_id,
            limit: None,
            notice: None,
            subscription: None,
        }
    }
    pub fn set_session(&mut self, session: Option<&Session>, cx: &mut Context<Self>) {
        let limit = session.and_then(|s| s.usage_limit);
        if limit == self.limit {
            return;
        }
        self.limit = limit;
        let Some(limit) = limit else {
            self.notice = None;
            self.subscription = None;
            cx.notify();
            return;
        };
        let limit = UsageLimit {
            resets_at: limit.resets_at,
            resume_at_reset: limit.resume_at_reset == Some(true),
        };
        if let Some(notice) = self.notice.as_ref() {
            notice.update(cx, |notice, cx| notice.set_limit(limit, cx));
        } else {
            let notice = cx.new(|cx| {
                UsageLimitNotice::new(
                    limit,
                    Rc::new(monocode_engine::attention::usage_limit::format_usage_limit_reset),
                    cx,
                )
            });
            let id = self.session_id.clone();
            self.subscription = Some(cx.subscribe(&notice, move |_, _, event, cx| match event {
                UsageLimitEvent::Resume => UsageLimits::resume(&id, cx),
                UsageLimitEvent::ResumeAtReset(enabled) => {
                    UsageLimits::set_resume_at_reset(&id, *enabled, cx)
                }
                UsageLimitEvent::Dismiss => UsageLimits::dismiss(&id, cx),
            }));
            self.notice = Some(notice);
        }
        cx.notify();
    }
}
impl Render for ComposerHeaders {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().children(self.notice.iter().cloned())
    }
}

/// The linked activity notice belongs inside the transcript's relative container.
pub struct LinkedActivity {
    notice: Entity<LinkedWorkItemUpdateNotice>,
}
impl LinkedActivity {
    pub fn new(
        session_id: String,
        workspace: WeakEntity<Workspace>,
        composer: WeakEntity<Composer>,
        cx: &mut Context<Self>,
    ) -> Self {
        let sessions = Engine::sessions(cx);
        let id = session_id.clone();
        let acknowledge = Rc::new(move |_: &mut Window, cx: &mut App| {
            let at = sessions
                .read(cx)
                .get(&id)
                .and_then(|s| s.linked_work_item_update_card.as_ref())
                .map(|card| card.updated_at);
            if let Some(at) = at {
                Inbox::global(cx).update(cx, |inbox, cx| {
                    inbox.mark_linked_session_update_seen(&id, at, cx)
                });
            }
        });
        let id = session_id.clone();
        let dismiss = Rc::new(move |_: &mut Window, cx: &mut App| {
            Inbox::global(cx).update(cx, |inbox, cx| {
                inbox.dismiss_linked_work_item_update_card(&id, cx)
            });
        });
        let id = session_id.clone();
        let open = Rc::new(move |_: &mut Window, cx: &mut App| {
            let item = Engine::sessions(cx)
                .read(cx)
                .get(&id)
                .and_then(|s| s.linked_work_item.clone());
            if let Some(item) = item {
                Inbox::global(cx)
                    .update(cx, |inbox, cx| inbox.open_linked_work_item(item, &id, cx));
            }
        });
        let id = session_id.clone();
        let archive = Rc::new(move |cx: &mut App| {
            let task = HistoryPackage::history(cx)
                .update(cx, |history, cx| history.archive_session(&id, true, cx));
            cx.spawn(async move |_| Ok(task.await))
        });
        let id = session_id.clone();
        let delete = Rc::new(move |cx: &mut App| {
            let task = HistoryPackage::history(cx)
                .update(cx, |history, cx| history.delete_session(&id, cx));
            cx.spawn(async move |_| Ok(task.await))
        });
        let handlers = NoticeHandlers {
            on_acknowledge: acknowledge,
            on_dismiss: dismiss,
            on_open_discussion: open,
            on_add_to_chat: Rc::new(move |card, window, cx| {
                let text = monocode_engine::inbox::linked_work_item_activity::linked_work_item_activity_prompt(card);
                let _ = composer.update(cx, |composer, cx| {
                    let draft = composer.draft();
                    let value = if draft.trim().is_empty() {
                        text
                    } else {
                        format!("{draft}\n\n{text}")
                    };
                    composer.set_text(&value, cx);
                    composer.focus(window, cx);
                });
            }),
            on_announce: Some(Rc::new(|id, card, cx| {
                let notifier = Attention::global(cx).notifier.clone();
                notifier.update(cx, |notifier, _| {
                    notifier.announce_linked_activity(id, Some(card))
                });
            })),
            on_archive_session: Some(archive),
            on_delete_session: Some(delete),
        };
        let workspace = workspace
            .upgrade()
            .expect("Session cards belong to an open workspace");
        let services = Rc::new(crate::adapters::inbox::InboxAdapter::new(workspace, cx));
        let notice =
            cx.new(|cx| LinkedWorkItemUpdateNotice::new(services, session_id, None, handlers, cx));
        Self { notice }
    }
    pub fn set_session(&mut self, session: Option<&Session>, cx: &mut Context<Self>) {
        let id = session.map_or_else(String::new, |s| s.id.clone());
        let card = session.and_then(|s| s.linked_work_item_update_card.clone());
        self.notice
            .update(cx, |notice, cx| notice.set_card(id, card, cx));
    }
}
impl Render for LinkedActivity {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.notice.clone()
    }
}
