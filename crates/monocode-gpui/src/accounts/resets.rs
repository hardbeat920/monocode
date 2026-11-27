//! Banked Codex resets are shared across footers so only one claim runs per account.
use super::*;

#[derive(Clone)]
pub(crate) struct ResetRow {
    pub key: String,
    pub credit: Option<usage::ResetCredit>,
    pub title: String,
}

pub(crate) fn rows(snapshot: Option<&Snapshot>) -> Vec<ResetRow> {
    let Some(credits) = snapshot.and_then(|s| s.reset_credits.as_ref()) else {
        return Vec::new();
    };
    if credits.available_count == 0 {
        return Vec::new();
    }
    let mut rows: Vec<_> = credits
        .credits
        .iter()
        .filter(|c| matches!(c.status.as_str(), "available" | "unknown"))
        .enumerate()
        .map(|(i, credit)| ResetRow {
            key: format!("credit:{}", credit.id),
            credit: Some(credit.clone()),
            title: credit
                .title
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("Banked reset {}", i + 1)),
        })
        .collect();
    for i in rows.len()..credits.available_count as usize {
        rows.push(ResetRow {
            key: format!("unlisted:{i}"),
            credit: None,
            title: format!("Banked reset {}", i + 1),
        });
    }
    rows
}

#[derive(Clone)]
pub(crate) struct ResetAction {
    /// Retained after a failed refresh to prevent spending a stale card again.
    pub row: Option<String>,
    pub result: Option<Result<String, String>>,
}
impl ResetAction {
    pub(crate) fn applying(&self) -> bool {
        self.result.is_none()
    }
    pub(crate) fn message(&self) -> Option<&str> {
        Some(match self.result.as_ref()? {
            Ok(outcome) => match outcome.as_str() {
                "reset" => "Codex usage was reset.",
                "nothingToReset" => "There’s no active usage to reset.",
                "noCredit" => "No banked resets are available.",
                "alreadyRedeemed" => "That reset was already used.",
                _ => "Codex returned an unknown reset result.",
            },
            Err(error) => error,
        })
    }
}

impl Accounts {
    pub(crate) fn consume_reset(&mut self, key: Key, row: &str, cx: &mut Context<Self>) {
        self.consume_reset_with(
            key,
            row,
            |profile, credit| {
                let result = usage::consume_reset(profile, credit);
                // Read even after errors: the server may have applied the claim.
                let refreshed = usage::fetch_for(Kind::Codex, profile, None);
                (result, refreshed)
            },
            cx,
        );
    }

