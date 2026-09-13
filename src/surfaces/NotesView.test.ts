// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Note, NoteUpsert } from "../lib/notes";
import { NotesView } from "./NotesView";
import { savePinnedProjects, saveProjectRailOrder } from "../lib/recents";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => false,
    onResized: async () => () => {},
  }),
}));

let root: Root;
let container: HTMLDivElement;
let stored: Note;
let onClose: ReturnType<typeof vi.fn>;
const recents = [
  { path: "/work/Edefyn", openedAt: 2 },
  { path: "/work/portognjeeen", openedAt: 1 },
];

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const storage = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
    removeItem: (key: string) => storage.delete(key),
  });
  stored = {
    id: "note-project-test",
    slug: "plan",
    title: "Plan",
    body: "Keep this text.",
    tags: ["ideas"],
    sourceCwd: "/work/Edefyn",
    sourceSessionId: "original-session",
    createdAt: 1,
    updatedAt: 1,
  };
  invoke.mockReset();
  invoke.mockImplementation(
    async (command: string, args?: { note: NoteUpsert }) => {
      if (command === "notes_list") return [{ ...stored }];
      if (command === "notes_upsert") {
        stored = { ...stored, ...args!.note, updatedAt: stored.updatedAt + 1 };
        return { ...stored };
      }
      throw new Error(`Unexpected command: ${command}`);
    },
  );
  onClose = vi.fn();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

async function render(projects = recents, cwd = "/work/Edefyn") {
  await act(async () =>
    root.render(
      createElement(NotesView, {
        cwd,
        recents: projects,
        onClose,
      }),
    ),
  );
}

it("offers all other rail projects in rail order in one move menu", async () => {
  const projects = [
    ...recents,
    { path: "/work/Third", openedAt: 3 },
    { path: "/work/Fourth", openedAt: 4 },
    { path: "/work/Fifth", openedAt: 5 },
    { path: "/work/Sixth", openedAt: 6 },
    { path: "/work/Seventh", openedAt: 7 },
  ];
  saveProjectRailOrder([
    "/work/Fourth",
    "/work/Third",
    "/work/Edefyn",
    "/work/portognjeeen",
    "/work/Fifth",
    "/work/Sixth",
    "/work/Seventh",
    "/work/Active",
  ]);
  savePinnedProjects(["/work/Seventh"]);
  await render(projects, "/work/Active");
  await act(async () => projectButton()!.click());
  const menu = document.querySelector('[aria-label="Project picker"]')!;
  const items = [
    ...menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
  ];
  expect(items.map((item) => item.title)).toEqual([
    "/work/Seventh",
    "/work/Fourth",
    "/work/Third",
    "/work/portognjeeen",
    "/work/Fifth",
    "/work/Sixth",
    "/work/Active",
  ]);
  expect(menu.textContent).toContain("Move to project");
  expect(menu.textContent).not.toContain("Recent projects");
  expect(menu.textContent).not.toContain("More Projects");
  await act(async () => items.at(-1)!.click());
  expect(stored.sourceCwd).toBe("/work/Active");
});

function projectButton() {
  return container.querySelector<HTMLButtonElement>(
    'header button[aria-haspopup="menu"]',
  );
}

async function chooseProject() {
  const button = projectButton();
  expect(
    button,
    "The note header should offer a project picker",
  ).not.toBeNull();
  await act(async () => button!.click());
  const item = [
    ...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
  ].find((element) => element.title === "/work/portognjeeen");
  expect(item).toBeDefined();
  await act(async () => item!.click());
}

it("moves the existing note and keeps its content when reopened", async () => {
  await render();
  await chooseProject();
  expect(stored).toMatchObject({
    id: "note-project-test",
    slug: "plan",
    title: "Plan",
    body: "Keep this text.",
    tags: ["ideas"],
    sourceCwd: "/work/portognjeeen",
    sourceSessionId: "original-session",
    createdAt: 1,
  });
  expect(projectButton()?.textContent).toContain("portognjeeen");
  expect(
    container.querySelector('li [aria-current="true"]')?.textContent,
  ).toContain("portognjeeen");
  expect(onClose).not.toHaveBeenCalled();
  await act(async () => root.unmount());
  root = createRoot(container);
  await render();
  expect(projectButton()?.textContent).toContain("portognjeeen");
});

