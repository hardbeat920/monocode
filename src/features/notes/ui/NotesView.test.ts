// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invalidateNotes, loadNotes, NOTES_CHANGED_EVENT, type Note, type NoteUpsert } from "../notes";
import { NotesView } from "./NotesView";
import { savePinnedProjects, saveProjectRailOrder } from "../../projects/model/recents";

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
  invalidateNotes();
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

function typeInto(field: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const prototype = field instanceof HTMLInputElement
    ? HTMLInputElement.prototype
    : HTMLTextAreaElement.prototype;
  Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(field, value);
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

// https://github.com/hardbeat920/monocode/issues/768
it.each([
  { title: "Untitled", body: "This is the note body.", fallback: "This is the note body." },
  { title: "Custom title", body: "Intro\n# Generated title\nBody", fallback: "Generated title" },
])("keeps a cleared $title draft until the title field blurs", async (note) => {
  vi.useFakeTimers();
  stored = { ...stored, id: `note-title-${note.title}`, title: note.title, body: note.body };
  await render();
  const title = container.querySelector<HTMLInputElement>('[aria-label="Note title"]')!;
  act(() => {
    title.focus();
    typeInto(title, "");
  });
  await act(async () => vi.advanceTimersByTime(800));

  expect(document.activeElement).toBe(title);
  expect(title.value).toBe("");
  expect(stored.title).toBe(note.title);

  await act(async () => title.blur());
  expect(title.value).toBe(note.fallback);
  expect(stored.title).toBe(note.fallback);
});

it("autosaves a replacement title without trimming the focused draft", async () => {
  vi.useFakeTimers();
  stored = { ...stored, id: "note-title-replacement" };
  await render();
  const title = container.querySelector<HTMLInputElement>('[aria-label="Note title"]')!;
  act(() => {
    title.focus();
    typeInto(title, " Replacement title ");
  });
  await act(async () => vi.advanceTimersByTime(400));

  expect(stored.title).toBe("Replacement title");
  expect(title.value).toBe(" Replacement title ");
  await act(async () => title.blur());
  expect(title.value).toBe("Replacement title");
});

it.each(["refocus", "unmount"] as const)(
  "retains a cleared title when a body save completes after %s",
  async (action) => {
    vi.useFakeTimers();
    stored = { ...stored, id: `note-title-body-save-${action}` };
    await render();
    const source = [...container.querySelectorAll<HTMLButtonElement>('[role="tab"]')]
      .find((button) => button.textContent === "Source")!;
    await act(async () => source.click());
    const body = container.querySelector<HTMLTextAreaElement>("textarea.markdown-source-field")!;
    const title = container.querySelector<HTMLInputElement>('[aria-label="Note title"]')!;
    const save = invoke.getMockImplementation()!;
    let finishSave!: () => void;
    const saving = new Promise<void>((resolve) => { finishSave = resolve; });
    let holdSave = true;
    invoke.mockImplementation(async (command, args) => {
      if (command === "notes_upsert" && holdSave) {
        holdSave = false;
        await saving;
      }
      return save(command, args);
    });
    act(() => {
      typeInto(body, "Updated body.");
      title.focus();
      typeInto(title, "");
    });
    await act(async () => vi.advanceTimersByTime(400));
    expect(invoke).toHaveBeenCalledWith("notes_upsert", {
      note: expect.objectContaining({ body: "Updated body." }),
    });
    if (action === "refocus") {
      // Blur queues a commit, then a new edit begins before the save finishes.
      await act(async () => title.blur());
      act(() => {
        title.focus();
        typeInto(title, "");
      });
    } else {
      await act(async () => root.unmount());
      root = createRoot(container);
    }
    await act(async () => finishSave());

    expect(stored.body).toBe("Updated body.");
    if (action === "refocus") {
      expect(document.activeElement).toBe(title);
      expect(title.value).toBe("");
      await act(async () => title.blur());
    }
    expect(stored.title).toBe("Updated body.");
  },
);

it("commits a cleared title when its focused editor unmounts", async () => {
  vi.useFakeTimers();
  stored = { ...stored, id: "note-title-unmount" };
  await render();
  const title = container.querySelector<HTMLInputElement>('[aria-label="Note title"]')!;
  act(() => {
    title.focus();
    typeInto(title, "");
  });
  await act(async () => root.unmount());
  root = createRoot(container);
  expect(stored.title).toBe("Keep this text.");
  expect(stored.body).toBe("Keep this text.");
});

it("shows a preloaded note immediately while refreshing in the background", async () => {
  await loadNotes();
  let finish!: (notes: Note[]) => void;
  const refresh = new Promise<Note[]>((resolve) => { finish = resolve; });
  invoke.mockReturnValue(refresh);
  await render();
  expect(container.querySelector<HTMLInputElement>('[aria-label="Note title"]')?.value)
    .toBe("Plan");
  expect(container.textContent).toContain("Keep this text.");
  expect(container.textContent).not.toContain("Select a note");
  expect(container.querySelector(".animate-spin")).toBeNull();

  await act(async () => finish([{ ...stored, title: "Updated plan" }]));
  expect(container.querySelector<HTMLInputElement>('[aria-label="Note title"]')?.value)
    .toBe("Updated plan");
});

it("refreshes an open note after an Operator write", async () => {
  await render();
  stored = { ...stored, title: "Updated by Operator", body: "New text", updatedAt: 2 };
  await act(async () => window.dispatchEvent(new Event(NOTES_CHANGED_EVENT)));
  expect(container.querySelector<HTMLInputElement>('[aria-label="Note title"]')?.value)
    .toBe("Updated by Operator");
  expect(container.textContent).toContain("New text");
});

// https://github.com/hardbeat920/monocode/issues/591
it("keeps a note's consecutive lines on their own lines", async () => {
  stored = { ...stored, body: "> first line\n> second line\n> third line" };
  await render();

  const preview = container.querySelector<HTMLElement>('[data-streamdown="blockquote"]')!;
  expect(preview.querySelector("p")?.innerHTML).toBe(
    "first line<br>second line<br>third line",
  );
});

it("uses the searchable rail project picker when moving a note", async () => {
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
  const items = [...menu.querySelectorAll<HTMLButtonElement>("button[title]")];
  expect(items.map((item) => item.title)).toEqual([
    "/work/Edefyn",
    "/work/Seventh",
    "/work/Fourth",
    "/work/Third",
    "/work/portognjeeen",
    "/work/Fifth",
    "/work/Sixth",
    "/work/Active",
  ]);
  const search = menu.querySelector<HTMLInputElement>(
    'input[placeholder="Search projects..."]',
  );
  expect(search).not.toBeNull();
  expect(document.activeElement).toBe(search);
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(search, "active");
    search!.dispatchEvent(new Event("input", { bubbles: true }));
  });
  const filteredItems = [
    ...menu.querySelectorAll<HTMLButtonElement>("button[title]"),
  ];
  expect(filteredItems.map((item) => item.title)).toEqual(["/work/Active"]);
  await act(async () => filteredItems[0]!.click());
  expect(stored.sourceCwd).toBe("/work/Active");
});

