//! Port of src/shared/lib/concurrent.ts.

use std::cell::Cell;
use std::future::Future;

/// `forEachConcurrent`: run `task` over `items` with at most `concurrency`
/// in flight. Workers stop taking new items once `should_continue` is false.
/// The workers are futures polled together on the caller's task, so they
/// share one thread like the JavaScript ones did.
pub async fn for_each_concurrent<T, F, Fut>(
    items: &[T],
    concurrency: usize,
    task: F,
    should_continue: impl Fn() -> bool,
) where
    F: Fn(&T, usize) -> Fut,
    Fut: Future<Output = ()>,
{
    let worker_count = items.len().min(concurrency.max(1));
    let next_index = Cell::new(0);
    let worker = || async {
        while should_continue() {
            let index = next_index.get();
            next_index.set(index + 1);
            if index >= items.len() {
                return;
            }
            task(&items[index], index).await;
        }
    };
    futures::future::join_all((0..worker_count).map(|_| worker())).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn bounds_in_flight_work_and_visits_every_item() {
        let active = Cell::new(0);
        let peak = Cell::new(0);
        let seen = RefCell::new(Vec::new());
        smol::block_on(for_each_concurrent(
            &[0, 1, 2, 3, 4, 5],
            2,
            |item, _| {
                let item = *item;
                let (active, peak, seen) = (&active, &peak, &seen);
                async move {
                    active.set(active.get() + 1);
                    peak.set(peak.get().max(active.get()));
                    smol::future::yield_now().await;
                    seen.borrow_mut().push(item);
                    active.set(active.get() - 1);
                }
            },
            || true,
        ));
        assert_eq!(peak.get(), 2);
        let mut seen = seen.into_inner();
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn stops_assigning_new_work_after_cancellation() {
        let running = Cell::new(true);
        let seen = RefCell::new(Vec::new());
        smol::block_on(for_each_concurrent(
            &[0, 1, 2, 3],
            1,
            |item, _| {
                seen.borrow_mut().push(*item);
                running.set(false);
                async {}
            },
            || running.get(),
        ));
        assert_eq!(seen.into_inner(), vec![0]);
    }
}
