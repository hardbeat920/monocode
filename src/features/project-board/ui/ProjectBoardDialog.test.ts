// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BOARD_LANES,
  PROJECT_BOARD_CHANGED_EVENT,
  PROJECT_BOARD_HARDENING,
  type BoardMediaRef,
  type ProjectBoardCard,
  type ProjectBoardCardInput,
  type ProjectBoardRepository,
} from "./types";
import { ProjectBoardDialog } from "./ProjectBoardDialog";
import { clearActiveDialogStack } from "./useDialogFocusTrap";

const sampleProjectCwd = "/Users/test/my-project";

const sampleCard: ProjectBoardCard = {
  id: "card-1",
  projectCwd: sampleProjectCwd,
  title: "Setup authentication",
  description: "Configure OAuth provider and tokens",
  status: "backlog",
  priority: "high",
  linkedSessionIds: ["sess-abc-123"],
  media: [
    {
      id: "media-1",
      name: "diagram.png",
      mimeType: "image/png",
      byteLength: 2048,
    },
  ],
  createdAt: 1000,
  updatedAt: 1000,
};

function createMockRepository(initialCards: ProjectBoardCard[] = []): {
  repository: ProjectBoardRepository;
  cards: ProjectBoardCard[];
  listCalls: number;
} {
  const cards = [...initialCards];
  let listCalls = 0;

  const repository: ProjectBoardRepository = {
    list: vi.fn(async (cwd: string) => {
      listCalls++;
      return cards.filter((c) => c.projectCwd === cwd);
    }),
    upsertCard: vi.fn(async (input: ProjectBoardCardInput) => {
      const idx = cards.findIndex((c) => c.id === input.id);
      const updated: ProjectBoardCard = {
        ...input,
        media: idx >= 0 ? cards[idx].media : [],
        createdAt: idx >= 0 ? cards[idx].createdAt : Date.now(),
        updatedAt: Date.now(),
      };
      if (idx >= 0) {
        cards[idx] = updated;
      } else {
        cards.push(updated);
      }
      return updated;
    }),
    deleteCard: vi.fn(async (cwd: string, cardId: string) => {
      const idx = cards.findIndex((c) => c.id === cardId && c.projectCwd === cwd);
      if (idx >= 0) cards.splice(idx, 1);
    }),
    addMedia: vi.fn(async ({ cardId, name, mimeType, dataBase64 }) => {
      const ref: BoardMediaRef = {
        id: "media-" + Math.random().toString(36).slice(2, 6),
        name,
        mimeType,
        byteLength: Math.round((dataBase64.length * 3) / 4),
      };
      const card = cards.find((c) => c.id === cardId);
      if (card) {
        card.media = [...(card.media ?? []), ref];
      }
      return ref;
    }),
    getMedia: vi.fn(async (_cwd, _cardId, mediaId) => {
      return {
        id: mediaId,
        name: "mock-image.png",
        mimeType: "image/png",
        byteLength: 1024,
        dataBase64: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
      };
    }),
    deleteMedia: vi.fn(async (_cwd, cardId, mediaId) => {
      const card = cards.find((c) => c.id === cardId);
      if (card) {
        card.media = (card.media ?? []).filter((m) => m.id !== mediaId);
      }
    }),
  };

  return {
    repository,
    cards,
    get listCalls() {
      return listCalls;
    },
  };
}

function setInputValue(
  element: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement,
  value: string,
) {
  const proto =
    element instanceof HTMLSelectElement
      ? window.HTMLSelectElement.prototype
      : element instanceof HTMLTextAreaElement
        ? window.HTMLTextAreaElement.prototype
        : window.HTMLInputElement.prototype;
  const descriptor = Object.getOwnPropertyDescriptor(proto, "value");
  descriptor?.set?.call(element, value);
  element.dispatchEvent(new Event("input", { bubbles: true }));
  element.dispatchEvent(new Event("change", { bubbles: true }));
}

