// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  EXPLORER_FILE_POINTER_DRAG_EVENT,
  type ExplorerFilePointerDragDetail,
} from "../../../shared/lib/drag";
import { Composer } from "./Composer";

const { invoke, dragDrop } = vi.hoisted(() => ({
  invoke: vi.fn(),
  dragDrop: {
    handler: null as null | ((event: { payload: unknown }) => void),
  },
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (fn: (event: { payload: unknown }) => void) => {
      dragDrop.handler = fn;
      return () => {
        dragDrop.handler = null;
      };
    },
  }),
}));
vi.mock("./useComposerSkills", () => ({
  useComposerSkills: () => ({
    contextKey: "codex:~",
    contextToken: { key: "codex:~", generation: 0 },
    isCurrent: () => true,
    refresh: async () => [],
    skills: [],
  }),
}));

let container: HTMLDivElement;
let root: Root;

function explorerDrag(detail: ExplorerFilePointerDragDetail) {
  return new CustomEvent<ExplorerFilePointerDragDetail>(
    EXPLORER_FILE_POINTER_DRAG_EVENT,
    { detail },
  );
}

function rect(
  left: number,
  top: number,
  width: number,
  height: number,
): DOMRect {
  return {
    left,
    top,
    right: left + width,
    bottom: top + height,
    width,
    height,
    x: left,
    y: top,
    toJSON: () => undefined,
  } as DOMRect;
}

function stubRect(el: HTMLElement, box: DOMRect) {
  el.getBoundingClientRect = () => box;
}

function composerProps(queued = false) {
  return {
    focused: false,
    harness: "codex" as const,
    model: "",
    runtimeMode: "supervised" as const,
    executionCwd: "~",
    hideTopBar: true,
    onFocus: vi.fn(),
    onCwdChange: vi.fn(),
    onModelChange: vi.fn(),
    onRuntimeModeChange: vi.fn(),
    onSubmit: vi.fn(),
    ...(queued
      ? {
          queuedMessages: [{ id: "q1", text: "follow up", attachments: [] }],
        }
      : {}),
  };
}

async function renderSession(queued = false) {
  await act(async () => {
    root.render(
      createElement(
        "div",
        { "data-session-drop": "session-1" },
        createElement(Composer, composerProps(queued)),
      ),
    );
  });
  const session = container.querySelector<HTMLElement>("[data-session-drop]")!;
  stubRect(session, rect(0, 0, 500, 500));
  await act(async () => undefined);
  return session;
}

async function startQueuedEdit(session: HTMLElement) {
  await act(async () =>
    session
      .querySelector<HTMLButtonElement>('[aria-label="Edit queued message"]')!
      .click(),
  );
  const queued = session.querySelector<HTMLElement>(
    "[data-queued-message-edit]",
  )!;
  stubRect(queued, rect(0, 0, 500, 80));
  return queued;
}

