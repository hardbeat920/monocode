// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newEditorPane, newFileTab } from "../../workspace/model/layout";
import { FilePane } from "./FilePane";

// A Markdown-string label carries its own backticks.
const SOURCE = 'flowchart LR\n  App["`**App**`"] --> Disk\n  Disk --> Done';

const invoke = vi.hoisted(() =>
  vi.fn(async (command: string) => {
    if (command === "read_text_file") return SOURCE;
    if (command === "git_diff_files") return { files: [] };
    if (command === "stat_files") return [];
    throw new Error(`Unexpected command: ${command}`);
  }),
);
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));

const engine = vi.hoisted(() => ({
  initialize: vi.fn(),
  render: vi.fn(async (_id: string, _source: string) => ({
    svg: '<svg data-testid="diagram"></svg>',
  })),
}));
vi.mock("mermaid", () => ({ default: engine }));

describe("Mermaid file preview", () => {
  let root: Root;
  let container: HTMLDivElement;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.clearAllMocks();
    vi.unstubAllGlobals();
  });

  async function open(path: string) {
    const pane = newEditorPane(newFileTab(path, "/repo"));
    await act(async () =>
      root.render(
        createElement(FilePane, {
          pane,
          focused: true,
          dirtyFileIds: new Set<string>(),
          fileErrorCounts: new Map<string, number>(),
          sessions: [],
          onFocus: () => {},
          onSelectFile: () => {},
          onCloseFile: () => {},
          onCloseOtherFiles: () => {},
          onDirtyChange: () => {},
          onErrorCountChange: () => {},
          onReorderFiles: () => {},
          onOpenFile: () => {},
          onUpdatePlan: () => {},
          onBuildPlan: () => {},
          editorNavigation: null,
        }),
      ),
    );
  }

  async function settled(check: () => void) {
    await vi.waitFor(
      async () => {
        await act(async () => {
          await vi.dynamicImportSettled();
        });
        check();
      },
      { timeout: 5000 },
    );
  }

  it("draws the whole file as a diagram, with the source a tab away", async () => {
    await open("/repo/flow.mmd");
    await settled(() =>
      expect(container.querySelector('[data-testid="diagram"]')).not.toBeNull(),
    );

    expect(engine.render).toHaveBeenCalledTimes(1);
    expect(engine.render.mock.calls[0]![1].trimEnd()).toBe(SOURCE);
    const tabs = [
      ...container.querySelectorAll(
        '[aria-label="Markdown view"] [role="tab"]',
      ),
    ].map((tab) => tab.textContent);
    expect(tabs).toEqual(["Preview", "Source"]);
  });

  it("shows the source in the preview when the diagram does not parse", async () => {
    engine.render.mockRejectedValueOnce(new Error("Parse error"));
    await open("/repo/broken.mermaid");
    await settled(() =>
      expect(
        container.querySelector('[aria-label="Markdown preview"]')?.textContent,
      ).toContain("Disk --> Done"),
    );

    expect(container.querySelector('[data-testid="diagram"]')).toBeNull();
  });
});
