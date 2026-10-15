// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newEditorPane, newFileTab } from "../../workspace/model/layout";
import { FilePane } from "./FilePane";
import { openFindInActiveFilePreview } from "./FilePreviewSearch";

const LOG = [
  '{"type":"start","at":"2026-10-15T09:00:00Z","config":{"retries":3}}',
  "",
  '{"type":"step","tags":["a","b"]',
  '{"type":"done","ok":true}',
].join("\n");

const invoke = vi.hoisted(() =>
  vi.fn(async (command: string) => {
    if (command === "read_text_file") return LOG;
    if (command === "stat_files") return [];
    throw new Error(`Unexpected command: ${command}`);
  }),
);
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));

describe("JSON Lines preview", () => {
  let root: Root;
  let container: HTMLDivElement;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const storage = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
    });
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("opens a .jsonl file as one row per record with invalid lines kept", async () => {
    const path = "/repo/run.jsonl";
    const noop = () => {};
    await act(async () =>
      root.render(
        createElement(FilePane, {
          pane: newEditorPane(newFileTab(path, "/repo")),
          focused: true,
          dirtyFileIds: new Set<string>(),
          fileErrorCounts: new Map<string, number>(),
          sessions: [],
          onFocus: noop,
          onSelectFile: noop,
          onCloseFile: noop,
          onCloseOtherFiles: noop,
          onDirtyChange: noop,
          onErrorCountChange: noop,
          onReorderFiles: noop,
          onOpenFile: noop,
          onUpdatePlan: noop,
          onBuildPlan: noop,
        }),
      ),
    );
    const preview = () =>
      container.querySelector('[aria-label="JSON Lines preview"]');
    await vi.waitFor(
      async () => {
        await act(async () => {
          await vi.dynamicImportSettled();
        });
        expect(preview()?.textContent).toContain("3 records");
      },
      { timeout: 5000 },
    );

    expect(
      container.querySelector(
        '[aria-label="Markdown view"] [role="tab"][aria-selected="true"]',
      )?.textContent,
    ).toBe("Preview");
    const rows = [
      ...preview()!.querySelectorAll<HTMLButtonElement>(
        "button[aria-expanded]",
      ),
    ];
    expect(rows.map((row) => row.textContent)).toEqual([
      '1type: "start", at: "2026-10-15T09:00:00Z", config: {1 key}',
      `3Invalid JSON${'{"type":"step","tags":["a","b"]'}`,
      '4type: "done", ok: true',
    ]);
    expect(preview()!.textContent).toContain("1 invalid line");
    expect(preview()!.textContent).not.toContain('"retries": 3');

    await act(async () => {
      expect(openFindInActiveFilePreview()).toBe(true);
    });
    const input = container.querySelector<HTMLInputElement>(
      'input[aria-label="Find"]',
    )!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )!.set!.call(input, "type");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(container.textContent).toContain("1 of 3");

    // Closing Find hands focus back to the preview, as it does for Markdown.
    await act(async () => {
      input.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
      await new Promise((resolve) => requestAnimationFrame(resolve));
    });
    expect(document.activeElement).toBe(preview());

    await act(async () => rows[0].click());
    expect(rows[0].getAttribute("aria-expanded")).toBe("true");
    expect(preview()!.textContent).toContain('"retries": 3');

    let parseError = "";
    try {
      JSON.parse('{"type":"step","tags":["a","b"]');
    } catch (error) {
      parseError = (error as Error).message;
    }
    expect(preview()!.textContent).not.toContain(parseError);
    await act(async () => rows[1].click());
    expect(preview()!.textContent).toContain(parseError);
  });
});