    fn consume_reset_with(
        &mut self,
        key: Key,
        row: &str,
        consume: impl FnOnce(
            &Profile,
            Option<&str>,
        ) -> (Result<String, String>, Result<Snapshot, FetchError>)
        + Send
        + 'static,
        cx: &mut Context<Self>,
    ) {
        if key.kind != Kind::Codex
            || (key.profile.id != accounts::DEFAULT && !self.book.exists(key.kind, &key.profile.id))
            || self.reset_actions.get(&key).is_some_and(|a| {
                a.applying()
                    || (a.row.as_deref() == Some(row)
                        && a.result.as_ref().is_some_and(Result::is_ok))
            })
        {
            return;
        }
        let Some(row) = rows(self.usage.get(&key).and_then(|s| s.snapshot.as_ref()))
            .into_iter()
            .find(|r| r.key == row)
        else {
            return;
        };
        let state = self.usage.entry(key.clone()).or_default();
        // Older usage reads must not overwrite the post-claim snapshot.
        state.generation += 1;
        state.fetching = true;
        let generation = state.generation;
        self.reset_actions.insert(
            key.clone(),
            ResetAction {
                row: Some(row.key),
                result: None,
            },
        );
        let profile = key.profile.clone();
        let work = cx
            .background_executor()
            .spawn(async move { consume(&profile, row.credit.as_ref().map(|c| c.id.as_str())) });
        cx.spawn(async move |this, cx| {
            let (result, refreshed) = work.await;
            let _ = this.update(cx, |this, cx| {
                let Some(state) = this.usage.get_mut(&key) else {
                    return;
                };
                if state.generation != generation {
                    return;
                }
                let refreshed_ok = refreshed.is_ok();
                let queued = std::mem::take(&mut state.queued);
                state.attempted = Some(Instant::now());
                state.complete(refreshed);
                if let Some(identity) = state.snapshot.as_ref().and_then(|s| s.identity.clone()) {
                    this.identities
                        .insert((key.kind, key.profile.id.clone()), identity);
                }
                if let Some(action) = this.reset_actions.get_mut(&key) {
                    action.result = Some(result);
                    if refreshed_ok {
                        action.row = None;
                    }
                }
                if queued {
                    this.refresh(key, true, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn snapshot(count: u64) -> Snapshot {
        Snapshot {
            reset_credits: Some(usage::ResetCredits {
                available_count: count,
                credits: Vec::new(),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn banked_rows_keep_details_filter_spent_credits_and_fill_aggregate_count() {
        let mut data = snapshot(3);
        data.reset_credits.as_mut().unwrap().credits =
            ["available", "redeemed", "unknown", "redeeming"]
                .into_iter()
                .enumerate()
                .map(|(i, status)| usage::ResetCredit {
                    id: i.to_string(),
                    status: status.into(),
                    reset_type: "codexRateLimits".into(),
                    granted_at: None,
                    expires_at: Some(2_000_000),
                    title: Some("A reward".into()),
                    description: Some("One Codex reset".into()),
                })
                .collect();
        let cards = rows(Some(&data));
        assert_eq!(cards.len(), 3);
        assert_eq!(cards[0].key, "credit:0");
        assert_eq!(cards[1].key, "credit:2");
        assert_eq!(cards[0].title, "A reward");
        assert_eq!(
            cards[0].credit.as_ref().unwrap().expires_at,
            Some(2_000_000)
        );
        assert_eq!(
            cards[0].credit.as_ref().unwrap().description.as_deref(),
            Some("One Codex reset")
        );
        assert_eq!(cards[2].key, "unlisted:2");
        assert!(cards[2].credit.is_none());
        assert_eq!(rows(Some(&snapshot(2))).len(), 2);
        assert!(rows(Some(&snapshot(0))).is_empty());
        assert!(rows(None).is_empty());
    }

    #[gpui::test]
    fn banked_claims_are_serialized_per_account_and_refresh_the_right_cache(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let center =
            cx.new(|cx| Accounts::new(PathBuf::from("/fixture"), Book::default(), store, cx));
        let calls = Arc::new(AtomicUsize::new(0));
        let key = Key::new(Kind::Codex, Profile::default(), None);
        center.update(cx, |accounts, cx| {
            accounts.book.accounts.push(Account {
                id: "other".into(),
                provider: "codex".into(),
                label: "Other".into(),
            });
            accounts
                .usage
                .entry(key.clone())
                .or_default()
                .complete(Ok(snapshot(2)));
            let other = Key::new(Kind::Codex, accounts.profile("other"), None);
            let mut other_snapshot = snapshot(3);
            other_snapshot
                .reset_credits
                .as_mut()
                .unwrap()
                .credits
                .push(usage::ResetCredit {
                    id: "specific-credit".into(),
                    status: "available".into(),
                    reset_type: "codexRateLimits".into(),
                    granted_at: None,
                    expires_at: None,
                    title: None,
                    description: None,
                });
            accounts
                .usage
                .entry(other.clone())
                .or_default()
                .complete(Ok(other_snapshot));
            for _ in 0..2 {
                let calls = calls.clone();
                accounts.consume_reset_with(
                    key.clone(),
                    "unlisted:0",
                    move |profile, credit| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(profile.id, accounts::DEFAULT);
                        assert!(credit.is_none());
                        (Ok("reset".into()), Ok(snapshot(1)))
                    },
                    cx,
                );
            }
            accounts.consume_reset_with(
                other.clone(),
                "credit:specific-credit",
                |profile, credit| {
                    assert_eq!(profile.id, "other");
                    assert_eq!(credit, Some("specific-credit"));
                    (Ok("nothingToReset".into()), Ok(snapshot(3)))
                },
                cx,
            );
            assert!(accounts.reset_actions[&key].applying());
            assert!(accounts.reset_actions[&other].applying());
        });
        cx.run_until_parked();
        center.update(cx, |accounts, _| {
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(
                accounts.usage[&key]
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .reset_credits
                    .as_ref()
                    .unwrap()
                    .available_count,
                1
            );
            assert_eq!(
                accounts.reset_actions[&key].message(),
                Some("Codex usage was reset.")
            );
            assert!(accounts.reset_actions[&key].row.is_none());
            let other = Key::new(Kind::Codex, accounts.profile("other"), None);
            assert_eq!(
                accounts.reset_actions[&other].message(),
                Some("There’s no active usage to reset.")
            );
            assert!(!accounts.usage[&key].fetching);
        });
    }

    #[gpui::test]
    fn banked_claim_errors_preserve_credits_and_invalidated_replies_stay_discarded(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(Path::new(":memory:")).unwrap(),
        ));
        let center =
            cx.new(|cx| Accounts::new(PathBuf::from("/fixture"), Book::default(), store, cx));
        let key = Key::new(Kind::Codex, Profile::default(), None);
        center.update(cx, |accounts, cx| {
            accounts
                .usage
                .entry(key.clone())
                .or_default()
                .complete(Ok(snapshot(2)));
            accounts.consume_reset_with(
                key.clone(),
                "unlisted:0",
                |_, _| {
                    (
                        Err("Network unavailable".into()),
                        Err(FetchError {
                            message: "Refresh failed".into(),
                            unavailable: false,
                        }),
                    )
                },
                cx,
            );
        });
        cx.run_until_parked();
        center.update(cx, |accounts, cx| {
            assert_eq!(rows(accounts.usage[&key].snapshot.as_ref()).len(), 2);
            assert_eq!(
                accounts.reset_actions[&key].message(),
                Some("Network unavailable")
            );
            assert!(!accounts.reset_actions[&key].applying());
            accounts.consume_reset_with(
                key.clone(),
                "unlisted:0",
                |_, _| (Ok("reset".into()), Ok(snapshot(0))),
                cx,
            );
            // Match sign-out's generation change while the old account request is pending.
            let cached = accounts.usage.get_mut(&key).unwrap();
            cached.generation += 1;
            cached.complete(Ok(snapshot(4)));
            accounts.reset_actions.remove(&key);
        });
        cx.run_until_parked();
        center.update(cx, |accounts, _| {
            assert_eq!(rows(accounts.usage[&key].snapshot.as_ref()).len(), 4);
            assert!(!accounts.reset_actions.contains_key(&key));
        });
    }
}