it("closes the project menu with Escape without leaving Notes", async () => {
  await render();
  await act(async () => projectButton()!.click());
  expect(
    document.querySelector('[aria-label="Project picker"]'),
  ).not.toBeNull();
  await act(async () =>
    projectButton()!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(document.querySelector('[aria-label="Project picker"]')).toBeNull();
  expect(onClose).not.toHaveBeenCalled();
});

it("serializes title edits behind an in-flight project change", async () => {
  await render();
  const save = invoke.getMockImplementation()!;
  let finishMove!: () => void;
  const moving = new Promise<void>((resolve) => {
    finishMove = resolve;
  });
  let inFlight = 0;
  let maximumInFlight = 0;
  invoke.mockImplementation(async (command, args) => {
    if (command !== "notes_upsert") return save(command, args);
    inFlight += 1;
    maximumInFlight = Math.max(maximumInFlight, inFlight);
    if (args.note.title === "Plan") await moving;
    const result = await save(command, args);
    inFlight -= 1;
    return result;
  });
  await chooseProject();
  const title = container.querySelector<HTMLInputElement>(
    '[aria-label="Note title"]',
  )!;
  act(() => title.focus());
  act(() => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(title, "Updated plan");
    title.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => title.blur());
  await act(async () => finishMove());
  expect(maximumInFlight).toBe(1);
  expect(stored.title).toBe("Updated plan");
  expect(stored.sourceCwd).toBe("/work/portognjeeen");
});

it("keeps newer edits after reopening a note during a project move", async () => {
  const second = {
    ...stored,
    id: "second-note",
    slug: "second",
    title: "Second",
  };
  const save = invoke.getMockImplementation()!;
  let finishMove!: () => void;
  const moving = new Promise<void>((resolve) => {
    finishMove = resolve;
  });
  let inFlight = 0;
  let maximumInFlight = 0;
  invoke.mockImplementation(async (command, args) => {
    if (command === "notes_list") return [{ ...stored }, { ...second }];
    if (command !== "notes_upsert") return save(command, args);
    inFlight += 1;
    maximumInFlight = Math.max(maximumInFlight, inFlight);
    if (args.note.sourceCwd === "/work/portognjeeen") await moving;
    const result = await save(command, args);
    inFlight -= 1;
    return result;
  });
  const selectNote = async (title: string) => {
    const button = [
      ...container.querySelectorAll<HTMLButtonElement>("li button"),
    ].find((item) => item.textContent?.includes(title));
    expect(button).toBeDefined();
    await act(async () => button!.click());
  };

  await render();
  await chooseProject();
  await selectNote("Second");
  await selectNote("Plan");
  const title = container.querySelector<HTMLInputElement>(
    '[aria-label="Note title"]',
  )!;
  act(() => title.focus());
  act(() => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(title, "Updated after reopening");
    title.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => title.blur());
  await act(async () => finishMove());

  expect(stored.title).toBe("Updated after reopening");
  expect(stored.sourceCwd).toBe("/work/portognjeeen");
  expect(maximumInFlight).toBe(1);
  await selectNote("Second");
  await selectNote("Updated after reopening");
  expect(
    container.querySelector<HTMLInputElement>('[aria-label="Note title"]')
      ?.value,
  ).toBe("Updated after reopening");
  expect(projectButton()?.textContent).toContain("portognjeeen");
});

it.each(["title", "body", "tags"] as const)(
  "preserves earlier edits when changing %s after reopening during a move",
  async (field) => {
    vi.useFakeTimers();
    const second = { ...stored, id: "second-note", title: "Second" };
    const save = invoke.getMockImplementation()!;
    let finishMove!: () => void;
    const moving = new Promise<void>((resolve) => {
      finishMove = resolve;
    });
    invoke.mockImplementation(async (command, args) => {
      if (command === "notes_list") return [{ ...stored }, { ...second }];
      if (
        command === "notes_upsert" &&
        args.note.sourceCwd === "/work/portognjeeen"
      )
        await moving;
      return save(command, args);
    });
    const selectNote = async (title: string) => {
      const button = [
        ...container.querySelectorAll<HTMLButtonElement>("li button"),
      ].find((item) => item.textContent?.includes(title));
      expect(button).toBeDefined();
      await act(async () => button!.click());
    };
    const editInput = async (label: string, value: string) => {
      const input = container.querySelector<HTMLInputElement>(
        `[aria-label="${label}"]`,
      )!;
      await act(async () => {
        Object.getOwnPropertyDescriptor(
          HTMLInputElement.prototype,
          "value",
        )!.set!.call(input, value);
        input.dispatchEvent(new Event("input", { bubbles: true }));
      });
    };
    const editBody = async (value: string) => {
      const source = [
        ...container.querySelectorAll<HTMLButtonElement>('[role="tab"]'),
      ].find((button) => button.textContent === "Source")!;
      await act(async () => source.click());
      const input = container.querySelector<HTMLTextAreaElement>(
        "textarea.markdown-source-field",
      )!;
      await act(async () => {
        Object.getOwnPropertyDescriptor(
          HTMLTextAreaElement.prototype,
          "value",
        )!.set!.call(input, value);
        input.dispatchEvent(new Event("input", { bubbles: true }));
      });
    };

    await render();
    await selectNote("Plan");
    await editInput("Note title", "Title before moving");
    await editBody("Content before moving");
    await editInput("Add note tag", "before,");
    await chooseProject();
    await selectNote("Second");
    await selectNote("Plan");
    if (field === "title")
      await editInput("Note title", "Title after reopening");
    if (field === "body") await editBody("Content after reopening");
    if (field === "tags") await editInput("Add note tag", "after,");
    await act(async () => vi.advanceTimersByTime(400));
    // Cover both an open editor and saves finishing after it unmounts.
    if (field !== "title") await selectNote("Second");
    await act(async () => finishMove());

    const expected = {
      title:
        field === "title" ? "Title after reopening" : "Title before moving",
      body:
        field === "body" ? "Content after reopening" : "Content before moving",
      tags: field === "tags" ? ["ideas", "after"] : ["ideas", "before"],
      sourceCwd: "/work/portognjeeen",
    };
    expect(stored).toMatchObject(expected);
    await selectNote(expected.title);
    expect(
      container.querySelector<HTMLInputElement>('[aria-label="Note title"]')
        ?.value,
    ).toBe(expected.title);
    expect(
      container.querySelector<HTMLTextAreaElement>(
        "textarea.markdown-source-field",
      )?.value,
    ).toBe(expected.body);
    expect(
      [...container.querySelectorAll('[aria-label="Tags"] span')].map(
        (tag) => tag.textContent,
      ),
    ).toEqual(expect.arrayContaining(expected.tags.map((tag) => `#${tag}`)));
    expect(projectButton()?.textContent).toContain("portognjeeen");
  },
);

it("lets the user retry a project change after saving fails", async () => {
  await render();
  invoke.mockRejectedValueOnce(new Error("Disk full"));
  await chooseProject();
  expect(stored.sourceCwd).toBe("/work/Edefyn");
  expect(container.textContent).toContain("Disk full");
  const retry = [
    ...container.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) => button.textContent === "Retry");
  expect(retry, "An unsaved project choice needs a retry action").toBeDefined();
  let failRetry!: (error: Error) => void;
  const retrying = new Promise<never>((_, reject) => {
    failRetry = reject;
  });
  invoke.mockImplementationOnce(() => retrying);
  await act(async () => retry!.click());
  expect(container.querySelector('[role="alert"]')).toBeNull();
  expect(stored.sourceCwd).toBe("/work/Edefyn");

  await act(async () => failRetry(new Error("Still no space")));
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "Still no space",
  );
  const nextRetry = [
    ...container.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) => button.textContent === "Retry");
  expect(nextRetry).toBeDefined();
  await act(async () => nextRetry!.click());
  expect(stored.sourceCwd).toBe("/work/portognjeeen");
  expect(container.querySelector('[role="alert"]')).toBeNull();
});

