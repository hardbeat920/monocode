// @vitest-environment happy-dom
import { act, createElement, Profiler } from "react";
import { createRoot, type Root } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SkillsPage } from "./SkillsPage";
import { SettingsView } from "./SettingsView";
import type { DiscoveredSkill } from "../lib/fs";
import { loadDisabledSkillPaths, saveDisabledSkillPaths } from "../lib/skills";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => false,
    onResized: async () => () => {},
  }),
}));

const skills: DiscoveredSkill[] = [
  {
    name: "Project guide",
    description: "Project instructions",
    path: "D:/repo/.agents/skills/guide/SKILL.md",
    scope: "project",
    source: "agents",
  },
  {
    name: "Personal guide",
    description: "Personal instructions",
    path: "C:/Users/test/.agents/skills/guide/SKILL.md",
    scope: "user",
    source: "agents",
  },
  {
    name: "Other guide",
    description: "Harness instructions",
    path: "C:/Users/test/.claude/skills/guide/SKILL.md",
    scope: "user",
    source: "claude",
  },
];
const markdown =
  "---\nname: guide\n---\n\n# Full instructions\n\nRead **everything**.\n\nLast paragraph.\n";
let container: HTMLDivElement;
let root: Root;

function button(
  label: string,
  scope: ParentNode = document,
): HTMLButtonElement {
  const found = Array.from(
    scope.querySelectorAll<HTMLButtonElement>("button"),
  ).find(
    (item) =>
      item.getAttribute("aria-label") === label || item.textContent === label,
  );
  expect(found, `Button: ${label}`).toBeDefined();
  return found!;
}

async function click(label: string) {
  const target = button(label);
  await act(async () => {
    target.focus();
    target.click();
  });
  return target;
}

async function render() {
  await act(async () =>
    root.render(createElement(SkillsPage, { cwd: "D:/repo" })),
  );
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "list_skills") return skills;
    if (command === "read_text_file") return markdown;
    throw new Error(`Unexpected command: ${command}`);
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  localStorage.clear();
  vi.unstubAllGlobals();
});

