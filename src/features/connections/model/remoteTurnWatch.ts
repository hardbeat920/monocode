import type { ControlOutcome } from "../../orchestration/model/orchestration";
import type { HostSession } from "./protocol";

// Matches the host engine's message for a turn the user stopped.
const STOPPED = "Stopped by you.";
// A dispatched turn the host never shows is treated as lost after this many
// idle reads, so a waiting caller is not left without an outcome.
const MISSING_READS = 3;

const watches = new Map<string, Set<(outcome: ControlOutcome) => void>>();

/** How an accepted host turn ended, or undefined while it is still running. */
export function remoteTurnOutcome(
  snapshot: HostSession,
  commandId: string,
): ControlOutcome | undefined {
  if (snapshot.status === "running" || snapshot.session.busy) return undefined;
  const blocks = snapshot.session.blocks;
  const start = blocks.findIndex((block) => block.id === commandId);
  if (start < 0) return undefined;
  const turn = blocks.slice(start + 1);
  const reply =
    [...turn]
      .reverse()
      .find(
        (block) =>
          block.role === "assistant" &&
          !block.tool &&
          !block.internal &&
          block.text.trim(),
      )?.text ?? "";
  const last = turn[turn.length - 1];
  // The host ends a stopped or failed turn with a plain system message.
  const ended =
    last?.role === "system" && !last.statusKey && !last.interjection
      ? last.text
      : undefined;
  if (ended === STOPPED) return { status: "cancelled", text: reply };
  if (snapshot.status === "interrupted" || ended)
    return {
      status: "failed",
      text: reply,
      error: ended ?? "The host interrupted this turn",
    };
  return { status: "completed", text: reply };
}

/** Follows an accepted host turn until it settles, whether or not the tab's
 * pane is mounted, and reports its outcome once. */
export function watchRemoteTurn(
  shellId: string,
  options: {
    commandId: string;
    load: (known?: HostSession) => Promise<HostSession>;
    onSnapshot?: (snapshot: HostSession) => void;
    onSettled: (outcome: ControlOutcome) => void;
  },
): void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let done = false;
  let known: HostSession | undefined;
  let failed = 0;
  let missing = 0;
  const finish = (outcome: ControlOutcome) => {
    if (done) return;
    done = true;
    clearTimeout(timer);
    const active = watches.get(shellId);
    active?.delete(finish);
    if (!active?.size) watches.delete(shellId);
    options.onSettled(outcome);
  };
  const poll = async () => {
    try {
      const next = await options.load(known);
      if (done) return;
      known = next;
      failed = 0;
      options.onSnapshot?.(next);
      const outcome = remoteTurnOutcome(next, options.commandId);
      if (outcome) return finish(outcome);
      const idle = next.status !== "running" && !next.session.busy;
      missing = idle ? missing + 1 : 0;
      if (missing >= MISSING_READS)
        return finish({
          status: "failed",
          text: "",
          error: "The host has no record of this turn",
        });
    } catch {
      if (done) return;
      // Keep waiting through SSH interruptions, backing off like the pane.
      failed++;
    }
    timer = setTimeout(
      () => void poll(),
      failed ? Math.min(10_000, 750 * 2 ** Math.min(failed, 4)) : 1_500,
    );
  };
  const active = watches.get(shellId) ?? new Set();
  active.add(finish);
  watches.set(shellId, active);
  void poll();
}

/** Ends watches for tabs that are gone, as removing a local session ends its turn. */
export function pruneRemoteTurnWatches(open: (shellId: string) => boolean) {
  for (const [shellId, active] of [...watches])
    if (!open(shellId))
      for (const finish of [...active])
        finish({
          status: "cancelled",
          text: "",
          error: "The session closed before its turn finished",
        });
}
