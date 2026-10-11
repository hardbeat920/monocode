// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newEditorPane, newFileTab } from "../../workspace/model/layout";
import { FilePane } from "./FilePane";

const invoke = vi.hoisted(() =>
  vi.fn(async (command: string) => {
    if (command === "read_text_file") return "# Title\n\nBody";
    if (command === "git_diff_files") return { files: [] };
    if (command === "git_file_diff")
      return {
        original: "# Title",
        current: "# Title\n\nBody",
        binary: false,
        tooLarge: false,
      };
    if (command === "stat_files") return [];
    throw new Error(`Unexpected command: ${command}`);
  }),
);
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));

describe("previewable file view mode", () => {
  let storage: Map<string, string>;
  let mounted: { root: Root; container: HTMLDivElement }[];

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    storage = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value);
      },
      removeItem: (key: string) => {
        storage.delete(key);
      },
    });
    invoke.mockClear();
    mounted = [];
  });

  afterEach(async () => {
    for (const { root, container } of mounted) {
      await act(async () => root.unmount());
      container.remove();
    }
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  function paneElement(
    pane: ReturnType<typeof newEditorPane>,
    path: string,
    options: { review?: boolean; line?: number },
  ) {
    return createElement(FilePane, {
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
      editorNavigation:
        options.line === undefined
          ? null
          : { path, line: options.line, column: 1, token: 1 },
    });
  }

  async function open(
    path: string,
    options: { review?: boolean; line?: number } = {},
  ) {
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    mounted.push({ root, container });
    const pane = newEditorPane(newFileTab(path, "/repo", options.review));
    await act(async () => root.render(paneElement(pane, path, options)));
    await vi.waitFor(
      async () => {
        await act(async () => {
          await vi.dynamicImportSettled();
        });
        expect(
          container.querySelector('[aria-label="Markdown view"]'),
        ).not.toBeNull();
      },
      { timeout: 5000 },
    );
    const tab = (label: "Preview" | "Source") =>
      [
        ...container.querySelectorAll<HTMLButtonElement>(
          '[aria-label="Markdown view"] [role="tab"]',
        ),
      ].find((button) => button.textContent === label)!;
    return {
      selected: () =>
        container.querySelector(
          '[aria-label="Markdown view"] [role="tab"][aria-selected="true"]',
        )?.textContent,
      choose: (label: "Preview" | "Source") =>
        act(async () => tab(label).click()),
      rerender: () =>
        act(async () => root.render(paneElement(pane, path, options))),
    };
  }

  it("opens a new Markdown file in the view chosen last", async () => {
    const first = await open("/repo/first.md");
    expect(first.selected()).toBe("Preview");
    await first.choose("Source");

    const next = await open("/repo/next.md");
    expect(next.selected()).toBe("Source");
  });

  it("keeps review tabs in Source without changing the file default", async () => {
    const file = await open("/repo/notes.md");
    await file.choose("Source");

    const review = await open("/repo/changed.md", { review: true });
    expect(review.selected()).toBe("Source");
    await review.choose("Preview");

    const next = await open("/repo/later.md");
    expect(next.selected()).toBe("Source");
  });

  it("leaves files that are already open in their current view", async () => {
    const open1 = await open("/repo/left.md");
    const open2 = await open("/repo/right.md");
    await open1.choose("Source");

    await open2.rerender();
    expect(open2.selected()).toBe("Preview");
  });

  it("opens in the view saved by an earlier run", async () => {
    storage.set("monocode.fileViewMode", "source");

    const file = await open("/repo/restarted.md");
    expect(file.selected()).toBe("Source");
  });

  it("keeps the last choice for this run when storage rejects it", async () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => {
        throw new Error("quota");
      },
    });
    const file = await open("/repo/unsaved.md");
    await file.choose("Source");

    const next = await open("/repo/unsaved-next.md");
    expect(next.selected()).toBe("Source");
    // The in-memory choice lasts for the module; leave it at the default.
    await next.choose("Preview");
  });

  it("does not change the file default when navigation opens the source", async () => {
    const linked = await open("/repo/linked.md", { line: 1 });
    await vi.waitFor(() => expect(linked.selected()).toBe("Source"));

    const next = await open("/repo/unlinked.md");
    expect(next.selected()).toBe("Preview");
  });
});
