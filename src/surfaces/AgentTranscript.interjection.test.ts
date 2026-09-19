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

function advisor(id: string, text: string, severity: "blocker" | "concern" = "concern"): Block {
  return {
    id,
    role: "system",
    text,
    interjection: { customType: "advisor", severity },
  };
}

const note = "Check the fallback.\n```ts\nconst result = read();\nif (!result) throw new Error('missing');\n```";

function render(blocks: Block[], busy = true) {
  act(() => root.render(createElement(AgentTranscript, { blocks, busy })));
}

describe("AgentTranscript exchange rows", () => {
  it("keeps a routine advisor note behind one slim row with its preview", () => {
    render([advisor("n1", note)]);
    const toggle = container.querySelector<HTMLButtonElement>(
      "[data-exchange] > button",
    )!;
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(toggle.textContent).toContain("Advisor");
    expect(toggle.textContent).toContain("Concern");
    expect(toggle.textContent).toContain("Check the fallback.");
    // Collapsed: the body is out of the DOM entirely.
    expect(container.querySelector("pre")).toBeNull();
  });

  it("expands to the labeled note body and collapses again", () => {
    render([advisor("n1", note)]);
    const toggle = container.querySelector<HTMLButtonElement>(
      "[data-exchange] > button",
    )!;
    act(() => toggle.click());
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    const pre = container.querySelector("pre")!;
    expect(pre.textContent).toBe(note);
    expect(pre.classList.contains("line-clamp-2")).toBe(false);
    // An overflowing body still gets its own Show less control.
    expect(container.querySelectorAll("button").length).toBeGreaterThan(1);
    act(() => toggle.click());
    expect(container.querySelector("pre")).toBeNull();
  });

  it("starts open for a blocker and for non-advisor channels", () => {
    render([advisor("n1", "Blocks the release.", "blocker")]);
    expect(
      container
        .querySelector("[data-exchange] > button")
        ?.getAttribute("aria-expanded"),
    ).toBe("true");
    act(() => root.unmount());
    root = createRoot(container);
    render([
      {
        id: "a1",
        role: "system",
        text: "Job done: 3 files",
        interjection: { customType: "async-result" },
      },
    ]);
    expect(
      container
        .querySelector("[data-exchange] > button")
        ?.getAttribute("aria-expanded"),
    ).toBe("true");
    expect(container.querySelector("pre")?.textContent).toBe("Job done: 3 files");
  });

  it("collapses a run of advisor notes into a single counted row", () => {
    render([
      advisor("n1", "First note."),
      advisor("n2", "Second note."),
      advisor("n3", "Latest note."),
    ]);
    const rows = container.querySelectorAll("[data-exchange]");
    expect(rows).toHaveLength(1);
    const toggle = rows[0].querySelector("button")!;
    expect(toggle.textContent).toContain("Advisor ×3");
    expect(toggle.textContent).toContain("Latest note.");
    act(() => toggle.click());
    const bodies = [...container.querySelectorAll("pre")].map(
      (pre) => pre.textContent,
    );
    expect(bodies).toEqual(["First note.", "Second note.", "Latest note."]);
  });

  it("names mixed channels instead of calling them all advisor", () => {
    render([
      advisor("n1", "Check this."),
      {
        id: "a1",
        role: "system",
        text: "Finished.",
        interjection: { customType: "async-result" },
      },
    ]);
    const toggle = container.querySelector<HTMLButtonElement>(
      "[data-exchange] > button",
    )!;
    expect(toggle.textContent).toContain("Side inputs");
    expect(toggle.textContent).toContain("Advisor");
    expect(toggle.textContent).toContain("async-result");
  });

  it("absorbs the prose reply addressed to the notes, labeled as the reply", () => {
    render(
      [
        { id: "u", role: "user", text: "Go" },
        { id: "p1", role: "assistant", text: "Main answer." },
        advisor("n1", "Try the other path."),
        { id: "p2", role: "assistant", text: "Switched to the other path." },
      ],
      false,
    );
    const toggle = container.querySelector<HTMLButtonElement>(
      "[data-exchange] > button",
    )!;
    expect(toggle.textContent).toContain("reply");
    act(() => toggle.click());
    const reply = container.querySelector("[data-exchange] .border-l-2")!;
    expect(reply.textContent).toContain("Reply");
    expect(reply.textContent).toContain("Switched to the other path.");
    // The main answer never moves.
    expect(container.textContent).toContain("Main answer.");
  });

  it("keeps a post-note continuation in the main flow when the note split the answer", () => {
    render([
      { id: "u", role: "user", text: "Go" },
      { id: "p1", role: "assistant", text: "First half." },
      {
        id: "n1",
        role: "system",
        text: "Noted.",
        interjection: { customType: "advisor", splitStream: true },
      },
      { id: "p2", role: "assistant", text: "Second half." },
    ]);
    const toggle = container.querySelector("[data-exchange] > button")!;
    expect(toggle.textContent).not.toContain("reply");
    expect(container.textContent).toContain("Second half.");
    act(() => toggle.click());
    expect(container.querySelector("[data-exchange] .border-l-2")).toBeNull();
  });
});