describe("ProjectBoardDialog", () => {
  let root: Root;
  let container: HTMLDivElement;

  beforeEach(() => {
    vi.clearAllMocks();
    clearActiveDialogStack();
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    clearActiveDialogStack();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("exports the frozen hardening contract values", () => {
    expect(PROJECT_BOARD_HARDENING.textWrapUtility).toBe("wrap-anywhere");
    expect(PROJECT_BOARD_HARDENING.freeformSessionLinks).toBe(false);
    expect(PROJECT_BOARD_HARDENING.cardDeleteConfirmation).toBe(true);
    expect(PROJECT_BOARD_HARDENING.failedMediaDeleteKeepsPreviewOpen).toBe(true);
    expect(PROJECT_BOARD_HARDENING.topmostDialogFocusTrap).toBe(true);
  });

  it("renders five kanban lanes and empty state when no cards exist", async () => {
    const { repository } = createMockRepository([]);
    const onClose = vi.fn();
    const onOpenSession = vi.fn();

    await act(async () => {
      root.render(
        createElement(ProjectBoardDialog, {
          projectCwd: sampleProjectCwd,
          onClose,
          onOpenSession,
          repository,
        }),
      );
    });

    expect(container.textContent).toContain("No cards on this board yet");
    expect(container.textContent).toContain("Create your first card");
  });

  it("renders cards in their corresponding lanes with priorities and session links", async () => {
    const { repository } = createMockRepository([sampleCard]);
    const onClose = vi.fn();
    const onOpenSession = vi.fn();

    await act(async () => {
      root.render(
        createElement(ProjectBoardDialog, {
          projectCwd: sampleProjectCwd,
          onClose,
          onOpenSession,
          repository,
        }),
      );
    });

    for (const lane of BOARD_LANES) {
      expect(
        container.querySelector('[data-lane-status="' + lane.status + '"]'),
      ).not.toBeNull();
    }

    const backlogLane = container.querySelector(
      '[data-lane-status="backlog"]',
    )!;
    expect(backlogLane.textContent).toContain("Setup authentication");
    expect(backlogLane.textContent).toContain("High");
    expect(backlogLane.textContent).toContain("diagram.png");

    const sessionBtn = backlogLane.querySelector<HTMLButtonElement>(
      'button[aria-label="Open session sess-abc-123"]',
    );
    expect(sessionBtn).not.toBeNull();
    await act(async () => {
      sessionBtn?.click();
    });
    expect(onOpenSession).toHaveBeenCalledWith("sess-abc-123");
  });

  it("creates a new card through the card editor modal", async () => {
    const { repository } = createMockRepository([]);
    const onClose = vi.fn();
    const onOpenSession = vi.fn();

    await act(async () => {
      root.render(
        createElement(ProjectBoardDialog, {
          projectCwd: sampleProjectCwd,
          onClose,
          onOpenSession,
          repository,
        }),
      );
    });

    const createBtn = Array.from(
      container.querySelectorAll("button"),
    ).find((b) => b.textContent?.includes("first card"));
    expect(createBtn).not.toBeUndefined();

    await act(async () => {
      createBtn?.click();
    });

    expect(container.querySelector('[aria-label="New card"]')).not.toBeNull();

    const titleInput = container.querySelector<HTMLInputElement>(
      "#card-title-input",
    )!;
    const descTextarea = container.querySelector<HTMLTextAreaElement>(
      "#card-desc-textarea",
    )!;
    const prioritySelect = container.querySelector<HTMLSelectElement>(
      "#card-priority-select",
    )!;

    await act(async () => {
      setInputValue(titleInput, "New task title");
      setInputValue(descTextarea, "New task description");
      setInputValue(prioritySelect, "high");
    });

    const form = container.querySelector("form")!;
    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });

    expect(repository.upsertCard).toHaveBeenCalledWith(
      expect.objectContaining({
        title: "New task title",
        description: "New task description",
        priority: "high",
        status: "backlog",
        linkedSessionIds: [],
      }),
    );
  });

  it("edits an existing card and preserves existing linkedSessionIds exactly", async () => {
    const multiLinkCard: ProjectBoardCard = {
      ...sampleCard,
      linkedSessionIds: ["sess-abc-123", "sess-def-456"],
    };
    const { repository } = createMockRepository([multiLinkCard]);
    const onClose = vi.fn();
    const onOpenSession = vi.fn();

    await act(async () => {
      root.render(
        createElement(ProjectBoardDialog, {
          projectCwd: sampleProjectCwd,
          onClose,
          onOpenSession,
          repository,
        }),
      );
    });

    const editBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Edit card Setup authentication"]',
    )!;
    expect(editBtn).not.toBeNull();

    await act(async () => {
      editBtn.click();
    });

    const modal = container.querySelector('[aria-label="Edit card"]')!;
    expect(modal).not.toBeNull();

    // Verify free-form session editor input is REMOVED
    expect(container.querySelector("#card-linked-sessions")).toBeNull();

    // Verify existing sessions are displayed read-only
    expect(container.querySelector('[data-linked-session-id="sess-abc-123"]')).not.toBeNull();
    expect(container.querySelector('[data-linked-session-id="sess-def-456"]')).not.toBeNull();
    expect(modal.textContent).toContain("Mono-started threads appear here.");

    // Update title and save
    const titleInput = container.querySelector<HTMLInputElement>(
      "#card-title-input",
    )!;
    await act(async () => {
      setInputValue(titleInput, "Refactored authentication");
    });

    const form = container.querySelector("form")!;
    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });

    // Verify existing linked sessions were preserved exactly
    expect(repository.upsertCard).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "card-1",
        title: "Refactored authentication",
        linkedSessionIds: ["sess-abc-123", "sess-def-456"],
      }),
    );
  });

  it("moves a card to a different status lane", async () => {
    const { repository } = createMockRepository([sampleCard]);
    const onClose = vi.fn();
    const onOpenSession = vi.fn();

    await act(async () => {
      root.render(
        createElement(ProjectBoardDialog, {
          projectCwd: sampleProjectCwd,
          onClose,
          onOpenSession,
          repository,
        }),
      );
    });

    const moveSelect = container.querySelector<HTMLSelectElement>(
      'select[aria-label="Move card Setup authentication to lane"]',
    )!;
    expect(moveSelect).not.toBeNull();

    await act(async () => {
      moveSelect.value = "in-progress";
      moveSelect.dispatchEvent(new Event("change", { bubbles: true }));
    });

    expect(repository.upsertCard).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "card-1",
        status: "in-progress",
      }),
    );

    const inProgressLane = container.querySelector(
      '[data-lane-status="in-progress"]',
    )!;
    expect(inProgressLane.textContent).toContain("Setup authentication");
  });

  it("requires confirmation for card deletion and cancellation never calls deleteCard", async () => {
    const { repository } = createMockRepository([sampleCard]);
    const onClose = vi.fn();
    const onOpenSession = vi.fn();

    await act(async () => {
      root.render(
        createElement(ProjectBoardDialog, {
          projectCwd: sampleProjectCwd,
          onClose,
          onOpenSession,
          repository,
        }),
      );
    });

    const deleteTrigger = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Delete card Setup authentication"]',
    )!;
    expect(deleteTrigger).not.toBeNull();

    // 1. Click delete trigger - opens confirmation dialog
    await act(async () => {
      deleteTrigger.click();
    });

    const confirmModal = container.querySelector('[aria-labelledby="delete-dialog-title"]');
    expect(confirmModal).not.toBeNull();
    expect(confirmModal?.textContent).toContain('Are you sure you want to delete "Setup authentication"?');
    expect(confirmModal?.textContent).toContain("Attachment cascade warning");
    expect(confirmModal?.textContent).toContain("permanently removes all card data and its 1 associated image attachment");

    // Initial focus on Cancel button
    const cancelBtn = Array.from(confirmModal!.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Cancel",
    );
    expect(cancelBtn).not.toBeUndefined();
    expect(document.activeElement).toBe(cancelBtn);

    // 2. Click Cancel: dialog closes and deleteCard is NOT called
    await act(async () => {
      cancelBtn?.click();
    });

    expect(repository.deleteCard).not.toHaveBeenCalled();
    expect(container.querySelector('[aria-labelledby="delete-dialog-title"]')).toBeNull();

    // 3. Click delete trigger again, then confirm deletion
    await act(async () => {
      deleteTrigger.click();
    });

    const deleteConfirmBtn = Array.from(
      container.querySelectorAll("button"),
    ).find((b) => b.textContent?.includes("Delete card") && b.closest('[role="dialog"]'));
    expect(deleteConfirmBtn).not.toBeUndefined();

    await act(async () => {
      deleteConfirmBtn?.click();
    });

    expect(repository.deleteCard).toHaveBeenCalledWith(
      sampleProjectCwd,
      "card-1",
    );
  });

  it("awaits media deletion and keeps preview open with recovery text on failure", async () => {
    const { repository } = createMockRepository([sampleCard]);
    repository.deleteMedia = vi.fn(async () => {
      throw new Error("Disk read-only error");
    });

    await act(async () => {
      root.render(
        createElement(ProjectBoardDialog, {
          projectCwd: sampleProjectCwd,
          onClose: vi.fn(),
          onOpenSession: vi.fn(),
          repository,
        }),
      );
    });

    // Open media preview
    const viewAttachBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="View attachment diagram.png"]',
    )!;
    expect(viewAttachBtn).not.toBeNull();

    await act(async () => {
      viewAttachBtn.click();
    });

    const previewModal = container.querySelector(
      '[aria-label="Image preview: diagram.png"]',
    );
    expect(previewModal).not.toBeNull();

    // Click delete inside preview
    const deleteAttachBtn = previewModal!.querySelector<HTMLButtonElement>(
      'button[aria-label="Delete attachment diagram.png"]',
    )!;
    expect(deleteAttachBtn).not.toBeNull();

    await act(async () => {
      deleteAttachBtn.click();
    });

    // Verify deletion was attempted
    expect(repository.deleteMedia).toHaveBeenCalledWith(
      sampleProjectCwd,
      "card-1",
      "media-1",
    );

    // Modal MUST remain open on failure
    expect(
      container.querySelector('[aria-label="Image preview: diagram.png"]'),
    ).not.toBeNull();

    // Recovery text must be displayed
    expect(container.textContent).toContain("Failed to delete attachment: Disk read-only error. Please retry or check file permissions.");
  });

  it("handles image paste and rejects files exceeding 5 MiB", async () => {
    const { repository } = createMockRepository([]);
    const onClose = vi.fn();
    const onOpenSession = vi.fn();

    await act(async () => {
      root.render(
        createElement(ProjectBoardDialog, {
          projectCwd: sampleProjectCwd,
          onClose,
          onOpenSession,
          repository,
        }),
      );
    });

    const newCardBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Add new card"]',
    )!;
    await act(async () => {
      newCardBtn.click();
    });

    const modal = container.querySelector('[aria-label="New card"]')!;

    const bigFile = new File(["x".repeat(6 * 1024 * 1024)], "oversized.png", {
      type: "image/png",
    });
    const fakeDataTransferBig = {
      items: [
        {
          type: "image/png",
          getAsFile: () => bigFile,
        },
      ],
    };
    const pasteEventBig = new Event("paste", { bubbles: true }) as any;
    pasteEventBig.clipboardData = fakeDataTransferBig;

    await act(async () => {
      modal.dispatchEvent(pasteEventBig);
    });

    expect(container.textContent).toContain("exceeds the 5 MiB limit");

    const invalidFile = new File(["dummy"], "doc.pdf", {
      type: "application/pdf",
    });
    const fakeDataTransferInvalid = {
      items: [
        {
          type: "application/pdf",
          getAsFile: () => invalidFile,
        },
      ],
    };
    const pasteEventInvalid = new Event("paste", { bubbles: true }) as any;
    pasteEventInvalid.clipboardData = fakeDataTransferInvalid;

    await act(async () => {
      modal.dispatchEvent(pasteEventInvalid);
    });

    expect(container.textContent).toContain("Unsupported image type");
  });

  it("displays error banner on loading failure and retries successfully", async () => {
    let failFirst = true;
    const repository = createMockRepository().repository;
    repository.list = vi.fn(async () => {
      if (failFirst) {
        failFirst = false;
        throw new Error("Network database timeout");
      }
      return [sampleCard];
    });

    await act(async () => {
      root.render(
        createElement(ProjectBoardDialog, {
          projectCwd: sampleProjectCwd,
          onClose: vi.fn(),
          onOpenSession: vi.fn(),
          repository,
        }),
      );
    });

    expect(container.textContent).toContain("Network database timeout");

    const retryBtn = Array.from(
      container.querySelectorAll("button"),
    ).find((b) => b.textContent === "Retry");
    expect(retryBtn).not.toBeUndefined();

    await act(async () => {
      retryBtn?.click();
    });

    expect(container.textContent).toContain("Setup authentication");
    expect(container.textContent).not.toContain("Network database timeout");
  });

  it("refreshes cards on monocode:project-board-changed event", async () => {
    const { repository } = createMockRepository([sampleCard]);

    await act(async () => {
      root.render(
        createElement(ProjectBoardDialog, {
          projectCwd: sampleProjectCwd,
          onClose: vi.fn(),
          onOpenSession: vi.fn(),
          repository,
        }),
      );
    });

    expect(repository.list).toHaveBeenCalledTimes(1);

    await act(async () => {
      window.dispatchEvent(
        new CustomEvent(PROJECT_BOARD_CHANGED_EVENT, {
          detail: { projectCwd: sampleProjectCwd },
        }),
      );
    });

    expect(repository.list).toHaveBeenCalledTimes(2);

    await act(async () => {
      window.dispatchEvent(
        new CustomEvent(PROJECT_BOARD_CHANGED_EVENT, {
          detail: { projectCwd: "/Users/test/other-project" },
        }),
      );
    });

    expect(repository.list).toHaveBeenCalledTimes(2);
  });

  it("maintains focus containment in topmost modal and Escape closes only the topmost modal, restoring focus to opener", async () => {
    const { repository } = createMockRepository([sampleCard]);
    const onClose = vi.fn();

    const outsideOpener = document.createElement("button");
    outsideOpener.id = "outside-board-opener";
    document.body.appendChild(outsideOpener);
    outsideOpener.focus();

    await act(async () => {
      root.render(
        createElement(ProjectBoardDialog, {
          projectCwd: sampleProjectCwd,
          onClose,
          onOpenSession: vi.fn(),
          repository,
        }),
      );
    });

    // 1. ProjectBoardDialog initial focus on Close button
    const boardCloseBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Close project board"]',
    )!;
    expect(document.activeElement).toBe(boardCloseBtn);

    // 2. Open CardEditorModal from "Add new card" button
    const addCardBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Add new card"]',
    )!;
    addCardBtn.focus();
    expect(document.activeElement).toBe(addCardBtn);

    await act(async () => {
      addCardBtn.click();
    });

    const cardModal = container.querySelector<HTMLElement>('[aria-label="New card"]')!;
    expect(cardModal).not.toBeNull();

    // Initial focus in CardEditorModal is on title input
    const titleInput = container.querySelector<HTMLInputElement>("#card-title-input")!;
    expect(document.activeElement).toBe(titleInput);

    // Tab containment test: from last element to first element
    const submitModalBtn = Array.from(cardModal.querySelectorAll("button")).find(
      (b) => b.getAttribute("type") === "submit",
    );
    const closeCardModalBtn = cardModal.querySelector<HTMLButtonElement>('button[aria-label="Close"]')!;

    submitModalBtn?.focus();
    expect(document.activeElement).toBe(submitModalBtn);

    // Press Tab on the last element in CardEditorModal
    await act(async () => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }),
      );
    });
    expect(document.activeElement).toBe(closeCardModalBtn);

    // Shift+Tab from first element wraps to last element
    await act(async () => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true, cancelable: true }),
      );
    });
    expect(document.activeElement).toBe(submitModalBtn);

    // 3. Press Escape in CardEditorModal: closes ONLY CardEditorModal, restores focus to opener
    await act(async () => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      );
    });

    expect(container.querySelector('[aria-label="New card"]')).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(addCardBtn);

    // 4. Test stacking with MediaPreviewModal
    const viewAttachBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="View attachment diagram.png"]',
    )!;
    viewAttachBtn.focus();
    expect(document.activeElement).toBe(viewAttachBtn);

    await act(async () => {
      viewAttachBtn.click();
    });

    const previewModal = container.querySelector<HTMLElement>(
      '[aria-label="Image preview: diagram.png"]',
    )!;
    expect(previewModal).not.toBeNull();

    // Initial focus in preview modal is on close preview button
    const previewCloseBtn = previewModal.querySelector<HTMLButtonElement>(
      'button[aria-label="Close preview"]',
    )!;
    expect(document.activeElement).toBe(previewCloseBtn);

    // Press Escape in MediaPreviewModal: closes ONLY MediaPreviewModal
    await act(async () => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      );
    });

    expect(container.querySelector('[aria-label="Image preview: diagram.png"]')).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(viewAttachBtn);

    // 5. Press Escape when ProjectBoardDialog is topmost: closes ProjectBoardDialog
    await act(async () => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      );
    });

    expect(onClose).toHaveBeenCalledTimes(1);

    outsideOpener.remove();
  });

  it("uses wrap-anywhere utility on card titles and descriptions across 375px, 768px, and 1440px viewports without break-words", async () => {
    const unbrokenToken = "A".repeat(240);
    const wideCard: ProjectBoardCard = {
      ...sampleCard,
      id: "card-wide",
      title: unbrokenToken,
      description: "B".repeat(500),
    };
    const { repository } = createMockRepository([wideCard]);

    const viewports = [375, 768, 1440];

    for (const width of viewports) {
      container.style.width = width + "px";

      await act(async () => {
        root.render(
          createElement(ProjectBoardDialog, {
            projectCwd: sampleProjectCwd,
            onClose: vi.fn(),
            onOpenSession: vi.fn(),
            repository,
          }),
        );
      });

      const lanesContainer = container.querySelector("[data-lanes-container]");
      expect(lanesContainer).not.toBeNull();
      expect(lanesContainer?.className).toContain("overflow-x-auto");

      const cardTitle = container.querySelector("h4");
      expect(cardTitle).not.toBeNull();
      expect(cardTitle?.className).toContain("wrap-anywhere");
      expect(cardTitle?.className).not.toContain("overflow-wrap-anywhere");
      expect(cardTitle?.className).not.toContain("break-words");

      const cardDesc = container.querySelector("p.whitespace-pre-wrap");
      expect(cardDesc).not.toBeNull();
      expect(cardDesc?.className).toContain("wrap-anywhere");
      expect(cardDesc?.className).not.toContain("overflow-wrap-anywhere");
      expect(cardDesc?.className).not.toContain("break-words");
    }
  });
});
