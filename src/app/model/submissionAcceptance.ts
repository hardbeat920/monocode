import type { ProjectLocationSync } from "../../features/projects/model/projectLocation";
import { ProjectNotFoundError } from "../../features/projects/model/projectLocationError";

/** Resolves when the user turn is accepted, not when the agent finishes. */
export type SubmissionAcceptance = boolean | Promise<boolean>;

export function getOrStartProjectLocationSync<T>(
  syncs: Map<string, Promise<T>>,
  key: string,
  start: () => Promise<T>,
  signal?: AbortSignal,
): Promise<T> {
  let sync = syncs.get(key);
  if (!sync) {
    sync = start();
    syncs.set(key, sync);
    const clear = () => {
      if (syncs.get(key) === sync) syncs.delete(key);
    };
    void sync.then(clear, clear);
  }

  if (signal) {
    const invalidate = () => {
      if (syncs.get(key) === sync) syncs.delete(key);
    };
    signal.addEventListener("abort", invalidate, { once: true });
    void sync.then(
      () => signal.removeEventListener("abort", invalidate),
      () => signal.removeEventListener("abort", invalidate),
    );
    if (signal.aborted) invalidate();
  }
  return sync;
}

export async function submitAfterProjectSync(options: {
  cwd: string;
  sync: Promise<ProjectLocationSync | null>;
  applyLocationChange: (from: string, to: string) => Promise<void>;
  submit: () => SubmissionAcceptance;
  onError: (error: unknown) => void;
  signal?: AbortSignal;
}): Promise<boolean> {
  try {
    let location: ProjectLocationSync | null;
    if (options.signal) {
      const signal = options.signal;
      if (signal.aborted) return false;
      const aborted = Symbol("aborted");
      let onAbort!: () => void;
      const abort = new Promise<typeof aborted>((resolve) => {
        onAbort = () => resolve(aborted);
        signal.addEventListener("abort", onAbort, { once: true });
      });
      try {
        const result = await Promise.race([options.sync, abort]);
        if (result === aborted || signal.aborted) return false;
        location = result;
      } finally {
        signal.removeEventListener("abort", onAbort);
      }
    } else {
      location = await options.sync;
    }
    if (!location) {
      throw new ProjectNotFoundError(options.cwd);
    }
    if (location.moved) {
      await options.applyLocationChange(options.cwd, location.path);
    }
    if (options.signal?.aborted) return false;
    return await options.submit();
  } catch (error: unknown) {
    options.onError(error);
    // Preserve permanent failures for callers that decide whether to retry.
    if (error instanceof ProjectNotFoundError) throw error;
    return false;
  }
}
