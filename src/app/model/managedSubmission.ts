import type { ControlOutcome } from "../../features/orchestration/model/orchestration";
import type { SubmissionAcceptance } from "./submissionAcceptance";

/** Await acceptance and guarantee one terminal callback, including rejection
 * before a turn exists. Successful acceptance does not wait for the agent. */
export async function submitWithSettlement(options: {
  submit: (
    onSettled: (outcome: ControlOutcome) => void,
  ) => SubmissionAcceptance;
  onSettled: (outcome: ControlOutcome) => void;
  rejectionMessage: string;
}): Promise<boolean> {
  let settled = false;
  const settle = (outcome: ControlOutcome) => {
    if (settled) return;
    settled = true;
    options.onSettled(outcome);
  };
  let accepted: boolean;
  try {
    accepted = await options.submit(settle);
  } catch (error: unknown) {
    settle({
      status: "failed",
      text: "",
      error: error instanceof Error ? error.message : String(error),
    });
    return false;
  }
  if (!accepted) {
    settle({ status: "failed", text: "", error: options.rejectionMessage });
  }
  return accepted;
}
