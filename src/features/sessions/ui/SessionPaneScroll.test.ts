// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SessionPane, type SessionPaneProps } from "./SessionPane";
import { TranscriptPool, TranscriptPoolOutlet } from "./TranscriptPool";
import { getMonoTranscriptPage } from "../data/sessionStore";

const probes = vi.hoisted(() => ({
  composer: vi.fn(() => null),
  review: vi.fn(() => null),
  runs: [],
}));

vi.mock("./Composer", () => ({ Composer: () => null }));
vi.mock("../../monos/ui/MonoComposer", () => ({
  MonoComposer: probes.composer,
}));
vi.mock("./SessionReview", () => ({ SessionReview: probes.review }));
vi.mock("../data/sessionStore", async (original) => ({
  ...(await original<typeof import("../data/sessionStore")>()),
  getMonoTranscriptPage: vi.fn(),
}));
vi.mock("../../orchestration/model/orchestration", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("../../orchestration/model/orchestration")
  >()),
  orchestrator: {
    subscribe: () => () => {},
    snapshot: () => probes.runs,
    hydrate: async () => {},
  },
}));
vi.mock("../../monos/model/mono", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../monos/model/mono")>()),
  monoForSession: () => ({
    id: "mono",
    sessionId: "chat",
    name: "Captain Awesome",
    mascot: "cat",
    color: "#6ba",
    projects: [],
  }),
}));

let container: HTMLDivElement;
let root: Root;
let observers: Array<{ targets: Element[]; resize: () => void }>;

beforeEach(() => {
  probes.composer.mockClear();
  probes.review.mockClear();
  vi.mocked(getMonoTranscriptPage).mockReset();
  observers = [];
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "requestAnimationFrame",
    vi.fn(() => 1),
  );
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  vi.stubGlobal(
    "ResizeObserver",
    class {
      targets: Element[] = [];
      constructor(readonly callback: ResizeObserverCallback) {
        observers.push({
          targets: this.targets,
          resize: () => callback([], this),
        });
      }
      observe(target: Element) {
        this.targets.push(target);
      }
      unobserve() {}
      disconnect() {
        this.targets.length = 0;
      }
    },
  );
  localStorage.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  localStorage.clear();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const noop = () => {};

function props(transcriptPool?: TranscriptPool): SessionPaneProps {
  return {
    session: {
      id: "chat",
      title: "Chat",
      cwd: "/repo",
      harness: "codex",
      model: "",
      modelSettings: {},
      runtimeMode: "supervised",
      blocks: [
        { id: "user", role: "user", text: "Hello" },
        { id: "reply", role: "assistant", text: "Answer" },
      ],
    },
    visible: true,
    focused: false,
    inSplit: false,
    composerFocused: false,
    recents: [],
    transcriptPool,
    onFocus: noop,
    onClose: noop,
    onCwdChange: noop,
    onBranchChange: noop,
    onWorkspaceModeChange: noop,
    onWorktreeBaseChange: noop,
    onModelChange: noop,
    onModelSettingsChange: noop,
    onRuntimeModeChange: noop,
    onSubmit: noop,
    onSaveDraft: noop,
    onRemoveDraft: noop,
    onStop: noop,
    onCompactContext: () => false,
    onPlaceSessionInFolder: noop,
    onDeleteQueuedMessage: noop,
    onEditQueuedMessage: noop,
    onQueuedMessageEditingChange: noop,
    onSteerQueuedMessage: noop,
    onResumeQueue: noop,
    onUsageLimitResume: noop,
    onUsageLimitResumeAtReset: noop,
    onUsageLimitDismiss: noop,
    onApproval: noop,
    onQuestionReply: noop,
    onOpenFile: noop,
    onOpenDiff: noop,
    onOpenPlan: noop,
    onBuildPlan: noop,
    onNewTerminal: noop,
  };
}

it("routes a Mono footer activity click to its session and selected turn", () => {
  const pane = props();
  pane.session.blocks = [
    { id: "user", role: "user", text: "Inspect", durationMs: 23000 },
    { id: "call", role: "tool", text: "ls", tool: { kind: "shell", status: "completed" } },
    { id: "reply", role: "assistant", text: "Done" },
  ];
  const onShowMonoActivity = vi.fn();
  act(() => root.render(createElement(SessionPane, { ...pane, onShowMonoActivity })));
  act(() => container.querySelector<HTMLButtonElement>('[data-turn-actions] [aria-label="Show activity"]')!.click());
  expect(onShowMonoActivity).toHaveBeenCalledWith("chat", "user", pane.session.blocks);
  act(() => root.render(createElement(SessionPane, { ...pane, onShowMonoActivity, monoActivityTurnId: "user" })));
  expect(container.querySelector('[data-turn-actions] [aria-label="Hide activity"]')?.getAttribute("aria-expanded")).toBe("true");
});

it("routes launched sessions from a Mono footer to its session and turn", () => {
  const pane = props();
  pane.session.blocks[0] = {
    ...pane.session.blocks[0],
    monoSpawnedSessions: [
      {
        sessionId: "app-review",
        cwd: "/repo",
        title: "Review",
        harness: "codex",
        model: "gpt-6",
      },
    ],
  };
  const onShowMonoSessions = vi.fn();
  act(() =>
    root.render(createElement(SessionPane, { ...pane, onShowMonoSessions })),
  );
  act(() =>
    container
      .querySelector<HTMLButtonElement>(
        '[data-turn-actions] [aria-label="Show sessions"]',
      )!
      .click(),
  );
  expect(onShowMonoSessions).toHaveBeenCalledWith(
    "chat",
    "user",
    pane.session.blocks,
  );
  act(() =>
    root.render(
      createElement(SessionPane, {
        ...pane,
        onShowMonoSessions,
        monoSessionsTurnId: "user",
      }),
    ),
  );
  expect(
    container
      .querySelector('[aria-label="Hide sessions"]')
      ?.getAttribute("aria-expanded"),
  ).toBe("true");
});

it("renders older replies immediately when scrolling up loads a Mono page", async () => {
  const pane = props();
  pane.session.monoTranscript = { before: 20, firstBlockId: "user" };
  vi.mocked(getMonoTranscriptPage).mockResolvedValueOnce({
    blocks: [
      { id: "older-user", role: "user", text: "Earlier question" },
      { id: "older-reply", role: "assistant", text: "Earlier answer in full." },
    ],
    before: null,
    hasNewer: true,
  });
  act(() => root.render(createElement(SessionPane, pane)));
  const scroller = container.querySelector<HTMLDivElement>(".agent-transcript")!;
  await act(async () => {
    scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -100 }));
  });
  expect(getMonoTranscriptPage).toHaveBeenCalledOnce();
  expect(getMonoTranscriptPage).toHaveBeenCalledWith("chat", { before: 20 });
  const olderReply = container.querySelector(
    '[data-chat-message="older-reply"]',
  )!;
  expect(olderReply.textContent).toBe("Earlier answer in full.");
  expect(olderReply.querySelector(".word-fading")).toBeNull();
  expect(olderReply.querySelector("[data-word-fade]")).toBeNull();
  expect(
    container.querySelector('[data-chat-message="reply"]')?.textContent,
  ).toBe("Answer");
});

