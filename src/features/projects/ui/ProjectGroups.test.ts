// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { pathKey } from "../../../shared/lib/paths";
import {
  loadProjectGroupAssignments,
  loadProjectGroups,
  reorderProjectGroups,
  saveProjectGroupAssignments,
  saveProjectGroups,
} from "../model/projectGroups";
import { savePinnedProjects } from "../model/recents";
import { ProjectRail } from "../../../app/shell/ProjectRail";
import { useProjectDiffStats } from "../../source-control/hooks/useProjectDiffStats";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
  convertFileSrc: (path: string) => path,
}));
vi.mock("../model/projectGroups", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../model/projectGroups")>();
  return {
    ...actual,
    reorderProjectGroups: vi.fn(actual.reorderProjectGroups),
  };
});
vi.mock("../../source-control/hooks/useProjectDiffStats", () => ({
  useProjectDiffStats: vi.fn(() => null),
}));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(useProjectDiffStats).mockClear();
  vi.mocked(reorderProjectGroups).mockClear();
  localStorage.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

/** Renders the project rail with the mocked project groups. */
async function renderRail(
  visible = true,
  recents = [
    { path: "/work/client", openedAt: 1 },
    { path: "/work/personal", openedAt: 2 },
  ],
) {
  await act(async () =>
    root.render(
      createElement(ProjectRail, {
        visible,
        cwd: "/work/personal",
        recents,
        onSelectProject: vi.fn(),
        onOpenProject: vi.fn(),
      }),
    ),
  );
}

it("suspends project Git stats while the rail is hidden", async () => {
  await renderRail();
  expect(
    vi.mocked(useProjectDiffStats).mock.calls.some(([, enabled]) => enabled),
  ).toBe(true);

  vi.mocked(useProjectDiffStats).mockClear();
  await renderRail(false);
  expect(vi.mocked(useProjectDiffStats).mock.calls.length).toBeGreaterThan(0);
  expect(
    vi.mocked(useProjectDiffStats).mock.calls.every(([, enabled]) => !enabled),
  ).toBe(true);
  expect(container.querySelector('nav[aria-label="Projects"]')).not.toBeNull();
});

function button(label: string): HTMLButtonElement {
  const found = [
    ...document.querySelectorAll<HTMLButtonElement>("button"),
  ].find(
    (item) =>
      item.getAttribute("aria-label") === label || item.textContent === label,
  );
  expect(found, label).toBeDefined();
  return found!;
}

function sectionLabels(): string[] {
  return [...container.querySelectorAll("span")]
    .map((element) => element.textContent ?? "")
    .filter((text) => ["Pinned", "Groups", "Projects"].includes(text));
}

it("renders assigned projects in persistent collapsible groups", async () => {
  savePinnedProjects(["/work/personal"]);
  saveProjectGroups([
    { id: "clients", name: "Client work", collapsed: false, colorIndex: 4 },
  ]);
  saveProjectGroupAssignments({ [pathKey("/work/client")]: "clients" });
  await renderRail();

  expect(button("personal")).toBeDefined();
  expect(button("client")).toBeDefined();
  expect(sectionLabels()).toEqual(["Pinned", "Groups", "Projects"]);

  const group = container.querySelector<HTMLElement>(
    '[data-project-group="clients"]',
  )!;
  const groupRow = group.firstElementChild as HTMLElement;
  expect(groupRow.classList).toContain("project-reorder-item");
  expect(groupRow.classList).toContain("h-8");
  expect(groupRow.classList).toContain("px-2");
  expect(button("Client work group options").className).toBe(
    container.querySelector<HTMLButtonElement>(
      'button[aria-label="Project options"]',
    )!.className,
  );

  const header = button("Client work, 1 project");
  expect(header.getAttribute("aria-expanded")).toBe("true");
  expect(group.classList).toContain("overflow-hidden");
  expect(group.classList).toContain("rounded-md");
  expect(group.classList).toContain("bg-content/5");
  expect(group.getAttribute("style")).toBeNull();
  expect(
    group.querySelector("[data-project-group-items]")?.classList,
  ).toContain("p-1");
  expect(group.querySelector("[data-group-chevron]")).not.toBeNull();
  expect(group.querySelector("[data-group-mascot]")).toBeNull();

  act(() => header.click());
  expect(document.querySelector('button[aria-label="client"]')).toBeNull();
  expect(loadProjectGroups()[0].collapsed).toBe(true);
  const collapsedGroup = container.querySelector<HTMLElement>(
    '[data-project-group="clients"]',
  )!;
  expect(collapsedGroup.classList).not.toContain("bg-content/5");
  expect(collapsedGroup.querySelector("[data-project-group-items]")).toBeNull();
  expect(
    collapsedGroup.querySelector("[data-group-mascot]")?.classList,
  ).toContain("group-hover:hidden");
  expect(
    collapsedGroup.querySelector("[data-group-chevron]")?.classList,
  ).toContain("group-hover:block");

  act(() => root.unmount());
  root = createRoot(container);
  await renderRail();
  expect(button("Client work, 1 project").getAttribute("aria-expanded")).toBe(
    "false",
  );
  expect(document.querySelector('button[aria-label="client"]')).toBeNull();
  expect(button("personal")).toBeDefined();
});

