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
  type ProjectBoardCardPatch,
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
  repository: ProjectBoardRepository & {
    patchCard: ReturnType<typeof vi.fn>;
  };
  cards: ProjectBoardCard[];
  listCalls: number;
} {
  const cards = [...initialCards];
  let listCalls = 0;

  const repository: ProjectBoardRepository & {
    patchCard: ReturnType<typeof vi.fn>;
  } = {
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
    patchCard: vi.fn(async (cwd: string, cardId: string, patch: ProjectBoardCardPatch) => {
      const idx = cards.findIndex((c) => c.id === cardId && c.projectCwd === cwd);
      if (idx < 0) throw new Error("Card not found");
      const current = cards[idx];
      const updated: ProjectBoardCard = {
        ...current,
        ...(patch.title !== undefined ? { title: patch.title } : {}),
        ...(patch.description !== undefined ? { description: patch.description } : {}),
        ...(patch.status !== undefined ? { status: patch.status } : {}),
        ...(patch.priority !== undefined ? { priority: patch.priority } : {}),
        updatedAt: Date.now(),
      };
      cards[idx] = updated;
      return updated;
    }),
    linkSession: vi.fn(async (cwd: string, cardId: string, sessionId: string) => {
      const idx = cards.findIndex((c) => c.id === cardId && c.projectCwd === cwd);
      if (idx < 0) return null;
      const current = cards[idx];
      const existing = current.linkedSessionIds ?? [];
      const updated = {
        ...current,
        linkedSessionIds: existing.includes(sessionId) ? existing : [...existing, sessionId],
        status: "in-progress" as const,
        updatedAt: Date.now(),
      };
      cards[idx] = updated;
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

    // Verify partial patch was called with only changed fields and upsertCard was not called
    expect(repository.patchCard).toHaveBeenCalledWith(
      sampleProjectCwd,
      "card-1",
      { title: "Refactored authentication" },
    );
    expect(repository.upsertCard).not.toHaveBeenCalled();
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

    expect(repository.patchCard).toHaveBeenCalledWith(
      sampleProjectCwd,
      "card-1",
      { status: "in-progress" },
    );
    expect(repository.upsertCard).not.toHaveBeenCalled();

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

  it("enforces editorCancelAfterRemoval: removing an attachment in editor is staged and cancelling preserves persisted media", async () => {
    const { repository, cards } = createMockRepository([sampleCard]);

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

    // 1. Open edit card modal
    const editBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Edit card Setup authentication"]',
    )!;
    await act(async () => {
      editBtn.click();
    });

    const editModal = container.querySelector('[aria-label="Edit card"]')!;
    expect(editModal).not.toBeNull();

    // Verify existing attachment is shown
    const removeBtn = editModal.querySelector<HTMLButtonElement>(
      'button[aria-label="Remove attachment diagram.png"]',
    )!;
    expect(removeBtn).not.toBeNull();

    // 2. Remove the attachment in the editor
    await act(async () => {
      removeBtn.click();
    });

    // Staged removal: attachment disappeared from editor modal
    expect(editModal.querySelector('button[aria-label="Remove attachment diagram.png"]')).toBeNull();

    // CRITICAL: repository.deleteMedia must NOT have been called yet
    expect(repository.deleteMedia).not.toHaveBeenCalled();
    expect(cards[0].media).toHaveLength(1);

    // 3. Cancel the editor by clicking the close button
    const closeBtn = editModal.querySelector<HTMLButtonElement>('button[aria-label="Close"]')!;
    await act(async () => {
      closeBtn.click();
    });

    // Modal closed
    expect(container.querySelector('[aria-label="Edit card"]')).toBeNull();

    // deleteMedia was NEVER called
    expect(repository.deleteMedia).not.toHaveBeenCalled();

    // Card in the board view still has diagram.png view button
    const viewAttachBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="View attachment diagram.png"]',
    );
    expect(viewAttachBtn).not.toBeNull();
  });

  it("enforces editorSaveAfterRemoval: staged removals are deleted on save, but failed save leaves persisted media intact", async () => {
    const { repository, cards } = createMockRepository([sampleCard]);

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

    // 1. Open edit modal
    const editBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Edit card Setup authentication"]',
    )!;
    await act(async () => {
      editBtn.click();
    });

    const editModal = container.querySelector('[aria-label="Edit card"]')!;

    // 2. Remove the attachment
    const removeBtn = editModal.querySelector<HTMLButtonElement>(
      'button[aria-label="Remove attachment diagram.png"]',
    )!;
    await act(async () => {
      removeBtn.click();
    });

    // Test failed save boundary: patchCard throws
    repository.patchCard = vi.fn(async () => {
      throw new Error("Failed to persist card update");
    });

    const form = editModal.querySelector("form")!;
    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });

    // Verify error is shown and deleteMedia was NOT called
    expect(editModal.textContent).toContain("Failed to persist card update");
    expect(repository.deleteMedia).not.toHaveBeenCalled();
    expect(cards[0].media).toHaveLength(1);

    // Now restore successful patchCard and submit again
    const mockRepo = createMockRepository([sampleCard]).repository;
    repository.patchCard = mockRepo.patchCard;

    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });

    // Verify deleteMedia was called upon successful save
    expect(repository.deleteMedia).toHaveBeenCalledWith(
      sampleProjectCwd,
      "card-1",
      "media-1",
    );

    // Editor modal closed
    expect(container.querySelector('[aria-label="Edit card"]')).toBeNull();

    // Board view updated: diagram.png is no longer on the board card
    expect(
      container.querySelector('button[aria-label="View attachment diagram.png"]'),
    ).toBeNull();
  });

  it("enforces boardMediaDeleteState: preview delete failure leaves card view intact, and success updates owner board state without mutating card.media directly", async () => {
    // Freeze the initial card object and its media array to prove zero in-place mutation
    const frozenCard: ProjectBoardCard = Object.freeze({
      ...sampleCard,
      media: Object.freeze([
        Object.freeze({
          id: "media-1",
          name: "diagram.png",
          mimeType: "image/png" as const,
          byteLength: 2048,
        }),
      ]),
    });

    const { repository } = createMockRepository([frozenCard]);

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

    // Open preview
    const viewAttachBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="View attachment diagram.png"]',
    )!;
    await act(async () => {
      viewAttachBtn.click();
    });

    const previewModal = container.querySelector(
      '[aria-label="Image preview: diagram.png"]',
    )!;
    const deleteBtn = previewModal.querySelector<HTMLButtonElement>(
      'button[aria-label="Delete attachment diagram.png"]',
    )!;

    // Boundary 1: Deletion failure
    repository.deleteMedia = vi.fn(async () => {
      throw new Error("Disk permission denied");
    });

    await act(async () => {
      deleteBtn.click();
    });

    // Preview modal stays open with error
    expect(previewModal.textContent).toContain("Failed to delete attachment: Disk permission denied");

    // Close preview modal via close button
    const closePreviewBtn = previewModal.querySelector<HTMLButtonElement>(
      'button[aria-label="Close preview"]',
    )!;
    await act(async () => {
      closePreviewBtn.click();
    });

    // Card view in board remains intact with attachment button visible
    expect(
      container.querySelector('button[aria-label="View attachment diagram.png"]'),
    ).not.toBeNull();

    // Boundary 2: Successful deletion updates owner state immutably without mutating frozen card
    repository.deleteMedia = vi.fn(async (_cwd, cardId, mediaId) => {});

    // Open preview again
    const viewAttachBtn2 = container.querySelector<HTMLButtonElement>(
      'button[aria-label="View attachment diagram.png"]',
    )!;
    await act(async () => {
      viewAttachBtn2.click();
    });

    const previewModal2 = container.querySelector(
      '[aria-label="Image preview: diagram.png"]',
    )!;
    const deleteBtn2 = previewModal2.querySelector<HTMLButtonElement>(
      'button[aria-label="Delete attachment diagram.png"]',
    )!;

    // Click delete inside preview - this succeeds
    await act(async () => {
      deleteBtn2.click();
    });

    // Deletion succeeded without throwing a TypeError (frozenCard was not mutated)
    expect(repository.deleteMedia).toHaveBeenCalledWith(
      sampleProjectCwd,
      "card-1",
      "media-1",
    );

    // Preview closed and board card view updated immutably: attachment button removed
    expect(container.querySelector('[aria-label="Image preview: diagram.png"]')).toBeNull();
    expect(
      container.querySelector('button[aria-label="View attachment diagram.png"]'),
    ).toBeNull();
  });

  it("submits only changed fields via patchCard and preserves concurrent untouched updates", async () => {
    const cardWithDetails: ProjectBoardCard = {
      ...sampleCard,
      title: "Initial title",
      description: "Initial description",
      priority: "medium",
      status: "backlog",
      linkedSessionIds: ["sess-init-1"],
    };
    const { repository, cards } = createMockRepository([cardWithDetails]);

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

    // Open edit modal
    const editBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Edit card Initial title"]',
    )!;
    await act(async () => {
      editBtn.click();
    });

    // Simulate concurrent update on untouched fields (e.g. background agent or another window)
    cards[0].description = "Concurrently updated description";
    cards[0].priority = "high";
    cards[0].linkedSessionIds = ["sess-init-1", "sess-concurrent-2"];

    // In editor, only change the title
    const titleInput = container.querySelector<HTMLInputElement>("#card-title-input")!;
    await act(async () => {
      setInputValue(titleInput, "Locally patched title");
    });

    // Submit the form
    const form = container.querySelector("form")!;
    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });

    // Verify patchCard was called with ONLY title; untouched fields were not submitted
    expect(repository.patchCard).toHaveBeenCalledWith(
      sampleProjectCwd,
      "card-1",
      { title: "Locally patched title" },
    );
    expect(repository.upsertCard).not.toHaveBeenCalled();

    // Verify concurrent updates to untouched fields are preserved in DB and displayed on the board
    expect(cards[0].title).toBe("Locally patched title");
    expect(cards[0].description).toBe("Concurrently updated description");
    expect(cards[0].priority).toBe("high");
    expect(cards[0].linkedSessionIds).toEqual(["sess-init-1", "sess-concurrent-2"]);

    expect(container.textContent).toContain("Locally patched title");
    expect(container.textContent).toContain("Concurrently updated description");
  });

  it("does not fall back to upsertCard when the required patchCard method is absent", async () => {
    const { repository } = createMockRepository([sampleCard]);
    const repositoryWithoutPatch = {
      ...repository,
      patchCard: undefined,
    } as unknown as ProjectBoardRepository;

    await act(async () => {
      root.render(
        createElement(ProjectBoardDialog, {
          projectCwd: sampleProjectCwd,
          onClose: vi.fn(),
          onOpenSession: vi.fn(),
          repository: repositoryWithoutPatch,
        }),
      );
    });

    const editButton = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Edit card Setup authentication"]',
    )!;
    await act(async () => {
      editButton.click();
    });

    const titleInput = container.querySelector<HTMLInputElement>("#card-title-input")!;
    await act(async () => {
      setInputValue(titleInput, "Updated title");
    });

    const form = container.querySelector("form")!;
    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });

    expect(container.querySelector('[aria-label="Edit card"]')?.textContent).toContain(
      "patchCard",
    );
    expect(repository.upsertCard).not.toHaveBeenCalled();
  });

  it("retries safely after partial media upload failure without re-uploading succeeded media", async () => {
    const cardWithoutMedia: ProjectBoardCard = {
      ...sampleCard,
      media: [],
    };
    const { repository, cards } = createMockRepository([cardWithoutMedia]);

    let uploadAttempts = 0;
    const addMediaSpy = vi.fn(async ({ name, mimeType, dataBase64 }) => {
      uploadAttempts++;
      if (uploadAttempts === 2) {
        throw new Error("Network drop on second attachment");
      }
      const ref: BoardMediaRef = {
        id: "media-persisted-" + uploadAttempts,
        name,
        mimeType,
        byteLength: 1024,
      };
      cards[0].media = [...(cards[0].media ?? []), ref];
      return ref;
    });
    repository.addMedia = addMediaSpy;

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

    const editBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Edit card Setup authentication"]',
    )!;
    await act(async () => {
      editBtn.click();
    });

    const editModal = container.querySelector('[aria-label="Edit card"]')!;

    // Paste image 1
    const file1 = new File(["dummy1"], "photo-1.png", { type: "image/png" });
    const paste1 = new Event("paste", { bubbles: true }) as any;
    paste1.clipboardData = {
      items: [{ type: "image/png", getAsFile: () => file1 }],
    };
    await act(async () => {
      editModal.dispatchEvent(paste1);
      await new Promise((r) => setTimeout(r, 50));
    });

    // Paste image 2
    const file2 = new File(["dummy2"], "photo-2.png", { type: "image/png" });
    const paste2 = new Event("paste", { bubbles: true }) as any;
    paste2.clipboardData = {
      items: [{ type: "image/png", getAsFile: () => file2 }],
    };
    await act(async () => {
      editModal.dispatchEvent(paste2);
      await new Promise((r) => setTimeout(r, 50));
    });

    // First submit attempt: image 1 succeeds, image 2 throws
    const form = editModal.querySelector("form")!;
    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });

    // Verify error is displayed and modal remains open
    expect(editModal.textContent).toContain("Network drop on second attachment");
    expect(container.querySelector('[aria-label="Edit card"]')).not.toBeNull();
    expect(addMediaSpy).toHaveBeenCalledTimes(2);

    // Verify image 1 was called on first attempt
    expect(addMediaSpy.mock.calls[0][0].name).toBe("photo-1.png");
    expect(addMediaSpy.mock.calls[1][0].name).toBe("photo-2.png");

    // Clear addMediaSpy call history for the retry check
    addMediaSpy.mockClear();

    // Now retry submission
    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });

    // CRITICAL ACCEPTANCE CHECK:
    // Only image 2 was re-uploaded; image 1 was marked existing with empty payload so retry did NOT duplicate it!
    expect(addMediaSpy).toHaveBeenCalledTimes(1);
    expect(addMediaSpy.mock.calls[0][0].name).toBe("photo-2.png");

    // Modal closed upon successful retry
    expect(container.querySelector('[aria-label="Edit card"]')).toBeNull();

    // Final card has both media items without duplication
    expect(cards[0].media).toHaveLength(2);
    expect(cards[0].media.map((m) => m.name)).toEqual(["photo-1.png", "photo-2.png"]);
  });

  it("preserves multiple successful uploads and completed deletions when a later staged deletion fails", async () => {
    const cardWithTwoMedia: ProjectBoardCard = {
      ...sampleCard,
      media: [
        {
          id: "media-1",
          name: "diagram.png",
          mimeType: "image/png",
          byteLength: 2048,
        },
        {
          id: "media-2",
          name: "notes.png",
          mimeType: "image/png",
          byteLength: 1024,
        },
      ],
    };
    const { repository, cards } = createMockRepository([cardWithTwoMedia]);

    let uploadId = 0;
    const addMediaSpy = vi.fn(async ({ name, mimeType }) => {
      uploadId++;
      const ref: BoardMediaRef = {
        id: "uploaded-" + uploadId,
        name,
        mimeType,
        byteLength: 1024,
      };
      cards[0].media = [...cards[0].media, ref];
      return ref;
    });
    repository.addMedia = addMediaSpy;

    let failSecondDeletionOnce = true;
    const deleteMediaSpy = vi.fn(async (_cwd, cardId, mediaId) => {
      if (mediaId === "media-2" && failSecondDeletionOnce) {
        failSecondDeletionOnce = false;
        throw new Error("Second staged deletion failed");
      }
      const card = cards.find((item) => item.id === cardId);
      if (card) {
        card.media = card.media.filter((media) => media.id !== mediaId);
      }
    });
    repository.deleteMedia = deleteMediaSpy;

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

    const editButton = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Edit card Setup authentication"]',
    )!;
    await act(async () => {
      editButton.click();
    });

    for (const name of ["diagram.png", "notes.png"]) {
      const removeButton = container.querySelector<HTMLButtonElement>(
        'button[aria-label="Remove attachment ' + name + '"]',
      )!;
      await act(async () => {
        removeButton.click();
      });
    }

    const editModal = container.querySelector('[aria-label="Edit card"]')!;
    for (const name of ["first.png", "second.png"]) {
      const file = new File([name], name, { type: "image/png" });
      const pasteEvent = new Event("paste", { bubbles: true }) as any;
      pasteEvent.clipboardData = {
        items: [{ type: "image/png", getAsFile: () => file }],
      };
      await act(async () => {
        editModal.dispatchEvent(pasteEvent);
        await new Promise((resolve) => setTimeout(resolve, 50));
      });
    }

    const form = container.querySelector("form")!;
    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });

    expect(container.querySelector('[aria-label="Edit card"]')?.textContent).toContain(
      "Second staged deletion failed",
    );
    expect(addMediaSpy).toHaveBeenCalledTimes(2);
    expect(addMediaSpy.mock.calls.map(([input]) => input.name)).toEqual([
      "first.png",
      "second.png",
    ]);
    expect(deleteMediaSpy.mock.calls.map(([, , mediaId]) => mediaId)).toEqual([
      "media-1",
      "media-2",
    ]);
    expect(cards[0].media.map((media) => media.id)).toEqual([
      "media-2",
      "uploaded-1",
      "uploaded-2",
    ]);

    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });

    expect(addMediaSpy).toHaveBeenCalledTimes(2);
    expect(deleteMediaSpy.mock.calls.map(([, , mediaId]) => mediaId)).toEqual([
      "media-1",
      "media-2",
      "media-2",
    ]);
    expect(container.querySelector('[aria-label="Edit card"]')).toBeNull();
    expect(cards[0].media.map((media) => media.id)).toEqual(["uploaded-1", "uploaded-2"]);
    expect(cards[0].media.map((media) => media.name)).toEqual(["first.png", "second.png"]);
  });
});
