//! `btwRequestsRef` from App.tsx: the side-thread requests in flight, keyed
//! by `"{sessionId}:{threadId}"`, with the abort controller of each.
//!
//! The map is shared between the `SideThreads` entity and the runtime hooks,
//! because the runtime calls `SideThreadHooks` while it updates `Sessions`,
//! where the entity may already be borrowed.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use monocode_harness::core::task::AbortSignal;

/// The `btwRequestsRef` key for one thread.
pub fn request_key(session_id: &str, thread_id: &str) -> String {
    format!("{session_id}:{thread_id}")
}

struct BtwRequest {
    session_id: String,
    signal: AbortSignal,
    /// Stands in for the controller's identity in the `finally` check.
    token: u64,
}

#[derive(Default)]
struct State {
    requests: HashMap<String, BtwRequest>,
    next_token: u64,
}

/// The requests in flight. Clones share one map.
#[derive(Clone, Default)]
pub(crate) struct BtwRequests(Rc<RefCell<State>>);

impl BtwRequests {
    /// Abort the request already under `key`, then register a new one.
    pub fn start(&self, key: &str, session_id: &str) -> (AbortSignal, u64) {
        let mut state = self.0.borrow_mut();
        if let Some(previous) = state.requests.get(key) {
            previous.signal.abort();
        }
        state.next_token += 1;
        let token = state.next_token;
        let signal = AbortSignal::new();
        state.requests.insert(
            key.to_string(),
            BtwRequest {
                session_id: session_id.to_string(),
                signal: signal.clone(),
                token,
            },
        );
        (signal, token)
    }

    /// `get(key)?.controller.abort()` and `delete(key)`.
    pub fn abort(&self, key: &str) {
        let removed = self.0.borrow_mut().requests.remove(key);
        if let Some(request) = removed {
            request.signal.abort();
        }
    }

    /// The `finally` cleanup: forget the request only when it is still this
    /// one.
    pub fn finish(&self, key: &str, token: u64) {
        let mut state = self.0.borrow_mut();
        if state
            .requests
            .get(key)
            .is_some_and(|request| request.token == token)
        {
            state.requests.remove(key);
        }
    }

    /// Abort the requests whose session is no longer open.
    pub fn abort_closed(&self, live_ids: &HashSet<String>) {
        let mut state = self.0.borrow_mut();
        state.requests.retain(|_, request| {
            if live_ids.contains(&request.session_id) {
                return true;
            }
            request.signal.abort();
            false
        });
    }

    /// Abort and forget every request.
    pub fn abort_all(&self) {
        let drained: Vec<BtwRequest> = self
            .0
            .borrow_mut()
            .requests
            .drain()
            .map(|(_, request)| request)
            .collect();
        for request in drained {
            request.signal.abort();
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.borrow().requests.contains_key(key)
    }
}
