/** Run async work with a small fixed worker pool. */
export async function forEachConcurrent<T>(
  items: readonly T[],
  concurrency: number,
  task: (item: T, index: number) => Promise<void>,
  shouldContinue: () => boolean = () => true,
): Promise<void> {
  const workerCount = Math.min(
    items.length,
    Math.max(1, Math.floor(concurrency) || 1),
  );
  let nextIndex = 0;

  const worker = async () => {
    while (shouldContinue()) {
      const index = nextIndex;
      nextIndex += 1;
      if (index >= items.length) return;
      await task(items[index], index);
    }
  };

  await Promise.all(Array.from({ length: workerCount }, worker));
}

export type SequentialResult<T> = {
  done: T[];
  failed: { item: T; error: unknown }[];
};

/**
 * Runs `task` on each item one after another, continuing past failures.
 * For commands that must not overlap (e.g. git's index lock).
 */
export async function runSequential<T>(
  items: readonly T[],
  task: (item: T) => Promise<unknown>,
): Promise<SequentialResult<T>> {
  const result: SequentialResult<T> = { done: [], failed: [] };
  for (const item of items) {
    try {
      await task(item);
      result.done.push(item);
    } catch (error) {
      result.failed.push({ item, error });
    }
  }
  return result;
}