function fileDragEvent(type: "dragover" | "drop", file: File) {
  const files = Object.assign([file], {
    item: (index: number) => (index === 0 ? file : null),
  }) as unknown as FileList;
  const event = new DragEvent(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", {
    value: {
      types: ["Files"],
      files,
      dropEffect: "none",
    },
  });
  return event;
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  dragDrop.handler = null;
  invoke.mockReset();
  invoke.mockImplementation(
    async (command: string, args: { paths?: string[] }) => {
      if (command === "inspect_paths") {
        return (args.paths ?? []).map((path) => ({
          path,
          name: path.split("/").pop() ?? path,
          size: 12,
          isDir: false,
        }));
      }
      return [];
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("Composer Explorer file drops", () => {
  it("attaches a file from the pointer-driven Explorer drag", async () => {
    await renderSession();

    act(() =>
      window.dispatchEvent(
        explorerDrag({
          type: "move",
          path: "/project/src/main.ts",
          x: 100,
          y: 100,
        }),
      ),
    );
    expect(container.textContent).toContain("Drop files to attach");

    await act(async () => {
      window.dispatchEvent(
        explorerDrag({
          type: "drop",
          path: "/project/src/main.ts",
          x: 100,
          y: 100,
        }),
      );
    });

    expect(invoke).toHaveBeenCalledWith("inspect_paths", {
      paths: ["/project/src/main.ts"],
    });
    expect(
      container.querySelector('[title="/project/src/main.ts"]'),
    ).not.toBeNull();
  });

  it("does not show the composer overlay while dragging over a queued editor", async () => {
    const session = await renderSession(true);
    await startQueuedEdit(session);

    act(() =>
      window.dispatchEvent(
        explorerDrag({
          type: "move",
          path: "/project/src/main.ts",
          x: 100,
          y: 40,
        }),
      ),
    );
    expect(container.textContent).not.toContain("Drop files to attach");
  });

  it("attaches an Explorer drop on the queued editor to that message", async () => {
    const session = await renderSession(true);
    await startQueuedEdit(session);

    await act(async () => {
      window.dispatchEvent(
        explorerDrag({
          type: "drop",
          path: "/project/src/main.ts",
          x: 100,
          y: 40,
        }),
      );
    });

    expect(
      container.querySelector(
        '[data-queued-message-edit] [title="/project/src/main.ts"]',
      ),
    ).not.toBeNull();
    expect(
      container.querySelector(
        '[data-composer-box] [title="/project/src/main.ts"]',
      ),
    ).toBeNull();
  });

  it("still attaches an Explorer drop below the queued editor to the composer", async () => {
    const session = await renderSession(true);
    await startQueuedEdit(session);

    await act(async () => {
      window.dispatchEvent(
        explorerDrag({
          type: "drop",
          path: "/project/src/notes.md",
          x: 100,
          y: 200,
        }),
      );
    });

    expect(
      container.querySelector(
        '[data-composer-box] [title="/project/src/notes.md"]',
      ),
    ).not.toBeNull();
    expect(
      container.querySelector(
        '[data-queued-message-edit] [title="/project/src/notes.md"]',
      ),
    ).toBeNull();
  });

  it("discards a late Explorer drop after canceling and editing the same message", async () => {
    let finishInspect: (value?: unknown) => void = () => undefined;
    invoke.mockImplementation((command: string, args: { paths?: string[] }) => {
      if (command === "inspect_paths") {
        return new Promise((resolve) => {
          finishInspect = resolve;
        }).then(() =>
          (args.paths ?? []).map((path) => ({
            path,
            name: path.split("/").pop() ?? path,
            size: 12,
            isDir: false,
          })),
        );
      }
      return Promise.resolve([]);
    });

    const session = await renderSession(true);
    await startQueuedEdit(session);

    await act(async () => {
      window.dispatchEvent(
        explorerDrag({
          type: "drop",
          path: "/project/src/late.ts",
          x: 100,
          y: 40,
        }),
      );
    });

    await act(async () =>
      session
        .querySelector<HTMLButtonElement>(
          '[aria-label="Cancel queued message edit"]',
        )!
        .click(),
    );
    await startQueuedEdit(session);

    await act(async () => {
      finishInspect();
    });

    expect(
      container.querySelector(
        '[data-queued-message-edit] [title="/project/src/late.ts"]',
      ),
    ).toBeNull();
    expect(
      container.querySelector(
        '[data-composer-box] [title="/project/src/late.ts"]',
      ),
    ).toBeNull();
  });
});

describe("Composer native and Tauri file drops", () => {
  it("attaches a native drop on the queued editor to that message", async () => {
    const session = await renderSession(true);
    const queued = await startQueuedEdit(session);
    const file = new File(["hello"], "notes.md", {
      type: "text/markdown",
    }) as File & { path?: string };
    file.path = "/tmp/notes.md";

    await act(async () => {
      queued.dispatchEvent(fileDragEvent("dragover", file));
    });
    expect(container.textContent).not.toContain("Drop files to attach");

    await act(async () => {
      queued.dispatchEvent(fileDragEvent("drop", file));
    });

    expect(
      container.querySelector(
        '[data-queued-message-edit] [title="/tmp/notes.md"]',
      ),
    ).not.toBeNull();
    expect(
      container.querySelector('[data-composer-box] [title="/tmp/notes.md"]'),
    ).toBeNull();
  });

  it("routes a Tauri drop over the queued editor to that message", async () => {
    const session = await renderSession(true);
    await startQueuedEdit(session);
    expect(dragDrop.handler).not.toBeNull();

    await act(async () => {
      dragDrop.handler!({
        payload: {
          type: "over",
          position: { x: 100, y: 40 },
        },
      });
    });
    expect(container.textContent).not.toContain("Drop files to attach");

    await act(async () => {
      dragDrop.handler!({
        payload: {
          type: "drop",
          position: { x: 100, y: 40 },
          paths: ["/project/src/queued.ts"],
        },
      });
    });

    expect(
      container.querySelector(
        '[data-queued-message-edit] [title="/project/src/queued.ts"]',
      ),
    ).not.toBeNull();
    expect(
      container.querySelector(
        '[data-composer-box] [title="/project/src/queued.ts"]',
      ),
    ).toBeNull();
  });
});
