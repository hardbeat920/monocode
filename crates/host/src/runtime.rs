//! The host's async runtime: a small smol executor on its own threads.
//!
//! The Node host ran everything on one event loop. Provider adapters here
//! are runtime-agnostic and take a spawner, so the host hands them one over
//! this executor. Blocking callers, such as the server's connection
//! threads, wait on futures with `smol::block_on`.

use std::sync::Arc;
use std::thread::JoinHandle;

use futures::future::BoxFuture;
use monocode_harness::core::task::SharedSpawner;
use parking_lot::Mutex;

pub struct HostRuntime {
    executor: Arc<smol::Executor<'static>>,
    stop: async_channel::Sender<()>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl HostRuntime {
    /// Starts `threads` worker threads.
    pub fn new(threads: usize) -> Self {
        let executor = Arc::new(smol::Executor::new());
        let (stop, stopped) = async_channel::bounded::<()>(1);
        let threads = (0..threads.max(1))
            .map(|index| {
                let executor = executor.clone();
                let stopped = stopped.clone();
                std::thread::Builder::new()
                    .name(format!("monocode-host-{index}"))
                    .spawn(move || {
                        smol::block_on(executor.run(async move {
                            let _ = stopped.recv().await;
                        }))
                    })
                    .expect("start a host runtime thread")
            })
            .collect();
        Self {
            executor,
            stop,
            threads: Mutex::new(threads),
        }
    }

    /// Spawns detached tasks on this runtime.
    pub fn spawner(&self) -> SharedSpawner {
        let executor = self.executor.clone();
        Arc::new(move |future: BoxFuture<'static, ()>| {
            executor.spawn(future).detach();
        })
    }

    /// Stops the worker threads. Tasks still pending are dropped.
    pub fn shutdown(&self) {
        self.stop.close();
        let threads = std::mem::take(&mut *self.threads.lock());
        let current = std::thread::current().id();
        for thread in threads {
            if thread.thread().id() != current {
                let _ = thread.join();
            }
        }
    }
}

impl Drop for HostRuntime {
    fn drop(&mut self) {
        self.stop.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_spawned_tasks_until_shutdown() {
        let runtime = HostRuntime::new(2);
        let (sender, receiver) = async_channel::bounded(1);
        runtime.spawner().spawn(Box::pin(async move {
            smol::Timer::after(std::time::Duration::from_millis(5)).await;
            let _ = sender.send(7).await;
        }));
        assert_eq!(smol::block_on(receiver.recv()).unwrap(), 7);
        runtime.shutdown();
    }
}
