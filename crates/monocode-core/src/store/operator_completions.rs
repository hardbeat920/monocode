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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperatorSubscription {
    pub session: i64,
    pub title: String,
    pub turn: u64,
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
                        turn: 41,
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
                .remove(&caller)
                .unwrap();
            assert_eq!(subscriptions.len(), 1);
            assert_eq!(subscriptions[0].session, child);
            assert_eq!(subscriptions[0].outcome, Some(WorkerOutcome::Failed));
            subscriptions[0].delivered = true;
            store
                .save_operator_subscriptions(caller, &subscriptions)
                .unwrap();
            drop(store);
            let store = open();
            assert!(store.operator_subscriptions().unwrap()[&caller][0].delivered);
            store.save_operator_subscriptions(caller, &[]).unwrap();
            drop(store);
            assert!(open().operator_subscriptions().unwrap()[&caller].is_empty());
        }
    }
}
