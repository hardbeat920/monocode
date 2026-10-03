//! Port of src/features/inbox/ui/InboxNotificationIndicators.test.ts and the
//! list behaviors of InboxView.tsx: read state, selection, source tabs,
//! filters, and the Ask panel.

use std::rc::Rc;

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{draw, init};
use crate::data::*;
use crate::fixtures::{FakeList, FakeServices, NOW, sample_items, sample_list_state};
use crate::list::card::card_label;
use crate::list::view::{InboxView, InboxViewConfig};

struct Harness<'a> {
    services: Rc<FakeServices>,
    list: FakeList,
    view: Entity<InboxView>,
    cx: &'a mut VisualTestContext,
}

fn mount(cx: &mut TestAppContext, list: FakeList) -> Harness<'_> {
    cx.update(init);
    let services = FakeServices::new(NOW);
    let services_dyn: Rc<dyn InboxServices> = services.clone();
    let list_dyn: Rc<dyn InboxListData> = Rc::new(list.clone());
    let (view, cx) = cx.add_window_view(move |window, cx| {
        let mut view = InboxView::new(
            services_dyn,
            list_dyn,
            InboxViewConfig {
                can_close: true,
                can_start: true,
                ..Default::default()
            },
            window,
            cx,
        );
        view.set_animate(false, cx);
        view
    });
    draw(cx);
    Harness {
        services,
        list,
        view,
        cx,
    }
}

fn single(provider: InboxProvider, kind: InboxKind) -> (InboxListState, Vec<ListedItem>) {
    let mut item = InboxItem::github(kind, "acme/app", 42, "Notification indicator regression");
    item.provider = provider;
    item.updated_at = "2030-01-15T11:59:00Z".into();
    item.project_path = "/tmp/app".into();
    if provider == InboxProvider::Linear {
        item.id = Some("linear-42".into());
        item.identifier = Some("ENG-42".into());
    }
    let state = InboxListState {
        cwd: "/tmp/app".into(),
        source: provider,
        visible_sources: vec![provider],
        ..Default::default()
    };
    let listed = ListedItem {
        key: format!("{provider:?}:42"),
        item,
        unseen: true,
        related_sessions: Vec::new(),
        project_mark: Default::default(),
    };
    (state, vec![listed])
}

impl Harness<'_> {
    fn visible(&mut self) -> Vec<ListedItem> {
        self.view
            .read_with(self.cx, |view, cx| view.visible_items(cx))
    }
}

#[gpui::test]
fn reports_a_failed_mark_all_write_in_inbox_and_clears_the_error_after_retry(
    cx: &mut TestAppContext,
) {
    let (state, items) = single(InboxProvider::Github, InboxKind::Issue);
    let list = FakeList::new(state, items);
    *list.fail_writes.borrow_mut() = true;
    let mut h = mount(cx, list);
    assert!(
        h.view
            .read_with(h.cx, |view, _| view.state().source_has_unseen)
    );
    h.view.update(h.cx, |view, cx| view.mark_all_read(cx));
    draw(h.cx);
    assert_eq!(
        h.view
            .read_with(h.cx, |view, _| view.state().read_status_error.clone())
            .as_deref(),
        Some("Could not save read status. Please try again.")
    );
    assert!(h.visible()[0].unseen);
    *h.list.fail_writes.borrow_mut() = false;
    h.view.update(h.cx, |view, cx| view.mark_all_read(cx));
    draw(h.cx);
    assert!(!h.visible()[0].unseen);
    assert!(
        !h.view
            .read_with(h.cx, |view, _| view.state().source_has_unseen)
    );
    assert!(
        h.view
            .read_with(h.cx, |view, _| view.state().read_status_error.clone())
            .is_none()
    );
}

fn keeps_unread_until_opened(cx: &mut TestAppContext, provider: InboxProvider, kind: InboxKind) {
    let (state, items) = single(provider, kind);
    let mut h = mount(cx, FakeList::new(state, items));
    let listed = h.visible()[0].clone();
    assert!(card_label(&listed).contains(", new"));
    h.cx.update(|window, cx| {
        h.view
            .update(cx, |view, cx| view.select(&listed, window, cx))
    });
    draw(h.cx);
    let listed = h.visible()[0].clone();
    assert!(!card_label(&listed).contains(", new"));
}

#[gpui::test]
fn keeps_the_github_issue_visibly_unread_until_opened(cx: &mut TestAppContext) {
    keeps_unread_until_opened(cx, InboxProvider::Github, InboxKind::Issue);
}