it("creates, styles, assigns, and deletes a group from the rail", async () => {
  await renderRail();
  expect(sectionLabels()).toEqual(["Projects"]);
  expect(
    document.querySelector('button[aria-label="New project group"]'),
  ).toBeNull();

  const personal = button("personal");
  await act(async () => {
    personal.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        clientX: 20,
        clientY: 40,
      }),
    );
  });
  act(() => button("Move to group").click());
  const moveMenu = document.querySelector(
    '[role="menu"][aria-label="Move to group"]',
  )!;
  const newGroup = [...moveMenu.querySelectorAll("button")].find(
    (item) => item.textContent === "New group…",
  )!;
  act(() => newGroup.click());

  const input = document.querySelector<HTMLInputElement>(
    '[role="menu"][aria-label="Project group actions"] input[aria-label="Group name"]',
  )!;
  expect(input.value).toBe("New group");
  const setter = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )!.set!;
  act(() => {
    setter.call(input, "Side projects");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
  });
  expect(loadProjectGroups()[0].name).toBe("Side projects");
  const groupId = loadProjectGroups()[0].id;
  expect(loadProjectGroupAssignments()).toEqual({
    [pathKey("/work/personal")]: groupId,
  });
  expect(button("Side projects, 1 project")).toBeDefined();

  act(() => button("Side projects group options").click());
  act(() => button("Mascot ghost").click());
  expect(loadProjectGroups()[0].mascot).toBe("ghost");
  act(() => button("Delete group").click());
  expect(loadProjectGroups()).toEqual([]);
  expect(loadProjectGroupAssignments()).toEqual({});
  expect(button("personal")).toBeDefined();
  expect(sectionLabels()).toEqual(["Projects"]);
  expect(
    document.querySelector('button[aria-label="New project group"]'),
  ).toBeNull();
});

