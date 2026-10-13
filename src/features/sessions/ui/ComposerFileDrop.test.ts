// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DragDropEvent } from "@tauri-apps/api/webview";
import {
  EXPLORER_FILE_POINTER_DRAG_EVENT,
  type ExplorerFilePointerDragDetail,
} from "../../../shared/lib/drag";
import { Composer } from "./Composer";

const { invoke, listen, platform } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  platform: { isWin: false },
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: listen,
  }),
}));
vi.mock("../../../platform/tauri/platform", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("../../../platform/tauri/platform")
  >()),
  get IS_WIN() {
    return platform.isWin;
  },
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
let submit: ReturnType<typeof vi.fn>;
let handlers: Set<(event: { payload: DragDropEvent }) => void>;

async function render(props: Record<string, unknown> = {}) {
  await act(async () => {
    root.render(
      createElement(
        "div",
        { "data-session-drop": "session-1" },
        createElement(Composer, {
          focused: false,
          harness: "codex",
          model: "",
          runtimeMode: "supervised",
          executionCwd: "~",
          hideTopBar: true,
          onFocus: vi.fn(),
          onCwdChange: vi.fn(),
          onModelChange: vi.fn(),
          onRuntimeModeChange: vi.fn(),
          onSubmit: submit,
          ...props,
        }),
      ),
    );
  });
  setDropRect(0, 0, 500, 500);
  return container.querySelector<HTMLElement>("[data-session-drop]")!;
}

function setDropRect(left: number, top: number, right: number, bottom: number) {
  const session = container.querySelector<HTMLElement>("[data-session-drop]")!;
  session.getBoundingClientRect = () =>
    ({
      left,
      top,
      right,
      bottom,
      width: right - left,
      height: bottom - top,
      x: left,
      y: top,
      toJSON: () => undefined,
    }) as DOMRect;
}

async function nativeDrop(paths: string[], x = 100, y = 100) {
  await act(async () => {
    for (const handler of handlers) {
      handler({
        payload: { type: "drop", paths, position: { x, y } } as DragDropEvent,
      });
    }
  });
}

function domDrag(
  target: HTMLElement,
  type: string,
  files: File[] = [],
  data: Record<string, unknown> = {},
) {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", {
    value: { types: ["Files"], files, dropEffect: "none", ...data },
  });
  target.dispatchEvent(event);
  return event;
}

function attachmentCount() {
  return container.querySelectorAll('[aria-label^="Remove "]').length;
}

function explorerDrag(detail: ExplorerFilePointerDragDetail) {
  return new CustomEvent<ExplorerFilePointerDragDetail>(
    EXPLORER_FILE_POINTER_DRAG_EVENT,
    { detail },
  );
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  platform.isWin = false;
  submit = vi.fn();
  handlers = new Set();
  listen.mockReset();
  listen.mockImplementation(async (handler) => {
    handlers.add(handler);
    return () => handlers.delete(handler);
  });
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
      if (command === "read_file_base64") return "aW1hZ2U=";
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
    await render();

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
});