it.each([
  { pooled: false, nativeOverscroll: false },
  { pooled: false, nativeOverscroll: true },
  { pooled: true, nativeOverscroll: false },
  { pooled: true, nativeOverscroll: true },
])(
  "leaves the first upward scroll to the browser (pooled: $pooled, native overscroll: $nativeOverscroll)",
  ({ pooled, nativeOverscroll }) => {
    vi.stubGlobal(
      "CSS",
      new Proxy(CSS, {
        get(target, property, receiver) {
          return property === "supports"
            ? () => nativeOverscroll
            : Reflect.get(target, property, receiver);
        },
      }),
    );
    const pool = pooled ? new TranscriptPool() : undefined;
    act(() =>
      root.render(
        createElement(
          "div",
          null,
          createElement(SessionPane, props(pool)),
          pool ? createElement(TranscriptPoolOutlet, { pool }) : null,
        ),
      ),
    );
    const scroller =
      container.querySelector<HTMLDivElement>(".agent-transcript")!;
    const content = scroller.querySelector<HTMLElement>(
      "[data-transcript-content]",
    )!;
    content.lastElementChild!.getBoundingClientRect = () =>
      ({
        top:
          100 -
          top +
          Number.parseFloat(
            content.style.transform.match(/translateY\((.*)px\)/)?.[1] ?? "0",
          ),
      }) as DOMRect;
    let top = 0;
    let height = 1000;
    const writes: number[] = [];
    Object.defineProperties(scroller, {
      scrollHeight: { get: () => height },
      clientHeight: { get: () => 400 },
      clientWidth: { get: () => 640 },
      scrollTop: {
        get: () => top,
        set: (value: number) => {
          writes.push(value);
          top = Math.max(0, Math.min(value, height - 400));
        },
      },
    });
    act(() => {
      for (const observer of observers) {
        if (observer.targets.includes(scroller)) observer.resize();
      }
    });
    expect(top).toBe(600);
    expect(container.querySelector("[data-jump-to-bottom]")).toBeNull();
    const downward = new WheelEvent("wheel", { deltaY: 40, cancelable: true });
    act(() => scroller.dispatchEvent(downward));
    expect(downward.defaultPrevented).toBe(!nativeOverscroll);
    const composerRenders = probes.composer.mock.calls.length;
    const reviewRenders = probes.review.mock.calls.length;
    writes.length = 0;

    act(() => {
      scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -4 }));
      top -= 4;
      scroller.dispatchEvent(new Event("scroll"));
    });
    expect(container.querySelector("[data-jump-to-bottom]")).not.toBeNull();
    expect(top).toBe(596);
    expect(writes).toEqual([]);
    expect(probes.composer).toHaveBeenCalledTimes(composerRenders);
    expect(probes.review).toHaveBeenCalledTimes(reviewRenders);

    act(() =>
      container
        .querySelector<HTMLButtonElement>("[data-jump-to-bottom]")!
        .click(),
    );
    expect(top).toBe(600);
    expect(container.querySelector("[data-jump-to-bottom]")).toBeNull();
    expect(probes.composer).toHaveBeenCalledTimes(composerRenders);
    expect(probes.review).toHaveBeenCalledTimes(reviewRenders);

    // An idle chat follows directly; revisiting history doesn't ease its layout.
    height += 40;
    act(() => {
      for (const observer of observers) {
        if (observer.targets.includes(scroller)) observer.resize();
      }
    });
    expect(top).toBe(640);
    expect(content.style.transform).toBe("");
  },
);
