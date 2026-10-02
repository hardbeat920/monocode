// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  close: vi.fn(),
  request: vi.fn(),
  resolveBinary: vi.fn(),
  spawnChild: vi.fn(),
  killChild: vi.fn(),
  writeChild: vi.fn(),
  frames: [] as Array<(record: Record<string, unknown>) => void>,
}));

vi.mock("../../core/child", () => ({
  killChild: mocks.killChild,
  resolveOmpBinary: vi.fn(),
  resolvePiBinary: mocks.resolveBinary,
  spawnChild: mocks.spawnChild,
  unwatchChild: vi.fn(),
  watchChild: vi.fn(),
  writeChild: mocks.writeChild,
}));

vi.mock("./piClient", () => ({
  PiRpc: class {
    constructor(
      _sessionId: string,
      onFrame: (record: Record<string, unknown>) => void,
    ) {
      mocks.frames.push(onFrame);
    }

    request = mocks.request;
    close = mocks.close;
    pushLine = vi.fn();
  },
}));

import { cancelPiTurn, compactPiContext, stopPiSession } from "./pi";
import { piAdapter } from "./piAdapter";
import type { HarnessEvent } from "../../core/types";
import { applyHarnessEvent } from "../../core/apply";
import { newSession } from "../../../../features/sessions/model/session";
import { QuestionForm } from "../../../../features/sessions/ui/QuestionForm";

