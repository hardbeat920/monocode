// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import type { FloatingMonoView } from "../model/floatingMono";
import { FloatingMonoChat } from "./FloatingMonoChat";

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ hide: vi.fn() }),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({ listen: native.listen }),
}));
vi.mock("../../sessions/hooks/useFileDrop", () => ({
  useFileDrop: () => false,
}));
vi.mock("../../sessions/ui/AgentMarkdown", () => ({
  AgentMarkdown: ({ text }: { text: string }) =>
    createElement("div", null, text),
}));

let container: HTMLDivElement;
let root: Root;
let receive: (event: { payload: FloatingMonoView }) => void;
let observers: Array<{ targets: Element[]; resize: () => void }>;
let height: number;
let top: number;
const initial: FloatingMonoView = {
  monos: [
    {
      id: "first",
      name: "Captain",
      mascot: "crab",
      color: "#aaf",
      sessionId: "motion-chat",
    },
  ],
  monoId: "first",
  error: null,
  session: {
    ...newSession("codex", "/tmp", "default", "auto"),
    id: "motion-chat",
    busy: true,
    blocks: [
      { id: "question", role: "user", text: "Tell me more" },
      { id: "answer", role: "assistant", text: "The reply", streaming: true },
    ],
  },
};

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers({
    toFake: ["requestAnimationFrame", "cancelAnimationFrame", "performance"],
  });
  vi.spyOn(HTMLElement.prototype, "animate").mockImplementation(
    () => ({ cancel: vi.fn() }) as unknown as Animation,
  );
  height = 600;
  top = 0;
  observers = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      targets: Element[] = [];
      constructor(readonly resize: () => void) {
        observers.push(this);
      }
      observe(target: Element) {
        this.targets.push(target);
      }
      disconnect() {
        this.targets = [];
      }
    },
  );
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(390);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(
    function () {
      return this.classList.contains("agent-transcript") ? height : 400;
    },
  );
  vi.spyOn(HTMLElement.prototype, "scrollTop", "get").mockImplementation(
    function () {
      return this.classList.contains("agent-transcript") ? top : 0;
    },
  );
  vi.spyOn(HTMLElement.prototype, "scrollTop", "set").mockImplementation(
    function (value) {
      if (this.classList.contains("agent-transcript"))
        top = Math.max(0, Math.min(value, height - 400));
    },
  );
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function () {
      const content = this.closest<HTMLElement>("[data-transcript-content]");
      const offset = Number.parseFloat(
        content?.style.transform.match(/translateY\((.*)px\)/)?.[1] ?? "0",
      );
      const y = this.classList.contains("transcript-turn")
        ? 100 - top + offset
        : 0;
      return { top: y, bottom: y + 100, height: 100 } as DOMRect;
    },
  );
  native.invoke.mockReset().mockResolvedValue(initial);
  native.listen.mockReset().mockImplementation(async (_event, handler) => {
    receive = handler;
    return () => {};
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("eases streamed IPC snapshots at the bottom and lets the reader scroll away", async () => {
  await act(async () =>
    root.render(createElement(FloatingMonoChat, { onShown: () => {} })),
  );
  const scroller =
    container.querySelector<HTMLDivElement>(".agent-transcript")!;
  const content = scroller.querySelector<HTMLElement>(
    "[data-transcript-content]",
  )!;
  const observer = observers.find((item) => item.targets.includes(content))!;
  act(() => observer.resize());
  expect(top).toBe(200);

  // Native events deserialize every block again, as opposed to reusing the
  // main renderer's object references. Keep the same mounted transcript.
  const next: FloatingMonoView = JSON.parse(JSON.stringify(initial));
  next.session!.blocks[1].text += " keeps streaming";
  height += 80;
  act(() => receive({ payload: next }));
  expect(container.querySelector(".agent-transcript")).toBe(scroller);
  expect(top).toBe(280);
  const offset = () =>
    Number.parseFloat(
      content.style.transform.match(/translateY\((.*)px\)/)?.[1] ?? "0",
    );
  expect(offset()).toBe(80);
  act(() => vi.advanceTimersByTime(32));
  expect(offset()).toBeGreaterThan(0);
  expect(offset()).toBeLessThan(80);

  // The status/stop controls now live in the header, whose height stays the
  // same when the reply ends. Its final text and footer can keep easing.
  const finishing: FloatingMonoView = JSON.parse(JSON.stringify(next));
  finishing.session!.busy = false;
  finishing.session!.blocks[1].streaming = false;
  const previousOffset = offset();
  height += 40;
  act(() => receive({ payload: finishing }));
  expect(offset()).toBeGreaterThan(previousOffset);
  const finishOffset = offset();
  act(() => vi.advanceTimersByTime(32));
  expect(offset()).toBeGreaterThan(0);
  expect(offset()).toBeLessThan(finishOffset);

  act(() => scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -100 })));
  const paused = top;
  height += 60;
  act(() => observer.resize());
  expect(top).toBe(paused);
  expect(content.style.transform).toBe("");
});
