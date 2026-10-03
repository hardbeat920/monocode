//! LinkSessionWorkItemDialog.tsx has no test file; these cover its submit
//! rules.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::TestAppContext;

use super::{draw, init};
use crate::data::{LinkedWorkItem, WorkItemKind};
use crate::pr::link_dialog::{LinkDialogEvent, LinkSessionWorkItemDialog};

#[gpui::test]
fn rejects_anything_but_a_github_issue_or_pull_request_url(cx: &mut TestAppContext) {
    cx.update(init);
    let (dialog, cx) = cx.add_window_view(|window, cx| {
        let mut dialog = LinkSessionWorkItemDialog::new(None, "Session".into(), window, cx);
        dialog.set_animate(false);
        dialog
    });
    let events: Rc<RefCell<Vec<LinkDialogEvent>>> = Rc::default();
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&dialog, move |_, event: &LinkDialogEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    draw(cx);
    cx.update(|window, cx| {
        dialog.update(cx, |dialog, cx| {
            dialog.set_url("https://gitlab.com/acme/web/-/issues/3", window, cx);
            dialog.submit(cx);
        })
    });
    draw(cx);
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.error().to_string()),
        "Enter a valid GitHub issue or pull request URL."
    );
    assert!(events.borrow().is_empty());
    cx.update(|window, cx| {
        dialog.update(cx, |dialog, cx| {
            dialog.set_url(" https://github.com/acme/web/pull/42 ", window, cx);
            dialog.submit(cx);
        })
    });
    draw(cx);
    let saved = events.borrow().clone();
    assert_eq!(
        saved,
        [LinkDialogEvent::Save(Some(LinkedWorkItem {
            kind: WorkItemKind::Pr,
            repo: "acme/web".into(),
            number: 42,
            url: "https://github.com/acme/web/pull/42".into(),
            extra: Default::default(),
        }))]
    );
}
