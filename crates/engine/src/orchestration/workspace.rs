//! Port of src/features/orchestration/model/orchestrationWorkspace.ts: worker
//! ownership on sessions and tabs.

use std::collections::HashMap;
use std::future::Future;

use monocode_core::Session;
use monocode_layout::{WorkspaceTab, close_leaf, leaf_ids};

use super::state::OrchestrationRun;

/// `releaseOrchestrationWorker`: drop live and transcript ownership by
/// `lead_id`. `None` when the session has none (the TypeScript returned the
/// same object).
pub fn release_orchestration_worker(session: &Session, lead_id: &str) -> Option<Session> {
    if session.orchestration_lead_id.as_deref() != Some(lead_id)
        && !session
            .blocks
            .iter()
            .any(|block| block.orchestration_lead_id.as_deref() == Some(lead_id))
    {
        return None;
    }
    let mut next = session.clone();
    if next.orchestration_lead_id.as_deref() == Some(lead_id) {
        next.orchestration_lead_id = None;
    }
    for block in &mut next.blocks {
        if block.orchestration_lead_id.as_deref() == Some(lead_id) {
            block.orchestration_lead_id = None;
        }
    }
    Some(next)
}

/// A worker whose transcript should open beside its lead.
pub trait WorkerRef {
    fn lead_id(&self) -> &str;
    fn session_id(&self) -> &str;
}

/// The calls `prepareOrchestrationWorkerDetails` makes.
pub trait WorkerDetailsHost {
    fn open_lead(&self, id: &str) -> impl Future<Output = ()>;
    fn open_worker(&self, id: &str) -> impl Future<Output = ()>;
    fn has_session(&self, id: &str) -> bool;
}

/// The lead and the workers whose sessions loaded.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerDetails<T> {
    pub lead_id: String,
    pub workers: Vec<T>,
}

/// `prepareOrchestrationWorkerDetails`: load the lead first so a fast worker
/// read cannot publish panes without a tab.
pub async fn prepare_orchestration_worker_details<T: WorkerRef + Clone>(
    workers: &[T],
    host: &impl WorkerDetailsHost,
) -> Option<WorkerDetails<T>> {
    let list: Vec<T> = workers
        .iter()
        .filter(|worker| !worker.lead_id().is_empty() && worker.lead_id() != worker.session_id())
        .cloned()
        .collect();
    let lead_id = list.first()?.lead_id().to_string();
    host.open_lead(&lead_id).await;
    if !host.has_session(&lead_id) {
        return None;
    }
    futures::future::join_all(
        list.iter()
            .map(|worker| host.open_worker(worker.session_id())),
    )
    .await;
    if !host.has_session(&lead_id) {
        return None;
    }
    Some(WorkerDetails {
        lead_id,
        workers: list
            .into_iter()
            .filter(|worker| host.has_session(worker.session_id()))
            .collect(),
    })
}

/// What `consolidateOrchestrationTabs` returns. `tabs` is `None` when nothing
/// changed (the TypeScript returned the same array).
#[derive(Debug, Clone, PartialEq)]
pub struct ConsolidatedTabs {
    pub tabs: Option<Vec<WorkspaceTab>>,
    pub active_tab_id: String,
}

