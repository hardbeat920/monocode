//! Port of src/features/inbox/model/ciRepairTracking.ts: the CI repairs
//! started from the inbox and how each one ended. Each repair is its own
//! localStorage entry under `monocode.ciRepairs.v1.<id>`, so windows do not
//! overwrite each other's records; the history keeps the newest 200.
//!
//! localStorage becomes `Kv` with the same keys and JSON. A `Kv` write
//! cannot fail, so the in-memory `pendingWrites` fallback is gone. The
//! module-level list becomes `CiRepairTracker`; its listeners become the
//! owner's `cx.notify()`, and `onStorage` becomes `handle_storage_event`.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use monocode_settings::Kv;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ci_repair::{CiRepairCheck, CiRepairRequest};
use super::time::now_ms;
use crate::runtime::util::project_path::same_project_path;

/// `KEY`: the legacy array of every repair.
pub const CI_REPAIRS_KEY: &str = "monocode.ciRepairs.v1";
/// `ENTRY_PREFIX`: one entry per repair.
pub const CI_REPAIR_ENTRY_PREFIX: &str = "monocode.ciRepairs.v1.";
const MAX_REPAIRS: usize = 200;

/// `CiRepairOutcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CiRepairOutcome {
    Completed,
    Failed,
    Cancelled,
}

/// `TrackedCiRepair["phase"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CiRepairPhase {
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

impl From<CiRepairOutcome> for CiRepairPhase {
    fn from(outcome: CiRepairOutcome) -> Self {
        match outcome {
            CiRepairOutcome::Completed => Self::Completed,
            CiRepairOutcome::Failed => Self::Failed,
            CiRepairOutcome::Cancelled => Self::Cancelled,
        }
    }
}

/// `TrackedCiRepair`: the repair target plus its chat and phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedCiRepair {
    pub repo: String,
    pub number: i64,
    pub head_oid: String,
    pub checks: Vec<CiRepairCheck>,
    pub id: String,
    pub cwd: String,
    pub session_id: String,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<i64>,
    pub phase: CiRepairPhase,
}

fn newest_first(a: &TrackedCiRepair, b: &TrackedCiRepair) -> Ordering {
    b.started_at
        .cmp(&a.started_at)
        .then_with(|| b.sequence.unwrap_or(0).cmp(&a.sequence.unwrap_or(0)))
}

fn non_empty_string(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
}

fn safe_integer(value: &Value) -> Option<i64> {
    let number = value.as_f64()?;
    (number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_991.0).then_some(number as i64)
}

