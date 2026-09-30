// @vitest-environment happy-dom
import { EditorView } from "@codemirror/view";
import { Storage } from "happy-dom";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FILE_EDITOR_AUTOSAVE_DELAY_MS, FileEditor } from "./FileEditor";
import { saveAutosave } from "../../settings/model/settings";
import { invalidateWatchedFiles } from "../model/fileWatch";

const disk = vi.hoisted(() => ({ content: "" }));
const written = vi.hoisted(() => ({ content: null as string | null }));
const formatText = vi.hoisted(() => vi.fn(async () => null));
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
vi.mock("../../../shared/lib/format", () => ({ formatText }));
const defaultInvoke = invoke.getMockImplementation()!;

describe("file editor line endings", () => {
  let root: Root;
  let container: HTMLDivElement;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("localStorage", new Storage());
    invoke.mockClear();
    formatText.mockReset();
    formatText.mockResolvedValue(null);
    written.content = null;
    saveAutosave(true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    vi.useRealTimers();
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

  it("automatically saves after typing stops", async () => {
    disk.content = "alpha\n";
    const view = await renderEditor("/repo/notes.txt");
    vi.useFakeTimers();

    await act(async () => {
      view.dispatch({ changes: { from: 0, insert: "first " } });
      await vi.advanceTimersByTimeAsync(FILE_EDITOR_AUTOSAVE_DELAY_MS - 1);
    });
    expect(written.content).toBeNull();

    await act(async () => {
      view.dispatch({ changes: { from: 0, insert: "second " } });
      await vi.advanceTimersByTimeAsync(FILE_EDITOR_AUTOSAVE_DELAY_MS - 1);
    });
    expect(written.content).toBeNull();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(written.content).toBe("second first alpha\n");
  });

  it("keeps changes dirty when autosave is disabled", async () => {
    disk.content = "alpha\n";
    saveAutosave(false);
    const view = await renderEditor("/repo/notes.txt");
    vi.useFakeTimers();

    await act(async () => {
      view.dispatch({ changes: { from: 0, insert: "changed " } });
      await vi.advanceTimersByTimeAsync(FILE_EDITOR_AUTOSAVE_DELAY_MS);
    });

    expect(written.content).toBeNull();
  });

  it("does not autosave over an external file change", async () => {
    disk.content = "alpha\n";
    const path = "/repo/notes.txt";
    const view = await renderEditor(path);
    vi.useFakeTimers();

    await act(async () => {
      view.dispatch({ changes: { from: 0, insert: "local " } });
      disk.content = "external\n";
      invalidateWatchedFiles([path]);
      await vi.advanceTimersByTimeAsync(FILE_EDITOR_AUTOSAVE_DELAY_MS);
    });

    expect(written.content).toBeNull();
    expect(view.state.doc.toString()).toBe("local alpha\n");
  });

  it("does not autosave an external change detected during formatting", async () => {
    disk.content = "alpha\n";
    const path = "/repo/notes.ts";
    let finishFormatting!: (value: {
      formatted: string;
      cursorOffset: number;
    }) => void;
    formatText.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishFormatting = resolve;
        }),
    );
    const view = await renderEditor(path);
    vi.useFakeTimers();

    await act(async () => {
      view.dispatch({ changes: { from: 0, insert: "local " } });
      await vi.advanceTimersByTimeAsync(FILE_EDITOR_AUTOSAVE_DELAY_MS);
    });
    expect(formatText).toHaveBeenCalledOnce();

    await act(async () => {
      disk.content = "external\n";
      invalidateWatchedFiles([path]);
      finishFormatting({ formatted: "local alpha\n", cursorOffset: 6 });
      await Promise.resolve();
    });

    expect(written.content).toBeNull();
    expect(view.state.doc.toString()).toBe("local alpha\n");
  });

  it("restores a pending autosave after a manual save fails", async () => {
    disk.content = "alpha\n";
    let writeAttempts = 0;
    invoke.mockImplementation(async (command, args) => {
      if (command === "write_text_file" && ++writeAttempts === 1) {
        throw new Error("disk unavailable");
      }
      return defaultInvoke(command, args);
    });
    const view = await renderEditor("/repo/notes.txt");
    vi.useFakeTimers();

    await act(async () => {
      view.dispatch({ changes: { from: 0, insert: "changed " } });
      view.contentDOM.dispatchEvent(
        new KeyboardEvent("keydown", { key: "s", ctrlKey: true }),
      );
      await Promise.resolve();
    });
    expect(writeAttempts).toBe(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(FILE_EDITOR_AUTOSAVE_DELAY_MS);
    });
    expect(writeAttempts).toBe(2);
    expect(written.content).toBe("changed alpha\n");
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