describe("Settings skill preview", () => {
  it.each(["document", "error"])(
    "never shows the previous %s under a newly selected skill while its read is pending",
    async (previousState) => {
      const pending = deferred<string>();
      const frames: string[] = [];
      await act(async () =>
        root.render(
          createElement(
            Profiler,
            {
              id: "skill-preview",
              // Observe committed DOM before passive effects clear stale state.
              onRender: () => {
                const panel = container.querySelector(
                  '[aria-label="Skill preview"]',
                );
                if (panel?.querySelector("h2")?.textContent === "Other guide") {
                  frames.push(panel.textContent ?? "");
                }
              },
            },
            createElement(SkillsPage, { cwd: "D:/repo" }),
          ),
        ),
      );
      if (previousState === "document") {
        vi.mocked(invoke).mockResolvedValueOnce("# Previous document");
      } else {
        vi.mocked(invoke).mockRejectedValueOnce(
          new Error("Previous read failed"),
        );
      }
      await click("Project guide");
      const previousText =
        previousState === "document"
          ? "Previous document"
          : "Previous read failed";
      expect(
        container.querySelector('[aria-label="Skill preview"]')?.textContent,
      ).toContain(previousText);
      vi.mocked(invoke).mockImplementationOnce(() => pending.promise);
      await click("Other guide");
      expect(frames.length).toBeGreaterThan(0);
      for (const frame of frames) {
        expect(frame).not.toContain(previousText);
        expect(frame).toContain("Loading skill");
      }
      await act(async () => pending.resolve("# Selected document"));
      expect(
        container.querySelector('[aria-label="Skill preview"] h1')?.textContent,
      ).toBe("Selected document");
    },
  );

  it.each(["Project guide", "Preview skill Project guide"])(
    "restores focus to %s after refreshing the list",
    async (label) => {
      await render();
      const opener = await click(label);
      const reload = deferred<DiscoveredSkill[]>();
      vi.mocked(invoke).mockImplementationOnce(() => reload.promise);
      await click("Refresh skills");
      expect(opener.isConnected).toBe(false);
      await act(async () => reload.resolve(skills));
      const replacement = button(label);
      await click("Close skill preview");
      expect(document.activeElement === replacement).toBe(true);
    },
  );

  it("returns focus to the filter if the preview opener disappears after refresh", async () => {
    await render();
    await click("Project guide");
    vi.mocked(invoke).mockResolvedValueOnce([skills[2]]);
    await click("Refresh skills");
    await click("Close skill preview");
    expect(document.activeElement?.getAttribute("aria-label")).toBe(
      "Filter skills",
    );
  });

  it("lets Escape cancel Add skill before closing the preview or Settings", async () => {
    const closeSettings = vi.fn();
    await act(async () =>
      root.render(
        createElement(SettingsView, {
          section: "skills",
          cwd: "D:/repo",
          sessions: [],
          onClose: closeSettings,
          onOpenSession: vi.fn(),
          onArchiveSession: vi.fn(),
          onDeleteSession: vi.fn(),
          onOpenWhatsNew: vi.fn(),
        }),
      ),
    );
    await click("Project guide");
    await click("Add skill");
    expect(document.activeElement?.getAttribute("aria-label")).toBe(
      "Skill name",
    );
    const escape = () =>
      act(() =>
        document.activeElement!.dispatchEvent(
          new KeyboardEvent("keydown", {
            key: "Escape",
            bubbles: true,
            cancelable: true,
          }),
        ),
      );
    escape();
    expect(container.querySelector("form")).toBeNull();
    expect(
      container.querySelector('[aria-label="Skill preview"]'),
    ).not.toBeNull();
    expect(document.activeElement === button("Add skill")).toBe(true);
    expect(closeSettings).not.toHaveBeenCalled();
    escape();
    expect(container.querySelector('[aria-label="Skill preview"]')).toBeNull();
    expect(document.activeElement === button("Project guide")).toBe(true);
    expect(closeSettings).not.toHaveBeenCalled();
    escape();
    expect(closeSettings).toHaveBeenCalledOnce();
  });

  it("separates YAML metadata from Markdown headings, renders tables, and preserves the original source", async () => {
    const source =
      "---\nname: guide\ndescription: |\n  Review the change.\n  Keep all metadata.\nallowed-tools:\n  - Read\n---\n\n# Instructions\n\n| Check | Result |\n| --- | --- |\n| Tests | Passed |\n";
    await render();
    vi.mocked(invoke).mockResolvedValueOnce(source);
    await click("Project guide");
    const panel = container.querySelector('[aria-label="Skill preview"]')!;
    const metadata = panel.querySelector("details");
    expect(metadata).not.toBeNull();
    expect(metadata?.querySelector("summary")?.textContent).toBe(
      "Skill metadata",
    );
    expect(metadata?.textContent).toContain("allowed-tools:\n  - Read");
    expect(
      Array.from(
        panel.querySelectorAll("h1, h2, h3"),
        (heading) => heading.textContent,
      ),
    ).toEqual(["Project guide", "Instructions"]);
    expect(
      Array.from(panel.querySelectorAll("th"), (cell) => cell.textContent),
    ).toEqual(["Check", "Result"]);
    expect(
      Array.from(panel.querySelectorAll("td"), (cell) => cell.textContent),
    ).toEqual(["Tests", "Passed"]);
    await click("Source");
    expect(panel.querySelector("pre")?.textContent).toBe(source);
    await click("Preview");
  });

  it("opens an inline panel from an eye icon and switches skills without closing it", async () => {
    await render();
    const opener = button("Preview skill Project guide");
    expect(opener.textContent).toBe("");
    expect(opener.querySelector("svg")).not.toBeNull();
    await click("Preview skill Project guide");
    const panel = container.querySelector('[aria-label="Skill preview"]');
    expect(panel).not.toBeNull();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(opener.getAttribute("aria-expanded")).toBe("true");
    await click("Other guide");
    expect(container.querySelector('[aria-label="Skill preview"]')).toBe(panel);
    expect(panel?.textContent).toContain(skills[2].path);
    expect(opener.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(button("Other guide"));
  });

  it("Escape closes the preview before Settings", async () => {
    const closeSettings = vi.fn();
    await act(async () =>
      root.render(
        createElement(SettingsView, {
          section: "skills",
          cwd: "D:/repo",
          sessions: [],
          onClose: closeSettings,
          onOpenSession: vi.fn(),
          onArchiveSession: vi.fn(),
          onDeleteSession: vi.fn(),
          onOpenWhatsNew: vi.fn(),
        }),
      ),
    );
    await click("Project guide");
    act(() =>
      document.activeElement!.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Escape",
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(document.querySelector('[aria-label="Skill preview"]')).toBeNull();
    expect(closeSettings).not.toHaveBeenCalled();
    expect(container.querySelector('[aria-label="Settings"]')).not.toBeNull();
    act(() =>
      document.activeElement!.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Escape",
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(closeSettings).toHaveBeenCalledOnce();
  });

  it("allows Tab to leave the preview and keeps the document keyboard-scrollable", async () => {
    await render();
    await click("Project guide");
    const close = button("Close skill preview");
    const preview = document.querySelector<HTMLElement>(
      '[aria-label="Markdown preview"]',
    );
    expect(preview?.tabIndex).toBe(0);
    const tab = new KeyboardEvent("keydown", {
      key: "Tab",
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    act(() => close.dispatchEvent(tab));
    expect(tab.defaultPrevented).toBe(false);
    await click("Other guide");
    expect(document.activeElement).toBe(button("Other guide"));
    await click("Source");
    expect(
      document.querySelector<HTMLElement>('[aria-label="Markdown source"]')
        ?.tabIndex,
    ).toBe(0);
    await click("Preview");
  });

  it.each(["Close skill preview", "Escape"])(
    "closes with %s, restores opener focus and preserves the filter",
    async (action) => {
      await render();
      const filter = container.querySelector<HTMLInputElement>(
        '[aria-label="Filter skills"]',
      )!;
      act(() => {
        Object.getOwnPropertyDescriptor(
          HTMLInputElement.prototype,
          "value",
        )!.set!.call(filter, "project");
        filter.dispatchEvent(new Event("input", { bubbles: true }));
      });
      const opener = await click("Project guide");
      expect(document.activeElement).toBe(button("Close skill preview"));
      if (action === "Close skill preview") await click("Close skill preview");
      else
        act(() =>
          document.activeElement!.dispatchEvent(
            new KeyboardEvent("keydown", {
              key: "Escape",
              bubbles: true,
              cancelable: true,
            }),
          ),
        );
      expect(document.querySelector('[aria-label="Skill preview"]')).toBeNull();
      expect(document.activeElement).toBe(opener);
      expect(filter.value).toBe("project");
      expect(container.querySelectorAll('[role="switch"]')).toHaveLength(1);
    },
  );

  it.each([new Error("File not found"), "Permission denied"])(
    "shows a readable error and can open another skill after %s",
    async (error) => {
      await render();
      vi.mocked(invoke).mockRejectedValueOnce(error);
      await click("Project guide");
      const alert = document.querySelector(
        '[aria-label="Skill preview"] [role="alert"]',
      );
      expect(alert?.textContent).toContain("Could not read SKILL.md");
      expect(alert?.textContent).toContain(
        error instanceof Error ? error.message : error,
      );
      expect(document.querySelector('[role="status"]')).toBeNull();
      await click("Close skill preview");
      await click("Other guide");
      expect(
        document.querySelector('[aria-label="Skill preview"] [role="alert"]'),
      ).toBeNull();
      expect(
        document.querySelector('[aria-label="Skill preview"] h1')?.textContent,
      ).toBe("Full instructions");
    },
  );

  it.each(["resolves", "rejects"])(
    "keeps the current skill when a previous read %s after switching",
    async (outcome) => {
      const previous = deferred<string>();
      const current = deferred<string>();
      await render();
      vi.mocked(invoke).mockImplementationOnce(() => previous.promise);
      await click("Project guide");
      expect(document.querySelector('[role="status"]')?.textContent).toContain(
        "Loading skill",
      );
      vi.mocked(invoke).mockImplementationOnce(() => current.promise);
      await click("Other guide");
      await act(async () => current.resolve("# Current instructions"));
      expect(
        document.querySelector('[aria-label="Skill preview"] h1')?.textContent,
      ).toBe("Current instructions");
      await act(async () => {
        if (outcome === "resolves") previous.resolve("# Old instructions");
        else previous.reject(new Error("Old read failed"));
      });
      expect(
        document.querySelector('[aria-label="Skill preview"] h1')?.textContent,
      ).toBe("Current instructions");
      expect(
        document.querySelector('[aria-label="Skill preview"]')?.textContent,
      ).not.toContain("Old instructions");
      expect(
        document.querySelector('[aria-label="Skill preview"] [role="alert"]'),
      ).toBeNull();
    },
  );

  it("opens project and harness skills using Preview skill", async () => {
    await render();
    for (const skill of [skills[0], skills[2]]) {
      await click(`Preview skill ${skill.name}`);
      expect(
        document.querySelector('[aria-label="Skill preview"]')?.textContent,
      ).toContain(skill.path);
      expect(invoke).toHaveBeenCalledWith("read_text_file", {
        path: skill.path,
      });
      await click("Close skill preview");
    }
  });

  it("opens a disabled personal skill by name and shows the full Markdown and source without changing its preference", async () => {
    saveDisabledSkillPaths([skills[1].path]);
    await render();
    await click("Personal guide");

    const dialog = document.querySelector('[aria-label="Skill preview"]')!;
    expect(dialog).not.toBeNull();
    expect(dialog.textContent).toContain(skills[1].path);
    expect(dialog.querySelector("h1")?.textContent).toBe("Full instructions");
    expect(dialog.textContent).toContain("Read everything.");
    expect(dialog.textContent).toContain("Last paragraph.");
    expect(invoke).toHaveBeenCalledWith("read_text_file", {
      path: skills[1].path,
    });
    await click("Source");
    expect(dialog.querySelector("pre")?.textContent).toBe(markdown);
    expect(button("Source").getAttribute("aria-selected")).toBe("true");
    expect(loadDisabledSkillPaths()).toEqual([skills[1].path]);
    expect(
      container
        .querySelector(
          '[aria-label="Include Personal guide in MonoCode catalog"]',
        )
        ?.getAttribute("aria-checked"),
    ).toBe("false");
    expect(button("Copy path of Personal guide")).toBeDefined();
    expect(button("Reveal Personal guide in file explorer")).toBeDefined();
  });
});