#[gpui::test]
fn keeps_the_github_pr_visibly_unread_until_opened(cx: &mut TestAppContext) {
    keeps_unread_until_opened(cx, InboxProvider::Github, InboxKind::Pr);
}

#[gpui::test]
fn keeps_the_gitlab_issue_visibly_unread_until_opened(cx: &mut TestAppContext) {
    keeps_unread_until_opened(cx, InboxProvider::Gitlab, InboxKind::Issue);
}

#[gpui::test]
fn keeps_the_gitlab_pr_visibly_unread_until_opened(cx: &mut TestAppContext) {
    keeps_unread_until_opened(cx, InboxProvider::Gitlab, InboxKind::Pr);
}

#[gpui::test]
fn keeps_the_linear_issue_visibly_unread_until_opened(cx: &mut TestAppContext) {
    keeps_unread_until_opened(cx, InboxProvider::Linear, InboxKind::Linear);
}

#[gpui::test]
fn selects_the_first_card_and_follows_the_source_tab(cx: &mut TestAppContext) {
    let h = mount(cx, FakeList::new(sample_list_state(), sample_items()));
    assert_eq!(
        h.view
            .read_with(h.cx, |view, _| view.selected_key().map(str::to_string)),
        Some("github:monocode/monocode:pr:412".into())
    );
    let detail = h
        .view
        .read_with(h.cx, |view, _| view.detail().cloned())
        .unwrap();
    assert_eq!(
        detail.read_with(h.cx, |detail, _| detail.item().number),
        412
    );
    h.view
        .update(h.cx, |view, cx| view.set_source(InboxProvider::Linear, cx));
    draw(h.cx);
    assert_eq!(
        h.view
            .read_with(h.cx, |view, _| view.selected_key().map(str::to_string)),
        Some("linear:lin-231".into())
    );
    assert!(h.services.calls().contains(&"open_detail #231".to_string()));
}

#[gpui::test]
fn narrows_the_list_with_the_filter_field(cx: &mut TestAppContext) {
    let mut h = mount(cx, FakeList::new(sample_list_state(), sample_items()));
    h.cx.update(|window, cx| {
        h.view
            .update(cx, |view, cx| view.set_search("composer", window, cx))
    });
    draw(h.cx);
    let visible = h.visible();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].item.number, 417);
    assert_eq!(
        h.view
            .read_with(h.cx, |view, _| view.selected_key().map(str::to_string)),
        Some("github:monocode/monocode:issue:417".into())
    );
}

#[gpui::test]
fn opens_the_filter_and_connect_menus(cx: &mut TestAppContext) {
    let h = mount(cx, FakeList::new(sample_list_state(), sample_items()));
    h.view.update(h.cx, |view, cx| {
        view.toggle_filter_menu(cx);
        view.toggle_connect_menu(cx);
    });
    draw(h.cx);
}

#[gpui::test]
fn opens_the_ask_panel_for_the_selected_item(cx: &mut TestAppContext) {
    let mut h = mount(cx, FakeList::new(sample_list_state(), sample_items()));
    h.cx.update(|window, cx| {
        h.view
            .update(cx, |view, cx| view.open_discussion(window, cx))
    });
    draw(h.cx);
    assert!(h.services.calls().contains(&"ask #412".to_string()));
    let panel = h
        .view
        .read_with(h.cx, |view, _| view.discussion().cloned())
        .expect("ask panel");
    assert_eq!(
        panel.read_with(h.cx, |panel, _| panel.session_id().map(str::to_string)),
        Some("ask-412".into())
    );
    let listed = h.visible()[1].clone();
    h.cx.update(|window, cx| {
        h.view
            .update(cx, |view, cx| view.select(&listed, window, cx))
    });
    draw(h.cx);
    assert!(h.services.calls().contains(&"ask #417".to_string()));
}

#[gpui::test]
fn applies_filter_menu_rows_to_the_list(cx: &mut TestAppContext) {
    let h = mount(cx, FakeList::new(sample_list_state(), sample_items()));
    let state = h.view.read_with(h.cx, |view, _| view.state().clone());
    let change = crate::list::filters_menu::apply_filter_action(
        &crate::list::filters_menu::FilterAction::AssignedToMe,
        state.source,
        &state,
    );
    let list = h.list.clone();
    h.cx.update(|_, cx| list.set_filters(change.filters.unwrap(), cx));
    draw(h.cx);
    assert!(
        h.view
            .read_with(h.cx, |view, _| view.state().filters.assigned_to_me)
    );
    assert!(
        h.view
            .read_with(h.cx, |view, _| view.state().filters_active)
    );
}
