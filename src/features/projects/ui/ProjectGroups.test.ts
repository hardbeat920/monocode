// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { pathKey } from "../../../shared/lib/paths";
import {
  loadProjectGroupAssignments,
  loadProjectGroups,
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
vi.mock("../../source-control/hooks/useProjectDiffStats", () => ({
  useProjectDiffStats: vi.fn(() => null),
}));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(useProjectDiffStats).mockClear();
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

async function renderRail(visible = true) {
  await act(async () =>
    root.render(
      createElement(ProjectRail, {
        visible,
        cwd: "/work/personal",
        recents: [
          { path: "/work/client", openedAt: 1 },
          { path: "/work/personal", openedAt: 2 },
        ],
        onSelectProject: vi.fn(),
        onOpenProject: vi.fn(),
      }),
    ),
  );
}

it("suspends project Git stats while the rail is hidden", async () => {
  await renderRail();
  expect(vi.mocked(useProjectDiffStats).mock.calls.some(([, enabled]) => enabled)).toBe(true);

  vi.mocked(useProjectDiffStats).mockClear();
  await renderRail(false);
  expect(vi.mocked(useProjectDiffStats).mock.calls.length).toBeGreaterThan(0);
  expect(vi.mocked(useProjectDiffStats).mock.calls.every(([, enabled]) => !enabled)).toBe(true);
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

it("reorders groups by dragging the group header", async () => {
  saveProjectGroups([
    { id: "clients", name: "Clients", collapsed: true },
    { id: "personal", name: "Personal", collapsed: true },
  ]);
  await renderRail();

  const first = container.querySelector<HTMLElement>(
    '[data-project-group="clients"]',
  )!;
  const second = container.querySelector<HTMLElement>(
    '[data-project-group="personal"]',
  )!;
  const header = first.querySelector(".project-reorder-item") as HTMLElement;
  const firstRect = { top: 100, bottom: 132, left: 0, right: 200, height: 32, width: 200 };
  const secondRect = { top: 140, bottom: 172, left: 0, right: 200, height: 32, width: 200 };
  vi.spyOn(first, "getBoundingClientRect").mockReturnValue(
    firstRect as DOMRect,
  );
  vi.spyOn(second, "getBoundingClientRect").mockReturnValue(
    secondRect as DOMRect,
  );

  await act(async () => {
    header.dispatchEvent(
      new PointerEvent("pointerdown", {
        bubbles: true,
        button: 0,
        clientX: 20,
        clientY: 116,
        pointerId: 1,
      }),
    );
    window.dispatchEvent(
      new PointerEvent("pointermove", {
        bubbles: true,
        clientX: 20,
        clientY: 156,
        pointerId: 1,
      }),
    );
    window.dispatchEvent(
      new PointerEvent("pointerup", {
        bubbles: true,
        clientX: 20,
        clientY: 156,
        pointerId: 1,
      }),
    );
  });

  expect(loadProjectGroups().map((group) => group.id)).toEqual([
    "personal",
    "clients",
  ]);
  const groups = [
    ...container.querySelectorAll<HTMLElement>("[data-project-group]"),
  ].map((el) => el.getAttribute("data-project-group"));
  expect(groups).toEqual(["personal", "clients"]);
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