it("keeps a completed move after returning to the note while it saves", async () => {
  const second = {
    ...stored,
    id: "second-note",
    slug: "second",
    title: "Second",
  };
  const save = invoke.getMockImplementation()!;
  let finishMove!: () => void;
  const moving = new Promise<void>((resolve) => {
    finishMove = resolve;
  });
  invoke.mockImplementation(async (command, args) => {
    if (command === "notes_list") return [{ ...stored }, { ...second }];
    if (
      command === "notes_upsert" &&
      args.note.sourceCwd === "/work/portognjeeen"
    )
      await moving;
    return save(command, args);
  });
  const selectNote = async (title: string) => {
    const button = [
      ...container.querySelectorAll<HTMLButtonElement>("li button"),
    ].find((item) => item.textContent?.includes(title));
    expect(button).toBeDefined();
    await act(async () => button!.click());
  };

  await render();
  await chooseProject();
  await selectNote("Second");
  await selectNote("Plan");
  await act(async () => finishMove());
  expect(stored.sourceCwd).toBe("/work/portognjeeen");
  expect(projectButton()?.textContent).toContain("portognjeeen");
  await selectNote("Second");
  expect(stored.sourceCwd).toBe("/work/portognjeeen");
});

it("clears a failed move error when the saved project is selected again", async () => {
  await render();
  invoke.mockRejectedValueOnce(new Error("Disk full"));
  await chooseProject();
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "Disk full",
  );

  await act(async () => projectButton()!.click());
  const original = [
    ...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
  ].find((item) => item.title === "/work/Edefyn");
  expect(original).toBeDefined();
  await act(async () => original!.click());

  expect(stored.sourceCwd).toBe("/work/Edefyn");
  expect(projectButton()?.textContent).toContain("Edefyn");
  expect(container.querySelector('[role="alert"]')).toBeNull();
});