function projectButton() {
  return container.querySelector<HTMLButtonElement>(
    'header button[aria-label^="Move note to project"]',
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
    ...document.querySelectorAll<HTMLButtonElement>(
      '[aria-label="Project picker"] button[title]',
    ),
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
    ...document.querySelectorAll<HTMLButtonElement>(
      '[aria-label="Project picker"] button[title]',
    ),
  ].find((item) => item.title === "/work/Edefyn");
  expect(original).toBeDefined();
  await act(async () => original!.click());

  expect(stored.sourceCwd).toBe("/work/Edefyn");
  expect(projectButton()?.textContent).toContain("Edefyn");
  expect(container.querySelector('[role="alert"]')).toBeNull();
});

// Models the backend: a placeholder slug is replaced only on a finalizing save
// with a real title; real slugs stay stable afterwards.
const upserts = () =>
  invoke.mock.calls
    .filter(([command]) => command === "notes_upsert")
    .map(([, args]) => (args as { note: NoteUpsert }).note);

function useSlugBackend(upsertHook?: (note: NoteUpsert) => Promise<void>) {
  const slugify = (title: string) =>
    title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "untitled";
  const placeholder = (slug: string) => /^untitled(-\d+)?$/.test(slug);
  invoke.mockImplementation(async (command: string, args?: { note: NoteUpsert }) => {
    if (command === "notes_list") return [{ ...stored }];
    if (command !== "notes_upsert") throw new Error(`Unexpected command: ${command}`);
    const { finalizeSlug, ...note } = args!.note;
    await upsertHook?.(args!.note);
    const slug =
      finalizeSlug && placeholder(stored.slug) && !placeholder(slugify(note.title))
        ? slugify(note.title)
        : stored.slug;
    stored = { ...stored, ...note, slug, updatedAt: stored.updatedAt + 1 };
    return { ...stored };
  });
}

async function renderUntitled(id: string) {
  vi.useFakeTimers();
  stored = { ...stored, id, slug: "untitled", title: "Untitled", body: "" };
  useSlugBackend();
  await render();
  return container.querySelector<HTMLInputElement>('[aria-label="Note title"]')!;
}