describe("Composer image drops", () => {
  it("accepts drops after a disabled composer becomes ready", async () => {
    await render({ disabled: true });
    const session = await render({ disabled: false });

    act(() => domDrag(session, "dragover"));
    expect(container.textContent).toContain("Drop files to attach");
    await nativeDrop(["/project/image.png"]);

    expect(attachmentCount()).toBe(1);
  });

  it("stops accepting drops while the composer is disabled", async () => {
    const session = await render();
    await render({ disabled: true });

    act(() => domDrag(session, "dragover"));
    await nativeDrop(["/project/image.png"]);

    expect(container.textContent).not.toContain("Drop files to attach");
    expect(attachmentCount()).toBe(0);
  });

  it("keeps the native listener while switching between supported harnesses", async () => {
    await render({ harness: "codex" });
    await render({ harness: "claude" });
    await nativeDrop(["/project/image.png"]);

    expect(listen).toHaveBeenCalledTimes(1);
    expect(attachmentCount()).toBe(1);
  });

  it("uses current attachment capabilities without replacing the listener", async () => {
    const session = await render();
    await render({ harness: "fx" });
    act(() => domDrag(session, "dragover"));
    await nativeDrop(["/project/image.png"]);

    expect(listen).toHaveBeenCalledTimes(1);
    expect(container.textContent).not.toContain("Drop files to attach");
    expect(attachmentCount()).toBe(0);

    await render({ harness: "codex" });
    await nativeDrop(["/project/image.png"]);
    expect(attachmentCount()).toBe(1);
  });

  it("ignores a native listener whose registration finishes after disabling", async () => {
    let finishRegistration: (() => void) | undefined;
    listen.mockImplementation((handler) => {
      handlers.add(handler);
      return new Promise<() => void>((resolve) => {
        finishRegistration = () =>
          resolve(() => {
            handlers.delete(handler);
          });
      });
    });
    await render();
    await render({ disabled: true });
    await nativeDrop(["/project/image.png"]);
    expect(attachmentCount()).toBe(0);
    expect(invoke).not.toHaveBeenCalledWith("inspect_paths", expect.anything());

    await act(async () => {
      finishRegistration?.();
    });
    expect(handlers.size).toBe(0);
  });

  it("scales Windows native positions even inside the CSS viewport", async () => {
    platform.isWin = true;
    vi.stubGlobal("devicePixelRatio", 2);
    vi.stubGlobal("innerWidth", 1200);
    vi.stubGlobal("innerHeight", 1000);
    await render();
    setDropRect(100, 200, 600, 450);

    await nativeDrop(["C:/project/image.png"], 600, 600);

    expect(attachmentCount()).toBe(1);
  });

  it("keeps macOS logical coordinates on a Retina display", async () => {
    vi.stubGlobal("devicePixelRatio", 2);
    await render();
    setDropRect(100, 200, 600, 450);

    await nativeDrop(["/project/image.png"], 300, 300);

    expect(attachmentCount()).toBe(1);
  });

  it("does not attach native drops outside the session pane", async () => {
    await render();
    await nativeDrop(["/project/image.png"], 600, 600);
    expect(invoke).not.toHaveBeenCalledWith("inspect_paths", expect.anything());
    expect(attachmentCount()).toBe(0);
  });

  it("waits for a dropped image to finish reading before Send", async () => {
    let release: (() => void) | undefined;
    invoke.mockImplementation(
      async (command: string, args: { paths?: string[] }) => {
        if (command === "inspect_paths") {
          return (args.paths ?? []).map((path) => ({
            path,
            name: "image.png",
            size: 12,
            isDir: false,
          }));
        }
        if (command === "read_file_base64") {
          await new Promise<void>((resolve) => {
            release = resolve;
          });
          return "aW1hZ2U=";
        }
        return [];
      },
    );
    await render({ initialDraft: "look" });
    await nativeDrop(["/project/image.png"]);
    await act(async () => {
      container
        .querySelector<HTMLButtonElement>('button[aria-label="Send"]')!
        .click();
    });
    expect(submit).not.toHaveBeenCalled();

    await act(async () => {
      release?.();
    });

    expect(submit).toHaveBeenCalledWith(
      "look",
      [expect.objectContaining({ name: "image.png", data: "aW1hZ2U=" })],
      expect.anything(),
    );
    expect(attachmentCount()).toBe(0);
  });

  it("discards a drop that finishes reading after a draft reset", async () => {
    let release: (() => void) | undefined;
    invoke.mockImplementation(async (command: string) => {
      if (command === "inspect_paths") {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
        return [
          {
            path: "/project/image.png",
            name: "image.png",
            size: 12,
            isDir: false,
          },
        ];
      }
      return "aW1hZ2U=";
    });
    await render();
    await nativeDrop(["/project/image.png"]);
    await render({ draftResetToken: 1 });
    await act(async () => {
      release?.();
    });

    expect(attachmentCount()).toBe(0);
  });

  it("reports a failed file inspection", async () => {
    await render();
    invoke.mockRejectedValue(new Error("Cannot read the dropped file."));
    await nativeDrop(["/project/image.png"]);

    expect(container.querySelector('[role="alert"]')?.textContent).toBe(
      "Cannot read the dropped file.",
    );
  });

  it("reports when the dropped path no longer exists", async () => {
    await render();
    invoke.mockResolvedValue([]);
    await nativeDrop(["/project/image.png"]);

    expect(container.querySelector('[role="alert"]')?.textContent).toContain(
      "Nothing to attach",
    );
  });

  it("allows a DOM image drop after an empty native drop", async () => {
    const session = await render();
    await nativeDrop([]);
    await act(async () => {
      domDrag(session, "drop", [
        Object.assign(new File(["image"], "image.png", { type: "image/png" }), {
          path: "/project/image.png",
        }),
      ]);
    });

    expect(attachmentCount()).toBe(1);
  });

  it("accepts image items when the browser leaves FileList and types empty", async () => {
    const session = await render();
    const file = Object.assign(
      new File(["image"], "image.png", { type: "image/png" }),
      { path: "/project/image.png" },
    );
    await act(async () => {
      domDrag(session, "drop", [], {
        types: [],
        items: [{ kind: "file", type: "image/png", getAsFile: () => file }],
      });
    });

    expect(attachmentCount()).toBe(1);
  });

  it("processes a native drop once when the browser reports it too", async () => {
    const session = await render();
    await nativeDrop(["/project/image.png"]);
    await act(async () => {
      domDrag(session, "drop", [
        Object.assign(new File(["image"], "image.png", { type: "image/png" }), {
          path: "/project/image.png",
        }),
      ]);
    });

    expect(
      invoke.mock.calls.filter(([command]) => command === "inspect_paths"),
    ).toHaveLength(1);
    expect(attachmentCount()).toBe(1);
  });
});
