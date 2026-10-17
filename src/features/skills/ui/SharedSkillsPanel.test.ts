// @vitest-environment happy-dom
import { act, createElement, Profiler } from "react";
import { createRoot, type Root } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SharedSkillsPanel } from "./SharedSkillsPanel";
import type {
  SharedSkillEntry,
  SharedSkillsSnapshot,
} from "../../../platform/tauri/sharedSkills";
import { invalidateSkills, SKILLS_CHANGE_EVENT } from "../model/skills";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn() }));
vi.mock("../model/skills", () => ({
  invalidateSkills: vi.fn(),
  SKILLS_CHANGE_EVENT: "monocode:skills-change",
}));

const review: SharedSkillEntry = {
  id: "review-id",
  name: "review",
  description: "Review a change.",
  digest: "digest-1",
  revision: 1,
  shared: true,
  sourcePath: "/data/skills/sources/review-id",
  previewPath: "/data/skills/objects/digest-1/SKILL.md",
  origins: ["/import/review"],
  statuses: [
    {
      targetKey: "agents",
      providers: ["codex", "pi"],
      path: "/home/.agents/skills/review",
      state: "exported",
      detail: "Owned copy matches the applied skill.",
    },
    {
      targetKey: "claude",
      providers: ["claude"],
      path: "/home/.claude/skills/review",
      state: "conflict",
      detail: "An unmanaged skill already exists. Its files were preserved.",
    },
  ],
  warnings: [],
};
const plan: SharedSkillEntry = {
  ...review,
  id: "plan-id",
  name: "plan",
  sourcePath: "/data/skills/sources/plan-id",
  previewPath: "/data/skills/objects/digest-plan/SKILL.md",
};
const initial: SharedSkillsSnapshot = {
  generation: 1,
  entries: [review],
  targets: [],
};
let container: HTMLDivElement;
let root: Root;

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function button(label: string): HTMLButtonElement {
  const found = [
    ...container.querySelectorAll<HTMLButtonElement>("button"),
  ].find(
    (target) =>
      target.getAttribute("aria-label") === label ||
      target.textContent === label,
  );
  expect(found, `Button: ${label}`).toBeDefined();
  return found!;
}

async function click(label: string) {
  await act(async () => button(label).click());
}

async function render() {
  await act(async () => root.render(createElement(SharedSkillsPanel)));
}

