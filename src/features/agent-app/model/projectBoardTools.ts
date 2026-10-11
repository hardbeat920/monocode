/**
 * MonoCode Project Kanban Board Tools.
 *
 * Isolated dispatcher and tool definitions for board.list, board.create,
 * board.update, and board.start actions.
 */

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
  Pick<ProjectBoardCard, "title" | "description" | "status" | "priority" | "linkedSessionIds">
>;

export interface BoardMedia extends BoardMediaRef {
  dataBase64: string;
}

/** Every query/mutation is scoped by projectCwd and cardId. */
export interface ProjectBoardRepository {
  list(projectCwd: string): Promise<ProjectBoardCard[]>;
  upsertCard(input: ProjectBoardCardInput): Promise<ProjectBoardCard>;
  deleteCard(projectCwd: string, cardId: string): Promise<void>;
  addMedia(input: {
    projectCwd: string;
    cardId: string;
    name: string;
    mimeType: BoardMediaRef["mimeType"];
    dataBase64: string;
  }): Promise<BoardMediaRef>;
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

export const BOARD_VALIDATION = {
  titleMax: 160,
  descriptionMax: 12000,
  linkedSessionsMax: 50,
  mediaBytesMax: 5242880,
  mediaTypes: ["image/png", "image/jpeg", "image/gif", "image/webp"] as const,
  startCardIdsMax: 10,
} as const;

export const VALID_STATUSES: readonly BoardStatus[] = [
  "backlog",
  "ready",
  "in-progress",
  "blocked",
  "done",
];

export const VALID_PRIORITIES: readonly BoardPriority[] = [
  "low",
  "medium",
  "high",
];

export const VALID_PLACEMENTS: readonly BoardPlacement[] = [
  "tab",
  "right",
  "down",
];

export const PROJECT_BOARD_FIELDS = new Map<string, readonly string[]>([
  ["board.list", ["status"]],
  ["board.create", ["id", "title", "description", "status", "priority"]],
  ["board.update", ["id", "cardId", "title", "description", "status", "priority"]],
  ["board.start", ["cardIds", "ids", "placement"]],
]);

export interface ProjectBoardToolDefinition {
  name: "board.list" | "board.create" | "board.update" | "board.start";
  description: string;
  parameters: {
    type: "object";
    properties: Record<
      string,
      {
        type: string;
        description: string;
        items?: { type: string };
        enum?: readonly string[];
      }
    >;
    required?: readonly string[];
  };
}

export const PROJECT_BOARD_TOOL_DEFINITIONS: readonly ProjectBoardToolDefinition[] = [
  {
    name: "board.list",
    description:
      "List Kanban cards for the authorized project. Use when the user asks to see or inspect the project board. Inspect before acting if card identity is ambiguous.",
    parameters: {
      type: "object",
      properties: {
        status: {
          type: "string",
          enum: VALID_STATUSES,
          description: "Optional column status filter (backlog, ready, in-progress, blocked, done).",
        },
      },
    },
  },
  {
    name: "board.create",
    description:
      "Create a new card on the authorized project Kanban board. The card will belong strictly to the authorized project.",
    parameters: {
      type: "object",
      properties: {
        title: {
          type: "string",
          description: "Card title (required, non-empty, up to 160 characters).",
        },
        description: {
          type: "string",
          description: "Card description with task details and context (up to 12,000 characters).",
        },
        status: {
          type: "string",
          enum: VALID_STATUSES,
          description: "Card column status. Defaults to backlog.",
        },
        priority: {
          type: "string",
          enum: VALID_PRIORITIES,
          description: "Card priority level (low, medium, high). Defaults to medium.",
        },
        id: {
          type: "string",
          description: "Optional custom unique card ID.",
        },
      },
      required: ["title"],
    },
  },
  {
    name: "board.update",
    description:
      "Update an existing card on the authorized project Kanban board. Card must belong to the authorized project.",
    parameters: {
      type: "object",
      properties: {
        id: {
          type: "string",
          description: "ID of the card to update.",
        },
        cardId: {
          type: "string",
          description: "Alias for id.",
        },
        title: {
          type: "string",
          description: "Updated card title (up to 160 characters).",
        },
        description: {
          type: "string",
          description: "Updated card description (up to 12,000 characters).",
        },
        status: {
          type: "string",
          enum: VALID_STATUSES,
          description: "Updated column status.",
        },
        priority: {
          type: "string",
          enum: VALID_PRIORITIES,
          description: "Updated priority level.",
        },
      },
      required: ["id"],
    },
  },
  {
    name: "board.start",
    description:
      "Start regular worker sessions for 1 to 10 cards on the authorized project board. Launches one session per card, links session IDs, and moves successful cards to In progress.",
    parameters: {
      type: "object",
      properties: {
        cardIds: {
          type: "array",
          items: { type: "string" },
          description: "Array of 1 to 10 card IDs to start work on.",
        },
        ids: {
          type: "array",
          items: { type: "string" },
          description: "Alias for cardIds.",
        },
        placement: {
          type: "string",
          enum: VALID_PLACEMENTS,
          description: "Optional session placement (tab, right, down).",
        },
      },
    },
  },
];

export const PROJECT_BOARD_MONO_GUIDANCE =
  "Use board tools only for explicit board requests. Inspect or list cards (board.list) before acting when card identity is ambiguous. Accurately report started session IDs and per-card results to the user.";

export interface BoardStartCardResult {
  cardId: string;
  success: boolean;
  sessionId?: string;
  status?: BoardStatus | "started-but-unlinked";
  error?: string;
}

export interface BoardStartResult {
  cards: BoardStartCardResult[];
  results: BoardStartCardResult[];
  startedCount: number;
  failedCount: number;
}

export const PROJECT_BOARD_HARDENING = {
  textWrapUtility: "wrap-anywhere",
  freeformSessionLinks: false,
  cardDeleteConfirmation: true,
  failedMediaDeleteKeepsPreviewOpen: true,
  topmostDialogFocusTrap: true,
  boardStartIsOnlySessionLinkWriter: true,
  maxChildRequestIdLength: 128,
  boardStartRefresh: "reread-latest-card-after-session-start",
  boardStartMutableFields: ["status", "linkedSessionIds"] as const,
  deletedDuringStart: "do-not-recreate-card",
  cardIdValidation: "validate-before-request-id-generation",
} as const;

function validateFields(action: string, input: Record<string, unknown>): void {
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    throw new Error("Payload must be a JSON object");
  }
  if ("project" in input || "projectCwd" in input) {
    throw new Error(
      "Project path must not be supplied in tool payload; project is determined by context",
    );
  }
  if (action === "board.update" && "linkedSessionIds" in input) {
    throw new Error(
      "linkedSessionIds cannot be modified via board.update; links are created only by board.start",
    );
  }
  const allowed = PROJECT_BOARD_FIELDS.get(action);
  if (!allowed) {
    throw new Error("Unknown board action: " + action);
  }
  const unknown = Object.keys(input).filter((k) => !allowed.includes(k));
  if (unknown.length > 0) {
    throw new Error("Unknown " + action + " fields: " + unknown.join(", "));
  }
}

