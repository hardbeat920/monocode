// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  loadComposerAutocorrect,
  loadMaskEmails,
  loadShowRemainingUsage,
  loadVisualReplies,
  saveComposerAutocorrect,
  saveMaskEmails,
  saveShowRemainingUsage,
  saveVisualReplies,
  subscribeComposerAutocorrect,
  subscribeMaskEmails,
  subscribeShowRemainingUsage,
  subscribeVisualReplies,
} from "./displayPrefs";

const prefs = [
  {
    name: "show remaining usage",
    key: "monocode.showRemainingUsage",
    defaultValue: false,
    load: loadShowRemainingUsage,
    save: saveShowRemainingUsage,
    subscribe: subscribeShowRemainingUsage,
  },
  {
    name: "mask emails",
    key: "monocode.maskEmails",
    defaultValue: true,
    load: loadMaskEmails,
    save: saveMaskEmails,
    subscribe: subscribeMaskEmails,
  },
  {
    name: "composer autocorrect",
    key: "monocode.composerAutocorrect",
    defaultValue: true,
    load: loadComposerAutocorrect,
    save: saveComposerAutocorrect,
    subscribe: subscribeComposerAutocorrect,
  },
  {
    name: "visual replies",
    key: "monocode.visualReplies",
    defaultValue: true,
    load: loadVisualReplies,
    save: saveVisualReplies,
    subscribe: subscribeVisualReplies,
  },
];

afterEach(() => {
  vi.restoreAllMocks();
  prefs.forEach((pref) => pref.save(pref.defaultValue));
  localStorage.clear();
});

describe.each(prefs)("$name preference", (pref) => {
  it("uses its default and notifies this window when saved", () => {
    const listener = vi.fn();
    const unsubscribe = pref.subscribe(listener);
    expect(pref.load()).toBe(pref.defaultValue);

    const value = !pref.defaultValue;
    pref.save(value);

    expect(pref.load()).toBe(value);
    expect(localStorage.getItem(pref.key)).toBe(value ? "1" : "0");
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

    const value = !pref.defaultValue;
    localStorage.setItem(pref.key, value ? "1" : "0");
    window.dispatchEvent(new StorageEvent("storage", { key: pref.key }));
    expect(listener).toHaveBeenCalledTimes(1);
    expect(pref.load()).toBe(value);

    window.dispatchEvent(new StorageEvent("storage", { key: "unrelated" }));
    expect(listener).toHaveBeenCalledTimes(1);

    localStorage.removeItem(pref.key);
    window.dispatchEvent(new StorageEvent("storage", { key: null }));
    expect(listener).toHaveBeenCalledTimes(2);
    expect(pref.load()).toBe(pref.defaultValue);
    unsubscribe();
  });
});
