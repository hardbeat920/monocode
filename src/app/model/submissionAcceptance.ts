import type { ProjectLocationSync } from "../../features/projects/model/projectLocation";
import { ProjectNotFoundError } from "../../features/projects/model/projectLocationError";

/** Resolves when the user turn is accepted, not when the agent finishes. */
export type SubmissionAcceptance = boolean | Promise<boolean>;

export async function submitAfterProjectSync(options: {
  cwd: string;
  sync: Promise<ProjectLocationSync | null>;
  applyLocationChange: (from: string, to: string) => Promise<void>;
  submit: () => SubmissionAcceptance;
  onError: (error: unknown) => void;
}): Promise<boolean> {
  try {
    const location = await options.sync;
    if (!location) {
      throw new ProjectNotFoundError(options.cwd);
    }
    if (location.moved) {
      await options.applyLocationChange(options.cwd, location.path);
    }
    return await options.submit();
  } catch (error: unknown) {
    options.onError(error);
    // Preserve permanent failures for callers that decide whether to retry.
    if (error instanceof ProjectNotFoundError) throw error;
    return false;
  }
}
