// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  EXPLORER_FILE_POINTER_DRAG_EVENT,
  type ExplorerFilePointerDragDetail,
} from "../../../shared/lib/drag";
import { Composer } from "./Composer";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async () => () => undefined,
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

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
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
            onSubmit: vi.fn(),
          }),
        ),
      );
    });
    const session = container.querySelector<HTMLElement>(
      "[data-session-drop]",
    )!;
    session.getBoundingClientRect = () =>
      ({
        left: 0,
        top: 0,
        right: 500,
        bottom: 500,
        width: 500,
        height: 500,
        x: 0,
        y: 0,
        toJSON: () => undefined,
      }) as DOMRect;

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
