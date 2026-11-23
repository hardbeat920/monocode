// @vitest-environment happy-dom
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type Session } from "../../sessions/model/session";
import {
  buildActivity,
  DISCORD_STATUS_LABEL,
  discordPresenceStatus,
  loadDiscordPresenceEnabled,
  rememberDiscordActivity,
  saveDiscordPresenceEnabled,
  setDiscordPresence,
  subscribeDiscordPresence,
  truncateUtf8,
} from "./discord";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

beforeEach(() => {
  localStorage.clear();
  vi.mocked(invoke).mockReset().mockResolvedValue(undefined);
  rememberDiscordActivity(null);
});

afterEach(() => {
  localStorage.clear();
});

describe("discord presence preference", () => {
  it("defaults off and persists changes with a change event", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeDiscordPresence(listener);
    expect(loadDiscordPresenceEnabled()).toBe(false);

    saveDiscordPresenceEnabled(true);

    expect(loadDiscordPresenceEnabled()).toBe(true);
    expect(localStorage.getItem("monocode.discordPresence")).toBe("1");
    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  it("notices a change made in another window", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeDiscordPresence(listener);
    localStorage.setItem("monocode.discordPresence", "1");
    window.dispatchEvent(
      new StorageEvent("storage", { key: "monocode.discordPresence" }),
    );
    expect(listener).toHaveBeenCalledTimes(1);
    expect(loadDiscordPresenceEnabled()).toBe(true);
    unsubscribe();
  });
});

describe("discord presence commands", () => {
  it("sends the enabled flag and activity to the backend", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("connected");
    const activity = { state: "Claude · repo", details: "Fix login", startedAt: 42 };

    expect(await setDiscordPresence(true, activity)).toBe("connected");
    expect(invoke).toHaveBeenCalledWith("discord_presence_set", {
      enabled: true,
      activity,
    });
  });

  it("resends the remembered activity when toggled from Settings", async () => {
    const activity = { state: "state", details: "details", startedAt: 7 };
    rememberDiscordActivity(activity);

    await setDiscordPresence(false);

    expect(invoke).toHaveBeenCalledWith("discord_presence_set", {
      enabled: false,
      activity,
    });
  });

  it("reports an error when the command fails or returns an unknown status", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("no IPC"));
    expect(await setDiscordPresence(true)).toBe("error");

    vi.mocked(invoke).mockResolvedValueOnce("surprise");
    expect(await discordPresenceStatus()).toBe("error");
  });

  it("labels every backend status", () => {
    expect(Object.keys(DISCORD_STATUS_LABEL)).toEqual([
      "disabled",
      "connecting",
      "connected",
      "unavailable",
      "error",
    ]);
    expect(DISCORD_STATUS_LABEL.unavailable).toBe("Discord not running");
  });
});

describe("discord activity details", () => {
  const session = (title: string, busy: boolean) =>
    ({
      title,
      harness: "claude",
      busy,
      cwd: "/repo",
    }) as unknown as Session;

  it("trims at a character boundary without splitting multi-byte characters", () => {
    const trimmed = truncateUtf8("😀".repeat(40), 10);
    expect(trimmed).toBe("😀😀");
    expect(new TextEncoder().encode(trimmed).length).toBe(8);
  });

  it("reserves room for the idle suffix", () => {
    const details = buildActivity(session("A".repeat(200), false), "/repo", 0, 1)
      .details;
    expect(details.endsWith(" · idle")).toBe(true);
    expect(new TextEncoder().encode(details).length).toBeLessThanOrEqual(128);
  });

  it("bounds busy details and browsing details", () => {
    const busy = buildActivity(session("😀".repeat(100), true), "/repo", 0, 1);
    expect(new TextEncoder().encode(busy.details).length).toBeLessThanOrEqual(
      128,
    );
    const browsing = buildActivity(undefined, "/repo", 3, 1);
    expect(
      new TextEncoder().encode(browsing.details).length,
    ).toBeLessThanOrEqual(128);
  });
});
