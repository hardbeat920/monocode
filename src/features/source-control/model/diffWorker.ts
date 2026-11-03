import type { UnifiedFileDiff } from "./unifiedDiff";

type Job = {
  id: number;
  original: string;
  current: string;
  priority: number;
  resolve: (result: UnifiedFileDiff) => void;
  reject: (error: Error) => void;
  cleanup: () => void;
};
let worker: Worker | undefined;
let active: Job | undefined;
let sequence = 0;
const queued: Job[] = [];

function fail(error: Error) {
  worker?.terminate();
  worker = undefined;
  const jobs = [...(active ? [active] : []), ...queued.splice(0)];
  active = undefined;
  for (const job of jobs) {
    job.cleanup();
    job.reject(error);
  }
}

function pump() {
  if (active || !queued.length) return;
  queued.sort((a, b) => a.priority - b.priority || a.id - b.id);
  active = queued.shift()!;
  try {
    if (!worker) {
      worker = new Worker(new URL("./unifiedDiff.worker.ts", import.meta.url), {
        type: "module",
      });
      worker.onmessage = ({
        data,
      }: MessageEvent<{
        id: number;
        result: UnifiedFileDiff;
        error?: string;
      }>) => {
        const job = active;
        if (!job || job.id !== data.id) return;
        active = undefined;
        job.cleanup();
        if (data.error) job.reject(new Error(data.error));
        else job.resolve(data.result);
        pump();
      };
      const currentWorker = worker;
      worker.onerror = () => {
        if (worker === currentWorker)
          fail(new Error("Could not prepare the diff"));
      };
    }
    const { id, original, current } = active;
    worker.postMessage({ id, original, current });
  } catch (error) {
    fail(error instanceof Error ? error : new Error(String(error)));
  }
}

/** One diff computation at a time; navigation cancels queued and running work. */
export function prepareUnifiedDiff(
  original: string,
  current: string,
  signal: AbortSignal,
  priority = 1,
): Promise<UnifiedFileDiff> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(new DOMException("Diff cancelled", "AbortError"));
      return;
    }
    const job: Job = {
      id: ++sequence,
      original,
      current,
      priority,
      resolve,
      reject,
      cleanup: () => signal.removeEventListener("abort", cancel),
    };
    const cancel = () => {
      const index = queued.indexOf(job);
      if (index >= 0) queued.splice(index, 1);
      if (active === job) {
        worker?.terminate();
        worker = undefined;
        active = undefined;
      }
      job.cleanup();
      reject(new DOMException("Diff cancelled", "AbortError"));
      // A group abort removes all its jobs before choosing the next one.
      queueMicrotask(pump);
    };
    signal.addEventListener("abort", cancel, { once: true });
    queued.push(job);
    pump();
  });
}
