//! Port of src/integrations/harness/core/abortTextPrompt.ts: race an
//! isolated text prompt against an abort signal without stopping the shared
//! text backend.

use std::future::Future;

use anyhow::anyhow;
use futures::FutureExt;

use super::task::{AbortSignal, BoxFuture};

/// `btwTextCancelled`.
pub fn btw_text_cancelled() -> anyhow::Error {
    anyhow!("By-the-way request cancelled")
}

/// `abortTextPromptRace`. Returns `None` without a signal. Otherwise the
/// future resolves, after `on_abort` finishes, with the cancellation error
/// once the signal aborts. Dropping the future is `detach()`.
///
/// Unlike the promise, the future only watches the signal while something
/// polls it, so race it against the prompt (see [`with_text_prompt_abort`]).
pub fn abort_text_prompt_race<F, Fut>(
    signal: Option<&AbortSignal>,
    on_abort: F,
) -> Option<BoxFuture<'static, anyhow::Error>>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let signal = signal?.clone();
    Some(
        async move {
            signal.aborted().await;
            on_abort().await;
            btw_text_cancelled()
        }
        .boxed(),
    )
}

/// Run `prompt`, or fail with [`btw_text_cancelled`] once `signal` aborts
/// and `on_abort` has run.
pub async fn with_text_prompt_abort<T, P, F, Fut>(
    signal: Option<&AbortSignal>,
    on_abort: F,
    prompt: P,
) -> anyhow::Result<T>
where
    P: Future<Output = anyhow::Result<T>>,
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    match abort_text_prompt_race(signal, on_abort) {
        None => prompt.await,
        Some(race) => smol::future::or(prompt, async move { Err(race.await) }).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn rejects_with_a_btw_cancellation_error_and_runs_the_abort_hook() {
        let signal = AbortSignal::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let race = abort_text_prompt_race(Some(&signal), move || async move {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        signal.abort();
        let error = smol::block_on(race);
        assert_eq!(error.to_string(), btw_text_cancelled().to_string());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn has_no_race_without_a_signal() {
        assert!(abort_text_prompt_race(None, || async {}).is_none());
        let value = smol::block_on(with_text_prompt_abort(None, || async {}, async { Ok(5) }));
        assert_eq!(value.unwrap(), 5);
    }

    #[test]
    fn an_abort_wins_over_a_pending_prompt() {
        let signal = AbortSignal::new();
        signal.abort();
        let result = smol::block_on(with_text_prompt_abort(
            Some(&signal),
            || async {},
            futures::future::pending::<anyhow::Result<()>>(),
        ));
        assert_eq!(
            result.unwrap_err().to_string(),
            "By-the-way request cancelled"
        );
    }
}
