import { invoke } from "@tauri-apps/api/core";

/** Frozen cross-lane contract for MonoCode project Kanban. */
export type BoardStatus =
  | "backlog"
  | "ready"
  | "in-progress"
  | "blocked"
  | "done";

export type BoardPriority = "low" | "medium" | "high";

export type BoardPlacement = "tab" | "right" | "down";

export interface BoardMediaRef {
  id: string;
  name: string;
  mimeType: "image/png" | "image/jpeg" | "image/gif" | "image/webp";
  byteLength: number;
}

export interface ProjectBoardCard {
  id: string;
  projectCwd: string;
  title: string;
  description: string;
  status: BoardStatus;
  priority: BoardPriority;
  linkedSessionIds: string[];
  media: BoardMediaRef[];
  createdAt: number;
  updatedAt: number;
}

export type ProjectBoardCardInput = Pick<
  ProjectBoardCard,
  "id" | "projectCwd" | "title" | "description" | "status" | "priority" | "linkedSessionIds"
>;

export type ProjectBoardCardPatch = Partial<
  Pick<ProjectBoardCard, "title" | "description" | "status" | "priority">
>;

export interface AddMediaInput {
  id?: string;
  projectCwd: string;
  cardId: string;
  name: string;
  mimeType: BoardMediaRef["mimeType"];
  dataBase64: string;
}

export interface BoardMedia extends BoardMediaRef {
  dataBase64: string;
}

/** Every query/mutation is scoped by projectCwd and cardId. */
export interface ProjectBoardRepository {
  list(projectCwd: string): Promise<ProjectBoardCard[]>;
  upsertCard(input: ProjectBoardCardInput): Promise<ProjectBoardCard>;
  patchCard(projectCwd: string, cardId: string, patch: ProjectBoardCardPatch): Promise<ProjectBoardCard>;
  linkSession(projectCwd: string, cardId: string, sessionId: string): Promise<ProjectBoardCard | null>;
  deleteCard(projectCwd: string, cardId: string): Promise<void>;
  addMedia(input: AddMediaInput): Promise<BoardMediaRef>;
  getMedia(projectCwd: string, cardId: string, mediaId: string): Promise<BoardMedia | null>;
  deleteMedia(projectCwd: string, cardId: string, mediaId: string): Promise<void>;
}

export interface ProjectBoardToolHost {
  repository: ProjectBoardRepository;
  startSession(input: {
    projectCwd: string;
    requestId: string;
    prompt: string;
    placement: BoardPlacement;
  }): Promise<{ sessionId: string }>;
}

export interface ProjectBoardToolContext {
  projectCwd: string;
  requestId: string;
  placement?: BoardPlacement;
}

export interface ProjectBoardDialogProps {
  projectCwd: string;
  onClose(): void;
  onOpenSession(sessionId: string): void;
}

export interface ProjectRailBoardActionProps {
  onOpenProjectBoard?: (projectCwd: string) => void;
}

export const BOARD_VALIDATION: Readonly<{
  titleMax: 160;
  descriptionMax: 12000;
  linkedSessionsMax: 50;
  mediaBytesMax: 5242880;
  mediaTypes: readonly BoardMediaRef["mimeType"][];
  startCardIdsMax: 10;
}> = Object.freeze({
  titleMax: 160,
  descriptionMax: 12000,
  linkedSessionsMax: 50,
  mediaBytesMax: 5242880,
  mediaTypes: Object.freeze([
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
  ] as const),
  startCardIdsMax: 10,
} as const);

export const PROJECT_BOARD_CHANGED_EVENT = "monocode:project-board-changed";

export interface ProjectBoardChangedDetail {
  projectCwd: string;
}

export function dispatchProjectBoardChanged(projectCwd: string): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<ProjectBoardChangedDetail>(PROJECT_BOARD_CHANGED_EVENT, {
      detail: { projectCwd },
    }),
  );
}

export function subscribeProjectBoard(
  projectCwd: string,
  onChange: () => void,
): () => void {
  if (typeof window === "undefined") return () => {};
  const listener = (event: Event) => {
    const detail = (event as CustomEvent).detail;
    const changed =
      typeof detail === "string"
        ? detail
        : detail?.projectCwd ?? detail?.cwd;
    if (!changed || changed === projectCwd) {
      onChange();
    }
  };
  window.addEventListener(PROJECT_BOARD_CHANGED_EVENT, listener);
  return () => window.removeEventListener(PROJECT_BOARD_CHANGED_EVENT, listener);
}