describe("dragging groups of different heights", () => {
  const groupTop = 100;
  const tops = new Map<string, number>();

  // happy-dom does not lay out CSS, so stand in for the rail's spacing
  // contract: groups stack in a `gap-px` column and expanded groups add
  // `mb-1.5`. Heights and margins come from the rendered markup, so a change
  // to either side of that contract shows up here.
  /** Stubs measured rects so groups stack with their expanded/collapsed heights. */
  function layoutGroups() {
    const groups = [
      ...container.querySelectorAll<HTMLElement>("[data-project-group]"),
    ];
    const gap = groups[0].parentElement!.classList.contains("gap-px") ? 1 : 0;
    let top = groupTop;
    tops.clear();
    for (const group of groups) {
      const rows = group.querySelectorAll(
        "[data-project-group-items] > *",
      ).length;
      const height = 32 + (rows ? 8 + rows * 33 - 1 : 0);
      const marginBottom = group.classList.contains("mb-1.5") ? 6 : 0;
      const groupStart = top;
      tops.set(group.dataset.projectGroup!, groupStart);
      group.getBoundingClientRect = () =>
        new DOMRect(0, groupStart, 200, height);
      const captured = new Set<number>();
      group.setPointerCapture = (pointerId) => void captured.add(pointerId);
      group.hasPointerCapture = (pointerId) => captured.has(pointerId);
      group.releasePointerCapture = (pointerId) =>
        void captured.delete(pointerId);
      top += height + marginBottom + gap;
    }
    return groups;
  }

  /** The group element for `id`. */
  function group(id: string) {
    return container.querySelector<HTMLElement>(
      `[data-project-group="${id}"]`,
    )!;
  }

  /** Current translateY, in pixels, of the group element for `id`. */
  function translateY(id: string) {
    const match = /translate3d\(0, (-?[\d.]+)px, 0\)/.exec(
      group(id).style.transform,
    );
    return match ? Number(match[1]) : 0;
  }

  /** Dispatches a pointer event at vertical position `clientY`. */
  function pointer(target: EventTarget, type: string, clientY: number) {
    act(() => {
      target.dispatchEvent(
        new PointerEvent(type, {
          bubbles: true,
          button: 0,
          pointerId: 1,
          clientX: 20,
          clientY,
        }),
      );
    });
  }

  beforeEach(async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    vi.spyOn(window, "requestAnimationFrame").mockImplementation(
      (callback) => setTimeout(() => callback(0), 16) as unknown as number,
    );
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((frame) =>
      clearTimeout(frame),
    );
    const getComputedStyle = window.getComputedStyle.bind(window);
    vi.spyOn(window, "getComputedStyle").mockImplementation((element) => {
      const style = getComputedStyle(element);
      if (!(element as HTMLElement).dataset?.projectGroup) return style;
      const marginBottom = element.classList.contains("mb-1.5") ? "6px" : "0px";
      return {
        marginTop: "0px",
        marginBottom,
        getPropertyValue: (name: string) => style.getPropertyValue(name),
      } as CSSStyleDeclaration;
    });
    document.documentElement.style.setProperty(
      "--motion-reorder-duration",
      "160ms",
    );
    saveProjectGroups([
      { id: "a", name: "Alpha", collapsed: false },
      { id: "b", name: "Beta", collapsed: true },
    ]);
    saveProjectGroupAssignments({
      [pathKey("/work/a1")]: "a",
      [pathKey("/work/a2")]: "a",
      [pathKey("/work/a3")]: "a",
      [pathKey("/work/b1")]: "b",
    });
    await renderRail(
      true,
      ["/work/a1", "/work/a2", "/work/a3", "/work/b1"].map((path, index) => ({
        path,
        openedAt: index + 1,
      })),
    );
    layoutGroups();
  });

  afterEach(() => {
    document.documentElement.style.removeProperty("--motion-reorder-duration");
    vi.clearAllTimers();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  /** Presses the group header of `id` and moves it `by` pixels; returns the pointer position. */
  function drag(id: string, by: number) {
    const start = tops.get(id)! + 16;
    pointer(group(id).firstElementChild!, "pointerdown", start);
    pointer(window, "pointermove", start + by);
    act(() => vi.advanceTimersByTime(16));
    return start + by;
  }

  it("uses the expanded/collapsed spacing contract", () => {
    expect(group("a").classList).toContain("mb-1.5");
    expect(group("b").classList).not.toContain("mb-1.5");
    expect(group("a").parentElement!.classList).toContain("gap-px");
  });

  it("moves a tall expanded group below a short collapsed one", () => {
    const end = drag("a", 400);
    expect(translateY("b")).toBe(groupTop - tops.get("b")!);
    pointer(window, "pointerup", end);
    act(() => vi.runAllTimers());

    expect(reorderProjectGroups).toHaveBeenCalledExactlyOnceWith(
      ["b", "a"],
      "a",
    );
    expect(loadProjectGroups().map(({ id }) => id)).toEqual(["b", "a"]);
    expect(
      [...container.querySelectorAll("[data-project-group]")].map(
        (element) => (element as HTMLElement).dataset.projectGroup,
      ),
    ).toEqual(["b", "a"]);
  });

  it("previews and settles a collapsed last group where the commit puts it", () => {
    const before = new Map(tops);
    const end = drag("b", -400);
    const previewTops = () =>
      new Map(["a", "b"].map((id) => [id, before.get(id)! + translateY(id)]));
    const preview = previewTops();
    // Pausing does not shift the preview.
    act(() => vi.advanceTimersByTime(500));
    expect(previewTops()).toEqual(preview);
    pointer(window, "pointerup", end);
    const settled = previewTops();
    act(() => vi.runAllTimers());

    expect(reorderProjectGroups).toHaveBeenCalledExactlyOnceWith(
      ["b", "a"],
      "b",
    );
    expect(loadProjectGroups().map(({ id }) => id)).toEqual(["b", "a"]);
    expect(group("a").style.transform).toBe("");
    expect(group("b").style.transform).toBe("");
    layoutGroups();
    expect(preview).toEqual(tops);
    expect(settled).toEqual(tops);
  });
});