it("keeps the placeholder slug while typing with pauses, then finalizes on blur", async () => {
  const title = await renderUntitled("note-slug-typing");
  act(() => {
    title.focus();
    typeInto(title, "This i");
  });
  await act(async () => vi.advanceTimersByTime(400));
  act(() => typeInto(title, "This is my first note"));
  await act(async () => vi.advanceTimersByTime(400));
  expect(stored.title).toBe("This is my first note");
  expect(stored.slug).toBe("untitled");
  expect(upserts().every((note) => !note.finalizeSlug)).toBe(true);

  await act(async () => title.blur());
  expect(upserts().at(-1)).toMatchObject({ finalizeSlug: true });
  expect(stored.slug).toBe("this-is-my-first-note");

  act(() => {
    title.focus();
    typeInto(title, "Renamed");
  });
  await act(async () => title.blur());
  expect(stored.title).toBe("Renamed");
  expect(stored.slug).toBe("this-is-my-first-note");
});

it("finalizes the slug when Notes closes without blurring the title", async () => {
  const title = await renderUntitled("note-slug-unmount");
  act(() => {
    title.focus();
    typeInto(title, "Closing note");
  });
  await act(async () => vi.advanceTimersByTime(400));
  expect(stored.slug).toBe("untitled");
  await act(async () => root.unmount());
  expect(upserts().at(-1)).toMatchObject({ finalizeSlug: true });
  expect(stored.slug).toBe("closing-note");
  root = createRoot(container);
});

it("keeps the finalization request when it fails so Retry still finalizes", async () => {
  const title = await renderUntitled("note-slug-retry");
  act(() => {
    title.focus();
    typeInto(title, "Retry note");
  });
  await act(async () => vi.advanceTimersByTime(400));
  expect(stored.title).toBe("Retry note");
  expect(stored.slug).toBe("untitled");

  const ok = invoke.getMockImplementation()!;
  invoke.mockImplementation(async (command: string, args?: { note: NoteUpsert }) => {
    if (command === "notes_upsert") throw new Error("Disk full");
    return ok(command, args);
  });
  await act(async () => title.blur());
  expect(container.querySelector('[role="alert"]')?.textContent).toContain("Disk full");
  expect(stored.slug).toBe("untitled");

  invoke.mockImplementation(ok);
  const retry = [...container.querySelectorAll<HTMLButtonElement>("button")]
    .find((button) => button.textContent === "Retry")!;
  await act(async () => retry.click());
  expect(upserts().at(-1)).toMatchObject({ title: "Retry note", finalizeSlug: true });
  expect(stored.slug).toBe("retry-note");
  expect(container.querySelector('[role="alert"]')).toBeNull();
});

it("does not let a queued blur finalize a newer partial title", async () => {
  vi.useFakeTimers();
  stored = { ...stored, id: "note-slug-queued", slug: "untitled", title: "Untitled", body: "" };
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  let hold = false;
  useSlugBackend(async () => {
    if (hold) {
      hold = false;
      await gate;
    }
  });
  await render();
  const title = container.querySelector<HTMLInputElement>('[aria-label="Note title"]')!;
  act(() => {
    title.focus();
    typeInto(title, "Full title");
  });
  hold = true;
  await act(async () => vi.advanceTimersByTime(400)); // in-flight save
  await act(async () => title.blur()); // queued, finalizing "Full title"
  act(() => {
    title.focus();
    typeInto(title, "Full title ext");
  });
  await act(async () => release());

  expect(stored.slug).toBe("untitled");
  expect(upserts().some((note) => note.finalizeSlug && note.title !== "Full title")).toBe(false);

  await act(async () => title.blur());
  expect(stored.slug).toBe("full-title-ext");
});

it("keeps a newer finalization request when an older finalizing save completes", async () => {
  vi.useFakeTimers();
  stored = { ...stored, id: "note-slug-stale", slug: "untitled", title: "Untitled", body: "" };
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  let hold = true;
  useSlugBackend(async () => {
    if (hold) {
      hold = false;
      await gate;
    }
  });
  await render();
  const title = container.querySelector<HTMLInputElement>('[aria-label="Note title"]')!;
  await act(async () => {
    title.focus();
    title.blur(); // finalizing save for "Untitled" held in flight
  });
  act(() => {
    title.focus();
    typeInto(title, "Real title");
  });
  await act(async () => title.blur()); // queued, finalizing "Real title"
  await act(async () => release());

  expect(upserts().at(-1)).toMatchObject({ title: "Real title", finalizeSlug: true });
  expect(stored.slug).toBe("real-title");
});
