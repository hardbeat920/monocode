//! Runs blocking updater work on its own thread and hands the result back
//! through `async-channel`, so callers can await it under GPUI's executor,
//! `smol`, or a plain `block_on`.

use crate::error::{Error, Result};

/// Starts `work` on a named worker thread.
pub(crate) fn spawn(work: impl FnOnce() + Send + 'static) -> Result<()> {
    std::thread::Builder::new()
        .name("monocode-updater".into())
        .spawn(work)?;
    Ok(())
}

/// Runs `work` on a worker thread and resolves with its result.
pub(crate) async fn run_blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T> {
    let (tx, rx) = async_channel::bounded(1);
    spawn(move || {
        let _ = tx.send_blocking(work());
    })?;
    rx.recv().await.map_err(|_| Error::WorkerStopped)
}
