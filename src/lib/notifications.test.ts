import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  loadNotificationsEnabled,
  NOTIFICATIONS_DEFAULT,
  notificationText,
  saveNotificationsEnabled,
  shouldNotify,
} from "./notifications";
import { newSession, type Session } from "./session";

const KEY = "monocode.notifications";

function chat(patch: Partial<Session> = {}): Session {
  const session = newSession("claude", "/tmp/a");
  session.title = "claude · Fix the sidebar";
  session.blocks = [{ id: "u1", role: "user", text: "hello" }];
  return { ...session, ...patch, blocks: patch.blocks ?? session.blocks };
}

describe("notifications setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => localStorage.removeItem(KEY));

  it("is off until the user opts in", () => {
    expect(NOTIFICATIONS_DEFAULT).toBe(false);
    expect(loadNotificationsEnabled()).toBe(false);
  });

  it("round-trips", () => {
    saveNotificationsEnabled(true);
    expect(loadNotificationsEnabled()).toBe(true);
    saveNotificationsEnabled(false);
    expect(loadNotificationsEnabled()).toBe(false);
  });
});

describe("shouldNotify", () => {
  it("stays quiet while the session is on screen in a focused window", () => {
    expect(
      shouldNotify({
        enabled: true,
        permission: "granted",
        windowFocused: true,
        sessionVisible: true,
      }),
    ).toBe(false);
  });

  it("fires for a session that is not on screen even when focused", () => {
    expect(
      shouldNotify({
        enabled: true,
        permission: "granted",
        windowFocused: true,
        sessionVisible: false,
      }),
    ).toBe(true);
  });

  it("respects the toggle and the OS decision", () => {
    expect(
      shouldNotify({
        enabled: false,
        permission: "granted",
        windowFocused: false,
        sessionVisible: true,
      }),
    ).toBe(false);
    expect(
      shouldNotify({
        enabled: true,
        permission: "denied",
        windowFocused: false,
        sessionVisible: true,
      }),
    ).toBe(false);
    expect(
      shouldNotify({
        enabled: true,
        permission: "unsupported",
        windowFocused: false,
        sessionVisible: true,
      }),
    ).toBe(false);
  });

  it("fires when unfocused and allowed, or still undecided", () => {
    expect(
      shouldNotify({
        enabled: true,
        permission: "granted",
        windowFocused: false,
        sessionVisible: true,
      }),
    ).toBe(true);
    expect(
      shouldNotify({
        enabled: true,
        permission: "prompt",
        windowFocused: false,
        sessionVisible: true,
      }),
    ).toBe(true);
  });
});

describe("notificationText", () => {
  it("leads with the app, then the session title, then the reply", () => {
    const session = chat({
      blocks: [
        { id: "u1", role: "user", text: "hello" },
        {
          id: "a1",
          role: "assistant",
          text: "\n\nDone. Sidebar\nfixed.\n\nDetails below.",
        },
      ],
    });
    expect(notificationText(session, "finished")).toEqual({
      title: "MonoCode",
      subtitle: "Fix the sidebar",
      body: "Done. Sidebar fixed.",
    });
  });

  it("falls back to a generic body without a reply", () => {
    expect(notificationText(chat(), "finished").body).toBe(
      "Claude Code finished",
    );
  });

  it("clips long bodies", () => {
    const session = chat({
      blocks: [{ id: "a1", role: "assistant", text: "x".repeat(400) }],
    });
    const body = notificationText(session, "finished").body;
    expect(body.length).toBe(240);
    expect(body.endsWith("…")).toBe(true);
  });

  it("names the pending approval", () => {
    const session = chat({
      blocks: [
        {
          id: "p1",
          role: "approval",
          text: "Run npm test",
          tool: { title: "Run npm test" },
          approval: { requestId: 1 },
        },
      ],
    });
    expect(notificationText(session, "needsInput")).toEqual({
      title: "MonoCode",
      subtitle: "Fix the sidebar",
      body: "Approve: Run npm test",
    });
  });

  it("prefers the question prompt over an approval", () => {
    const session = chat({
      pendingQuestion: {
        requestId: 2,
        questions: [
          {
            id: "q",
            prompt: "Which database?",
            multiSelect: false,
            allowCustom: false,
            options: [],
          },
        ],
      },
    });
    expect(notificationText(session, "needsInput")).toEqual({
      title: "MonoCode",
      subtitle: "Fix the sidebar",
      body: "Which database?",
    });
  });
});

function mockLocalStorage() {
  const data = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => data.get(key) ?? null,
      setItem: (key: string, value: string) => {
        data.set(key, value);
      },
      removeItem: (key: string) => {
        data.delete(key);
      },
    },
  });
}