/// `consolidateOrchestrationTabs`: adopt worker tabs made by the earlier
/// preview into an already-open lead. A worker the user asked to inspect is
/// a tab inside an editor pane rather than a session leaf, so it never
/// reaches this.
pub fn consolidate_orchestration_tabs(
    tabs: &[WorkspaceTab],
    active_tab_id: &str,
    runs: &[OrchestrationRun],
) -> ConsolidatedTabs {
    let mut parents: HashMap<&str, &str> = HashMap::new();
    for run in runs {
        if tabs
            .iter()
            .any(|tab| leaf_ids(&tab.layout).contains(&run.lead_id))
        {
            for task in &run.tasks {
                parents.insert(&task.session_id, &run.lead_id);
            }
        }
    }
    let active = tabs.iter().find(|tab| tab.id == active_tab_id);
    let lead = active.and_then(|tab| parents.get(tab.focused_id.as_str()).copied());
    let mut changed = false;
    let mut next = Vec::new();
    for tab in tabs {
        let mut remaining = Some(tab.clone());
        for id in leaf_ids(&tab.layout) {
            if let Some(current) = &remaining
                && parents.contains_key(id.as_str())
            {
                remaining = close_leaf(current, &id);
                changed = true;
            }
        }
        if let Some(remaining) = remaining {
            next.push(remaining);
        }
    }
    let active_tab_id = match lead {
        Some(lead) => next
            .iter()
            .find(|tab| leaf_ids(&tab.layout).iter().any(|id| id == lead))
            .map(|tab| tab.id.clone())
            // TODO(port): the TypeScript dereferenced a missing tab here.
            .unwrap_or_else(|| active_tab_id.to_string()),
        None => active_tab_id.to_string(),
    };
    ConsolidatedTabs {
        tabs: changed.then_some(next),
        active_tab_id,
    }
}

