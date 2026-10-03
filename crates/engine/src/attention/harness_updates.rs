//! Port of src/features/providers/model/harnessUpdates.ts: the launch check
//! for harness CLI updates and the "harness updated" broadcast.
//!
//! Tauri sent `harness-updated` to every window with a per-window source id
//! so the sender skipped its own event. Here the windows share one app, so
//! the broadcast is a GPUI event on the `HarnessUpdates` entity and each
//! listener passes its own source id.

use futures::future::{BoxFuture, join_all};
use gpui::{App, AppContext, Context, Entity, EventEmitter, Subscription};
use monocode_core::HarnessId;
use monocode_harness::providers::opencode::protocol::{compare_semver, parse_open_code_version};
use serde::{Deserialize, Serialize};

/// `UPDATABLE_HARNESSES`: harnesses with an npm version feed and a
/// self-updater MonoCode can run.
pub const UPDATABLE_HARNESSES: [HarnessId; 4] = [
    HarnessId::Claude,
    HarnessId::Codex,
    HarnessId::Opencode,
    HarnessId::Pi,
];

/// `HarnessUpdate`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessUpdate {
    pub harness: HarnessId,
    pub installed: String,
    pub latest: String,
}

/// `installedVersion`: the harness's `--version` output, if installed.
pub type InstalledVersion =
    Box<dyn Fn(HarnessId) -> BoxFuture<'static, Result<Option<String>, String>>>;

/// `latestVersion`: the newest published release.
pub type LatestVersion = Box<dyn Fn(HarnessId) -> BoxFuture<'static, Result<String, String>>>;

/// `HarnessUpdateDeps`.
pub struct HarnessUpdateDeps {
    /// Installed harnesses the user has not hidden.
    pub harnesses: Vec<HarnessId>,
    pub installed_version: InstalledVersion,
    pub latest_version: LatestVersion,
}

/// `findHarnessUpdates`. A failed lookup, offline or otherwise, drops that
/// harness silently: this runs unprompted at launch and must never show an
/// error of its own. Nothing is remembered between launches, so a harness
/// still behind is offered again at whatever release is newest by then.
pub async fn find_harness_updates(deps: HarnessUpdateDeps) -> Vec<HarnessUpdate> {
    let checks = deps.harnesses.iter().map(|&harness| {
        let lookups = UPDATABLE_HARNESSES.contains(&harness).then(|| {
            (
                (deps.installed_version)(harness),
                (deps.latest_version)(harness),
            )
        });
        async move {
            let (installed, latest) = lookups?;
            let (installed, latest) = futures::join!(installed, latest);
            let installed = parse_open_code_version(installed.ok()?.as_deref().unwrap_or(""))?;
            let latest = parse_open_code_version(&latest.ok()?)?;
            if compare_semver(&latest, &installed) <= 0 {
                return None;
            }
            Some(HarnessUpdate {
                harness,
                installed,
                latest,
            })
        }
    });
    join_all(checks).await.into_iter().flatten().collect()
}

/// `claimLaunchHarnessUpdateCheck`: true for the first caller per app
/// process, so a window opened later does not repeat the launch check.
pub fn claim_launch_harness_update_check() -> bool {
    monocode_integrations::harness_updates::harness_update_check_claim()
}

/// `fetchLatestHarnessVersion`. Blocks on the network; run it off the UI
/// thread.
pub fn fetch_latest_harness_version(harness: HarnessId) -> Result<String, String> {
    monocode_integrations::harness_updates::harness_latest_version(harness.as_str().to_string())
}

/// `HarnessUpdatedEvent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessUpdated {
    pub harness: HarnessId,
    /// The window (or other caller) that ran the update and already
    /// refreshed its own catalog.
    pub source: u64,
}

/// The `harness-updated` broadcast.
#[derive(Default)]
pub struct HarnessUpdates {
    next_source: u64,
}

impl EventEmitter<HarnessUpdated> for HarnessUpdates {}

impl HarnessUpdates {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self::default())
    }

    /// A fresh source id for one window's listener and announcements.
    pub fn new_source(&mut self) -> u64 {
        self.next_source += 1;
        self.next_source
    }

    /// `announceHarnessUpdated`.
    pub fn announce(&mut self, harness: HarnessId, source: u64, cx: &mut Context<Self>) {
        cx.emit(HarnessUpdated { harness, source });
    }
}

