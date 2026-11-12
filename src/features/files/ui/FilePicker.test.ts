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
  loadArchivedProjects: vi.fn(() => [
    { path: "/repo/old", archivedAt: 1 },
    { path: "/work/other", archivedAt: 2 },
  ]),
  subscribeArchivedProjects: vi.fn(() => () => {}),
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
    expect(dialog.querySelector('[role="listbox"][aria-label="Commands"]')).toBeNull();
    expect(dialog.textContent).toContain("Type a command");
    expect(dialog.textContent).not.toContain("Reload MonoCode");
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

    inputText(input, "> arch");
    const labels = [
      ...dialog.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].map((button) => button.textContent ?? "");
    const archiveAt = labels.findIndex((label) =>
      label.includes("Archived Projects"),
    );
    const searchAt = labels.findIndex((label) => label.includes("Search"));
    expect(archiveAt).toBeGreaterThanOrEqual(0);
    expect(searchAt).toBeGreaterThan(archiveAt);
    expect(dialog.textContent).not.toContain("old");

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

  it("lists zoom and branch commands and runs the one that is chosen", () => {
    const callbacks = renderPicker(">");
    const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
    const input = dialog.querySelector<HTMLInputElement>("input")!;

    inputText(input, "> zoom");
    const zoomLabels = [
      ...dialog.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].map((button) => button.textContent ?? "");
    expect(zoomLabels.some((label) => label.includes("Zoom In"))).toBe(true);
    expect(zoomLabels.some((label) => label.includes("Zoom Out"))).toBe(true);
    const zoomIn = [
      ...dialog.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((button) => button.textContent?.includes("Zoom In"))!;
    act(() => zoomIn.click());
    expect(callbacks.onRunAction).toHaveBeenCalledExactlyOnceWith("zoom-in");

    callbacks.onRunAction.mockClear();
    callbacks.onClose.mockClear();
    inputText(input, "> branch");
    const branch = [
      ...dialog.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((button) => button.textContent?.includes("Switch Branch"))!;
    act(() => branch.click());
    expect(callbacks.onRunAction).toHaveBeenCalledExactlyOnceWith("switch-branch");
    expect(dialog.textContent).not.toContain("Search everything");
  });

  it("runs the highlighted archive row rather than the command at the old index", () => {
    const callbacks = renderPicker(">");
    const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    inputText(input, "> arch");
    const options = [
      ...dialog.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ];
    const target = options.findIndex((button) =>
      button.textContent?.includes("Archived Conversations"),
    );
    expect(target).toBeGreaterThan(0);
    for (let i = 0; i < target; i++) press(input, "ArrowDown");
    expect(
      dialog.querySelector('[role="option"][aria-selected="true"]')?.textContent,
    ).toContain("Archived Conversations");

    press(input, "Enter");
    expect(callbacks.onRunAction).not.toHaveBeenCalled();
    expect(callbacks.onClose).not.toHaveBeenCalled();
    expect(dialog.getAttribute("aria-label")).toBe("Command Palette");
    expect(dialog.textContent).toContain("No archived conversations");
  });

  it.each(["click", "Enter"] as const)(
    "runs the selected command and closes on %s",
    (method) => {
      const callbacks = renderPicker(">");
      const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
      const input = dialog.querySelector<HTMLInputElement>("input")!;
      inputText(input, ">s");
      inputText(input, ">");
      const reload = dialog.querySelector<HTMLButtonElement>(
        '[role="option"][aria-selected="true"]',
      )!;

      if (method === "click") act(() => reload.click());
      else press(input, "Enter");

      expect(callbacks.onRunAction).toHaveBeenCalledExactlyOnceWith("search");
      expect(callbacks.onClose).toHaveBeenCalledOnce();
      expect(callbacks.onOpenFile).not.toHaveBeenCalled();
    },
  );

  it("lists archived projects and restores the one that is chosen", () => {
    const callbacks = renderPicker(">");
    const onRestoreArchive = vi.fn();
    act(() =>
      root.render(
        createElement(FilePicker, {
          open: true,
          cwd: "/repo",
          initialQuery: ">",
          onOpenFile: callbacks.onOpenFile,
          onRunAction: callbacks.onRunAction,
          onRestoreArchive,
          archivedConversations: [
            {
              id: "session-1",
              title: "Pre-match message notifications",
            },
          ],
          onRestoreConversation: vi.fn(),
          onClose: callbacks.onClose,
        }),
      ),
    );
    const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    inputText(input, "> archive");
    const browse = [
      ...dialog.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((button) => button.textContent?.includes("Archived Projects"))!;
    act(() => browse.click());

    expect(callbacks.onRunAction).not.toHaveBeenCalled();
    expect(callbacks.onClose).not.toHaveBeenCalled();
    expect(dialog.getAttribute("aria-label")).toBe("Command Palette");
    expect(input.value).toBe(">");
    expect(dialog.textContent).toContain("old");
    expect(dialog.textContent).toContain("other");
    expect(dialog.textContent).not.toContain("Pre-match message notifications");

    inputText(input, "> other");
    expect(dialog.textContent).toContain("other");
    expect(dialog.textContent).not.toContain("old");

    press(input, "Enter");
    expect(onRestoreArchive).toHaveBeenCalledExactlyOnceWith("/work/other");
    expect(callbacks.onClose).toHaveBeenCalledOnce();
  });

  it("restores an archived project clicked as soon as the list opens", () => {
    const callbacks = renderPicker(">");
    const onRestoreArchive = vi.fn();
    act(() =>
      root.render(
        createElement(FilePicker, {
          open: true,
          cwd: "/repo",
          initialQuery: ">",
          onOpenFile: callbacks.onOpenFile,
          onRunAction: callbacks.onRunAction,
          onRestoreArchive,
          onClose: callbacks.onClose,
        }),
      ),
    );
    const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    inputText(input, "> archive");
    const browse = [
      ...dialog.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((button) => button.textContent?.includes("Archived Projects"))!;
    act(() => browse.click());
    const other = [
      ...dialog.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((button) => button.textContent?.includes("other"))!;
    act(() => other.click());
    expect(onRestoreArchive).toHaveBeenCalledExactlyOnceWith("/work/other");
    expect(callbacks.onClose).toHaveBeenCalledOnce();
    expect(callbacks.onRunAction).not.toHaveBeenCalled();
  });

  it("ignores an immediate second Enter so the first archive row is not restored", () => {
    const callbacks = renderPicker(">");
    const onRestoreArchive = vi.fn();
    act(() =>
      root.render(
        createElement(FilePicker, {
          open: true,
          cwd: "/repo",
          initialQuery: ">",
          onOpenFile: callbacks.onOpenFile,
          onRunAction: callbacks.onRunAction,
          onRestoreArchive,
          onClose: callbacks.onClose,
        }),
      ),
    );
    const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    inputText(input, "> archive");
    const browse = [
      ...dialog.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((button) => button.textContent?.includes("Archived Projects"))!;
    act(() => browse.click());
    press(input, "Enter");
    expect(onRestoreArchive).not.toHaveBeenCalled();
    expect(callbacks.onClose).not.toHaveBeenCalled();
  });

  it("returns from the archive list to commands when backspace clears the query", () => {
    renderPicker(">");
    const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    inputText(input, "> Archive");
    const browse = [
      ...dialog.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((button) => button.textContent?.includes("Archived Conversations"))!;
    act(() => browse.click());
    expect(dialog.getAttribute("aria-label")).toBe("Command Palette");
    expect(input.value).toBe(">");
    expect(dialog.textContent).toContain("No archived conversations");

    press(input, "Backspace");
    expect(dialog.getAttribute("aria-label")).toBe("Command Palette");
    expect(input.value).toBe(">");
    expect(dialog.textContent).toContain("Reload MonoCode");
  });

  it("lists this project's archived conversations in the palette", () => {
    const callbacks = renderPicker(">");
    const onRestoreConversation = vi.fn();
    act(() =>
      root.render(
        createElement(FilePicker, {
          open: true,
          cwd: "/repo",
          initialQuery: ">",
          onOpenFile: callbacks.onOpenFile,
          onRunAction: callbacks.onRunAction,
          onRestoreArchive: vi.fn(),
          archivedConversations: [
            {
              id: "session-1",
              title: "Pre-match message notifications",
              detail: "main",
            },
            {
              id: "session-2",
              title: "GitHub commit check",
            },
          ],
          onRestoreConversation,
          onClose: callbacks.onClose,
        }),
      ),
    );
    const dialog = document.querySelector<HTMLElement>("[data-file-picker]")!;
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    inputText(input, "> archived conversations");
    const browse = [
      ...dialog.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((button) => button.textContent?.includes("Archived Conversations"))!;
    act(() => {
      browse.click();
      input.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
      );
    });

    expect(callbacks.onRunAction).not.toHaveBeenCalled();
    expect(callbacks.onClose).not.toHaveBeenCalled();
    expect(onRestoreConversation).not.toHaveBeenCalled();
    expect(dialog.getAttribute("aria-label")).toBe("Command Palette");
    expect(input.value).toBe(">");
    expect(dialog.textContent).toContain("Pre-match message notifications");
    expect(dialog.textContent).toContain("GitHub commit check");
    expect(dialog.textContent).not.toContain("old");
    expect(dialog.textContent).not.toContain("Search everything");
    expect(
      [...dialog.querySelectorAll<HTMLButtonElement>('[role="option"]')].some(
        (button) => button.textContent === "Search" || button.textContent?.startsWith("Search"),
      ),
    ).toBe(false);

    inputText(input, "> commit");
    press(input, "Enter");
    expect(onRestoreConversation).toHaveBeenCalledExactlyOnceWith("session-2");
    expect(callbacks.onRunAction).not.toHaveBeenCalled();
  });
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