/// `attachOrchestrationWorkers`: mark every run's worker sessions with their
/// lead. `None` when nothing changed.
pub fn attach_orchestration_workers(
    sessions: &[Session],
    runs: &[OrchestrationRun],
) -> Option<Vec<Session>> {
    let mut parents: HashMap<&str, &str> = HashMap::new();
    for run in runs {
        for task in &run.tasks {
            parents.insert(&task.session_id, &run.lead_id);
        }
    }
    let mut changed = false;
    let next = sessions
        .iter()
        .map(|session| match parents.get(session.id.as_str()) {
            Some(parent) if session.orchestration_lead_id.as_deref() != Some(*parent) => {
                changed = true;
                Session {
                    orchestration_lead_id: Some(parent.to_string()),
                    ..session.clone()
                }
            }
            _ => session.clone(),
        })
        .collect();
    changed.then_some(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashSet;

    use futures::channel::oneshot;
    use monocode_core::block::BlockRole;
    use monocode_core::{Block, HarnessId};
    use monocode_layout::{SplitDir, new_tab, split_pane};

    use crate::orchestration::state::tests::{run as run_with, task};

    fn run() -> OrchestrationRun {
        run_with(vec![task("worker-a"), task("worker-b")])
    }

    fn session(id: &str) -> Session {
        Session::blank(id, HarnessId::Claude, "claude:test", "/repo")
    }

    #[test]
    fn releases_both_live_and_transcript_ownership_when_a_lead_is_deleted() {
        let mut worker = session("w");
        worker.orchestration_lead_id = Some("lead".into());
        worker.blocks.push(Block {
            orchestration_lead_id: Some("lead".into()),
            ..Block::new("u", BlockRole::User, "Task")
        });
        let released = release_orchestration_worker(&worker, "lead").unwrap();
        assert_eq!(released.orchestration_lead_id, None);
        assert_eq!(released.blocks[0].orchestration_lead_id, None);
        assert_eq!(released.blocks[0].text, "Task");
        assert!(release_orchestration_worker(&worker, "other").is_none());
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Worker {
        lead: String,
        session: String,
    }

    impl WorkerRef for Worker {
        fn lead_id(&self) -> &str {
            &self.lead
        }
        fn session_id(&self) -> &str {
            &self.session
        }
    }

    struct Host {
        sessions: RefCell<HashSet<String>>,
        lead_gate: RefCell<Option<oneshot::Receiver<()>>>,
        opened_workers: RefCell<Vec<String>>,
        drop_lead_on_worker: bool,
    }

    impl WorkerDetailsHost for Host {
        async fn open_lead(&self, _id: &str) {
            let gate = self.lead_gate.borrow_mut().take();
            if let Some(gate) = gate {
                let _ = gate.await;
                self.sessions.borrow_mut().insert("lead".into());
            }
        }
        async fn open_worker(&self, id: &str) {
            self.opened_workers.borrow_mut().push(id.to_string());
            if self.drop_lead_on_worker {
                self.sessions.borrow_mut().remove("lead");
            } else {
                self.sessions.borrow_mut().insert(id.to_string());
            }
        }
        fn has_session(&self, id: &str) -> bool {
            self.sessions.borrow().contains(id)
        }
    }

    #[test]
    fn waits_for_a_slow_lead_load_before_opening_fast_worker_transcripts() {
        let (finish_lead, gate) = oneshot::channel();
        let host = Host {
            sessions: RefCell::default(),
            lead_gate: RefCell::new(Some(gate)),
            opened_workers: RefCell::default(),
            drop_lead_on_worker: false,
        };
        let worker = Worker {
            lead: "lead".into(),
            session: "worker-a".into(),
        };
        let workers = [worker.clone()];
        let mut opening = Box::pin(prepare_orchestration_worker_details(&workers, &host));
        assert!(
            futures::executor::block_on(futures::future::poll_immediate(&mut opening)).is_none()
        );
        assert!(host.opened_workers.borrow().is_empty());
        finish_lead.send(()).unwrap();
        assert_eq!(
            futures::executor::block_on(opening),
            Some(WorkerDetails {
                lead_id: "lead".into(),
                workers: vec![worker],
            })
        );
        assert_eq!(*host.opened_workers.borrow(), vec!["worker-a".to_string()]);
    }

    #[test]
    fn does_not_publish_worker_panes_when_their_lead_is_missing_or_closed_during_loading() {
        let worker = Worker {
            lead: "lead".into(),
            session: "worker-a".into(),
        };
        let host = Host {
            sessions: RefCell::default(),
            lead_gate: RefCell::new(None),
            opened_workers: RefCell::default(),
            drop_lead_on_worker: true,
        };
        let workers = [worker];
        assert!(
            futures::executor::block_on(prepare_orchestration_worker_details(&workers, &host))
                .is_none()
        );
        assert!(host.opened_workers.borrow().is_empty());
        host.sessions.borrow_mut().insert("lead".into());
        assert!(
            futures::executor::block_on(prepare_orchestration_worker_details(&workers, &host))
                .is_none()
        );
    }

    #[test]
    fn folds_earlier_worker_tabs_into_the_lead_and_moves_focus_back_to_it() {
        let tabs = vec![
            new_tab("lead"),
            new_tab("worker-a"),
            new_tab("worker-b"),
            new_tab("unrelated"),
        ];
        let result = consolidate_orchestration_tabs(&tabs, &tabs[1].id, &[run()]);
        let next = result.tabs.clone().unwrap();
        assert_eq!(next, vec![tabs[0].clone(), tabs[3].clone()]);
        assert_eq!(result.active_tab_id, tabs[0].id);
        assert!(
            consolidate_orchestration_tabs(&next, &result.active_tab_id, &[run()])
                .tabs
                .is_none()
        );
    }

    #[test]
    fn keeps_unrelated_panes_when_a_worker_shared_a_split_tab() {
        let lead = new_tab("lead");
        let mut split = new_tab("worker-a");
        split.layout = split_pane(&split.layout, "worker-a", SplitDir::Right, "unrelated");
        let result = consolidate_orchestration_tabs(&[lead.clone(), split], &lead.id, &[run()]);
        let tabs = result.tabs.unwrap();
        assert_eq!(tabs.len(), 2);
        assert_eq!(leaf_ids(&tabs[1].layout), vec!["unrelated".to_string()]);
        assert_eq!(tabs[1].focused_id, "unrelated");
    }

    #[test]
    fn retains_worker_ownership_for_tabless_sessions_and_leaves_other_sessions_intact() {
        let sessions: Vec<Session> = ["lead", "worker-a", "worker-b", "unrelated"]
            .into_iter()
            .map(session)
            .collect();
        let next = attach_orchestration_workers(&sessions, &[run()]).unwrap();
        assert_eq!(next[0], sessions[0]);
        assert_eq!(next[1].orchestration_lead_id.as_deref(), Some("lead"));
        assert_eq!(next[2].orchestration_lead_id.as_deref(), Some("lead"));
        assert_eq!(next[3], sessions[3]);
        assert!(attach_orchestration_workers(&next, &[run()]).is_none());
    }
}
