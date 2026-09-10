// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../lib/session";
import { AgentTranscript } from "./AgentTranscript";

let container: HTMLDivElement;
let root: Root;
let bodyHeight: number;
let resize: () => void;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  // happy-dom has no layout; supply the measured dimensions, not an estimate
  // based on text length (a short string can wrap in a narrow transcript).
  bodyHeight = 200;
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(
    () => bodyHeight,
  );
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(
    () => 40,
  );
  vi.stubGlobal("ResizeObserver", class {
    constructor(private callback: () => void) {}
    observe(el: Element) {
      if (el.tagName === "PRE") resize = this.callback;
    }
    disconnect() {}
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function render(text: string, severity: "blocker" | "concern" = "concern") {
  const blocks: Block[] = [{
    id: "advisor",
    role: "system",
    text,
    interjection: { customType: "advisor", severity },
  }];
  act(() => root.render(createElement(AgentTranscript, { blocks, busy: false })));
}

const note = "Check the fallback.\n```ts\nconst result = read();\nif (!result) throw new Error('missing');\n```";

describe("AgentTranscript interjection preview", () => {
  it("clamps long blocker bodies by default while retaining the label and full text", () => {
    render(note, "blocker");
    expect(container.querySelector("pre")?.classList.contains("line-clamp-2")).toBe(true);
    expect(container.querySelector("pre")?.textContent).toBe(note);
    expect(container.querySelector('[aria-label="Interjection: Advisor"]')?.textContent).toContain("Blocker");
    expect(container.querySelector("button")?.getAttribute("aria-expanded")).toBe("false");
  });

  it("expands and reclamps without losing the collapse control", () => {
    render(note);
    const button = container.querySelector<HTMLButtonElement>("button")!;
    act(() => button.click());
    expect(button.getAttribute("aria-expanded")).toBe("true");
    expect(container.querySelector("pre")?.classList.contains("line-clamp-2")).toBe(false);
    expect(container.querySelector("pre")?.textContent).toBe(note);
    act(() => resize());
    expect(container.querySelector("button")).toBe(button);
    act(() => button.click());
    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(container.querySelector("pre")?.classList.contains("line-clamp-2")).toBe(true);
  });

  it("omits the toggle for fitting text and remeasures when the transcript reflows", () => {
    bodyHeight = 20;
    render("Check this.");
    expect(container.querySelector("button")).toBeNull();
    bodyHeight = 60;
    act(() => resize());
    expect(container.querySelector("button")?.getAttribute("aria-expanded")).toBe("false");
    bodyHeight = 40;
    act(() => resize());
    expect(container.querySelector("button")).toBeNull();
  });
});