describe("Pi live session", () => {
  beforeEach(() => {
    mocks.close.mockReset();
    mocks.request.mockReset();
    mocks.resolveBinary.mockReset();
    mocks.spawnChild.mockReset();
    mocks.killChild.mockReset();
    mocks.writeChild.mockReset();
    mocks.writeChild.mockResolvedValue(undefined);
    mocks.frames.length = 0;
    mocks.resolveBinary.mockResolvedValue({ path: "/fake/pi" });
    mocks.spawnChild.mockResolvedValue(undefined);
    mocks.killChild.mockResolvedValue(undefined);
    mocks.request.mockImplementation(
      async (command: Record<string, unknown>) => {
        if (command.type === "get_state") {
          return {
            data: {
              sessionId: "pi_session",
              model: { contextWindow: 200_000 },
            },
          };
        }
        if (command.type === "compact") {
          return { data: { estimatedTokensAfter: 32_000 } };
        }
        return { data: {} };
      },
    );
  });

  it("publishes the resolved Pi default model for provider usage", async () => {
    mocks.request.mockImplementation(async (command: Record<string, unknown>) => {
      if (command.type === "get_state") return { data: {
        sessionId: "pi_default",
        model: { provider: "openai-codex", id: "gpt-5.4", contextWindow: 200_000 },
      } };
      return { data: {} };
    });
    const events: HarnessEvent[] = [];
    await compactPiContext({
      sessionId: "pi-default", cwd: "/repo", model: "pi:default",
      runtimeMode: "supervised", onEvent: event => events.push(event),
    });
    expect(events).toContainEqual({
      type: "session.configChanged", model: "pi:openai-codex/gpt-5.4",
    });
    await stopPiSession("pi-default");
  });

  it("does not publish an intermediate default when explicit model selection fails", async () => {
    mocks.request.mockImplementation(async (command: Record<string, unknown>) => {
      if (command.type === "get_state") return { data: {
        sessionId: "pi_explicit",
        model: { provider: "anthropic", id: "claude-sonnet-5", contextWindow: 200_000 },
      } };
      if (command.type === "set_model") throw new Error("Model unavailable");
      return { data: {} };
    });
    const events: HarnessEvent[] = [];
    await expect(compactPiContext({
      sessionId: "pi-explicit", cwd: "/repo", model: "pi:openai-codex/gpt-5.4",
      runtimeMode: "supervised", onEvent: event => events.push(event),
    })).rejects.toThrow("Model unavailable");
    expect(events.filter(event => event.type === "session.configChanged")).toEqual([]);
    expect(mocks.request).toHaveBeenCalledWith({ type: "set_model", provider: "openai-codex", modelId: "gpt-5.4" });
    await stopPiSession("pi-explicit");
  });

  it("uses the compact RPC command and publishes the post-compact estimate", async () => {
    const events: HarnessEvent[] = [];

    await compactPiContext({
      sessionId: "pi-compact",
      cwd: "/repo",
      model: "pi:default",
      runtimeMode: "supervised",
      onEvent: (event) => events.push(event),
    });

    expect(mocks.request).toHaveBeenCalledWith(
      { type: "compact" },
      30 * 60_000,
    );
    expect(events).toContainEqual({
      type: "context",
      used: 32_000,
      window: 200_000,
    });
    await stopPiSession("pi-compact");
  });

  it("publishes readable Ponytail status and extension notifications", async () => {
    const events: HarnessEvent[] = [];
    await compactPiContext({
      sessionId: "pi-ansi",
      cwd: "/repo",
      model: "pi:default",
      runtimeMode: "supervised",
      onEvent: (event) => events.push(event),
    });
    const frame = mocks.frames[0]!;
    frame({
      type: "extension_ui_request",
      id: "ponytail-status",
      method: "setStatus",
      statusKey: "ponytail",
      statusText:
        "\u001b[38;5;241m○\u001b[39m \u001b[38;5;244mponytail:\u001b[39m \u001b[38;5;188m⚡ FULL\u001b[0m",
    });
    frame({
      type: "extension_ui_request",
      id: "plugin-notify",
      method: "notify",
      message: "\u001b[32mPlugin ready\u001b[0m",
    });
    frame({
      type: "extension_ui_request",
      id: "empty-status",
      method: "setStatus",
      statusText: "\u001b[0m",
    });
    expect(events.filter((event) => event.type === "status")).toEqual([
      { type: "status", text: "○ ponytail: ⚡ FULL" },
      { type: "status", text: "Plugin ready" },
    ]);
    await stopPiSession("pi-ansi");
  });

  describe("extension dialogs", () => {
    const events: HarnessEvent[] = [];
    const replies = () =>
      mocks.writeChild.mock.calls.map(([, line]) => JSON.parse(line as string));
    const asked = () => {
      const event = events.find((e) => e.type === "question.asked");
      if (event?.type !== "question.asked") throw new Error("no question");
      return event;
    };
    const open = async (sessionId: string) => {
      events.length = 0;
      await compactPiContext({
        sessionId,
        cwd: "/repo",
        model: "pi:default",
        runtimeMode: "supervised",
        onEvent: (event) => events.push(event),
      });
      return mocks.frames[0]!;
    };

    it("answers a select with the chosen option", async () => {
      const frame = await open("pi-select");
      frame({
        type: "extension_ui_request",
        id: "q1",
        method: "select",
        title: "Output",
        options: ["file", "stdout"],
      });
      piAdapter.respondQuestion!("pi-select", asked().requestId, {
        kind: "answered",
        answers: { q1: ["1"] },
      });
      await vi.waitFor(() =>
        expect(replies()).toContainEqual({
          type: "extension_ui_response",
          id: "q1",
          value: "stdout",
        }),
      );
      await stopPiSession("pi-select");
    });

    it("submits overlapping dialogs through the form and adapter", async () => {
      vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
      const frame = await open("pi-overlap");
      frame({
        type: "extension_ui_request",
        id: "first",
        method: "select",
        title: "First",
        options: ["Other"],
      });
      frame({
        type: "extension_ui_request",
        id: "second",
        method: "select",
        title: "Second",
        options: ["file"],
      });
      expect(events.filter((event) => event.type === "question.asked")).toHaveLength(1);

      let session = newSession("pi", "/repo");
      let applied = 0;
      const applyEvents = () => {
        for (const event of events.slice(applied))
          session = applyHarnessEvent(session, event);
        applied = events.length;
      };
      const container = document.createElement("div");
      document.body.append(container);
      const root = createRoot(container);
      const render = () =>
        act(() =>
          root.render(
            createElement(QuestionForm, {
              prompt: session.pendingQuestion!,
              onReply: (requestId, reply) =>
                piAdapter.respondQuestion!("pi-overlap", requestId, reply),
            }),
          ),
        );
      const submit = () => {
        act(() =>
          container.querySelector<HTMLButtonElement>('button[aria-pressed]')!.click(),
        );
        expect(
          container.querySelector<HTMLInputElement>(
            'input[placeholder="Type your answer"]',
          ),
        ).toBeNull();
        const button = container.querySelector<HTMLButtonElement>(
          'button[type="submit"]',
        )!;
        expect(button.disabled).toBe(false);
        act(() => button.click());
      };
      try {
        applyEvents();
        render();
        submit();
        await vi.waitFor(() =>
          expect(replies()).toContainEqual({
            type: "extension_ui_response",
            id: "first",
            value: "Other",
          }),
        );
        applyEvents();
        expect(session.pendingQuestion?.questions[0].id).toBe("second");
        render();
        submit();
        await vi.waitFor(() =>
          expect(replies()).toContainEqual({
            type: "extension_ui_response",
            id: "second",
            value: "file",
          }),
        );
        applyEvents();
        expect(session.pendingQuestion).toBeUndefined();
      } finally {
        act(() => root.unmount());
        container.remove();
        vi.unstubAllGlobals();
        await stopPiSession("pi-overlap");
      }
    });

    it("dismisses a timed-out dialog and shows the next one", async () => {
      const frame = await open("pi-timeout");
      vi.useFakeTimers();
      try {
        frame({ type: "extension_ui_request", id: "first", method: "input", title: "First", timeout: 20 });
        frame({ type: "extension_ui_request", id: "second", method: "input", title: "Second" });
        expect(events.filter((event) => event.type === "question.asked")).toHaveLength(1);
        await vi.advanceTimersByTimeAsync(20);
        expect(events.filter((event) => event.type === "question.asked")).toHaveLength(2);
        expect(events).toContainEqual({ type: "question.resolved", requestId: 1, decision: "skipped" });
        expect(replies()).toContainEqual({ type: "extension_ui_response", id: "first", cancelled: true });
      } finally {
        vi.useRealTimers();
        await stopPiSession("pi-timeout");
      }
    });

    it("never shows a queued dialog that times out", async () => {
      const frame = await open("pi-queued-timeout");
      vi.useFakeTimers();
      try {
        frame({ type: "extension_ui_request", id: "first", method: "select", title: "First", options: ["yes"] });
        frame({ type: "extension_ui_request", id: "expired", method: "input", title: "Expired", timeout: 20 });
        frame({ type: "extension_ui_request", id: "third", method: "select", title: "Third", options: ["no"] });
        await vi.advanceTimersByTimeAsync(20);
        expect(events.filter((event) => event.type === "question.asked")).toHaveLength(1);
        expect(replies()).toContainEqual({ type: "extension_ui_response", id: "expired", cancelled: true });
        piAdapter.respondQuestion!("pi-queued-timeout", 1, { kind: "answered", answers: { first: ["0"] } });
        await Promise.resolve();
        expect(events.filter((event) => event.type === "question.asked").map((event) => event.questions[0].id)).toEqual(["first", "third"]);
      } finally {
        vi.useRealTimers();
        await stopPiSession("pi-queued-timeout");
      }
    });

    it("cancels queued dialogs without showing them", async () => {
      const frame = await open("pi-cancel-queue");
      frame({ type: "extension_ui_request", id: "first", method: "input", title: "First" });
      frame({ type: "extension_ui_request", id: "second", method: "input", title: "Second" });
      await cancelPiTurn("pi-cancel-queue");
      expect(events.filter((event) => event.type === "question.asked")).toHaveLength(1);
      expect(replies()).toEqual(expect.arrayContaining([
        { type: "extension_ui_response", id: "first", cancelled: true },
        { type: "extension_ui_response", id: "second", cancelled: true },
      ]));
      await stopPiSession("pi-cancel-queue");
    });

    it("shows input placeholders and editor prefill", async () => {
      const frame = await open("pi-text");
      frame({
        type: "extension_ui_request",
        id: "i1",
        method: "input",
        title: "Name",
        placeholder: "e.g. main",
      });
      frame({
        type: "extension_ui_request",
        id: "e1",
        method: "editor",
        title: "Commit message",
        prefill: "fix: x\n\n  body",
      });
      expect(asked().questions[0]).toMatchObject({ placeholder: "e.g. main" });
      piAdapter.respondQuestion!("pi-text", asked().requestId, {
        kind: "answered", answers: {}, custom: { i1: "main" },
      });
      await vi.waitFor(() => expect(events.filter((e) => e.type === "question.asked")).toHaveLength(2));
      const editor = events.filter((e) => e.type === "question.asked")[1];
      if (editor?.type !== "question.asked") throw new Error("no editor question");
      expect(editor.questions[0]).toMatchObject({
        multiline: true,
        defaultText: "fix: x\n\n  body",
      });
      await stopPiSession("pi-text");
    });

    const typed = (text: string) =>
      ({ kind: "answered", answers: {}, custom: { d: text } }) as const;

    it.each([
      ["input", typed("main"), { value: "main" }],
      ["editor", typed("a\n  b"), { value: "a\n  b" }],
      ["input", typed(""), { value: "" }],
      ["input", { kind: "skipped" } as const, { cancelled: true }],
    ] as const)("replies to %s with %j", async (method, reply, response) => {
      const frame = await open("pi-reply");
      frame({ type: "extension_ui_request", id: "d", method, title: "Value" });
      piAdapter.respondQuestion!("pi-reply", asked().requestId, reply);
      await vi.waitFor(() =>
        expect(replies()).toContainEqual({
          type: "extension_ui_response",
          id: "d",
          ...response,
        }),
      );
      await stopPiSession("pi-reply");
    });
  });
});
