// @vitest-environment happy-dom
import { EditorView } from "@codemirror/view";
import { Storage } from "happy-dom";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FileEditor } from "./FileEditor";

const disk = vi.hoisted(() => ({ content: "" }));
const written = vi.hoisted(() => ({ content: null as string | null }));
const invoke = vi.hoisted(() =>
  vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "read_text_file") return disk.content;
    if (command === "write_text_file") {
      written.content = args?.content as string;
      return null;
    }
    if (command === "stat_files") return [];
    throw new Error(`Unexpected command: ${command}`);
  }),
);
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));
const defaultInvoke = invoke.getMockImplementation()!;

describe("file editor line endings", () => {
  let root: Root;
  let container: HTMLDivElement;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("localStorage", new Storage());
    invoke.mockClear();
    written.content = null;
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

  async function renderEditor(path: string) {
    // Keep the same FileEditor instance so its save queue survives the switch.
    await act(async () =>
      root.render(
        createElement(FileEditor, {
          path,
          cwd: "/repo",
          active: true,
          onDirtyChange: () => {},
        }),
      ),
    );
    await act(async () =>
      vi.waitFor(() =>
        expect(container.querySelector(".cm-editor")).not.toBeNull(),
      ),
    );
    return EditorView.findFromDOM(
      container.querySelector<HTMLElement>(".cm-editor")!,
    )!;
  }

  it("saves a CRLF file back with CRLF line endings", async () => {
    disk.content = "alpha\r\nbeta\r\n";

    const view = await renderEditor("/repo/notes.txt");
    // The document itself is LF-only.
    expect(view.state.doc.toString()).toBe("alpha\nbeta\n");

    await act(async () => {
      view.dispatch({ changes: { from: 0, insert: "intro\n" } });
      view.contentDOM.dispatchEvent(
        new KeyboardEvent("keydown", { key: "s", ctrlKey: true }),
      );
    });

    await act(async () =>
      vi.waitFor(() =>
        expect(written.content).toBe("intro\r\nalpha\r\nbeta\r\n"),
      ),
    );
    expect(invoke).toHaveBeenCalledWith("write_text_file", {
      path: "/repo/notes.txt",
      content: "intro\r\nalpha\r\nbeta\r\n",
    });
  });

  it("preserves queued save line endings after switching files", async () => {
    let releaseFirstWrite!: () => void;
    const firstWrite = new Promise<void>((resolve) => {
      releaseFirstWrite = resolve;
    });
    const writes: Record<string, unknown>[] = [];
    invoke.mockImplementation(async (command, args) => {
      if (command === "write_text_file") {
        writes.push(args!);
        if (writes.length === 1) await firstWrite;
      }
      return defaultInvoke(command, args);
    });

    try {
      disk.content = "alpha\r\nbeta\r\n";
      const view = await renderEditor("/repo/crlf.txt");
      await act(async () => {
        view.dispatch({ changes: { from: 0, insert: "first\n" } });
        view.contentDOM.dispatchEvent(
          new KeyboardEvent("keydown", { key: "s", ctrlKey: true }),
        );
      });
      expect(writes).toEqual([
        { path: "/repo/crlf.txt", content: "first\r\nalpha\r\nbeta\r\n" },
      ]);

      await act(async () => {
        view.dispatch({ changes: { from: 0, insert: "second\n" } });
        view.contentDOM.dispatchEvent(
          new KeyboardEvent("keydown", { key: "s", ctrlKey: true }),
        );
      });
      expect(writes).toHaveLength(1);

      disk.content = "other\nfile\n";
      const nextView = await renderEditor("/repo/lf.txt");
      expect(nextView.state.doc.toString()).toBe(disk.content);

      await act(async () => releaseFirstWrite());
      expect(writes).toEqual([
        { path: "/repo/crlf.txt", content: "first\r\nalpha\r\nbeta\r\n" },
        {
          path: "/repo/crlf.txt",
          content: "second\r\nfirst\r\nalpha\r\nbeta\r\n",
        },
      ]);
    } finally {
      await act(async () => releaseFirstWrite());
      invoke.mockImplementation(defaultInvoke);
    }
  });
});
