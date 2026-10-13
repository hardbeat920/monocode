export * from "../model/projectBoard";

import type {
  BoardStatus,
  ProjectBoardDialogProps as ModelProjectBoardDialogProps,
  ProjectBoardRepository,
} from "../model/projectBoard";

export interface ProjectBoardDialogProps extends ModelProjectBoardDialogProps {
  repository?: ProjectBoardRepository;
}

export const BOARD_LANES: ReadonlyArray<{
  status: BoardStatus;
  label: string;
  description: string;
}> = [
  { status: "backlog", label: "Backlog", description: "Tasks planned for the future" },
  { status: "ready", label: "Ready", description: "Tasks ready to be picked up" },
  { status: "in-progress", label: "In progress", description: "Tasks actively being worked on" },
  { status: "blocked", label: "Blocked", description: "Tasks waiting on dependencies or input" },
  { status: "done", label: "Done", description: "Completed tasks" },
];

/** Frozen acceptance contract for the PR2 review correction. */
export interface ProjectBoardHardeningContract {
  textWrapUtility: "wrap-anywhere";
  freeformSessionLinks: false;
  cardDeleteConfirmation: true;
  failedMediaDeleteKeepsPreviewOpen: true;
  topmostDialogFocusTrap: true;
  boardStartIsOnlySessionLinkWriter: true;
  maxChildRequestIdLength: 128;
}

export const PROJECT_BOARD_HARDENING: ProjectBoardHardeningContract = Object.freeze({
  textWrapUtility: "wrap-anywhere",
  freeformSessionLinks: false,
  cardDeleteConfirmation: true,
  failedMediaDeleteKeepsPreviewOpen: true,
  topmostDialogFocusTrap: true,
  boardStartIsOnlySessionLinkWriter: true,
  maxChildRequestIdLength: 128,
} as const);
