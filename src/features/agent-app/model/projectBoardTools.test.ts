import { describe, expect, it, vi, beforeEach } from "vitest";
import {
  BOARD_VALIDATION,
  PROJECT_BOARD_FIELDS,
  PROJECT_BOARD_HARDENING,
  PROJECT_BOARD_MONO_GUIDANCE,
  PROJECT_BOARD_TOOL_DEFINITIONS,
  VALID_PLACEMENTS,
  VALID_PRIORITIES,
  VALID_STATUSES,
  dispatchProjectBoard,
  dispatchProjectBoardAction,
  formatTaskBrief,
  generateChildRequestId,
  handleProjectBoardAction,
  sanitizeId,
  type ProjectBoardCard,
  type ProjectBoardCardInput,
  type ProjectBoardRepository,
  type ProjectBoardToolContext,
  type ProjectBoardToolHost,
} from "./projectBoardTools";

describe("ProjectBoardTools", () => {
  const projectA = "/path/to/project-a";
  const projectB = "/path/to/project-b";

  let cardsStore: Map<string, ProjectBoardCard[]>;
  let mockRepository: ProjectBoardRepository;
  let mockHost: ProjectBoardToolHost;
  let defaultContext: ProjectBoardToolContext;

  function createCard(
    id: string,
    projectCwd: string,
    overrides?: Partial<ProjectBoardCard>,
  ): ProjectBoardCard {
    return {
      id,
      projectCwd,
      title: `Card ${id}`,
      description: `Description for ${id}`,
      status: "backlog",
      priority: "medium",
      linkedSessionIds: [],
      media: [],
      createdAt: 1000,
      updatedAt: 1000,
      ...overrides,
    };
  }

  beforeEach(() => {
    cardsStore = new Map<string, ProjectBoardCard[]>();
    cardsStore.set(projectA, [
      createCard("card-1", projectA, {
        title: "Fix bug in parser",
        description: "Parser crashes on null byte",
        priority: "high",
        status: "ready",
      }),
      createCard("card-2", projectA, {
        title: "Add dark mode toggle",
        description: "User setting in preferences",
        priority: "low",
        status: "backlog",
      }),
    ]);
    cardsStore.set(projectB, [
      createCard("card-foreign", projectB, {
        title: "Foreign Project Card",
        description: "Should never be visible to project A",
      }),
    ]);

    mockRepository = {
      list: vi.fn(async (projectCwd: string) => {
        return cardsStore.get(projectCwd) ?? [];
      }),
      upsertCard: vi.fn(async (input: ProjectBoardCardInput) => {
        const list = cardsStore.get(input.projectCwd) ?? [];
        const index = list.findIndex((c) => c.id === input.id);
        const savedCard: ProjectBoardCard = {
          id: input.id,
          projectCwd: input.projectCwd,
          title: input.title,
          description: input.description,
          status: input.status,
          priority: input.priority,
          linkedSessionIds: input.linkedSessionIds,
          media: index >= 0 ? list[index].media : [],
          createdAt: index >= 0 ? list[index].createdAt : Date.now(),
          updatedAt: Date.now(),
        };
        if (index >= 0) {
          list[index] = savedCard;
        } else {
          list.push(savedCard);
        }
        cardsStore.set(input.projectCwd, list);
        return savedCard;
      }),
      deleteCard: vi.fn(async (projectCwd: string, cardId: string) => {
        const list = cardsStore.get(projectCwd) ?? [];
        cardsStore.set(
          projectCwd,
          list.filter((c) => c.id !== cardId),
        );
      }),
      addMedia: vi.fn(async () => {
        throw new Error("Not used by tool dispatcher");
      }),
      getMedia: vi.fn(async () => null),
      deleteMedia: vi.fn(async () => {}),
    };

    mockHost = {
      repository: mockRepository,
      startSession: vi.fn(async ({ requestId }) => {
        return { sessionId: `session-started-for-${requestId}` };
      }),
    };

    defaultContext = {
      projectCwd: projectA,
      requestId: "req-100",
      placement: "right",
    };
  });

  describe("Constants and tool definitions", () => {
    it("exports frozen validation limits", () => {
      expect(BOARD_VALIDATION.titleMax).toBe(160);
      expect(BOARD_VALIDATION.descriptionMax).toBe(12000);
      expect(BOARD_VALIDATION.linkedSessionsMax).toBe(50);
      expect(BOARD_VALIDATION.mediaBytesMax).toBe(5242880);
      expect(BOARD_VALIDATION.startCardIdsMax).toBe(10);
      expect(BOARD_VALIDATION.mediaTypes).toEqual([
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/webp",
      ]);
    });

    it("exports JSON-safe tool definitions for all 4 actions", () => {
      const names = PROJECT_BOARD_TOOL_DEFINITIONS.map((d) => d.name);
      expect(names).toEqual([
        "board.list",
        "board.create",
        "board.update",
        "board.start",
      ]);

      for (const def of PROJECT_BOARD_TOOL_DEFINITIONS) {
        expect(def.name).toBeTruthy();
        expect(def.description).toBeTruthy();
        expect(def.parameters.type).toBe("object");
        expect(typeof def.parameters.properties).toBe("object");
      }
    });

    it("exports field rules and guidance", () => {
      expect(PROJECT_BOARD_FIELDS.get("board.list")).toEqual(["status"]);
      expect(PROJECT_BOARD_FIELDS.get("board.create")).toEqual([
        "id",
        "title",
        "description",
        "status",
        "priority",
      ]);
expect(PROJECT_BOARD_FIELDS.get("board.update")).toEqual([
        "id",
        "cardId",
        "title",
        "description",
        "status",
        "priority",
      ]);
      expect(PROJECT_BOARD_FIELDS.get("board.start")).toEqual([
        "cardIds",
        "ids",
        "placement",
      ]);
      expect(PROJECT_BOARD_MONO_GUIDANCE).toContain("board.list");
      expect(PROJECT_BOARD_HARDENING.boardStartIsOnlySessionLinkWriter).toBe(true);
      expect(PROJECT_BOARD_HARDENING.freeformSessionLinks).toBe(false);
      expect(PROJECT_BOARD_HARDENING.maxChildRequestIdLength).toBe(128);
      expect(PROJECT_BOARD_HARDENING.boardStartRefresh).toBe(
        "reread-latest-card-after-session-start",
      );
      expect(PROJECT_BOARD_HARDENING.boardStartMutableFields).toEqual([
        "status",
        "linkedSessionIds",
      ]);
      expect(PROJECT_BOARD_HARDENING.deletedDuringStart).toBe(
        "do-not-recreate-card",
      );
      expect(PROJECT_BOARD_HARDENING.cardIdValidation).toBe(
        "validate-before-request-id-generation",
      );
    });
  });

  describe("Project Scope and Payload Security", () => {
    it("rejects user payload attempting to supply project path", async () => {
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.list",
          { project: "/malicious/path" },
          mockHost,
        ),
      ).rejects.toThrow(/Project path must not be supplied in tool payload/);

      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.create",
          { title: "Test", projectCwd: "/malicious/path" },
          mockHost,
        ),
      ).rejects.toThrow(/Project path must not be supplied in tool payload/);
    });

    it("rejects context without authorized project path", async () => {
      const badContext = { ...defaultContext, projectCwd: "   " };
      await expect(
        dispatchProjectBoardAction(badContext, "board.list", {}, mockHost),
      ).rejects.toThrow(/Authorized project path is required/);
    });

    it("rejects context without requestId", async () => {
      const badContext = { ...defaultContext, requestId: "" };
      await expect(
        dispatchProjectBoardAction(badContext, "board.list", {}, mockHost),
      ).rejects.toThrow(/requestId is required/);
    });

    it("rejects unknown actions and unknown fields", async () => {
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.destroy",
          {},
          mockHost,
        ),
      ).rejects.toThrow(/Unknown board action: board.destroy/);

      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.list",
          { unexpectedField: 123 },
          mockHost,
        ),
      ).rejects.toThrow(/Unknown board.list fields: unexpectedField/);
    });
  });

  describe("board.list", () => {
    it("lists cards scoped strictly to authorized project", async () => {
      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.list",
        {},
        mockHost,
      )) as { projectCwd: string; total: number; cards: ProjectBoardCard[] };

      expect(result.projectCwd).toBe(projectA);
      expect(result.total).toBe(2);
      expect(result.cards.map((c) => c.id)).toEqual(["card-1", "card-2"]);
      expect(result.cards.some((c) => c.id === "card-foreign")).toBe(false);
    });

    it("filters by status when provided", async () => {
      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.list",
        { status: "ready" },
        mockHost,
      )) as { total: number; cards: ProjectBoardCard[] };

      expect(result.total).toBe(1);
      expect(result.cards[0].id).toBe("card-1");
    });

    it("rejects invalid status values", async () => {
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.list",
          { status: "invalid-status" },
          mockHost,
        ),
      ).rejects.toThrow(/Invalid status: invalid-status/);
    });
  });

  describe("board.create", () => {
    it("creates a new card with defaults and saves to authorized project", async () => {
      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.create",
        { title: "Implement feature X", description: "Details for feature X" },
        mockHost,
      )) as { card: ProjectBoardCard; created: boolean; id: string };

      expect(result.created).toBe(true);
      expect(result.card.title).toBe("Implement feature X");
      expect(result.card.description).toBe("Details for feature X");
      expect(result.card.projectCwd).toBe(projectA);
      expect(result.card.status).toBe("backlog");
      expect(result.card.priority).toBe("medium");
      expect(result.card.linkedSessionIds).toEqual([]);
      expect(mockRepository.upsertCard).toHaveBeenCalled();
    });

    it("accepts custom ID and custom status/priority", async () => {
      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.create",
        {
          id: "custom-card-99",
          title: "Custom Card",
          status: "ready",
          priority: "high",
        },
        mockHost,
      )) as { card: ProjectBoardCard };

      expect(result.card.id).toBe("custom-card-99");
      expect(result.card.status).toBe("ready");
      expect(result.card.priority).toBe("high");
    });

    it("rejects duplicate ID within project", async () => {
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.create",
          { id: "card-1", title: "Duplicate Card" },
          mockHost,
        ),
      ).rejects.toThrow(/Card with ID "card-1" already exists/);
    });

    it("strictly validates title and description lengths", async () => {
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.create",
          { title: "   " },
          mockHost,
        ),
      ).rejects.toThrow(/title must be a non-empty string/);

      const tooLongTitle = "a".repeat(161);
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.create",
          { title: tooLongTitle },
          mockHost,
        ),
      ).rejects.toThrow(/title exceeds maximum length of 160/);

      const tooLongDesc = "d".repeat(12001);
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.create",
          { title: "Valid Title", description: tooLongDesc },
          mockHost,
        ),
      ).rejects.toThrow(/description exceeds maximum length of 12000/);
    });

    it("rejects invalid status and priority", async () => {
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.create",
          { title: "Valid", status: "not-a-status" },
          mockHost,
        ),
      ).rejects.toThrow(/Invalid status/);

      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.create",
          { title: "Valid", priority: "critical" },
          mockHost,
        ),
      ).rejects.toThrow(/Invalid priority/);
    });
  });

  describe("board.update", () => {
    it("updates card fields in authorized project", async () => {
      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.update",
        {
          id: "card-1",
          title: "Updated parser title",
          status: "in-progress",
          priority: "medium",
        },
        mockHost,
      )) as { card: ProjectBoardCard; updated: boolean };

      expect(result.updated).toBe(true);
      expect(result.card.title).toBe("Updated parser title");
      expect(result.card.status).toBe("in-progress");
      expect(result.card.priority).toBe("medium");
      // Unmodified fields are preserved
      expect(result.card.description).toBe("Parser crashes on null byte");
    });

    it("rejects update for card belonging to another project", async () => {
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.update",
          { id: "card-foreign", title: "Hijacked Card" },
          mockHost,
        ),
      ).rejects.toThrow(/Card "card-foreign" not found in authorized project/);
    });

    it("rejects update when no fields are supplied", async () => {
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.update",
          { id: "card-1" },
          mockHost,
        ),
      ).rejects.toThrow(/Supply at least one field to update/);
    });

    it("rejects linkedSessionIds in payload and preserves existing links", async () => {
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.update",
          { id: "card-1", linkedSessionIds: ["sess-arbitrary"] },
          mockHost,
        ),
      ).rejects.toThrow(/linkedSessionIds/);

      const cardsInA = cardsStore.get(projectA)!;
      const c1 = cardsInA.find((c) => c.id === "card-1")!;
      c1.linkedSessionIds = ["sess-persisted-1", "sess-persisted-2"];

      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.update",
        {
          id: "card-1",
          title: "Updated Title Preserving Links",
        },
        mockHost,
      )) as { card: ProjectBoardCard };

      expect(result.card.linkedSessionIds).toEqual([
        "sess-persisted-1",
        "sess-persisted-2",
      ]);
      expect(mockRepository.upsertCard).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "card-1",
          linkedSessionIds: ["sess-persisted-1", "sess-persisted-2"],
        }),
      );
    });
  });


  describe("generateChildRequestId and request ID boundary", () => {
    it("generates deterministic, unique IDs <=128 chars even for 128-char card IDs", () => {
      const card128_A = "a".repeat(127) + "1";
      const card128_B = "a".repeat(127) + "2";
      const reqIdA1 = generateChildRequestId("req-100", card128_A);
      const reqIdA2 = generateChildRequestId("req-100", card128_A);
      const reqIdB = generateChildRequestId("req-100", card128_B);

      expect(reqIdA1).toBe(reqIdA2);
      expect(reqIdA1).not.toBe(reqIdB);
      expect(reqIdA1.length).toBeLessThanOrEqual(128);
      expect(reqIdB.length).toBeLessThanOrEqual(128);
      expect(reqIdA1).toMatch(/^[A-Za-z0-9_-]{1,128}$/);
      expect(reqIdB).toMatch(/^[A-Za-z0-9_-]{1,128}$/);
    });

    it("bounds request IDs <=128 chars when both context requestId and card ID are 128 chars", () => {
      const req128 = "r".repeat(128);
      const card128 = "c".repeat(128);
      const childReqId = generateChildRequestId(req128, card128);

      expect(childReqId.length).toBeLessThanOrEqual(128);
      expect(childReqId).toMatch(/^[A-Za-z0-9_-]{1,128}$/);
    });

    it("passes child requestId <=128 chars to host.startSession for 128-char card ID", async () => {
      const longCardId = "c" + "x".repeat(127);
      cardsStore.get(projectA)!.push(
        createCard(longCardId, projectA, {
          title: "Boundary card",
          status: "backlog",
        }),
      );

      await dispatchProjectBoardAction(
        defaultContext,
        "board.start",
        { cardIds: [longCardId] },
        mockHost,
      );

      const calls = (mockHost.startSession as ReturnType<typeof vi.fn>).mock.calls;
      const lastCall = calls[calls.length - 1][0];
      expect(lastCall.requestId.length).toBeLessThanOrEqual(128);
      expect(lastCall.requestId).toMatch(/^[A-Za-z0-9_-]{1,128}$/);
    });

    it("validates card IDs before generating child request ID without transforming invalid chars", () => {
      expect(() => generateChildRequestId("req-100", "invalid card id")).toThrow(
        /Invalid card ID/,
      );
      expect(() => generateChildRequestId("req-100", "card!1")).toThrow(
        /Invalid card ID/,
      );
      expect(() => generateChildRequestId("req-100", "card/1")).toThrow(
        /Invalid card ID/,
      );
      expect(() => generateChildRequestId("req-100", "")).toThrow(
        /Invalid card ID/,
      );
      expect(() => generateChildRequestId("req-100", "c".repeat(129))).toThrow(
        /Invalid card ID/,
      );
    });

    it("sanitizeId accepts valid alphanumeric/dash/underscore IDs and rejects all others", () => {
      expect(sanitizeId("card-1_test")).toBe("card-1_test");
      expect(sanitizeId("  card-trimmed  ")).toBe("card-trimmed");
      expect(() => sanitizeId("card.with.dots")).toThrow(/Invalid card ID/);
      expect(() => sanitizeId("card with spaces")).toThrow(/Invalid card ID/);
      expect(() => sanitizeId("card$invalid")).toThrow(/Invalid card ID/);
    });
  });

  describe("formatTaskBrief", () => {
    it("formats task brief containing title, priority, and description", () => {
      const card = createCard("c1", projectA, {
        title: "Database Migration",
        priority: "high",
        description: "Migrate sqlite tables safely without data loss",
      });
      const brief = formatTaskBrief(card);
      expect(brief).toContain("Task: Database Migration");
      expect(brief).toContain("Priority: high");
      expect(brief).toContain(
        "Description:\nMigrate sqlite tables safely without data loss",
      );
    });
  });

  describe("board.start", () => {
    it("starts one session per card, links session IDs, and sets in-progress", async () => {
      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.start",
        { cardIds: ["card-1", "card-2"] },
        mockHost,
      )) as {
        startedCount: number;
        failedCount: number;
        cards: Array<{
          cardId: string;
          success: boolean;
          sessionId: string;
          status: string;
        }>;
      };

      expect(result.startedCount).toBe(2);
      expect(result.failedCount).toBe(0);
      expect(result.cards.length).toBe(2);

      expect(result.cards[0].success).toBe(true);
      expect(result.cards[0].status).toBe("in-progress");
      expect(result.cards[0].sessionId).toBe(
        "session-started-for-req-100-card-1",
      );

      expect(result.cards[1].success).toBe(true);
      expect(result.cards[1].status).toBe("in-progress");
      expect(result.cards[1].sessionId).toBe(
        "session-started-for-req-100-card-2",
      );

      // Verify host startSession received correct parameters
      expect(mockHost.startSession).toHaveBeenCalledTimes(2);
      expect(mockHost.startSession).toHaveBeenCalledWith({
        projectCwd: projectA,
        requestId: "req-100-card-1",
        prompt: expect.stringContaining("Fix bug in parser"),
        placement: "right",
      });

      // Verify cards in store have updated status and linked session IDs
      const cardsInA = cardsStore.get(projectA)!;
      const c1 = cardsInA.find((c) => c.id === "card-1")!;
      expect(c1.status).toBe("in-progress");
      expect(c1.linkedSessionIds).toContain(
        "session-started-for-req-100-card-1",
      );

      const c2 = cardsInA.find((c) => c.id === "card-2")!;
      expect(c2.status).toBe("in-progress");
      expect(c2.linkedSessionIds).toContain(
        "session-started-for-req-100-card-2",
      );
    });

    it("supports ids alias and explicit placement", async () => {
      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.start",
        { ids: ["card-1"], placement: "down" },
        mockHost,
      )) as { startedCount: number };

      expect(result.startedCount).toBe(1);
      expect(mockHost.startSession).toHaveBeenCalledWith({
        projectCwd: projectA,
        requestId: "req-100-card-1",
        prompt: expect.any(String),
        placement: "down",
      });
    });

    it("rejects invalid card input (empty, non-array, >10, duplicates)", async () => {
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.start",
          { cardIds: [] },
          mockHost,
        ),
      ).rejects.toThrow(/cardIds must contain between 1 and 10 card IDs/);

      const elevenIds = Array.from({ length: 11 }, (_, i) => `c-${i}`);
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.start",
          { cardIds: elevenIds },
          mockHost,
        ),
      ).rejects.toThrow(/cardIds must contain between 1 and 10 card IDs/);

      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.start",
          { cardIds: ["card-1", "card-1"] },
          mockHost,
        ),
      ).rejects.toThrow(/cardIds contains duplicate card IDs/);

      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.start",
          { cardIds: ["card-1", ""] },
          mockHost,
        ),
      ).rejects.toThrow(/Each card ID must be a non-empty string/);
    });

    it("FALSIFIABLE STOP CONDITION: one card's failure does not erase another card's successful start/link", async () => {
      // Add a third card to project A
      cardsStore.get(projectA)!.push(
        createCard("card-3", projectA, {
          title: "Third card",
          status: "backlog",
        }),
      );

      // Make card-2 fail in startSession
      (mockHost.startSession as ReturnType<typeof vi.fn>).mockImplementation(
        async ({ requestId }) => {
          if (requestId.includes("card-2")) {
            throw new Error("Quota exceeded for model runner");
          }
          return { sessionId: `session-ok-${requestId}` };
        },
      );

      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.start",
        { cardIds: ["card-1", "card-2", "card-3"] },
        mockHost,
      )) as {
        startedCount: number;
        failedCount: number;
        cards: Array<{
          cardId: string;
          success: boolean;
          sessionId?: string;
          error?: string;
          status?: string;
        }>;
      };

      // Exactly 2 succeeded and 1 failed
      expect(result.startedCount).toBe(2);
      expect(result.failedCount).toBe(1);

      const r1 = result.cards.find((c) => c.cardId === "card-1")!;
      const r2 = result.cards.find((c) => c.cardId === "card-2")!;
      const r3 = result.cards.find((c) => c.cardId === "card-3")!;

      expect(r1.success).toBe(true);
      expect(r1.status).toBe("in-progress");
      expect(r1.sessionId).toBe("session-ok-req-100-card-1");

      expect(r2.success).toBe(false);
      expect(r2.error).toContain("Quota exceeded for model runner");

      expect(r3.success).toBe(true);
      expect(r3.status).toBe("in-progress");
      expect(r3.sessionId).toBe("session-ok-req-100-card-3");

      // Verify card-1 and card-3 are still persisted as in-progress with linked sessions in the repository
      const cardsInA = cardsStore.get(projectA)!;
      const c1 = cardsInA.find((c) => c.id === "card-1")!;
      expect(c1.status).toBe("in-progress");
      expect(c1.linkedSessionIds).toContain("session-ok-req-100-card-1");

      const c2 = cardsInA.find((c) => c.id === "card-2")!;
      // card-2 was NOT updated to in-progress or linked
      expect(c2.status).toBe("backlog");
      expect(c2.linkedSessionIds).toEqual([]);

      const c3 = cardsInA.find((c) => c.id === "card-3")!;
      expect(c3.status).toBe("in-progress");
      expect(c3.linkedSessionIds).toContain("session-ok-req-100-card-3");
    });

    it("per-card reports failure when a card does not belong to authorized project, without blocking valid cards", async () => {
      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.start",
        { cardIds: ["card-1", "card-foreign"] },
        mockHost,
      )) as {
        startedCount: number;
        failedCount: number;
        cards: Array<{ cardId: string; success: boolean; error?: string }>;
      };

      expect(result.startedCount).toBe(1);
      expect(result.failedCount).toBe(1);

      const rForeign = result.cards.find((c) => c.cardId === "card-foreign")!;
      expect(rForeign.success).toBe(false);
      expect(rForeign.error).toContain(
        'Card "card-foreign" not found in authorized project',
      );

      const r1 = result.cards.find((c) => c.cardId === "card-1")!;
      expect(r1.success).toBe(true);
    });


    it("returns created session ID with explicit started-but-unlinked status/error if card upsert fails", async () => {
      cardsStore.get(projectA)!.push(
        createCard("card-3", projectA, {
          title: "Third card",
          status: "backlog",
        }),
      );

      (mockRepository.upsertCard as ReturnType<typeof vi.fn>).mockImplementation(
        async (input: ProjectBoardCardInput) => {
          if (input.id === "card-1") {
            throw new Error("Disk I/O error persisting card update");
          }
          const list = cardsStore.get(input.projectCwd) ?? [];
          const index = list.findIndex((c) => c.id === input.id);
          const savedCard = {
            ...list[index],
            ...input,
            updatedAt: Date.now(),
          };
          list[index] = savedCard as any;
          return savedCard;
        },
      );

      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.start",
        { cardIds: ["card-1", "card-3"] },
        mockHost,
      )) as {
        startedCount: number;
        failedCount: number;
        cards: Array<{
          cardId: string;
          success: boolean;
          sessionId?: string;
          status?: string;
          error?: string;
        }>;
      };

      expect(result.startedCount).toBe(1);
      expect(result.failedCount).toBe(1);
      expect(result.cards.length).toBe(2);

      const r1 = result.cards.find((c) => c.cardId === "card-1")!;
      expect(r1.success).toBe(false);
      expect(r1.sessionId).toBe("session-started-for-req-100-card-1");
      expect(r1.status).toBe("started-but-unlinked");
      expect(r1.error).toContain("started-but-unlinked");
      expect(r1.error).toContain("session-started-for-req-100-card-1");
      expect(r1.error).toContain("Disk I/O error persisting card update");

      const r3 = result.cards.find((c) => c.cardId === "card-3")!;
      expect(r3.success).toBe(true);
      expect(r3.status).toBe("in-progress");
      expect(r3.sessionId).toBe("session-started-for-req-100-card-3");
    });

    it("never claims start succeeded if host startSession returns empty/null sessionId", async () => {
      (mockHost.startSession as ReturnType<typeof vi.fn>).mockResolvedValue({
        sessionId: "",
      });

      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.start",
        { cardIds: ["card-1"] },
        mockHost,
      )) as {
        startedCount: number;
        failedCount: number;
        cards: Array<{ cardId: string; success: boolean; error?: string }>;
      };

      expect(result.startedCount).toBe(0);
      expect(result.failedCount).toBe(1);
      expect(result.cards[0].success).toBe(false);
      expect(result.cards[0].error).toContain(
        "Host startSession did not return a valid session ID",
      );

      const c1 = cardsStore.get(projectA)!.find((c) => c.id === "card-1")!;
      expect(c1.status).toBe("ready"); // not changed to in-progress
      expect(c1.linkedSessionIds).toEqual([]); // no empty session linked
    });

    it("enforces cardIdValidation: rejects invalid card IDs upfront before startSession or request derivation", async () => {
      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.start",
          { cardIds: ["invalid card id"] },
          mockHost,
        ),
      ).rejects.toThrow(/Invalid card ID/);

      await expect(
        dispatchProjectBoardAction(
          defaultContext,
          "board.start",
          { cardIds: ["card-1", "card!bad"] },
          mockHost,
        ),
      ).rejects.toThrow(/Invalid card ID/);

      expect(mockHost.startSession).not.toHaveBeenCalled();
    });

    it("enforces boardStartRefresh and boardStartMutableFields: preserves concurrent edits to title, description, priority, and links", async () => {
      const initialCard = cardsStore.get(projectA)!.find((c) => c.id === "card-1")!;
      initialCard.linkedSessionIds = ["pre-existing-session"];

      (mockHost.startSession as ReturnType<typeof vi.fn>).mockImplementationOnce(
        async ({ requestId }) => {
          const list = cardsStore.get(projectA)!;
          const idx = list.findIndex((c) => c.id === "card-1");
          list[idx] = {
            ...list[idx],
            title: "Concurrent Title Update",
            description: "Concurrent Description Update",
            priority: "low",
            linkedSessionIds: ["pre-existing-session", "concurrent-user-session"],
            updatedAt: Date.now() + 50,
          };
          return { sessionId: "session-new-" + requestId };
        },
      );

      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.start",
        { cardIds: ["card-1"] },
        mockHost,
      )) as {
        startedCount: number;
        failedCount: number;
        cards: Array<{ cardId: string; success: boolean; sessionId?: string; status?: string }>;
      };

      expect(result.startedCount).toBe(1);
      expect(result.failedCount).toBe(0);
      expect(result.cards[0].success).toBe(true);
      expect(result.cards[0].status).toBe("in-progress");

      const refreshed = cardsStore.get(projectA)!.find((c) => c.id === "card-1")!;
      expect(refreshed.title).toBe("Concurrent Title Update");
      expect(refreshed.description).toBe("Concurrent Description Update");
      expect(refreshed.priority).toBe("low");
      expect(refreshed.status).toBe("in-progress");
      expect(refreshed.linkedSessionIds).toEqual([
        "pre-existing-session",
        "concurrent-user-session",
        "session-new-req-100-card-1",
      ]);
    });

    it("enforces deletedDuringStart: does not recreate card if card was deleted while startSession was pending", async () => {
      (mockHost.startSession as ReturnType<typeof vi.fn>).mockImplementationOnce(
        async ({ requestId }) => {
          const list = cardsStore.get(projectA)!;
          cardsStore.set(
            projectA,
            list.filter((c) => c.id !== "card-1"),
          );
          return { sessionId: "session-for-deleted-" + requestId };
        },
      );

      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.start",
        { cardIds: ["card-1"] },
        mockHost,
      )) as {
        startedCount: number;
        failedCount: number;
        cards: Array<{
          cardId: string;
          success: boolean;
          sessionId?: string;
          status?: string;
          error?: string;
        }>;
      };

      expect(result.startedCount).toBe(0);
      expect(result.failedCount).toBe(1);
      expect(result.cards.length).toBe(1);

      const cardRes = result.cards[0];
      expect(cardRes.cardId).toBe("card-1");
      expect(cardRes.success).toBe(false);
      expect(cardRes.sessionId).toBe("session-for-deleted-req-100-card-1");
      expect(cardRes.status).toBe("started-but-unlinked");
      expect(cardRes.error).toContain("started-but-unlinked");
      expect(cardRes.error).toContain("deleted while launch was pending");

      const listAfter = cardsStore.get(projectA)!;
      expect(listAfter.find((c) => c.id === "card-1")).toBeUndefined();
      expect(mockRepository.upsertCard).not.toHaveBeenCalled();
    });

    it("handles multiple cards with concurrent edits properly in sequence", async () => {
      (mockHost.startSession as ReturnType<typeof vi.fn>).mockImplementation(
        async ({ requestId }) => {
          if (requestId.includes("card-1")) {
            const list = cardsStore.get(projectA)!;
            const idx2 = list.findIndex((c) => c.id === "card-2");
            list[idx2] = {
              ...list[idx2],
              title: "Card 2 updated while card 1 was starting",
            };
          }
          return { sessionId: "sess-" + requestId };
        },
      );

      const result = (await dispatchProjectBoardAction(
        defaultContext,
        "board.start",
        { cardIds: ["card-1", "card-2"] },
        mockHost,
      )) as { startedCount: number; failedCount: number };

      expect(result.startedCount).toBe(2);
      expect(result.failedCount).toBe(0);

      const c2 = cardsStore.get(projectA)!.find((c) => c.id === "card-2")!;
      expect(c2.title).toBe("Card 2 updated while card 1 was starting");
      expect(c2.status).toBe("in-progress");
      expect(c2.linkedSessionIds).toContain("sess-req-100-card-2");
    });
  });

  describe("dispatcher aliases", () => {
    it("supports handleProjectBoardAction alias", async () => {
      const res = await handleProjectBoardAction(
        defaultContext,
        "board.list",
        {},
        mockHost,
      );
      expect(res).toHaveProperty("cards");
    });

    it("supports dispatchProjectBoard alias with (action, input, context, host) signature", async () => {
      const res = await dispatchProjectBoard(
        "board.list",
        {},
        defaultContext,
        mockHost,
      );
      expect(res).toHaveProperty("cards");
    });
  });
});
