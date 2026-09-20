// @vitest-environment happy-dom
import { act, createElement, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../sessions/ui/useComposerSkills", () => ({
  useComposerSkills: () => ({
    skills: [
      {
        kind: "file",
        name: "deploy",
        invocation: "deploy",
        description: "Prepare a deployment",
        path: "/repo/.agents/skills/deploy/SKILL.md",
        scope: "project",
        source: "agents",
      },
      {
        kind: "file",
        name: "review",
        invocation: "review",
        description: "Review the current changes",
        path: "/repo/.agents/skills/review/SKILL.md",
        scope: "project",
        source: "agents",
      },
    ],
  }),
}));

import { SkillPromptField } from "./SkillPromptField";

describe("SkillPromptField", () => {
  let container: HTMLDivElement;
  let root: Root;
  let onChange: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.spyOn(HTMLElement.prototype, "scrollIntoView").mockImplementation(
      () => undefined,
    );
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    onChange = vi.fn();

    function Field() {
      const [value, setValue] = useState("");
      return createElement(SkillPromptField, {
        value,
        harness: "claude",
        cwd: "/repo",
        onChange: (next: string) => {
          onChange(next);
          setValue(next);
        },
      });
    }

    act(() => root.render(createElement(Field)));
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  function type(text: string) {
    const field = container.querySelector<HTMLTextAreaElement>("textarea")!;
    const setter = Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!;
    act(() => {
      setter.call(field, text);
      field.setSelectionRange(text.length, text.length);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    return field;
  }

  it("opens the shared picker for slash input and inserts a keyboard selection", () => {
    const field = type("/rev");

    const picker = document.querySelector<HTMLElement>("[data-skill-picker]");
    const frame = picker?.parentElement?.parentElement;
    expect(picker).not.toBeNull();
    expect(frame?.style.position).toBe("fixed");
    expect(frame?.style.width).toBe("300px");
    expect(document.body.textContent).toContain("/review");
    expect(document.body.textContent).not.toContain(
      "Review the current changes",
    );
    expect(document.body.textContent).not.toContain("New skill");
    expect(container.querySelector("[data-skill-anchor]")?.textContent).toBe(
      "/",
    );

    act(() => {
      field.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Enter",
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    expect(field.value).toBe("/review ");
    expect(document.querySelector("[data-skill-picker]")).toBeNull();
    expect(container.querySelector(".text-skill")?.textContent).toBe("/review");
  });

  it("inserts a clicked skill and returns focus to the instructions", () => {
    const field = type("Run /de");
    const deploy = [
      ...document.querySelectorAll<HTMLButtonElement>("button"),
    ].find((button) => button.textContent?.includes("/deploy"));

    expect(deploy).toBeDefined();
    act(() => deploy!.click());

    expect(field.value).toBe("Run /deploy ");
    expect(document.activeElement).toBe(field);
    expect(container.querySelector(".text-skill")?.textContent).toBe("/deploy");
  });

  it("keeps the picker dismissed after Escape", () => {
    const field = type("/rev");

    act(() => {
      field.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Escape",
          bubbles: true,
          cancelable: true,
        }),
      );
      field.dispatchEvent(
        new KeyboardEvent("keyup", { key: "Escape", bubbles: true }),
      );
    });

    expect(document.querySelector("[data-skill-picker]")).toBeNull();
    expect(field.value).toBe("/rev");
  });
});
