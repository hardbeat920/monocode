//! Port of src/features/source-control/ui/SwitchBranchDialog.test.ts.

use std::sync::atomic::Ordering;

use gpui::TestAppContext;

use super::support::{pending_generator, setup};
use crate::hooks::ScmHooks;
use crate::ui::dialogs::switch_branch::{SwitchBranchDialog, SwitchBranchEvent};

#[gpui::test]
fn lets_the_switch_dialog_cancel_generation_and_ignores_its_late_result(cx: &mut TestAppContext) {
    let (generator, sender, aborted, _) = pending_generator("unused");
    let setup = setup(
        cx,
        ScmHooks {
            generate_commit_message: Some(generator),
            ..Default::default()
        },
    );
    let scm = setup.scm.clone();
    let (dialog, cx) = cx.add_window_view(move |window, cx| {
        SwitchBranchDialog::new(scm, "/repo", "other", false, window, cx)
    });
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    {
        let events = events.clone();
        cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, event: &SwitchBranchEvent, _| {
                events.borrow_mut().push(event.clone())
            })
            .detach()
        });
    }

    dialog.update_in(cx, |dialog, window, cx| dialog.generate(window, cx));
    cx.run_until_parked();
    assert!(!aborted.load(Ordering::SeqCst));
    assert!(dialog.read_with(cx, |dialog, _| dialog.generating()));

    dialog.update_in(cx, |dialog, window, cx| dialog.cancel_generate(window, cx));
    cx.run_until_parked();
    assert!(aborted.load(Ordering::SeqCst));
    assert!(
        !dialog.read_with(cx, |dialog, _| dialog.generating()),
        "the generate button is back"
    );

    let late = sender
        .borrow_mut()
        .take()
        .unwrap()
        .send("Late message".into());
    assert!(late.is_err());
    cx.run_until_parked();
    assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.message(cx)), "");
    assert!(!events.borrow().contains(&SwitchBranchEvent::Cancel));
}
