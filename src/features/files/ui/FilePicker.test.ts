// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { projectFiles, remoteFiles, loadProjectFiles, rankProjectFiles } = vi.hoisted(() => {
  const projectFiles = [
    {
      path: "/repo/src/App.tsx",
      relative: "src/App.tsx",
      name: "App.tsx",
    },
  ];
  const remoteFiles = [{
    path: "remote://env/home/me/repo/src/App.tsx",
    relative: "src/App.tsx",
    name: "App.tsx",
  }];
  return {
    projectFiles,
    remoteFiles,
    loadProjectFiles: vi.fn((cwd: string) =>
      cwd.startsWith("remote://") ? Promise.resolve(remoteFiles) : new Promise(() => {}),
    ),
    rankProjectFiles: vi.fn((_files: unknown[], query: string) =>
      query.trim() && !query.toLowerCase().includes("app")
        ? []
        : (_files as typeof projectFiles).map((file) => ({
            ...file,
            score: 1,
            positions: [],
          })),
    ),
  };
});

vi.mock("../model/fileIndex", () => ({
  loadProjectFiles,
  peekProjectFiles: vi.fn((cwd: string) => cwd.startsWith("remote://") ? null : projectFiles),
  rankProjectFiles,
  recentOpenedFiles: vi.fn(() => []),
  rememberOpenedFile: vi.fn(),
}));

vi.mock("../../projects/model/recents", () => ({
  looksLikeProject: (path: string) => path !== "~",
}));

import { FilePicker, reloadActionHint } from "./FilePicker";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

function renderPicker(initialQuery = "") {
  const onOpenFile = vi.fn();
  const onRunAction = vi.fn();
  const onClose = vi.fn();
  act(() =>
    root.render(
      createElement(FilePicker, {
        open: true,
        cwd: "/repo",
        initialQuery,
        onOpenFile,
        onRunAction,
        onClose,
      }),
    ),
  );
  return { onOpenFile, onRunAction, onClose };
}

function inputText(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )!.set!;
  act(() => {
    setter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function press(input: HTMLInputElement, key: string) {
  act(() => {
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
    );
  });
}

describe("file picker command mode", () => {
  it("formats reload shortcut hints for macOS and non-macOS", () => {
    expect(reloadActionHint("⌘", "⇧")).toBe("⌘⇧R");
    expect(reloadActionHint("Ctrl+", "Shift+")).toBe("Ctrl+Shift+R");
  });
  it("opens from an initial > query without ranking project files", () => {
    renderPicker(">");

    const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    expect(dialog.getAttribute("aria-label")).toBe("Command Palette");
    expect(input.value).toBe(">");
    expect(input.placeholder).toContain("type > for commands");
    expect(
      dialog.querySelector('[role="listbox"][aria-label="Commands"]'),
    ).not.toBeNull();
    expect(dialog.textContent).toContain("Reload MonoCode");
    expect(dialog.textContent).toContain(reloadActionHint());
    expect(rankProjectFiles).not.toHaveBeenCalled();
  });

  it("only treats a leading > as command mode and reverts when it is deleted", () => {
    renderPicker();
    const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
    const input = dialog.querySelector<HTMLInputElement>("input")!;

    inputText(input, "App>");
    expect(dialog.getAttribute("aria-label")).toBe("Go to File");
    expect(
      dialog.querySelector('[role="listbox"][aria-label="Files"]'),
    ).not.toBeNull();

    rankProjectFiles.mockClear();
    inputText(input, ">");
    expect(dialog.getAttribute("aria-label")).toBe("Command Palette");
    expect(dialog.textContent).toContain("Reload MonoCode");
    expect(rankProjectFiles).not.toHaveBeenCalled();

    inputText(input, "App");
    expect(dialog.getAttribute("aria-label")).toBe("Go to File");
    expect(
      dialog.querySelector('[role="listbox"][aria-label="Files"]'),
    ).not.toBeNull();
    expect(rankProjectFiles).toHaveBeenCalled();
  });

  it("fuzzy-filters and highlights commands with palette-specific empty copy", () => {
    renderPicker(">");
    const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
    const input = dialog.querySelector<HTMLInputElement>("input")!;

    inputText(input, "> rmc");
    const reload = [
      ...dialog.querySelectorAll<HTMLButtonElement>("button"),
    ].find((button) => button.textContent?.includes("Reload MonoCode"))!;
    expect(reload).not.toBeUndefined();
    expect(reload.querySelectorAll(".text-accent")).toHaveLength(3);

    inputText(input, "> missing");
    expect(dialog.textContent).toContain("No matching commands");
    expect(dialog.textContent).not.toContain("No matching files");
  });

  it.each(["click", "Enter"] as const)(
    "runs the selected command and closes on %s",
    (method) => {
      const callbacks = renderPicker(">");
      const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
      const input = dialog.querySelector<HTMLInputElement>("input")!;
      const reload = dialog.querySelector<HTMLButtonElement>(
        '[role="option"][aria-selected="true"]',
      )!;

      if (method === "click") act(() => reload.click());
      else press(input, "Enter");

      expect(callbacks.onRunAction).toHaveBeenCalledExactlyOnceWith("reload");
      expect(callbacks.onClose).toHaveBeenCalledOnce();
      expect(callbacks.onOpenFile).not.toHaveBeenCalled();
    },
  );
});

describe("file picker on a remote project", () => {
  const cwd = "remote://env/home/me/repo";

  it("uses the shared file list and opens its remote path", async () => {
    const onOpenFile = vi.fn();
    const onClose = vi.fn();
    await act(async () =>
      root.render(
        createElement(FilePicker, {
          open: true,
          cwd,
          initialQuery: "app",
          onOpenFile,
          onRunAction: vi.fn(),
          onClose,
        }),
      ),
    );
    const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
    expect(loadProjectFiles).toHaveBeenCalledWith(cwd, true);
    expect(dialog.textContent).toContain("App.tsx");

    press(dialog.querySelector<HTMLInputElement>("input")!, "Enter");
    expect(onOpenFile).toHaveBeenCalledExactlyOnceWith(remoteFiles[0].path, undefined, { exact: true });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("shows a connection error from the shared file list", async () => {
    loadProjectFiles.mockRejectedValueOnce(new Error("Machine is not connected"));
    await act(async () =>
      root.render(
        createElement(FilePicker, {
          open: true,
          cwd,
          onOpenFile: vi.fn(),
          onRunAction: vi.fn(),
          onClose: vi.fn(),
        }),
      ),
    );
    expect(document.body.textContent).toContain("Machine is not connected");
  });
});
