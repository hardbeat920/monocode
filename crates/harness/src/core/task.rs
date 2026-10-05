//! Async support that the TypeScript got from the JavaScript event loop: a
//! spawner supplied by the caller, an abort signal, and smol timers.
//!
//! Nothing here names a runtime. The GPUI app passes a spawner over its
//! background executor, the headless host passes its own, and tests use
//! [`SmolSpawner`].

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

pub use futures::future::BoxFuture;

/// Runs a detached task. The TypeScript started work with promises that ran
/// whether or not anyone awaited them; code that needs that behavior takes a
/// spawner from its caller.
pub trait Spawner: Send + Sync {
    fn spawn(&self, future: BoxFuture<'static, ()>);
}

/// Spawns onto smol's global executor. For tests and tools.
#[derive(Debug, Clone, Copy, Default)]
pub struct SmolSpawner;

impl Spawner for SmolSpawner {
    fn spawn(&self, future: BoxFuture<'static, ()>) {
        smol::spawn(future).detach();
    }
}

impl<F> Spawner for F
where
    F: Fn(BoxFuture<'static, ()>) + Send + Sync,
{
    fn spawn(&self, future: BoxFuture<'static, ()>) {
        self(future)
    }
}

/// A shared spawner handle.
pub type SharedSpawner = Arc<dyn Spawner>;

/// `AbortController` and `AbortSignal` in one value. Clones share one state,
/// so the side that aborts and the side that listens hold the same type.
#[derive(Debug, Clone)]
pub struct AbortSignal {
    // The channel never carries a message. Closing it is the abort, and every
    // clone holds a sender, so dropping one clone never aborts the rest.
    tx: async_channel::Sender<()>,
    rx: async_channel::Receiver<()>,
}

impl Default for AbortSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl AbortSignal {
    pub fn new() -> Self {
        let (tx, rx) = async_channel::bounded(1);
        Self { tx, rx }
    }

    /// `controller.abort()`. Idempotent.
    pub fn abort(&self) {
        self.tx.close();
    }

    /// `signal.aborted`.
    pub fn is_aborted(&self) -> bool {
        self.tx.is_closed()
    }

    /// Resolves once the signal aborts. Resolves at once if it already has.
    pub async fn aborted(&self) {
        let _ = self.rx.recv().await;
    }

    /// `signal.throwIfAborted()`.
    pub fn throw_if_aborted(&self) -> anyhow::Result<()> {
        if self.is_aborted() {
            anyhow::bail!("This operation was aborted");
        }
        Ok(())
    }
}

/// `setTimeout` as a future.
pub async fn sleep(duration: Duration) {
    smol::Timer::after(duration).await;
}

/// Milliseconds as a `Duration`. Negative values read as zero.
pub fn ms(value: i64) -> Duration {
    Duration::from_millis(value.max(0) as u64)
}

/// Run `future`, or give up after `duration`. `None` means it timed out.
pub async fn timeout<F: Future>(duration: Duration, future: F) -> Option<F::Output> {
    smol::future::or(async { Some(future.await) }, async {
        sleep(duration).await;
        None
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abort_wakes_every_clone() {
        smol::block_on(async {
            let signal = AbortSignal::new();
            let listener = signal.clone();
            assert!(!listener.is_aborted());
            let waiting = smol::spawn(async move { listener.aborted().await });
            signal.abort();
            waiting.await;
            assert!(signal.is_aborted());
            assert!(signal.throw_if_aborted().is_err());
        });
    }

    #[test]
    fn dropping_a_clone_does_not_abort() {
        let signal = AbortSignal::new();
        drop(signal.clone());
        assert!(!signal.is_aborted());
    }

    #[test]
    fn timeout_reports_the_slow_side() {
        smol::block_on(async {
            assert_eq!(timeout(Duration::from_secs(5), async { 1 }).await, Some(1));
            assert_eq!(
                timeout(Duration::from_millis(5), futures::future::pending::<()>()).await,
                None
            );
        });
    }
}
