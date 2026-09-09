// @vitest-environment happy-dom
import { act, createElement, Fragment } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { QuestionForm } from "../../chrome/QuestionForm";
import { ApprovalToasts } from "../../chrome/ApprovalToasts";
import { AgentTranscript } from "../../surfaces/AgentTranscript";
import { hiddenApprovalNotices } from "../approvalToast";
import { newSession, type Session } from "../session";
import {
  notifySession,
  probeNotificationPermission,
  saveNotificationsEnabled,
  setWindowFocused,
} from "../notifications";
import { applyHarnessEvent } from "./apply";
import type { HarnessEvent } from "./types";

const sent: Array<Record<string, unknown>> = [];
let onLine: (line: string) => void;
const invoke = vi.hoisted(() =>
  vi.fn(async (command: string) =>
    command === "notification_permission" ? "granted" : undefined,
  ),
);
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("./child", () => ({
  resolveCodexBinary: async () => ({ path: "/fake/codex" }),
  spawnChild: async () => undefined,
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (line: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(JSON.parse(line));
  },
}));
const { codexAdapter } = await import("./codexAdapter");
const { __codexTestReset } = await import("./codex");

describe("Codex requests reach the chat and notifications", () => {
  let root: Root;
  let container: HTMLDivElement;
  let session: Session;
  let turn: Promise<void>;

  function render(event?: HarnessEvent) {
    if (event) session = applyHarnessEvent(session, event);
    root.render(
      createElement(
        Fragment,
        null,
        createElement(AgentTranscript, {
          blocks: session.blocks,
          busy: true,
          onApproval: (id, decision) =>
            codexAdapter.respondApproval(session.id, id, decision),
        }),
        session.pendingQuestion
          ? createElement(QuestionForm, {
              prompt: session.pendingQuestion,
              onReply: (id, reply) =>
                codexAdapter.respondQuestion!(session.id, id, reply),
            })
          : null,
        createElement(ApprovalToasts, {
          notices: hiddenApprovalNotices([session], "other", [], false),
          onFocusSession: () => undefined,
          onApproval: (sessionId, id, decision) =>
            codexAdapter.respondApproval(sessionId, id, decision),
        }),
      ),
    );
  }

  beforeEach(async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const storage = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value);
      },
      removeItem: (key: string) => {
        storage.delete(key);
      },
      clear: () => storage.clear(),
    });
    sent.length = 0;
    invoke.mockClear();
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    session = newSession("codex", "/repo", "codex:gpt-5.4", "supervised");
    session.id = "codex-ui";
    session.busy = true;
    await act(async () => {
      turn = codexAdapter.sendTurn({
        sessionId: session.id,
        cwd: session.cwd,
        model: session.model,
        runtimeMode: session.runtimeMode,
        text: "Inspect the project",
        onEvent: render,
      });
      for (const [method, result] of [
        ["initialize", {}],
        ["thread/start", { thread: { id: "thr_ui" } }],
        ["turn/start", { turn: { id: "turn_ui" } }],
      ] as const) {
        await vi.waitFor(() =>
          expect(sent.some((m) => m.method === method)).toBe(true),
        );
        onLine(
          JSON.stringify({
            id: sent.find((m) => m.method === method)!.id,
            result,
          }),
        );
      }
    });
    saveNotificationsEnabled(true);
    setWindowFocused(false);
    await probeNotificationPermission();
  });

  afterEach(async () => {
    await act(async () => {
      onLine(
        JSON.stringify({
          method: "turn/completed",
          params: { turn: { id: "turn_ui", status: "completed" } },
        }),
      );
      await turn;
      await codexAdapter.stopSession(session.id);
      root.unmount();
    });
    container.remove();
    localStorage.clear();
    __codexTestReset();
    vi.unstubAllGlobals();
  });

  it("renders a question, dispatches a banner, and sends the clicked answer", async () => {
    await act(async () =>
      onLine(
        JSON.stringify({
          id: 91,
          method: "item/tool/requestUserInput",
          params: {
            itemId: "q1",
            questions: [
              {
                id: "access",
                header: "Source",
                question: "Read the external source?",
                isOther: false,
                isSecret: false,
                options: [{ label: "Accept" }, { label: "Decline" }],
              },
            ],
          },
        }),
      ),
    );
    expect(
      document.querySelector("[data-question-form]")?.textContent,
    ).toContain("Read the external source?");
    expect(document.querySelector(".approval-toast")?.textContent).toContain(
      "Source",
    );
    expect(sent.some((m) => m.id === 91)).toBe(false);
    expect(await notifySession(session, "needsInput", false)).toBe(true);
    expect(invoke).toHaveBeenCalledWith(
      "show_notification",
      expect.objectContaining({ sessionId: session.id, body: "Source" }),
    );
    const buttons = () => Array.from(container.querySelectorAll("button"));
    await act(async () =>
      buttons()
        .find((b) => b.textContent?.trim() === "Decline")!
        .click(),
    );
    await act(async () =>
      buttons()
        .find((b) => b.textContent?.trim() === "Continue")!
        .click(),
    );
    expect(sent.find((m) => m.id === 91)?.result).toEqual({
      answers: { access: { answers: ["Decline"] } },
    });
    expect(document.querySelector("[data-question-form]")).toBeNull();
    expect(document.querySelector(".approval-toast")).toBeNull();
  });

  it("keeps command approval visible and sends Allow from the toast", async () => {
    await act(async () =>
      onLine(
        JSON.stringify({
          id: 91,
          method: "item/commandExecution/requestApproval",
          params: {
            itemId: "cmd1",
            command: "git status --short",
            reason: "Inspect the workspace",
          },
        }),
      ),
    );
    expect(container.textContent).toContain("Allow");
    expect(document.querySelector(".approval-toast")).not.toBeNull();
    expect(await notifySession(session, "needsInput", false)).toBe(true);
    expect(invoke).toHaveBeenCalledWith(
      "show_notification",
      expect.objectContaining({
        sessionId: session.id,
        body: expect.stringContaining("Approve:"),
      }),
    );
    await act(async () =>
      Array.from(document.querySelectorAll(".approval-toast button"))
        .find((b) => b.textContent?.trim() === "Allow")!
        .dispatchEvent(new MouseEvent("click", { bubbles: true })),
    );
    expect(sent.find((m) => m.id === 91)?.result).toEqual({
      decision: "accept",
    });
    expect(document.querySelector(".approval-toast")).toBeNull();
  });
});
