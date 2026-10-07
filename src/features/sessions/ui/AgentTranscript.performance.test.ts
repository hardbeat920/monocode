// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import * as activity from "../model/transcriptActivity";
import { AgentTranscript } from "./AgentTranscript";

vi.mock("../model/transcriptActivity", async (original) => {
  const actual = await original<typeof activity>();
  return { ...actual, groupTurnItems: vi.fn(actual.groupTurnItems) };
});
vi.mock("./AgentMarkdown", () => ({
  AgentMarkdown: ({ text }: { text: string }) =>
    createElement("div", null, text),
}));
vi.mock("../hooks/useTranscriptLayout", () => ({
  useTranscriptLayout: () => "full",
}));
vi.mock("../hooks/useTranscriptAnchor", () => ({
  useTranscriptAnchor: () => false,
}));

let host: HTMLDivElement;
let root: Root;
let observers: Array<{ target?: Element; resize: () => void }>;

beforeEach(() => {
  observers = [];
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      target?: Element;
      constructor(readonly resize: () => void) {
        observers.push(this);
      }
      observe(target: Element) {
        this.target = target;
      }
      disconnect() {
        this.target = undefined;
      }
    },
  );
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("regroups only the changing live turn, including after a tab revisit", async () => {
  const blocks: Block[] = Array.from({ length: 20 }, (_, index): Block[] => [
    { id: `u${index}`, role: "user", text: `Question ${index}` },
    { id: `a${index}`, role: "assistant", text: `Answer ${index}` },
  ]).flat();
  await act(async () =>
    root.render(
      createElement(AgentTranscript, {
        blocks,
        busy: true,
        visible: true,
      }),
    ),
  );
  expect(host.querySelectorAll("[data-transcript-turn]")).toHaveLength(20);
  vi.mocked(activity.groupTurnItems).mockClear();
  const updated = blocks.slice();
  updated[updated.length - 1] = {
    ...updated[updated.length - 1],
    text: "Latest output",
  };
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks: updated,
        busy: true,
        visible: true,
      }),
    ),
  );
  expect(activity.groupTurnItems).toHaveBeenCalledTimes(1);
  expect(vi.mocked(activity.groupTurnItems).mock.calls[0][1]).toEqual({
    settled: false,
  });
  expect(host.textContent).toContain("Latest output");

  vi.mocked(activity.groupTurnItems).mockClear();
  for (const visible of [false, true]) {
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: updated,
          busy: true,
          visible,
        }),
      ),
    );
  }
  expect(activity.groupTurnItems).not.toHaveBeenCalled();
});

it("leaves offscreen messages unmeasured until the browser reveals their turn", () => {
  let skipped = true;
  vi.spyOn(Element.prototype, "checkVisibility").mockImplementation(
    () => !skipped,
  );
  const blocks: Block[] = [{ id: "u", role: "user", text: "A long prompt" }];
  act(() => root.render(createElement(AgentTranscript, { blocks })));
  const text = host.querySelector<HTMLElement>(".user-message-bubble pre")!;
  const height = vi.fn(() => 120);
  Object.defineProperties(text, {
    scrollHeight: { configurable: true, get: height },
    clientHeight: { configurable: true, get: () => 40 },
  });
  const observer = observers.find((item) => item.target === text)!;
  act(() => observer.resize());
  expect(height).not.toHaveBeenCalled();

  skipped = false;
  const event = new Event("contentvisibilityautostatechange");
  Object.defineProperty(event, "skipped", { value: false });
  act(() => text.closest(".transcript-turn")!.dispatchEvent(event));
  expect(height).toHaveBeenCalled();
  expect(host.textContent).toContain("Show more");

  height.mockClear();
  act(() =>
    root.render(createElement(AgentTranscript, { blocks, visible: false })),
  );
  expect(observer.target).toBeUndefined();
  act(() => text.closest(".transcript-turn")!.dispatchEvent(event));
  expect(height).not.toHaveBeenCalled();
});
