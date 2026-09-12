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
  await act(async () => retry!.click());
  expect(stored.sourceCwd).toBe("/work/portognjeeen");
  expect(container.textContent).not.toContain("Disk full");
});
