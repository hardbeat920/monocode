import type { ControlOutcome } from "../../features/orchestration/model/orchestration";
import {
  updateAutomationRun,
  type AutomationRun,
} from "../../features/automations/model/automations";
import { submitWithSettlement } from "./managedSubmission";
import type { SubmissionAcceptance } from "./submissionAcceptance";

/** Release a head claim when a newer Inbox poll completed before dispatch. */
export async function releaseStaleInboxHeadClaim(
  run: AutomationRun,
  isCurrent: () => boolean,
): Promise<boolean> {
  if (run.event !== "pull_request_head_changed" || isCurrent()) return false;
  try {
    await updateAutomationRun(run.id, "pending", {
      error: "A newer Inbox observation arrived before dispatch; waiting for fresh GitHub state.",
    });
    return true;
  } catch {
    // Keep the durable reservation moving if it cannot be released.
    return false;
  }
}

export async function dispatchInboxAutomationRuns<T extends { run: AutomationRun }>(
  due: Promise<readonly T[]>,
  isCurrent: () => boolean,
  launch: (item: T) => Promise<unknown>,
): Promise<void> {
  for (const item of await due) {
    if (await releaseStaleInboxHeadClaim(item.run, isCurrent)) continue;
    await launch(item).catch(() => undefined);
  }
}

/** Preserve the durable head claim on launch rejection, but never retry an
 * accepted agent turn (even if that turn later fails or is cancelled). */
export async function submitAutomationRun(
  run: AutomationRun,
  sessionId: string,
  submit: (
    onSettled: (outcome: ControlOutcome) => void,
  ) => SubmissionAcceptance,
  release: () => void,
): Promise<void> {
  let accepted: boolean | undefined;
  let earlyOutcome: ControlOutcome | undefined;
  const settle = (outcome: ControlOutcome) => {
    const status =
      outcome.status === "completed"
        ? "succeeded"
        : outcome.status === "cancelled"
          ? "cancelled"
          : "failed";
    void updateAutomationRun(run.id, status, {
      sessionId,
      ...(outcome.error ? { error: outcome.error } : {}),
    })
      .catch(() => undefined)
      .finally(release);
  };
  accepted = await submitWithSettlement({
    submit,
    rejectionMessage: "The selected agent session could not start this run.",
    onSettled: (outcome) => {
      if (accepted === undefined) earlyOutcome = outcome;
      else settle(outcome);
    },
  });
  if (!accepted && run.event === "pull_request_head_changed") {
    try {
      await updateAutomationRun(run.id, "pending", {
        sessionId,
        error:
          earlyOutcome?.error ??
          "Launch was not accepted; retrying on the next GitHub refresh.",
      });
    } finally {
      release();
    }
  } else if (earlyOutcome) settle(earlyOutcome);
}
