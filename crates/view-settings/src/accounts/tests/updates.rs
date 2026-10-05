//! Port of HarnessUpdateNotice.test.ts.

use gpui::{AppContext as _, TestAppContext};

use super::*;
use crate::accounts::harness_update_notice::{HarnessUpdateNotice, RowState};
use crate::accounts::host::{HarnessUpdateHost, OnHarnessUpdated};

type Listener = Rc<dyn Fn(HarnessId, &mut App)>;

/// A fake host: Claude is installed at 2.1.284, its updater installs
/// 2.1.285, and other windows can announce updates.
pub(super) struct FakeUpdates {
    pub installed: RefCell<String>,
    pub refreshed: RefCell<Vec<HarnessId>>,
    pub announced: RefCell<Vec<HarnessId>>,
    pub dismissed: Cell<usize>,
    pub updater_installs: Cell<bool>,
    listeners: RefCell<Vec<Listener>>,
}

impl Default for FakeUpdates {
    fn default() -> Self {
        Self {
            installed: RefCell::new("2.1.284 (Claude Code)".into()),
            refreshed: RefCell::default(),
            announced: RefCell::default(),
            dismissed: Cell::new(0),
            updater_installs: Cell::new(true),
            listeners: RefCell::default(),
        }
    }
}

impl FakeUpdates {
    /// Another window says it updated `harness`.
    fn broadcast(&self, harness: HarnessId, cx: &mut App) {
        let listeners: Vec<Listener> = self.listeners.borrow().clone();
        for listener in listeners {
            listener(harness, cx);
        }
    }
}

impl HarnessUpdateHost for FakeUpdates {
    fn check_for_updates(&self, _: &mut App) -> Task<Vec<HarnessUpdate>> {
        Task::ready(vec![HarnessUpdate {
            harness: HarnessId::Claude,
            installed: "2.1.284".into(),
            latest: "2.1.285".into(),
        }])
    }

    fn dismiss_updates(&self, _: &mut App) {
        self.dismissed.set(self.dismissed.get() + 1);
    }

    fn update_cli(&self, _: HarnessId, _: &mut App) -> HostTask<()> {
        if self.updater_installs.get() {
            *self.installed.borrow_mut() = "2.1.285 (Claude Code)".into();
        }
        Task::ready(Ok(()))
    }

    fn installed_version(&self, _: HarnessId, _: &mut App) -> HostTask<Option<String>> {
        Task::ready(Ok(Some(self.installed.borrow().clone())))
    }

    fn refresh_catalogs(&self, harness: HarnessId, _: &mut App) -> Task<()> {
        self.refreshed.borrow_mut().push(harness);
        Task::ready(())
    }

    fn announce_updated(&self, harness: HarnessId, _: &mut App) {
        self.announced.borrow_mut().push(harness);
    }

    fn on_harness_updated(&self, on_update: OnHarnessUpdated, _: &mut App) -> Option<Subscription> {
        self.listeners.borrow_mut().push(Rc::from(on_update));
        None
    }
}

fn mount_notice(
    cx: &mut TestAppContext,
    host: Rc<FakeUpdates>,
) -> (Entity<HarnessUpdateNotice>, &'static mut VisualTestContext) {
    mount(cx, 900., 600., move |_, cx| {
        cx.new(|cx| HarnessUpdateNotice::new(host, 12., cx))
    })
}

#[gpui::test]
fn updates_from_the_card_and_refreshes_models_for_other_windows(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUpdates::default());
    let (notice, cx) = mount_notice(cx, host.clone());
    assert!(exists(cx, "status:Harness updates"));
    assert!(exists(cx, "text:Claude Code"));
    assert!(exists(cx, "text:2.1.284 → 2.1.285"));
    assert!(exists(cx, "text:Harness update available"));

    click(cx, "button:Update:claude");
    assert!(exists(cx, "text:Updated to 2.1.285"));
    assert_eq!(host.refreshed.borrow().as_slice(), [HarnessId::Claude]);
    assert_eq!(host.announced.borrow().as_slice(), [HarnessId::Claude]);
    assert!(exists(
        cx,
        "text:Model picker refreshed with the new version’s models."
    ));

    let broadcast = host.clone();
    cx.update(|_, cx| broadcast.broadcast(HarnessId::Claude, cx));
    assert_eq!(
        host.refreshed.borrow().as_slice(),
        [HarnessId::Claude, HarnessId::Claude]
    );
    assert_eq!(
        notice.read_with(cx, |notice, _| notice.row(HarnessId::Claude)),
        RowState::Updated("2.1.285".into())
    );
}

#[gpui::test]
fn reports_an_updater_that_left_the_old_version_and_can_be_dismissed(cx: &mut TestAppContext) {
    let host = Rc::new(FakeUpdates::default());
    host.updater_installs.set(false);
    let (notice, cx) = mount_notice(cx, host.clone());
    click(cx, "button:Update:claude");
    assert!(exists(cx, "text:Still on 2.1.284 after updating."));
    assert!(exists(cx, "button:Retry:claude"));
    assert!(host.refreshed.borrow().is_empty());
    click(cx, "button:Dismiss harness updates");
    assert_eq!(host.dismissed.get(), 1);
    assert!(notice.read_with(cx, |notice, _| notice.updates().is_empty()));
    assert!(!exists(cx, "status:Harness updates"));
}
