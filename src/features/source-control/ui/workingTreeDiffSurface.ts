import { lazySurface } from "../../../shared/ui/lazySurface";

/** Shared by Changes-panel warmup and the file pane's first render. */
export const WorkingTreeDiffSurface = lazySurface(async () => {
  const module = await import("./WorkingTreeDiff");
  return { default: module.WorkingTreeDiff };
});