/** Typed wrapper for listing cards in a project. */
export async function project_board_list(
  projectCwd: string,
): Promise<ProjectBoardCard[]> {
  return invoke<ProjectBoardCard[]>("project_board_list", { projectCwd });
}

/** Typed wrapper for creating or updating a card. Dispatches change event on success. */
export async function project_board_upsert_card(
  input: ProjectBoardCardInput,
): Promise<ProjectBoardCard> {
  const card = await invoke<ProjectBoardCard>("project_board_upsert_card", {
    input,
  });
  dispatchProjectBoardChanged(input.projectCwd);
  return card;
}


/** Typed wrapper for patching card fields. Dispatches change event on success. */
export async function project_board_patch_card(
  projectCwd: string,
  cardId: string,
  patch: ProjectBoardCardPatch,
): Promise<ProjectBoardCard> {
  const card = await invoke<ProjectBoardCard>("project_board_patch_card", {
    projectCwd,
    cardId,
    patch,
  });
  dispatchProjectBoardChanged(projectCwd);
  return card;
}

/** Typed wrapper for atomically linking a session to a card. Dispatches change event if card updated. */
export async function project_board_link_session(
  projectCwd: string,
  cardId: string,
  sessionId: string,
): Promise<ProjectBoardCard | null> {
  const card = await invoke<ProjectBoardCard | null>("project_board_link_session", {
    projectCwd,
    cardId,
    sessionId,
  });
  if (card) {
    dispatchProjectBoardChanged(projectCwd);
  }
  return card;
}

/** Typed wrapper for deleting a card. Dispatches change event on success. */
export async function project_board_delete_card(
  projectCwd: string,
  cardId: string,
): Promise<void> {
  await invoke<void>("project_board_delete_card", { projectCwd, cardId });
  dispatchProjectBoardChanged(projectCwd);
}

/** Typed wrapper for adding media to a card. Accepts base64, returns metadata ref. */
export async function project_board_add_media(input: {
  projectCwd: string;
  cardId: string;
  name: string;
  mimeType: BoardMediaRef["mimeType"];
  dataBase64: string;
}): Promise<BoardMediaRef> {
  const mediaRef = await invoke<BoardMediaRef>("project_board_add_media", {
    input,
  });
  dispatchProjectBoardChanged(input.projectCwd);
  return mediaRef;
}

/** Typed wrapper for retrieving media by ID, returning metadata plus base64 payload. */
export async function project_board_get_media(
  projectCwd: string,
  cardId: string,
  mediaId: string,
): Promise<BoardMedia | null> {
  return invoke<BoardMedia | null>("project_board_get_media", {
    projectCwd,
    cardId,
    mediaId,
  });
}

/** Typed wrapper for deleting media from a card. Dispatches change event on success. */
export async function project_board_delete_media(
  projectCwd: string,
  cardId: string,
  mediaId: string,
): Promise<void> {
  await invoke<void>("project_board_delete_media", {
    projectCwd,
    cardId,
    mediaId,
  });
  dispatchProjectBoardChanged(projectCwd);
}

// CamelCase aliases
export const projectBoardList = project_board_list;
export const projectBoardUpsertCard = project_board_upsert_card;
export const projectBoardPatchCard = project_board_patch_card;
export const projectBoardLinkSession = project_board_link_session;
export const projectBoardDeleteCard = project_board_delete_card;
export const projectBoardAddMedia = project_board_add_media;
export const projectBoardGetMedia = project_board_get_media;
export const projectBoardDeleteMedia = project_board_delete_media;

export const projectBoardRepository: ProjectBoardRepository = {
  list: project_board_list,
  upsertCard: project_board_upsert_card,
  patchCard: project_board_patch_card,
  linkSession: project_board_link_session,
  deleteCard: project_board_delete_card,
  addMedia: project_board_add_media,
  getMedia: project_board_get_media,
  deleteMedia: project_board_delete_media,
};

export function createProjectBoardRepository(): ProjectBoardRepository {
  return projectBoardRepository;
}
