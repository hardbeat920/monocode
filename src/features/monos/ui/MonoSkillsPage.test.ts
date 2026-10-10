// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { MonoFiles } from "../model/monoFiles";
import type { MonoSkill } from "../model/monoSkills";

const mocks = vi.hoisted(() => ({
  create: vi.fn(),
  assign: vi.fn(),
  remove: vi.fn(),
  read: vi.fn(),
  update: vi.fn(),
  list: vi.fn(),
}));
vi.mock("../model/monoSkills", () => ({
  createMonoSkill: mocks.create,
  assignMonoSkill: mocks.assign,
  removeMonoSkill: mocks.remove,
  readMonoSkill: mocks.read,
  updateMonoSkill: mocks.update,
}));
vi.mock("../../../platform/tauri/fs", () => ({ listSkills: mocks.list }));
vi.mock("../../skills/ui/SkillDocumentPreview", () => ({
  SkillDocumentPreview: ({ text }: { text: string }) =>
    createElement("p", null, text),
}));
import { MonoSkillsPage } from "./MonoSkillsPage";

let root: Root;
let container: HTMLDivElement;
const shared: MonoSkill = {
  name: "review-pr",
  description: "Review PRs",
  path: "/shared/review-pr/SKILL.md",
  hash: "h1",
  owned: false,
  available: true,
};
const files: MonoFiles = {
  id: "mono-a",
  dir: "/data/mono-a",
  soul: "",
  soulHash: "",
  memory: "",
  memoryHash: "",
  memoryPath: "",
  topics: [],
  skills: [],
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  mocks.create.mockResolvedValue(undefined);
  mocks.assign.mockResolvedValue(undefined);
  mocks.remove.mockResolvedValue(undefined);
  mocks.update.mockResolvedValue(undefined);
  mocks.read.mockResolvedValue({
    text: "Original Markdown",
    hash: "h1",
    path: shared.path,
    owned: false,
  });
  mocks.list.mockResolvedValue([
    { ...shared, scope: "user", source: "agents" },
  ]);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function render(skills: MonoSkill[] = []) {
  await act(async () =>
    root.render(
      createElement(MonoSkillsPage, {
        monoId: "mono-a",
        cwd: "/Users/me",
        projects: ["/code/app"],
        files: { ...files, skills },
        onBack: vi.fn(),
      }),
    ),
  );
}
async function click(label: string) {
  const button = [...container.querySelectorAll("button")].find(
    (item) =>
      item.getAttribute("aria-label") === label || item.textContent === label,
  );
  expect(button).toBeDefined();
  await act(async () => button!.click());
}
async function fill(label: string, text: string) {
  const input = container.querySelector('[aria-label="' + label + '"]') as
    HTMLInputElement | HTMLTextAreaElement;
  const prototype =
    input instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

it("creates a skill for the current Mono from name, trigger and instructions", async () => {
  await render();
  await click("New skill");
  await fill("Skill name", "review-pr");
  await fill("Skill description", "Review team PRs");
  await fill("Skill instructions", "Check tests and review the diff.");
  await click("Create");
  expect(mocks.create).toHaveBeenCalledWith("mono-a", {
    name: "review-pr",
    description: "Review team PRs",
    instructions: "Check tests and review the diff.",
  });
  expect(container.textContent).toContain("New skill");
});

it("assigns skills discovered in the Mono's personal and project catalogs", async () => {
  await render();
  await click("Assign");
  expect(mocks.list).toHaveBeenCalledWith("/Users/me");
  expect(mocks.list).toHaveBeenCalledWith("/code/app");
  await click("Assign skill review-pr from " + shared.path);
  expect(mocks.assign).toHaveBeenCalledWith(
    "mono-a",
    expect.objectContaining({ path: shared.path }),
  );
});

it("previews an assigned skill without exposing an editor, and can unassign it", async () => {
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
  await render([shared]);
  await click("review-prReview PRsAssigned · shared");
  expect(container.textContent).toContain("Original Markdown");
  expect(
    container.querySelector('[aria-label="Skill instructions"]'),
  ).toBeNull();
  expect(
    [...container.querySelectorAll("button")].some(
      (button) => button.textContent === "Save",
    ),
  ).toBe(false);
  act(() => root.unmount());
  root = createRoot(container);
  await render([shared]);
  await click("Unassign skill review-pr");
  expect(confirm).not.toHaveBeenCalled();
  expect(mocks.remove).toHaveBeenCalledWith("mono-a", shared);
  confirm.mockRestore();
});

it("asks before deleting an owned skill and leaves it when cancelled", async () => {
  const owned = { ...shared, owned: true };
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
  await render([owned]);
  await click("Delete skill review-pr");
  expect(confirm).toHaveBeenCalledWith(
    "Delete “review-pr”? This cannot be undone.",
  );
  expect(mocks.remove).not.toHaveBeenCalled();
  confirm.mockReturnValue(true);
  await click("Delete skill review-pr");
  expect(mocks.remove).toHaveBeenCalledWith("mono-a", owned);
  confirm.mockRestore();
});

it("preserves an owned skill's draft if its save conflicts", async () => {
  mocks.update.mockRejectedValueOnce(
    new Error("The file changed while you were editing"),
  );
  await render([{ ...shared, owned: true }]);
  await click("review-prReview PRsCreated for this Mono");
  await fill("Skill instructions", "My revised Markdown");
  await click("Save");
  expect(mocks.update).toHaveBeenCalledWith(
    "mono-a",
    "review-pr",
    "My revised Markdown",
    "h1",
  );
  expect(
    (
      container.querySelector(
        '[aria-label="Skill instructions"]',
      ) as HTMLTextAreaElement
    ).value,
  ).toBe("My revised Markdown");
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "changed",
  );
});
