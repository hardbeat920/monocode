import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const play = vi.fn();
const setEnabled = vi.fn();
const setVolume = vi.fn();

vi.mock("cuelume", () => ({
  play: (...args: unknown[]) => play(...args),
  setEnabled: (...args: unknown[]) => setEnabled(...args),
  setVolume: (...args: unknown[]) => setVolume(...args),
}));

import {
  announceUpdateAvailable,
  loadSoundsEnabled,
  playCue,
  resetSoundCues,
  saveSoundsEnabled,
  SOUNDS_DEFAULT,
  SOUNDS_VOLUME,
} from "./sounds";
import { updateNotificationPreferences } from "./notificationPreferences";

const KEY = "monocode.sounds";

function mockLocalStorage() {
  const data = new Map<string, string>();
  const storage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => {
      data.clear();
    },
    key: (index: number) => [...data.keys()][index] ?? null,
    get length() {
      return data.size;
    },
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: storage,
    configurable: true,
  });
}

describe("sounds", () => {
  beforeEach(() => {
    mockLocalStorage();
    play.mockClear();
    setEnabled.mockClear();
    setVolume.mockClear();
    resetSoundCues();
  });

  afterEach(() => {
    localStorage.removeItem(KEY);
    resetSoundCues();
  });

  it("defaults to on", () => {
    expect(SOUNDS_DEFAULT).toBe(true);
    expect(loadSoundsEnabled()).toBe(true);
  });

  it("persists an off switch", () => {
    saveSoundsEnabled(false);
    expect(localStorage.getItem(KEY)).toBe("0");
    expect(loadSoundsEnabled()).toBe(false);
    expect(setEnabled).toHaveBeenCalledWith(false);
    saveSoundsEnabled(true);
    expect(loadSoundsEnabled()).toBe(true);
    expect(setEnabled).toHaveBeenLastCalledWith(true);
  });

  it("plays the mapped cue when enabled", () => {
    playCue("turnFinished", { projectId: "work", category: "agentFinished" });
    expect(setVolume).toHaveBeenCalledWith(SOUNDS_VOLUME);
    expect(play).toHaveBeenCalledWith("success");
    playCue("inboxUnseen", { projectId: "work", category: "issues" });
    expect(play).toHaveBeenCalledWith("bloom");
    playCue("linkedActivity", { projectId: "work", category: "pullRequests" });
    expect(play).toHaveBeenCalledWith("chime");
    playCue("updateAvailable");
    expect(play).toHaveBeenCalledWith("arrival");
    playCue("switch");
    expect(play).toHaveBeenCalledWith("toggle");
    playCue("copy");
    expect(play).toHaveBeenCalledWith("scan");
  });

  it("is silent when muted", () => {
    saveSoundsEnabled(false);
    play.mockClear();
    playCue("turnFinished", { projectId: "work", category: "agentFinished" });
    expect(play).not.toHaveBeenCalled();
  });

  it("suppresses project cues without silencing allowed projects or app-wide cues", () => {
    updateNotificationPreferences(["private"], { mutedUntil: null });
    playCue("turnFinished", {
      projectId: "private",
      category: "agentFinished",
    });
    expect(play).not.toHaveBeenCalled();
    playCue("inboxUnseen", { projectId: "work", category: "pullRequests" });
    playCue("updateAvailable");
    expect(play.mock.calls).toEqual([["bloom"], ["arrival"]]);
  });

  it("does not catch up on project activity from before global sounds were re-enabled", () => {
    vi.useFakeTimers();
    vi.setSystemTime(1000);
    try {
      saveSoundsEnabled(false);
      vi.setSystemTime(2000);
      saveSoundsEnabled(true);
      playCue("inboxUnseen", {
        projectId: "work",
        category: "issues",
        occurredAt: 1500,
      });
      expect(play).not.toHaveBeenCalled();
      playCue("inboxUnseen", {
        projectId: "work",
        category: "issues",
        occurredAt: 2050,
      });
      expect(play).toHaveBeenCalledExactlyOnceWith("bloom");
    } finally {
      vi.useRealTimers();
    }
  });

  it("dings once per update version", () => {
    announceUpdateAvailable("0.2.0");
    expect(play).toHaveBeenCalledTimes(1);
    expect(play).toHaveBeenCalledWith("arrival");
    announceUpdateAvailable("0.2.0");
    expect(play).toHaveBeenCalledTimes(1);
    announceUpdateAvailable("0.2.1");
    expect(play).toHaveBeenCalledTimes(2);
  });
});