function setPath(path: string) {
  const input = container.querySelector<HTMLInputElement>(
    '[aria-label="Skill folder path"]',
  )!;
  act(() => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(input, path);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "shared_skills_snapshot") return initial;
    if (command === "read_text_file") return "# Applied review instructions";
    throw new Error(`Unexpected command: ${command}`);
  });
  vi.mocked(open).mockReset();
  vi.mocked(revealItemInDir).mockReset();
  vi.mocked(revealItemInDir).mockResolvedValue(undefined);
  vi.mocked(invalidateSkills).mockClear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("Shared skill library", () => {
  it("shows preserved conflicts and reads the applied snapshot instead of the editable source", async () => {
    await render();
    expect(container.textContent).toContain("1 conflict");
    expect(container.textContent).toContain("Its files were preserved.");
    expect(container.textContent).toContain(
      "Running sessions may need a restart",
    );
    expect(invoke).toHaveBeenCalledWith("read_text_file", {
      path: review.previewPath,
    });
    await click("Open source folder");
    expect(revealItemInDir).toHaveBeenCalledWith(review.sourcePath);
  });

  it("imports a path and invalidates composer catalogs after the mutation succeeds", async () => {
    const changed = vi.fn();
    window.addEventListener(SKILLS_CHANGE_EVENT, changed);
    await render();
    const pending = deferred<SharedSkillsSnapshot>();
    vi.mocked(invoke).mockImplementationOnce(() => pending.promise);
    setPath(" /import/new-skill ");
    await click("Import path");
    expect(invoke).toHaveBeenLastCalledWith("shared_skills_import", {
      path: "/import/new-skill",
    });
    expect(button("Repair sharing").disabled).toBe(true);
    expect(invalidateSkills).not.toHaveBeenCalled();
    await act(async () =>
      pending.resolve({ generation: 2, entries: [review, plan], targets: [] }),
    );
    expect(container.textContent).toContain("plan");
    expect(invalidateSkills).toHaveBeenCalledOnce();
    expect(changed).toHaveBeenCalledOnce();
    window.removeEventListener(SKILLS_CHANGE_EVENT, changed);
  });

  it("imports the native folder choice and treats cancellation as no change", async () => {
    await render();
    vi.mocked(open).mockResolvedValueOnce(null);
    await click("Choose folder");
    expect(open).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
      title: "Import skill folder",
    });
    expect(invalidateSkills).not.toHaveBeenCalled();
    vi.mocked(open).mockResolvedValueOnce("/chosen/review");
    vi.mocked(invoke).mockResolvedValueOnce({ ...initial, generation: 2 });
    await click("Choose folder");
    expect(invoke).toHaveBeenLastCalledWith("shared_skills_import", {
      path: "/chosen/review",
    });
    expect(container.querySelector<HTMLInputElement>("input")?.value).toBe(
      "/chosen/review",
    );
  });

  it("keeps sharing enabled on failure, then retries and applies edited source", async () => {
    await render();
    vi.mocked(invoke).mockRejectedValueOnce(
      new Error("The destination is locked."),
    );
    await click("Stop sharing");
    expect(container.querySelector('[role="alert"]')?.textContent).toContain(
      "destination is locked",
    );
    expect(
      button("Share review across providers").getAttribute("aria-checked"),
    ).toBe("true");
    expect(invalidateSkills).not.toHaveBeenCalled();
    vi.mocked(invoke).mockResolvedValueOnce({
      generation: 2,
      entries: [{ ...review, shared: false }],
      targets: [],
    });
    await click("Stop sharing");
    expect(invoke).toHaveBeenLastCalledWith("shared_skills_set_shared", {
      id: review.id,
      shared: false,
    });
    expect(
      button("Share review across providers").getAttribute("aria-checked"),
    ).toBe("false");
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "shared_skills_apply")
        return {
          generation: 3,
          entries: [
            {
              ...review,
              shared: false,
              revision: 2,
              previewPath: "/data/skills/objects/digest-2/SKILL.md",
            },
          ],
          targets: [],
        };
      if (command === "read_text_file") return "# Applied revision two";
      throw new Error(`Unexpected command: ${command}`);
    });
    await click("Apply edits");
    expect(invoke).toHaveBeenCalledWith("shared_skills_apply", {
      id: review.id,
    });
    expect(container.textContent).toContain("Revision 2");
    expect(container.textContent).toContain("Applied revision two");
  });

  it("repairs sharing and ignores an older snapshot generation", async () => {
    await render();
    vi.mocked(invoke).mockResolvedValueOnce({
      generation: 3,
      entries: [{ ...review, revision: 3 }],
      targets: [],
    });
    await click("Repair sharing");
    expect(invoke).toHaveBeenCalledWith("shared_skills_repair");
    expect(container.textContent).toContain("Revision 3");
    vi.mocked(invoke).mockResolvedValueOnce({
      generation: 2,
      entries: [plan],
      targets: [],
    });
    await click("Refresh library");
    expect(container.textContent).toContain("Revision 3");
    expect(button("Share review across providers")).toBeDefined();
    expect(
      container.querySelector('[aria-label="Applied instructions for review"]'),
    ).not.toBeNull();
  });

  it("never renders an old applied document under a new selection while its read is pending", async () => {
    const pending = deferred<string>();
    const frames: string[] = [];
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "shared_skills_snapshot")
        return { ...initial, entries: [review, plan] };
      if (command === "read_text_file")
        return args?.path === plan.previewPath
          ? pending.promise
          : "# Previous review document";
      throw new Error(`Unexpected command: ${command}`);
    });
    await act(async () =>
      root.render(
        createElement(
          Profiler,
          {
            id: "shared-document",
            onRender: () => {
              const document = container.querySelector(
                '[aria-label="Applied instructions for plan"]',
              );
              if (document) frames.push(document.textContent ?? "");
            },
          },
          createElement(SharedSkillsPanel),
        ),
      ),
    );
    await click("Select shared skill plan");
    expect(frames.length).toBeGreaterThan(0);
    for (const frame of frames) {
      expect(frame).not.toContain("Previous review document");
      expect(frame).toContain("Loading applied instructions");
    }
    await act(async () => pending.resolve("# Selected plan document"));
    expect(container.textContent).toContain("Selected plan document");
  });

  it("invalidates catalogs after a completed mutation even when Settings has closed", async () => {
    await render();
    const pending = deferred<SharedSkillsSnapshot>();
    vi.mocked(invoke).mockImplementationOnce(() => pending.promise);
    await click("Repair sharing");
    await act(async () => root.render(null));
    await act(async () => pending.resolve({ ...initial, generation: 2 }));
    expect(invalidateSkills).toHaveBeenCalledOnce();
    expect(container.textContent).toBe("");
  });

  it("recovers from a failed initial read through Refresh library", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Registry unavailable"));
    await render();
    expect(container.querySelector('[role="alert"]')?.textContent).toContain(
      "Registry unavailable",
    );
    expect(button("Refresh library").disabled).toBe(false);
    await click("Refresh library");
    expect(container.textContent).toContain("review");
    expect(container.querySelector('[role="alert"]')).toBeNull();
  });
});