function validateContext(context: ProjectBoardToolContext): void {
  if (
    !context ||
    typeof context.projectCwd !== "string" ||
    !context.projectCwd.trim()
  ) {
    throw new Error("Authorized project path is required");
  }
  if (
    typeof context.requestId !== "string" ||
    !context.requestId.trim()
  ) {
    throw new Error("requestId is required in context");
  }
}

export function hash128(str: string): string {
  let h1 = 0x811c9dc5;
  let h2 = 0x6c62272e;
  let h3 = 0x9e3779b9;
  let h4 = 0x243f6a88;
  for (let i = 0; i < str.length; i++) {
    const c = str.charCodeAt(i);
    h1 = Math.imul(h1 ^ c, 0x01000193) >>> 0;
    h2 = Math.imul(h2 ^ c, 0x5bd1e995) >>> 0;
    h3 = Math.imul(h3 ^ c, 0x27d4eb2f) >>> 0;
    h4 = Math.imul(h4 ^ c, 0x165667b1) >>> 0;
  }
  return (
    h1.toString(16).padStart(8, "0") +
    h2.toString(16).padStart(8, "0") +
    h3.toString(16).padStart(8, "0") +
    h4.toString(16).padStart(8, "0")
  );
}

export function generateChildRequestId(
  contextRequestId: string,
  cardId: string,
): string {
  const validCard = sanitizeId(cardId);
  const safeReq = contextRequestId.trim().replace(/[^A-Za-z0-9_-]/g, "_");
  const raw = safeReq + "-" + validCard;
  if (raw.length <= 128) {
    return raw;
  }
  const hash = hash128(contextRequestId + ":" + validCard);
  const maxPrefixLen = 128 - 1 - hash.length;
  const prefix = raw.slice(0, maxPrefixLen).replace(/[-_]+$/, "");
  const result = prefix ? prefix + "-" + hash : hash;
  return result.slice(0, 128);
}