/// `onHarnessUpdated`: call `handler` for updates other sources announced.
/// Dropping the subscription stops it.
pub fn on_harness_updated(
    updates: &Entity<HarnessUpdates>,
    source: u64,
    mut handler: impl FnMut(HarnessId, &mut App) + 'static,
    cx: &mut App,
) -> Subscription {
    cx.subscribe(updates, move |_, event: &HarnessUpdated, cx| {
        // The sender awaited its local refresh before announcing the update.
        if event.source != source {
            handler(event.harness, cx);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;
    use gpui::TestAppContext;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    fn installed() -> HashMap<HarnessId, &'static str> {
        [
            (HarnessId::Claude, "2.1.284 (Claude Code)"),
            (HarnessId::Codex, "codex-cli 0.159.2"),
            (HarnessId::Opencode, "1.18.31"),
            (HarnessId::Cursor, "2026.09.01-abc"),
        ]
        .into_iter()
        .collect()
    }

    fn find(
        harnesses: &[HarnessId],
        latest: HashMap<HarnessId, &'static str>,
    ) -> Vec<HarnessUpdate> {
        let installed = installed();
        smol::block_on(find_harness_updates(HarnessUpdateDeps {
            harnesses: harnesses.to_vec(),
            installed_version: Box::new(move |id| {
                let version = installed.get(&id).map(|v| v.to_string());
                async move { Ok(version) }.boxed()
            }),
            latest_version: Box::new(move |id| {
                let version = latest.get(&id).map(|v| v.to_string());
                async move { version.ok_or_else(|| "offline".to_string()) }.boxed()
            }),
        }))
    }

    fn latest() -> HashMap<HarnessId, &'static str> {
        [
            (HarnessId::Claude, "2.1.285"),
            (HarnessId::Codex, "0.159.2"),
            (HarnessId::Opencode, "1.18.33"),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn reports_only_harnesses_behind_the_published_release() {
        assert_eq!(
            find(
                &[HarnessId::Claude, HarnessId::Codex, HarnessId::Opencode],
                latest()
            ),
            vec![
                HarnessUpdate {
                    harness: HarnessId::Claude,
                    installed: "2.1.284".into(),
                    latest: "2.1.285".into(),
                },
                HarnessUpdate {
                    harness: HarnessId::Opencode,
                    installed: "1.18.31".into(),
                    latest: "1.18.33".into(),
                },
            ]
        );
    }

    #[test]
    fn skips_harnesses_without_a_version_feed_or_whose_lookup_fails() {
        assert_eq!(find(&[HarnessId::Cursor, HarnessId::Pi], latest()), vec![]);
    }

    #[test]
    fn offers_a_harness_still_behind_again_at_the_newest_release() {
        assert_eq!(find(&[HarnessId::Claude], latest())[0].latest, "2.1.285");
        let mut newer = latest();
        newer.insert(HarnessId::Claude, "2.1.286");
        assert_eq!(
            find(&[HarnessId::Claude], newer),
            vec![HarnessUpdate {
                harness: HarnessId::Claude,
                installed: "2.1.284".into(),
                latest: "2.1.286".into(),
            }]
        );
    }

    #[gpui::test]
    fn delivers_updates_to_other_windows_while_skipping_the_sender(cx: &mut TestAppContext) {
        let updates = cx.update(HarnessUpdates::new);
        let local_seen = Rc::new(RefCell::new(Vec::new()));
        let remote_seen = Rc::new(RefCell::new(Vec::new()));
        let (local, remote) = updates.update(cx, |updates, _| {
            (updates.new_source(), updates.new_source())
        });
        let stop_local = cx.update(|cx| {
            let seen = local_seen.clone();
            on_harness_updated(
                &updates,
                local,
                move |harness, _| seen.borrow_mut().push(harness),
                cx,
            )
        });
        let _stop_remote = cx.update(|cx| {
            let seen = remote_seen.clone();
            on_harness_updated(
                &updates,
                remote,
                move |harness, _| seen.borrow_mut().push(harness),
                cx,
            )
        });

        updates.update(cx, |updates, cx| {
            updates.announce(HarnessId::Claude, local, cx)
        });
        cx.run_until_parked();
        assert!(local_seen.borrow().is_empty());
        assert_eq!(*remote_seen.borrow(), vec![HarnessId::Claude]);

        updates.update(cx, |updates, cx| {
            updates.announce(HarnessId::Codex, remote, cx)
        });
        cx.run_until_parked();
        assert_eq!(*local_seen.borrow(), vec![HarnessId::Codex]);
        assert_eq!(remote_seen.borrow().len(), 1);

        drop(stop_local);
        updates.update(cx, |updates, cx| {
            updates.announce(HarnessId::Opencode, remote, cx)
        });
        cx.run_until_parked();
        assert_eq!(local_seen.borrow().len(), 1);
        assert_eq!(remote_seen.borrow().len(), 1);
    }
}