/// The record check in `loadStored`.
fn valid_repair(value: &Value) -> Option<TrackedCiRepair> {
    let item = value.as_object()?;
    if !["id", "cwd", "sessionId", "repo", "headOid"]
        .iter()
        .all(|key| non_empty_string(item.get(*key)))
    {
        return None;
    }
    let number = item
        .get("number")
        .and_then(safe_integer)
        .filter(|number| *number > 0)?;
    let started_at = item
        .get("startedAt")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())?;
    let sequence = match item.get("sequence") {
        None => None,
        Some(value) => Some(safe_integer(value).filter(|sequence| *sequence >= 0)?),
    };
    let phase: CiRepairPhase = serde_json::from_value(item.get("phase")?.clone()).ok()?;
    let checks = item.get("checks")?.as_array()?;
    if checks.is_empty() {
        return None;
    }
    let checks = checks
        .iter()
        .map(|check| {
            let check = check.as_object()?;
            let url = match check.get("url")? {
                Value::Null => None,
                Value::String(url) => Some(url.clone()),
                _ => return None,
            };
            Some(CiRepairCheck {
                name: check.get("name")?.as_str()?.to_string(),
                workflow: check.get("workflow")?.as_str()?.to_string(),
                url,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let text = |key: &str| {
        item.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Some(TrackedCiRepair {
        repo: text("repo"),
        number,
        head_oid: text("headOid"),
        checks,
        id: text("id"),
        cwd: text("cwd"),
        session_id: text("sessionId"),
        started_at: started_at as i64,
        sequence,
        phase,
    })
}

/// `loadStored`: the legacy array plus every entry, valid records only,
/// newest first. One damaged record does not hide the others.
fn load_stored(kv: &Kv) -> Vec<TrackedCiRepair> {
    let mut values: Vec<Value> = match kv
        .get_item(CI_REPAIRS_KEY)
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
    {
        Some(Value::Array(values)) => values,
        _ => Vec::new(),
    };
    for key in kv.keys() {
        if !key.starts_with(CI_REPAIR_ENTRY_PREFIX) {
            continue;
        }
        if let Some(value) = kv
            .get_item(&key)
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        {
            values.push(value);
        }
    }
    let mut repairs: Vec<TrackedCiRepair> = values.iter().filter_map(valid_repair).collect();
    repairs.sort_by(newest_first);
    repairs
}

/// `new Map(items.map(item => [item.id, item]))`, in first-seen order with
/// the last value.
fn by_id(items: impl IntoIterator<Item = TrackedCiRepair>) -> Vec<TrackedCiRepair> {
    let mut order: Vec<String> = Vec::new();
    let mut map: HashMap<String, TrackedCiRepair> = HashMap::new();
    for item in items {
        if !map.contains_key(&item.id) {
            order.push(item.id.clone());
        }
        map.insert(item.id.clone(), item);
    }
    order.into_iter().filter_map(|id| map.remove(&id)).collect()
}

/// `load`.
fn load(kv: &Kv) -> Vec<TrackedCiRepair> {
    let mut items = by_id(load_stored(kv));
    items.sort_by(newest_first);
    items
}

/// The CI repair history of this app.
pub struct CiRepairTracker {
    kv: Kv,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    repairs: Option<Vec<TrackedCiRepair>>,
    interrupted: HashSet<String>,
}

impl CiRepairTracker {
    pub fn new(kv: Kv) -> Self {
        Self::with_clock(kv, Arc::new(now_ms))
    }

    /// A tracker whose `Date.now()` is `clock`, for tests.
    pub fn with_clock(kv: Kv, clock: Arc<dyn Fn() -> i64 + Send + Sync>) -> Self {
        Self {
            kv,
            clock,
            repairs: None,
            interrupted: HashSet::new(),
        }
    }

    fn mark_interrupted(&self, items: Vec<TrackedCiRepair>) -> Vec<TrackedCiRepair> {
        items
            .into_iter()
            .take(MAX_REPAIRS)
            .map(|mut item| {
                if item.phase == CiRepairPhase::Running && self.interrupted.contains(&item.id) {
                    item.phase = CiRepairPhase::Interrupted;
                }
                item
            })
            .collect()
    }

    /// `getCiRepairs`: newest first. A repair still running when this app
    /// first reads the history was cut off by a restart.
    pub fn get_ci_repairs(&mut self) -> &[TrackedCiRepair] {
        if self.repairs.is_none() {
            let loaded: Vec<TrackedCiRepair> = load(&self.kv)
                .into_iter()
                .take(MAX_REPAIRS)
                .map(|mut item| {
                    if item.phase == CiRepairPhase::Running {
                        self.interrupted.insert(item.id.clone());
                        item.phase = CiRepairPhase::Interrupted;
                    }
                    item
                })
                .collect();
            self.repairs = Some(loaded);
        }
        self.repairs.as_deref().unwrap_or_default()
    }

    /// `save`.
    fn save(&mut self, repair: TrackedCiRepair, remove: bool) {
        let previous = self.get_ci_repairs().to_vec();
        let key = format!("{CI_REPAIR_ENTRY_PREFIX}{}", repair.id);
        if remove {
            self.kv.remove_item(&key);
        } else if let Ok(json) = serde_json::to_string(&repair) {
            self.kv.set_item(&key, &json);
        }
        let mut combined = by_id(
            std::iter::once(repair.clone())
                .chain(previous)
                .chain(load(&self.kv)),
        );
        if remove {
            combined.retain(|item| item.id != repair.id);
        } else if let Some(slot) = combined.iter_mut().find(|item| item.id == repair.id) {
            *slot = repair;
        }
        combined.sort_by(newest_first);
        for item in combined.iter().skip(MAX_REPAIRS) {
            self.kv
                .remove_item(&format!("{CI_REPAIR_ENTRY_PREFIX}{}", item.id));
        }
        self.repairs = Some(self.mark_interrupted(combined));
    }

    /// The first half of `trackCiRepair`: record a running repair.
    pub fn begin(
        &mut self,
        cwd: &str,
        request: &CiRepairRequest,
        session_id: &str,
        id: String,
    ) -> TrackedCiRepair {
        let stored = load(&self.kv);
        let highest = self
            .get_ci_repairs()
            .iter()
            .chain(stored.iter())
            .map(|item| item.sequence.unwrap_or(0))
            .fold(0, i64::max);
        let repair = TrackedCiRepair {
            repo: request.target.repo.clone(),
            number: request.target.number,
            head_oid: request.target.head_oid.clone(),
            checks: request.target.checks.clone(),
            id,
            cwd: cwd.to_string(),
            session_id: session_id.to_string(),
            started_at: (self.clock)(),
            sequence: Some(highest + 1),
            phase: CiRepairPhase::Running,
        };
        self.save(repair.clone(), false);
        repair
    }

    /// The `settle` callback: the repair's own agent turn ended.
    pub fn settle(&mut self, repair: &TrackedCiRepair, outcome: CiRepairOutcome) {
        let current = self
            .get_ci_repairs()
            .iter()
            .find(|item| item.id == repair.id)
            .cloned()
            .unwrap_or_else(|| repair.clone());
        self.save(
            TrackedCiRepair {
                phase: outcome.into(),
                ..current
            },
            false,
        );
    }

    /// The `catch` of `trackCiRepair`: forget a repair that never started.
    pub fn discard(&mut self, repair: &TrackedCiRepair) {
        self.save(repair.clone(), true);
    }

    /// `trackCiRepair`. `submit` starts the agent turn and returns whether
    /// the chat accepted it; it may settle the repair right away through
    /// the tracker it gets.
    pub fn track_ci_repair(
        &mut self,
        cwd: &str,
        request: &CiRepairRequest,
        session_id: &str,
        submit: impl FnOnce(&mut Self, &TrackedCiRepair) -> bool,
    ) -> Result<TrackedCiRepair, String> {
        let repair = self.begin(cwd, request, session_id, uuid::Uuid::new_v4().to_string());
        if !submit(self, &repair) {
            self.discard(&repair);
            return Err("Could not start this fix. Choose another chat and try again.".into());
        }
        Ok(repair)
    }

    /// `rebaseCiRepairs`: keep repairs linked to a project after its folder
    /// moves.
    pub fn rebase_ci_repairs(&mut self, from: &str, to: &str) {
        for repair in load(&self.kv) {
            if same_project_path(&repair.cwd, from) {
                self.save(
                    TrackedCiRepair {
                        cwd: to.to_string(),
                        ..repair
                    },
                    false,
                );
            }
        }
    }

    /// `onStorage`: another writer changed `key` (`None` for a clear).
    /// Returns whether the history was reloaded.
    pub fn handle_storage_event(&mut self, key: Option<&str>) -> bool {
        if let Some(key) = key
            && key != CI_REPAIRS_KEY
            && !key.starts_with(CI_REPAIR_ENTRY_PREFIX)
        {
            return false;
        }
        if let Some(id) = key.and_then(|key| key.strip_prefix(CI_REPAIR_ENTRY_PREFIX)) {
            self.interrupted.remove(id);
        }
        self.repairs = Some(self.mark_interrupted(load(&self.kv)));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::ci_repair::build_ci_repair_request;
    use crate::inbox::ci_repair::tests::failed;

    fn request() -> CiRepairRequest {
        build_ci_repair_request(
            "acme/web",
            42,
            "old-sha",
            &[failed(
                "tests",
                Some("https://github.com/acme/web/actions/runs/1/job/2"),
                None,
            )],
        )
    }

    fn phases(tracker: &mut CiRepairTracker) -> Vec<(String, CiRepairPhase)> {
        tracker
            .get_ci_repairs()
            .iter()
            .map(|repair| (repair.session_id.clone(), repair.phase))
            .collect()
    }

    #[test]
    fn tracks_a_submitted_repair_until_its_own_agent_turn_finishes() {
        let mut tracker = CiRepairTracker::new(Kv::in_memory());
        let repair = tracker
            .track_ci_repair("/web", &request(), "chat1", |_, _| true)
            .unwrap();
        let current = tracker.get_ci_repairs().to_vec();
        assert_eq!(current.len(), 1);
        assert_eq!(
            (
                current[0].repo.as_str(),
                current[0].number,
                current[0].head_oid.as_str()
            ),
            ("acme/web", 42, "old-sha")
        );
        assert_eq!(
            (current[0].cwd.as_str(), current[0].session_id.as_str()),
            ("/web", "chat1")
        );
        assert_eq!(current[0].phase, CiRepairPhase::Running);
        tracker.settle(&repair, CiRepairOutcome::Completed);
        assert_eq!(tracker.get_ci_repairs()[0].phase, CiRepairPhase::Completed);
    }

    #[test]
    fn keeps_an_active_repair_linked_to_a_project_after_its_folder_moves() {
        let kv = Kv::in_memory();
        let mut tracker = CiRepairTracker::new(kv.clone());
        let repair = tracker
            .track_ci_repair("/old-project", &request(), "chat1", |_, _| true)
            .unwrap();
        tracker.rebase_ci_repairs("/old-project", "/new-project");
        assert_eq!(tracker.get_ci_repairs()[0].cwd, "/new-project");
        tracker.settle(&repair, CiRepairOutcome::Completed);
        let mut reopened = CiRepairTracker::new(kv);
        let restored = &reopened.get_ci_repairs()[0];
        assert_eq!(
            (restored.cwd.as_str(), restored.phase),
            ("/new-project", CiRepairPhase::Completed)
        );
    }

    #[test]
    fn does_not_retain_a_repair_that_the_chat_could_not_start() {
        let mut tracker = CiRepairTracker::new(Kv::in_memory());
        let error = tracker
            .track_ci_repair("/web", &request(), "busy-chat", |_, _| false)
            .unwrap_err();
        assert!(error.contains("Could not start this fix"));
        assert!(tracker.get_ci_repairs().is_empty());
    }

    #[test]
    fn keeps_completed_repairs_after_reopening_and_marks_unfinished_work_as_interrupted() {
        let kv = Kv::in_memory();
        let mut tracker = CiRepairTracker::with_clock(kv.clone(), Arc::new(|| 1_900_000_000_000));
        tracker
            .track_ci_repair("/web", &request(), "finished-chat", |tracker, repair| {
                tracker.settle(repair, CiRepairOutcome::Completed);
                true
            })
            .unwrap();
        tracker
            .track_ci_repair("/web", &request(), "unfinished-chat", |_, _| true)
            .unwrap();
        let mut restored = CiRepairTracker::new(kv);
        assert_eq!(
            phases(&mut restored),
            [
                ("unfinished-chat".to_string(), CiRepairPhase::Interrupted),
                ("finished-chat".to_string(), CiRepairPhase::Completed),
            ]
        );
    }

    #[test]
    fn does_not_overwrite_repairs_saved_by_another_window_after_this_window_loaded() {
        let kv = Kv::in_memory();
        let mut first = CiRepairTracker::new(kv.clone());
        assert!(first.get_ci_repairs().is_empty());
        let mut second = CiRepairTracker::new(kv.clone());
        second
            .track_ci_repair("/web", &request(), "other-window", |tracker, repair| {
                tracker.settle(repair, CiRepairOutcome::Completed);
                true
            })
            .unwrap();
        first
            .track_ci_repair("/web", &request(), "this-window", |tracker, repair| {
                tracker.settle(repair, CiRepairOutcome::Completed);
                true
            })
            .unwrap();
        let mut reopened = CiRepairTracker::new(kv);
        let mut sessions: Vec<String> = reopened
            .get_ci_repairs()
            .iter()
            .map(|repair| repair.session_id.clone())
            .collect();
        sessions.sort();
        assert_eq!(sessions, ["other-window", "this-window"]);
    }

    #[test]
    fn updates_when_a_repair_finishes_in_another_window() {
        let kv = Kv::in_memory();
        let mut first = CiRepairTracker::new(kv.clone());
        let repair = first
            .track_ci_repair("/web", &request(), "owner", |_, _| true)
            .unwrap();
        let mut second = CiRepairTracker::new(kv);
        assert_eq!(second.get_ci_repairs()[0].phase, CiRepairPhase::Interrupted);
        first.settle(&repair, CiRepairOutcome::Completed);
        let key = format!("{CI_REPAIR_ENTRY_PREFIX}{}", first.get_ci_repairs()[0].id);
        assert!(second.handle_storage_event(Some(&key)));
        assert_eq!(second.get_ci_repairs()[0].phase, CiRepairPhase::Completed);
        assert!(!second.handle_storage_event(Some("monocode.other")));
    }

    #[test]
    fn writes_the_typescript_record_shape_and_skips_damaged_records() {
        let kv = Kv::in_memory();
        let mut tracker = CiRepairTracker::with_clock(kv.clone(), Arc::new(|| 5));
        let repair = tracker.begin("/web", &request(), "chat", "r1".into());
        assert_eq!(
            kv.get_item(&format!("{CI_REPAIR_ENTRY_PREFIX}r1")).unwrap(),
            r#"{"repo":"acme/web","number":42,"headOid":"old-sha","checks":[{"name":"tests","workflow":"CI","url":"https://github.com/acme/web/actions/runs/1/job/2"}],"id":"r1","cwd":"/web","sessionId":"chat","startedAt":5,"sequence":1,"phase":"running"}"#
        );
        kv.set_item(&format!("{CI_REPAIR_ENTRY_PREFIX}bad"), "{not json");
        kv.set_item(
            &format!("{CI_REPAIR_ENTRY_PREFIX}empty"),
            r#"{"id":"empty","checks":[]}"#,
        );
        let mut reopened = CiRepairTracker::new(kv);
        assert_eq!(reopened.get_ci_repairs().len(), 1);
        assert_eq!(reopened.get_ci_repairs()[0].id, repair.id);
    }
}
