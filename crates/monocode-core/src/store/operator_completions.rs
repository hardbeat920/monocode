use super::Store;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerOutcome {
    Completed,
    Failed,
    Stopped,
    Removed,
}
impl WorkerOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
            Self::Removed => "removed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OperatorCompletionGroup {
    Turn(u64),
    Label(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperatorSubscription {
    pub session: i64,
    pub title: String,
    pub group: OperatorCompletionGroup,
    pub outcome: Option<WorkerOutcome>,
    pub delivered: bool,
}

impl Store {
    pub fn operator_subscriptions(
        &self,
    ) -> Result<HashMap<i64, Vec<OperatorSubscription>>, String> {
        self.settings_with_prefix("operator-completions:")?
            .into_iter()
            .map(|(caller, value)| {
                serde_json::from_str(&value)
                    .map(|subscriptions| (caller, subscriptions))
                    .map_err(|e| e.to_string())
            })
            .collect()
    }
    pub fn save_operator_subscriptions(
        &self,
        caller: i64,
        subscriptions: &[OperatorSubscription],
    ) -> Result<(), String> {
        self.save_setting(
            &format!("operator-completions:{caller}"),
            &serde_json::to_string(subscriptions).map_err(|e| e.to_string())?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_subscriptions_survive_reopening_native_and_desktop_stores() {
        for desktop in [false, true] {
            for group in [
                OperatorCompletionGroup::Turn(41),
                OperatorCompletionGroup::Label("review".into()),
            ] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("callbacks.db");
                let open = || {
                    if desktop {
                        Store::open_monocode(&path)
                    } else {
                        Store::open(&path)
                    }
                    .unwrap()
                };
                let store = open();
                let caller = store.create("codex", "/work/callbacks").unwrap().id;
                let child = store.create("codex", "/work/callbacks").unwrap().id;
                store
                    .save_operator_subscriptions(
                        caller,
                        &[OperatorSubscription {
                            session: child,
                            title: "Worker".into(),
                            group: group.clone(),
                            outcome: Some(WorkerOutcome::Failed),
                            delivered: false,
                        }],
                    )
                    .unwrap();
                drop(store);
                let store = open();
                let mut subscriptions = store
                    .session_sidebar_metadata(&[])
                    .operator_subscriptions
                    .unwrap()
                    .remove(&caller)
                    .unwrap();
                assert_eq!(subscriptions.len(), 1);
                assert_eq!(subscriptions[0].session, child);
                assert_eq!(subscriptions[0].group, group);
                assert_eq!(subscriptions[0].outcome, Some(WorkerOutcome::Failed));
                subscriptions[0].delivered = true;
                store
                    .save_operator_subscriptions(caller, &subscriptions)
                    .unwrap();
                drop(store);
                let store = open();
                let restored = store.operator_subscriptions().unwrap();
                assert!(restored[&caller][0].delivered);
                assert_eq!(restored[&caller][0].group, group);
                store.save_operator_subscriptions(caller, &[]).unwrap();
                drop(store);
                assert!(open().operator_subscriptions().unwrap()[&caller].is_empty());
            }
        }
    }
    #[test]
    fn operator_subscription_decode_failure_reaches_sidebar_without_overwriting_state() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.save_setting("operator-completions:1", "[]").unwrap();
        store
            .save_setting("operator-completions:2", "broken JSON")
            .unwrap();
        assert!(store
            .session_sidebar_metadata(&[])
            .operator_subscriptions
            .is_err());
        assert_eq!(
            store.setting("operator-completions:1").unwrap().as_deref(),
            Some("[]")
        );
        assert_eq!(
            store.setting("operator-completions:2").unwrap().as_deref(),
            Some("broken JSON")
        );
        store.save_setting("operator-completions:2", "[]").unwrap();
        assert_eq!(
            store
                .session_sidebar_metadata(&[])
                .operator_subscriptions
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn operator_subscription_database_failure_is_not_an_empty_snapshot() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store
            .conn
            .execute_batch("DROP TABLE native_settings")
            .unwrap();
        assert!(store
            .session_sidebar_metadata(&[])
            .operator_subscriptions
            .is_err());
    }
}
