export function btwTextCancelled(): Error {
  return new Error("By-the-way request cancelled");
}

/** Races an isolated text prompt against an AbortSignal without stopping the backend globally. */
export function abortTextPromptRace(
  signal: AbortSignal | undefined,
  onAbort: () => void | Promise<void>,
): { promise: Promise<never> | null; detach: () => void } {
  if (!signal) return { promise: null, detach: () => undefined };
  let handler: (() => void) | undefined;
  const promise = new Promise<never>((_, reject) => {
    handler = () => {
      void Promise.resolve(onAbort()).finally(() => reject(btwTextCancelled()));
    };
    signal.addEventListener("abort", handler, { once: true });
    if (signal.aborted) handler();
  });
  return {
    promise,
    detach: () => {
      if (handler) signal.removeEventListener("abort", handler);
    },
  };
}
