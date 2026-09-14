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
  noteInboxUnseen,
  playCue,
  resetSoundCues,
  saveSoundsEnabled,
  SOUNDS_DEFAULT,
  SOUNDS_VOLUME,
} from "./sounds";

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
    playCue("turnFinished");
    expect(setVolume).toHaveBeenCalledWith(SOUNDS_VOLUME);
    expect(play).toHaveBeenCalledWith("success");
    playCue("inboxUnseen");
    expect(play).toHaveBeenCalledWith("bloom");
    playCue("linkedActivity");
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
    playCue("turnFinished");
    expect(play).not.toHaveBeenCalled();
  });

  it("does not ding for the first inbox snapshot", () => {
    noteInboxUnseen(true);
    expect(play).not.toHaveBeenCalled();
  });

  it("dings once when the inbox dot appears, then again after it clears", () => {
    noteInboxUnseen(false);
    noteInboxUnseen(true);
    expect(play).toHaveBeenCalledTimes(1);
    expect(play).toHaveBeenCalledWith("bloom");
    noteInboxUnseen(true);
    expect(play).toHaveBeenCalledTimes(1);
    noteInboxUnseen(false);
    noteInboxUnseen(true);
    expect(play).toHaveBeenCalledTimes(2);
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
