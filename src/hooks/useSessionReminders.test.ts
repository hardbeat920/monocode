// @vitest-environment happy-dom
import { act, createElement, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  REMINDER_OPEN,
  REMINDERS_CHANGED,
  type ReminderTarget,
  type SessionReminder,
} from "../lib/sessionReminders";
import { useSessionReminders } from "./useSessionReminders";

const { invoke, listen, message } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  message: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ message }));
vi.mock("../lib/sounds", () => ({
  loadSoundsEnabled: () => false,
  SOUNDS_CHANGE_EVENT: "sounds-change",
}));
vi.mock("../lib/notifications", () => ({
  loadNotificationsEnabled: () => false,
  NOTIFICATIONS_CHANGE_EVENT: "notifications-change",
}));

let root: Root;
let container: HTMLDivElement;
let api: ReturnType<typeof useSessionReminders>;
let stored: SessionReminder[];
let pending: ReminderTarget | null;
let listeners: Map<string, Set<() => void>>;
const onOpen = vi.fn();
const ensureSaved = vi.fn();
const reminder: SessionReminder = {
  sessionId: "saved-session",
  dueAt: 100,
  firedAt: null,
  title: "Continue this work",
  harness: "codex",
  cwd: "/another-project",
};

function Harness() {
  api = useSessionReminders(onOpen, ensureSaved, ["current-session"]);
  return null;
}

async function mount() {
  await act(async () =>
    root.render(createElement(StrictMode, null, createElement(Harness))),
  );
}

async function emit(event: string) {
  await act(async () => {
    listeners.get(event)?.forEach((listener) => listener());
  });
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  stored = [{ ...reminder }];
  pending = null;
  listeners = new Map();
  onOpen.mockReset().mockResolvedValue(undefined);
  ensureSaved.mockReset().mockResolvedValue(undefined);
  message.mockReset();
  invoke
    .mockReset()
    .mockImplementation(
      async (command: string, args: Record<string, any> = {}) => {
        if (command === "reminder_list")
          return stored.map((item) => ({ ...item }));
        if (command === "reminder_take_open") {
          const request = pending;
          pending = null;
          return request;
        }
        if (command === "reminder_open") {
          pending = { sessionId: args.sessionId, dueAt: args.dueAt };
          return;
        }
        if (command === "reminder_set") {
          stored = args.sessionIds.map((id: string) => ({
            ...reminder,
            sessionId: id,
            dueAt: args.dueAt,
            firedAt: null,
          }));
        }
        if (command === "reminder_clear") {
          stored = stored.filter(
            (item) =>
              !args.sessionIds.includes(item.sessionId) ||
              (args.expectedDueAt != null && item.dueAt !== args.expectedDueAt),
          );
        }
      },
    );
  listen
    .mockReset()
    .mockImplementation(async (event: string, handler: () => void) => {
      if (!listeners.has(event)) listeners.set(event, new Set());
      listeners.get(event)!.add(handler);
      return () => listeners.get(event)?.delete(handler);
    });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("saved session reminders", () => {
  it("shows missed reminders on startup even with desktop notifications off", async () => {
    await mount();
    expect(api.due).toEqual([reminder]);
    expect(invoke).toHaveBeenCalledWith("reminder_configure", {
      preferences: { notificationsEnabled: false, sound: false },
    });
    expect(listeners.get(REMINDERS_CHANGED)?.size).toBe(1);
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("saves the conversation before scheduling and does not claim success on failure", async () => {
    await mount();
    const failure = new Error("Save failed");
    ensureSaved.mockRejectedValueOnce(failure);
    await act(async () =>
      api.schedule([reminder.sessionId], Date.now() + 60_000),
    );
    expect(
      invoke.mock.calls.some(([command]) => command === "reminder_set"),
    ).toBe(false);
    expect(message).toHaveBeenCalledWith(
      "Error: Save failed",
      expect.anything(),
    );
    expect(api.reminders).toEqual([reminder]);
    await act(async () =>
      api.schedule([reminder.sessionId], Date.now() + 60_000),
    );
    expect(api.due).toEqual([]);
    expect(ensureSaved).toHaveBeenCalledWith([reminder.sessionId]);
  });

  it("routes opening through the backend, then loads the saved session and clears its due reminder", async () => {
    await mount();
    await act(async () => api.open(reminder));
    expect(onOpen).not.toHaveBeenCalled();
    await emit(REMINDER_OPEN);
    expect(onOpen).toHaveBeenCalledExactlyOnceWith(reminder.sessionId);
    expect(invoke).toHaveBeenCalledWith("reminder_clear", {
      sessionIds: [reminder.sessionId],
      expectedDueAt: reminder.dueAt,
    });
    expect(api.due).toEqual([]);
  });

  it("handles a notification click queued before the window mounted", async () => {
    pending = reminder;
    await mount();
    expect(onOpen).toHaveBeenCalledExactlyOnceWith(reminder.sessionId);
    expect(api.due).toEqual([]);
  });

  it("keeps the reminder if opening the session fails", async () => {
    onOpen.mockRejectedValue(new Error("Unavailable"));
    pending = reminder;
    await mount();
    expect(api.due).toEqual([reminder]);
    expect(
      invoke.mock.calls.some(([command]) => command === "reminder_clear"),
    ).toBe(false);
  });

  it("updates when another window cancels and removes subscriptions on unmount", async () => {
    await mount();
    stored = [];
    await emit(REMINDERS_CHANGED);
    expect(api.reminders).toEqual([]);
    act(() => root.unmount());
    expect(listeners.get(REMINDERS_CHANGED)?.size).toBe(0);
    expect(listeners.get(REMINDER_OPEN)?.size).toBe(0);
    root = createRoot(container);
  });
});
