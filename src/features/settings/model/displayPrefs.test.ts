// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  loadMaskEmails,
  loadShowRemainingUsage,
  saveMaskEmails,
  saveShowRemainingUsage,
  subscribeMaskEmails,
  subscribeShowRemainingUsage,
} from "./displayPrefs";

const prefs = [
  {
    name: "show remaining usage",
    key: "monocode.showRemainingUsage",
    load: loadShowRemainingUsage,
    save: saveShowRemainingUsage,
    subscribe: subscribeShowRemainingUsage,
  },
  {
    name: "mask emails",
    key: "monocode.maskEmails",
    load: loadMaskEmails,
    save: saveMaskEmails,
    subscribe: subscribeMaskEmails,
  },
];

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe.each(prefs)("$name preference", (pref) => {
  it("defaults off and notifies this window when saved", () => {
    const listener = vi.fn();
    const unsubscribe = pref.subscribe(listener);
    expect(pref.load()).toBe(false);

    pref.save(true);

    expect(pref.load()).toBe(true);
    expect(localStorage.getItem(pref.key)).toBe("1");
    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  it("keeps the saved value in memory when storage rejects the write", () => {
    vi.spyOn(localStorage, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    const listener = vi.fn();
    const unsubscribe = pref.subscribe(listener);
    listener.mockImplementation(() => expect(pref.load()).toBe(true));

    pref.save(true);

    expect(listener).toHaveBeenCalledTimes(1);
    expect(pref.load()).toBe(true);
    unsubscribe();
    pref.save(false);
    expect(pref.load()).toBe(false);
  });

  it("follows a change saved in another window", () => {
    const listener = vi.fn();
    const unsubscribe = pref.subscribe(listener);

    localStorage.setItem(pref.key, "1");
    window.dispatchEvent(new StorageEvent("storage", { key: pref.key }));
    expect(listener).toHaveBeenCalledTimes(1);
    expect(pref.load()).toBe(true);

    window.dispatchEvent(new StorageEvent("storage", { key: "unrelated" }));
    expect(listener).toHaveBeenCalledTimes(1);

    localStorage.removeItem(pref.key);
    window.dispatchEvent(new StorageEvent("storage", { key: null }));
    expect(listener).toHaveBeenCalledTimes(2);
    expect(pref.load()).toBe(false);
    unsubscribe();
  });
});
