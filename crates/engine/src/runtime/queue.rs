//! Keyed chains of async work: each key runs its operations one at a time,
//! in call order, while different keys run concurrently. This is the
//! `previous.then(operation)` pattern the TypeScript stores used.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared};
use gpui::{BackgroundExecutor, Task};
use parking_lot::Mutex;

/// Resolves once an operation finished, whether it succeeded or not.
pub type Tail = Shared<BoxFuture<'static, ()>>;

struct State<K> {
    queues: HashMap<K, (u64, Tail)>,
    next_id: u64,
}

/// Serial queues, one per key.
pub struct SerialQueues<K> {
    state: Arc<Mutex<State<K>>>,
    executor: BackgroundExecutor,
}

impl<K> Clone for SerialQueues<K> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            executor: self.executor.clone(),
        }
    }
}

impl<K: Eq + Hash + Clone + Send + 'static> SerialQueues<K> {
    pub fn new(executor: BackgroundExecutor) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                queues: HashMap::new(),
                next_id: 0,
            })),
            executor,
        }
    }

    /// Run `operation` after everything already queued for `key`. A failed
    /// or dropped earlier operation does not block this one.
    pub fn enqueue<T: Send + 'static>(
        &self,
        key: K,
        operation: impl FnOnce() -> BoxFuture<'static, T> + Send + 'static,
    ) -> Task<T> {
        let (done, finished) = oneshot::channel::<()>();
        let tail: Tail = finished.map(|_| ()).boxed().shared();
        let (previous, id) = {
            let mut state = self.state.lock();
            let id = state.next_id;
            state.next_id += 1;
            let previous = state
                .queues
                .insert(key.clone(), (id, tail))
                .map(|(_, tail)| tail);
            (previous, id)
        };
        let state = self.state.clone();
        self.executor.spawn(async move {
            if let Some(previous) = previous {
                previous.await;
            }
            let result = operation().await;
            drop(done);
            let mut state = state.lock();
            if state
                .queues
                .get(&key)
                .is_some_and(|(queued, _)| *queued == id)
            {
                state.queues.remove(&key);
            }
            result
        })
    }

    /// The last queued operation for `key`, while one is pending.
    pub fn tail(&self, key: &K) -> Option<Tail> {
        self.state
            .lock()
            .queues
            .get(key)
            .map(|(_, tail)| tail.clone())
    }

    /// Every pending key's last operation.
    pub fn tails(&self) -> Vec<Tail> {
        self.state
            .lock()
            .queues
            .values()
            .map(|(_, tail)| tail.clone())
            .collect()
    }
}
