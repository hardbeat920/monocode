// @vitest-environment happy-dom
import { act, createElement, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  probeNotificationPermission,
  saveNotificationsEnabled,
  setWindowFocused,
} from "../lib/notifications";
import { newSession, type Session } from "../lib/session";
import { useInputNotifications } from "./useInputNotifications";

const invoke = vi.hoisted(() =>
  vi.fn(async (command: string, _args?: Record<string, unknown>) =>
    command === "notification_permission" ? "granted" : undefined,
  ),
);
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

function Notifications({
  sessions,
  activeSessionId,
}: {
  sessions: Session[];
  activeSessionId?: string;
}) {
  useInputNotifications(sessions, activeSessionId);
  return null;
}

function approval(requestId: number, text: string): Session["blocks"][number] {
  return {
    id: `approval-${requestId}`,
    role: "approval",
    text,
    approval: { requestId },
  };
}

function question(
  requestId: number,
  title: string,
): Session["pendingQuestion"] {
  return { requestId, title, questions: [] };
}

function banners() {
  return invoke.mock.calls
    .filter(([command]) => command === "show_notification")
    .map(([, args]) => ({ sessionId: args?.sessionId, body: args?.body }));
}

describe("input notification delivery", () => {
  let root: Root;
  let container: HTMLDivElement;
  let session: Session;

  async function render(sessions: Session[], activeSessionId?: string) {
    await act(async () =>
      root.render(
        createElement(
          StrictMode,
          null,
          createElement(Notifications, { sessions, activeSessionId }),
        ),
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
    });
    invoke.mockClear();
    saveNotificationsEnabled(true);
    setWindowFocused(false);
    await probeNotificationPermission();
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    session = { ...newSession("codex", "/repo"), id: "first", blocks: [] };
    // Mount before requests arrive, matching an already-open application.
    await render([session]);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  it("describes a new approval while an older question is still pending", async () => {
    session = { ...session, pendingQuestion: question(1, "Which source?") };
    await render([session]);
    session = { ...session, blocks: [approval(2, "Read the changelog")] };
    await render([session]);
    expect(banners()).toEqual([
      { sessionId: "first", body: "Which source?" },
      { sessionId: "first", body: "Approve: Read the changelog" },
    ]);
    await render([{ ...session, pendingQuestion: undefined }]);
    expect(banners()).toHaveLength(2);
  });

  it("notifies the next concurrent approval after the first is resolved", async () => {
    session = {
      ...session,
      blocks: [approval(1, "First request"), approval(2, "Second request")],
    };
    await render([session]);
    expect(banners()).toHaveLength(1);
    session = { ...session, blocks: [approval(2, "Second request")] };
    await render([session]);
    expect(banners()).toEqual([
      { sessionId: "first", body: "Approve: First request" },
      { sessionId: "first", body: "Approve: Second request" },
    ]);
    await render([{ ...session, title: "Updated title" }]);
    expect(banners()).toHaveLength(2);
  });

  it("keeps concurrent questions, approvals and sessions with the same request ID distinct", async () => {
    session = {
      ...session,
      blocks: [approval(1, "Read source")],
      pendingQuestion: question(1, "Choose a source"),
    };
    const other = {
      ...session,
      id: "other",
      blocks: [],
      pendingQuestion: question(1, "Choose a branch"),
    };
    await render([session, other]);
    expect(banners()).toHaveLength(2);
    await render([{ ...session, blocks: [] }, other]);
    expect(banners()).toEqual([
      { sessionId: "first", body: "Approve: Read source" },
      { sessionId: "other", body: "Choose a branch" },
      { sessionId: "first", body: "Choose a source" },
    ]);
  });

  it("describes a new question while an approval remains pending", async () => {
    session = { ...session, blocks: [approval(1, "Read source")] };
    await render([session]);
    session = { ...session, pendingQuestion: question(2, "Which branch?") };
    await render([session]);
    expect(banners()).toEqual([
      { sessionId: "first", body: "Approve: Read source" },
      { sessionId: "first", body: "Which branch?" },
    ]);
  });

  it("forgets completed requests and does not replay pending ones on focus changes", async () => {
    session = { ...session, blocks: [approval(1, "Read source")] };
    const sessions = [session];
    await render(sessions, "first");
    await render(sessions, "other");
    expect(banners()).toHaveLength(1);
    await render([{ ...session, blocks: [] }]);
    await render([
      { ...session, blocks: [approval(1, "Read another source")] },
    ]);
    expect(banners()).toEqual([
      { sessionId: "first", body: "Approve: Read source" },
      { sessionId: "first", body: "Approve: Read another source" },
    ]);
  });

  it("retains the notification setting and focused-session policy", async () => {
    setWindowFocused(true);
    session = { ...session, blocks: [approval(1, "Visible request")] };
    await render([session], "first");
    expect(banners()).toEqual([]);
    session = {
      ...session,
      pendingQuestion: question(2, "Hidden session question"),
    };
    await render([session], "other");
    expect(banners()).toEqual([
      { sessionId: "first", body: "Hidden session question" },
    ]);
    saveNotificationsEnabled(false);
    await render(
      [{ ...session, blocks: [approval(3, "Disabled request")] }],
      "other",
    );
    expect(banners()).toHaveLength(1);
  });
});
