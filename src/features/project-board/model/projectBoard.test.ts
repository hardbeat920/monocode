// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  BOARD_VALIDATION,
  PROJECT_BOARD_CHANGED_EVENT,
  dispatchProjectBoardChanged,
  subscribeProjectBoard,
  project_board_list,
  project_board_upsert_card,
  project_board_delete_card,
  project_board_add_media,
  project_board_get_media,
  project_board_delete_media,
  projectBoardList,
  projectBoardUpsertCard,
  projectBoardDeleteCard,
  projectBoardAddMedia,
  projectBoardGetMedia,
  projectBoardDeleteMedia,
  projectBoardRepository,
  createProjectBoardRepository,
  type ProjectBoardCard,
  type ProjectBoardCardInput,
  type BoardMediaRef,
  type BoardMedia,
} from "./projectBoard";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("Project Board Model & Contract", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("exports frozen BOARD_VALIDATION matching the contract", () => {
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

  describe("Typed wrappers and change events", () => {
    const mockCard: ProjectBoardCard = {
      id: "card-1",
      projectCwd: "/workspace/project-a",
      title: "Feature card",
      description: "Description of card",
      status: "backlog",
      priority: "medium",
      linkedSessionIds: ["sess-1"],
      media: [],
      createdAt: 1000,
      updatedAt: 1000,
    };

    it("project_board_list invokes native command without dispatching change event", async () => {
      const dispatchSpy = vi.spyOn(window, "dispatchEvent");
      vi.mocked(invoke).mockResolvedValueOnce([mockCard]);

      const result = await project_board_list("/workspace/project-a");
      expect(invoke).toHaveBeenCalledWith("project_board_list", {
        projectCwd: "/workspace/project-a",
      });
      expect(result).toEqual([mockCard]);
      expect(dispatchSpy).not.toHaveBeenCalled();
    });

    it("project_board_upsert_card invokes native command and dispatches change event with project path", async () => {
      let eventDetail: unknown = null;
      const listener = (e: Event) => {
        eventDetail = (e as CustomEvent).detail;
      };
      window.addEventListener(PROJECT_BOARD_CHANGED_EVENT, listener);

      vi.mocked(invoke).mockResolvedValueOnce(mockCard);
      const input: ProjectBoardCardInput = {
        id: "card-1",
        projectCwd: "/workspace/project-a",
        title: "Feature card",
        description: "Description of card",
        status: "backlog",
        priority: "medium",
        linkedSessionIds: ["sess-1"],
      };

      const result = await project_board_upsert_card(input);
      expect(invoke).toHaveBeenCalledWith("project_board_upsert_card", { input });
      expect(result).toEqual(mockCard);
      expect(eventDetail).toEqual({ projectCwd: "/workspace/project-a" });

      window.removeEventListener(PROJECT_BOARD_CHANGED_EVENT, listener);
    });

    it("project_board_delete_card invokes native command and dispatches change event", async () => {
      let eventDetail: unknown = null;
      const listener = (e: Event) => {
        eventDetail = (e as CustomEvent).detail;
      };
      window.addEventListener(PROJECT_BOARD_CHANGED_EVENT, listener);

      vi.mocked(invoke).mockResolvedValueOnce(undefined);
      await project_board_delete_card("/workspace/project-a", "card-1");

      expect(invoke).toHaveBeenCalledWith("project_board_delete_card", {
        projectCwd: "/workspace/project-a",
        cardId: "card-1",
      });
      expect(eventDetail).toEqual({ projectCwd: "/workspace/project-a" });

      window.removeEventListener(PROJECT_BOARD_CHANGED_EVENT, listener);
    });

    it("project_board_add_media invokes native command and dispatches change event", async () => {
      let eventDetail: unknown = null;
      const listener = (e: Event) => {
        eventDetail = (e as CustomEvent).detail;
      };
      window.addEventListener(PROJECT_BOARD_CHANGED_EVENT, listener);

      const mediaRef: BoardMediaRef = {
        id: "media-1",
        name: "test.png",
        mimeType: "image/png",
        byteLength: 1234,
      };
      vi.mocked(invoke).mockResolvedValueOnce(mediaRef);

      const input = {
        projectCwd: "/workspace/project-a",
        cardId: "card-1",
        name: "test.png",
        mimeType: "image/png" as const,
        dataBase64: "dGVzdA==",
      };
      const result = await project_board_add_media(input);

      expect(invoke).toHaveBeenCalledWith("project_board_add_media", { input });
      expect(result).toEqual(mediaRef);
      expect(eventDetail).toEqual({ projectCwd: "/workspace/project-a" });

      window.removeEventListener(PROJECT_BOARD_CHANGED_EVENT, listener);
    });

    it("project_board_get_media retrieves media with base64 without dispatching change event", async () => {
      const dispatchSpy = vi.spyOn(window, "dispatchEvent");
      const media: BoardMedia = {
        id: "media-1",
        name: "test.png",
        mimeType: "image/png",
        byteLength: 1234,
        dataBase64: "dGVzdA==",
      };
      vi.mocked(invoke).mockResolvedValueOnce(media);

      const result = await project_board_get_media(
        "/workspace/project-a",
        "card-1",
        "media-1",
      );
      expect(invoke).toHaveBeenCalledWith("project_board_get_media", {
        projectCwd: "/workspace/project-a",
        cardId: "card-1",
        mediaId: "media-1",
      });
      expect(result).toEqual(media);
      expect(dispatchSpy).not.toHaveBeenCalled();
    });

    it("project_board_delete_media invokes native command and dispatches change event", async () => {
      let eventDetail: unknown = null;
      const listener = (e: Event) => {
        eventDetail = (e as CustomEvent).detail;
      };
      window.addEventListener(PROJECT_BOARD_CHANGED_EVENT, listener);

      vi.mocked(invoke).mockResolvedValueOnce(undefined);
      await project_board_delete_media(
        "/workspace/project-a",
        "card-1",
        "media-1",
      );

      expect(invoke).toHaveBeenCalledWith("project_board_delete_media", {
        projectCwd: "/workspace/project-a",
        cardId: "card-1",
        mediaId: "media-1",
      });
      expect(eventDetail).toEqual({ projectCwd: "/workspace/project-a" });

      window.removeEventListener(PROJECT_BOARD_CHANGED_EVENT, listener);
    });
  });

  describe("Subscription helper", () => {
    it("subscribeProjectBoard fires when the target project changes and ignores other projects", () => {
      const onProjectAChange = vi.fn();
      const unsubscribe = subscribeProjectBoard(
        "/workspace/project-a",
        onProjectAChange,
      );

      // Event for unrelated project B should not trigger callback
      dispatchProjectBoardChanged("/workspace/project-b");
      expect(onProjectAChange).not.toHaveBeenCalled();

      // Event for target project A should trigger callback
      dispatchProjectBoardChanged("/workspace/project-a");
      expect(onProjectAChange).toHaveBeenCalledTimes(1);

      // After unsubscription, no further calls
      unsubscribe();
      dispatchProjectBoardChanged("/workspace/project-a");
      expect(onProjectAChange).toHaveBeenCalledTimes(1);
    });

    it("subscribeProjectBoard also supports string detail event format", () => {
      const onChange = vi.fn();
      const unsubscribe = subscribeProjectBoard("/workspace/project-a", onChange);

      window.dispatchEvent(
        new CustomEvent(PROJECT_BOARD_CHANGED_EVENT, {
          detail: "/workspace/project-a",
        }),
      );
      expect(onChange).toHaveBeenCalledTimes(1);
      unsubscribe();
    });
  });

  describe("Repository interface and aliases", () => {
    it("projectBoardRepository provides all contract methods", () => {
      expect(typeof projectBoardRepository.list).toBe("function");
      expect(typeof projectBoardRepository.upsertCard).toBe("function");
      expect(typeof projectBoardRepository.deleteCard).toBe("function");
      expect(typeof projectBoardRepository.addMedia).toBe("function");
      expect(typeof projectBoardRepository.getMedia).toBe("function");
      expect(typeof projectBoardRepository.deleteMedia).toBe("function");
      expect(createProjectBoardRepository()).toBe(projectBoardRepository);
    });

    it("aliases match the snake_case functions", () => {
      expect(projectBoardList).toBe(project_board_list);
      expect(projectBoardUpsertCard).toBe(project_board_upsert_card);
      expect(projectBoardDeleteCard).toBe(project_board_delete_card);
      expect(projectBoardAddMedia).toBe(project_board_add_media);
      expect(projectBoardGetMedia).toBe(project_board_get_media);
      expect(projectBoardDeleteMedia).toBe(project_board_delete_media);
    });
  });
});
