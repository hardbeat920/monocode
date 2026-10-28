import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import {
  SessionNotifications,
  useNotificationsBlocked,
} from "@/features/notifications/SessionNotifications";

const state = vi.hoisted(() => ({
  os: "android",
  appState: "active",
  paired: true,
  enabled: true,
  sessions: [] as { id: string; projectId: string; busy: boolean; waiting: string | null }[],
  settings: { status: "undetermined", canAskAgain: true } as {
    status: string;
    canAskAgain: boolean;
    ios?: { status: number };
  },
  onAppState: undefined as ((next: string) => void) | undefined,
  getPermissions: vi.fn(),
  requestPermissions: vi.fn(),
  setChannel: vi.fn(),
  schedule: vi.fn(),
}));

vi.mock("react-native", () => ({
  Platform: { get OS() { return state.os; } },
  AppState: {
    get currentState() { return state.appState; },
    addEventListener: (_event: string, listener: (next: string) => void) => {
      state.onAppState = listener;
      return { remove: vi.fn() };
    },
  },
}));
vi.mock("expo-notifications", () => ({
  setNotificationHandler: vi.fn(),
  getPermissionsAsync: state.getPermissions,
  requestPermissionsAsync: state.requestPermissions,
  setNotificationChannelAsync: state.setChannel,
  scheduleNotificationAsync: state.schedule,
  useLastNotificationResponse: () => null,
  DEFAULT_ACTION_IDENTIFIER: "default",
  AndroidImportance: { DEFAULT: 3 },
  IosAuthorizationStatus: { NOT_DETERMINED: 0 },
}));
vi.mock("expo-router", () => ({ router: { push: vi.fn() } }));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({
    pairing: state.paired ? "desktop" : null,
    connected: state.paired,
    catalog: { sessions: state.sessions, projects: [] },
  }),
}));
vi.mock("@/features/settings/notifications", () => ({
  NOTIFICATION_CATEGORIES: [{ id: "agentFinished" }],
  useNotificationPreferences: () => ({ enabled: () => state.enabled }),
}));

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot>;
const render = () => act(async () => root.render(<SessionNotifications />));

beforeEach(() => {
  vi.clearAllMocks();
  state.os = "android";
  state.appState = "active";
  state.paired = true;
  state.enabled = true;
  state.sessions = [];
  state.settings = { status: "undetermined", canAskAgain: true };
  state.getPermissions.mockImplementation(async () => state.settings);
  state.requestPermissions.mockResolvedValue({ status: "granted" });
  state.setChannel.mockResolvedValue(null);
  state.schedule.mockResolvedValue("notification");
  root = createRoot(document.createElement("div"));
});
afterEach(() => act(() => root.unmount()));

it("creates the Android channel before requesting notification permission", async () => {
  // Expo may report denied before the first Android prompt, with canAskAgain true.
  state.settings.status = "denied";
  await render();
  expect(state.setChannel).toHaveBeenCalledWith("sessions", {
    name: "Session alerts", importance: 3,
  });
  expect(state.requestPermissions).toHaveBeenCalledOnce();
  expect(state.setChannel.mock.invocationCallOrder[0]).toBeLessThan(
    state.requestPermissions.mock.invocationCallOrder[0],
  );
});

it.each([
  ["unpaired", false, true],
  ["alerts disabled", true, false],
])("does not prompt when %s", async (_name, paired, enabled) => {
  state.paired = paired;
  state.enabled = enabled;
  await render();
  expect(state.setChannel).not.toHaveBeenCalled();
  expect(state.requestPermissions).not.toHaveBeenCalled();
});

it.each([
  ["granted", true],
  ["denied", false],
])("does not prompt when permission is %s and canAskAgain is %s", async (status, canAskAgain) => {
  state.settings = { status, canAskAgain };
  await render();
  expect(state.requestPermissions).not.toHaveBeenCalled();
});

it("keeps the iOS permission request without creating an Android channel", async () => {
  state.os = "ios";
  state.settings.ios = { status: 0 };
  await render();
  expect(state.requestPermissions).toHaveBeenCalledOnce();
  expect(state.setChannel).not.toHaveBeenCalled();
});

it("does not prompt again for provisional iOS authorization", async () => {
  state.os = "ios";
  state.settings.ios = { status: 3 };
  await render();
  expect(state.requestPermissions).not.toHaveBeenCalled();
});

it.each(["android", "ios"])("routes immediate session alerts on %s", async (os) => {
  state.os = os;
  state.sessions = [{ id: "session", projectId: "project", busy: true, waiting: null }];
  await render();
  state.appState = "background";
  state.sessions = [{ ...state.sessions[0], busy: false }];
  await render();
  expect(state.schedule).toHaveBeenCalledWith(expect.objectContaining({
    content: expect.objectContaining({ body: "Finished" }),
    trigger: os === "android" ? { channelId: "sessions" } : null,
  }));
});

it.each([true, false])("reflects Android notification denial with canAskAgain %s and updates after returning from Settings", async (canAskAgain) => {
  let blocked = false;
  function Probe() {
    blocked = useNotificationsBlocked();
    return null;
  }
  state.settings = { status: "denied", canAskAgain };
  await act(async () => root.render(<Probe />));
  expect(blocked).toBe(!canAskAgain);
  state.settings.status = "granted";
  await act(async () => state.onAppState?.("active"));
  expect(blocked).toBe(false);
});