export function sanitizeId(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed || !/^[A-Za-z0-9_-]{1,128}$/.test(trimmed)) {
    throw new Error(
      "Invalid card ID: must be 1-128 alphanumeric characters, dashes, or underscores",
    );
  }
  return trimmed;
}

export function formatTaskBrief(card: ProjectBoardCard): string {
  const parts: string[] = [
    "Task: " + card.title,
    "Priority: " + card.priority,
  ];
  if (card.description && card.description.trim()) {
    parts.push("Description:\n" + card.description.trim());
  }
  return parts.join("\n\n");
}

export async function dispatchProjectBoardAction(
  context: ProjectBoardToolContext,
  action: string,
  input: Record<string, unknown>,
  host: ProjectBoardToolHost,
): Promise<unknown> {
  validateContext(context);
  validateFields(action, input);

  switch (action) {
    case "board.list": {
      let statusFilter: BoardStatus | undefined;
      if (input.status !== undefined) {
        if (
          typeof input.status !== "string" ||
          !VALID_STATUSES.includes(input.status as BoardStatus)
        ) {
          throw new Error(
            "Invalid status: " + String(input.status) + ". Must be one of: " + VALID_STATUSES.join(", "),
          );
        }
        statusFilter = input.status as BoardStatus;
      }
      const cards = await host.repository.list(context.projectCwd);
      const filtered = statusFilter
        ? cards.filter((c) => c.status === statusFilter)
        : cards;
      return {
        projectCwd: context.projectCwd,
        total: filtered.length,
        cards: filtered,
      };
    }

    case "board.create": {
      if (typeof input.title !== "string" || !input.title.trim()) {
        throw new Error("title must be a non-empty string");
      }
      const title = input.title.trim();
      if (title.length > BOARD_VALIDATION.titleMax) {
        throw new Error(
          "title exceeds maximum length of " + BOARD_VALIDATION.titleMax + " characters",
        );
      }

      let description = "";
      if (input.description !== undefined) {
        if (typeof input.description !== "string") {
          throw new Error("description must be a string");
        }
        if (input.description.length > BOARD_VALIDATION.descriptionMax) {
          throw new Error(
            "description exceeds maximum length of " + BOARD_VALIDATION.descriptionMax + " characters",
          );
        }
        description = input.description;
      }

      let status: BoardStatus = "backlog";
      if (input.status !== undefined) {
        if (
          typeof input.status !== "string" ||
          !VALID_STATUSES.includes(input.status as BoardStatus)
        ) {
          throw new Error(
            "Invalid status: " + String(input.status) + ". Must be one of: " + VALID_STATUSES.join(", "),
          );
        }
        status = input.status as BoardStatus;
      }

      let priority: BoardPriority = "medium";
      if (input.priority !== undefined) {
        if (
          typeof input.priority !== "string" ||
          !VALID_PRIORITIES.includes(input.priority as BoardPriority)
        ) {
          throw new Error(
            "Invalid priority: " + String(input.priority) + ". Must be one of: " + VALID_PRIORITIES.join(", "),
          );
        }
        priority = input.priority as BoardPriority;
      }

      const existingCards = await host.repository.list(context.projectCwd);
      let id: string;
      if (input.id !== undefined) {
        if (typeof input.id !== "string") {
          throw new Error("id must be a string");
        }
        id = sanitizeId(input.id);
        if (existingCards.some((c) => c.id === id)) {
          throw new Error("Card with ID \"" + id + "\" already exists in project");
        }
      } else {
        const candidate = "card-" + Date.now() + "-" + Math.random().toString(36).slice(2, 9);
        id = sanitizeId(candidate);
      }

      const cardInput: ProjectBoardCardInput = {
        id,
        projectCwd: context.projectCwd,
        title,
        description,
        status,
        priority,
        linkedSessionIds: [],
      };

      const card = await host.repository.upsertCard(cardInput);
      return {
        ...card,
        card,
        created: true,
      };
    }

    case "board.update": {
      const rawId = input.id ?? input.cardId;
      if (typeof rawId !== "string" || !rawId.trim()) {
        throw new Error("Card id must be specified for board.update");
      }
      const cardId = sanitizeId(rawId);

      const existingCards = await host.repository.list(context.projectCwd);
      const existing = existingCards.find((c) => c.id === cardId);
      if (!existing) {
        throw new Error(
          "Card \"" + cardId + "\" not found in authorized project",
        );
      }

      const hasUpdateField =
        input.title !== undefined ||
        input.description !== undefined ||
        input.status !== undefined ||
        input.priority !== undefined;

      if (!hasUpdateField) {
        throw new Error("Supply at least one field to update");
      }

      let title = existing.title;
      if (input.title !== undefined) {
        if (typeof input.title !== "string" || !input.title.trim()) {
          throw new Error("title must be a non-empty string");
        }
        const trimmedTitle = input.title.trim();
        if (trimmedTitle.length > BOARD_VALIDATION.titleMax) {
          throw new Error(
            "title exceeds maximum length of " + BOARD_VALIDATION.titleMax + " characters",
          );
        }
        title = trimmedTitle;
      }

      let description = existing.description;
      if (input.description !== undefined) {
        if (typeof input.description !== "string") {
          throw new Error("description must be a string");
        }
        if (input.description.length > BOARD_VALIDATION.descriptionMax) {
          throw new Error(
            "description exceeds maximum length of " + BOARD_VALIDATION.descriptionMax + " characters",
          );
        }
        description = input.description;
      }

      let status = existing.status;
      if (input.status !== undefined) {
        if (
          typeof input.status !== "string" ||
          !VALID_STATUSES.includes(input.status as BoardStatus)
        ) {
          throw new Error(
            "Invalid status: " + String(input.status) + ". Must be one of: " + VALID_STATUSES.join(", "),
          );
        }
        status = input.status as BoardStatus;
      }

      let priority = existing.priority;
      if (input.priority !== undefined) {
        if (
          typeof input.priority !== "string" ||
          !VALID_PRIORITIES.includes(input.priority as BoardPriority)
        ) {
          throw new Error(
            "Invalid priority: " + String(input.priority) + ". Must be one of: " + VALID_PRIORITIES.join(", "),
          );
        }
        priority = input.priority as BoardPriority;
      }

      const patchInput: ProjectBoardCardInput = {
        id: existing.id,
        projectCwd: context.projectCwd,
        title,
        description,
        status,
        priority,
        linkedSessionIds: existing.linkedSessionIds,
      };

      const card = await host.repository.upsertCard(patchInput);
      return {
        ...card,
        card,
        updated: true,
      };
    }

    case "board.start": {
      const rawCardIds = input.cardIds ?? input.ids;
      if (!Array.isArray(rawCardIds)) {
        throw new Error("cardIds must be an array of card IDs");
      }
      if (
        rawCardIds.length === 0 ||
        rawCardIds.length > BOARD_VALIDATION.startCardIdsMax
      ) {
        throw new Error(
          "cardIds must contain between 1 and " + BOARD_VALIDATION.startCardIdsMax + " card IDs",
        );
      }

      for (const id of rawCardIds) {
        if (typeof id !== "string" || !id.trim()) {
          throw new Error("Each card ID must be a non-empty string");
        }
        sanitizeId(id);
      }

      const cardIds = rawCardIds.map((id) => sanitizeId(id));
      const uniqueIds = new Set(cardIds);
      if (uniqueIds.size !== cardIds.length) {
        throw new Error("cardIds contains duplicate card IDs");
      }

      let placement: BoardPlacement;
      if (input.placement !== undefined) {
        if (
          typeof input.placement !== "string" ||
          !VALID_PLACEMENTS.includes(input.placement as BoardPlacement)
        ) {
          throw new Error(
            "Invalid placement: " + String(input.placement) + ". Must be one of: " + VALID_PLACEMENTS.join(", "),
          );
        }
        placement = input.placement as BoardPlacement;
      } else {
        placement = context.placement ?? "tab";
      }

      const existingCards = await host.repository.list(context.projectCwd);
      const cardResults: BoardStartCardResult[] = [];

      for (const cardId of cardIds) {
        const card = existingCards.find((c) => c.id === cardId);
        if (!card) {
          cardResults.push({
            cardId,
            success: false,
            error: "Card \"" + cardId + "\" not found in authorized project",
          });
          continue;
        }

        const cardRequestId = generateChildRequestId(context.requestId, card.id);
        const prompt = formatTaskBrief(card);

        let sessionId: string;
        try {
          const sessionResult = await host.startSession({
            projectCwd: context.projectCwd,
            requestId: cardRequestId,
            prompt,
            placement,
          });

          if (
            !sessionResult ||
            typeof sessionResult.sessionId !== "string" ||
            !sessionResult.sessionId.trim()
          ) {
            throw new Error("Host startSession did not return a valid session ID");
          }

          sessionId = sessionResult.sessionId.trim();
        } catch (err) {
          cardResults.push({
            cardId,
            success: false,
            error: err instanceof Error ? err.message : String(err),
          });
          continue;
        }

        try {
          const latestCards = await host.repository.list(context.projectCwd);
          const latestCard = latestCards.find((c) => c.id === cardId);

          if (!latestCard) {
            cardResults.push({
              cardId,
              success: false,
              sessionId,
              status: "started-but-unlinked",
              error:
                "started-but-unlinked: session " +
                sessionId +
                " started but card was deleted while launch was pending",
            });
            continue;
          }

          const nextLinked = latestCard.linkedSessionIds.includes(sessionId)
            ? latestCard.linkedSessionIds
            : [...latestCard.linkedSessionIds, sessionId];

          if (nextLinked.length > BOARD_VALIDATION.linkedSessionsMax) {
            throw new Error(
              "Card exceeds maximum of " + BOARD_VALIDATION.linkedSessionsMax + " linked sessions",
            );
          }

          const updatedCard: ProjectBoardCardInput = {
            id: latestCard.id,
            projectCwd: context.projectCwd,
            title: latestCard.title,
            description: latestCard.description,
            status: "in-progress",
            priority: latestCard.priority,
            linkedSessionIds: nextLinked,
          };

          await host.repository.upsertCard(updatedCard);

          cardResults.push({
            cardId,
            success: true,
            sessionId,
            status: "in-progress",
          });
        } catch (err) {
          const reason = err instanceof Error ? err.message : String(err);
          cardResults.push({
            cardId,
            success: false,
            sessionId,
            status: "started-but-unlinked",
            error: "started-but-unlinked: session " + sessionId + " started but card update failed: " + reason,
          });
        }
      }

      return {
        cards: cardResults,
        results: cardResults,
        startedCount: cardResults.filter((r) => r.success).length,
        failedCount: cardResults.filter((r) => !r.success).length,
      };
    }

    default:
      throw new Error("Unknown board action: " + action);
  }
}

/** Convenience alias with (context, action, input, host) signature. */
export const handleProjectBoardAction = dispatchProjectBoardAction;

/** Convenience alias supporting (action, input, context, host) signature. */
export async function dispatchProjectBoard(
  action: string,
  input: Record<string, unknown>,
  context: ProjectBoardToolContext,
  host: ProjectBoardToolHost,
): Promise<unknown> {
  return dispatchProjectBoardAction(context, action, input, host);
}
