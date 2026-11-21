export type ModelChangeResult = "applied" | "superseded";

export class ModelChangeError extends Error {
  constructor(reason: unknown, readonly sending: boolean) {
    super(reason instanceof Error ? reason.message : String(reason));
  }
}

type PendingModelChange = {
  result: Promise<ModelChangeResult>;
  flushForSend: () => void;
  supersede: () => void;
};

/** Coalesce slider changes and wait for any request already on the wire. */
export function scheduleModelChange(
  previous: PendingModelChange | undefined,
  apply: () => Promise<void>,
): PendingModelChange {
  previous?.supersede();
  let sending = false;
  let superseded = false;
  let start!: () => void;
  const delay = new Promise<void>((resolve) => { start = resolve; });
  const timer = setTimeout(start, 400);
  const flush = () => {
    clearTimeout(timer);
    start();
  };
  const result = delay.then(async (): Promise<ModelChangeResult> => {
    await previous?.result.catch(() => {});
    if (superseded) return "superseded";
    try {
      await apply();
      return "applied";
    } catch (reason) {
      throw new ModelChangeError(reason, sending);
    }
  });
  return {
    result,
    flushForSend: () => {
      sending = true;
      flush();
    },
    supersede: () => {
      superseded = true;
      flush();
    },
  };
}
