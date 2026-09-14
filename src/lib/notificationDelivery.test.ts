// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vitest";
import {
  notifySession,
  saveNotificationsEnabled,
  setWindowFocused,
} from "./notifications";
import { updateNotificationPreferences } from "./notificationPreferences";
import { newSession } from "./session";

const { invoke, play } = vi.hoisted(() => ({ invoke: vi.fn(), play: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("cuelume", () => ({ play, setEnabled: vi.fn(), setVolume: vi.fn() }));
beforeEach(() => {
  localStorage.clear();
  invoke.mockReset();
  play.mockClear();
  invoke.mockImplementation(async (command: string) => {
    if (command === "git_notification_context")
      return {
        root: "/private",
        commonDir: "/private/.git",
        remote: "https://github.com/acme/private.git",
      };
  });
  saveNotificationsEnabled(true);
  setWindowFocused(false);
});

it("blocks every project banner, including approvals and questions, while muted", async () => {
  updateNotificationPreferences(["repository:github.com/acme/private"], {
    mutedUntil: null,
  });
  const session = newSession("claude", "/private");
  expect(await notifySession(session, "finished", false)).toBe(false);
  expect(
    await notifySession(session, { kind: "approval", requestId: 1 }, false),
  ).toBe(false);
  expect(
    await notifySession(session, { kind: "question", requestId: 2 }, false),
  ).toBe(false);
  expect(
    invoke.mock.calls.filter(([command]) => command === "show_notification"),
  ).toEqual([]);
});

it("does not deliver an input event from the mute period if identity resolution finishes after expiry", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(1000);
  let resolveContext!: (value: unknown) => void;
  invoke.mockImplementation((command: string) =>
    command === "git_notification_context"
      ? new Promise((resolve) => {
          resolveContext = resolve;
        })
      : Promise.resolve(),
  );
  try {
    updateNotificationPreferences(["repository:github.com/acme/private"], {
      mutedUntil: 2000,
    });
    const sent = notifySession(
      newSession("claude", "/private"),
      { kind: "question", requestId: 1 },
      false,
    );
    vi.setSystemTime(3000);
    resolveContext({
      root: "/private",
      commonDir: "/private/.git",
      remote: "https://github.com/acme/private.git",
    });
    expect(await sent).toBe(false);
  } finally {
    vi.useRealTimers();
  }
});
