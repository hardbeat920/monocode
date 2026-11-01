import { beforeEach, describe, expect, it, vi } from "vitest";

const store = vi.hoisted(() => new Map<string, string>());
vi.mock("expo-sqlite/kv-store", () => ({
  default: {
    getItemSync: (key: string) => store.get(key) ?? null,
    setItem: async (key: string, value: string) => void store.set(key, value),
  },
}));

import {
  DEFAULT_PREFS,
  parsePrefs,
  readMonoGroupPrefs,
  resetMonoGroupPrefs,
  setMonoGroupPrefs,
} from "@/features/sessions/monoGroupPrefs";

beforeEach(() => {
  store.clear();
  resetMonoGroupPrefs();
});

describe("the pinned Monos group's settings", () => {
  it("starts shown and open", () => {
    expect(readMonoGroupPrefs("desk")).toEqual({ shown: true, collapsed: false });
    expect(readMonoGroupPrefs(undefined)).toEqual(DEFAULT_PREFS);
  });

  it("is remembered per desktop", () => {
    setMonoGroupPrefs("one", { shown: false });
    setMonoGroupPrefs("two", { collapsed: true });
    expect(readMonoGroupPrefs("one")).toEqual({ shown: false, collapsed: false });
    expect(readMonoGroupPrefs("two")).toEqual({ shown: true, collapsed: true });
    // As the next launch reads it.
    resetMonoGroupPrefs();
    expect(readMonoGroupPrefs("one")).toEqual({ shown: false, collapsed: false });
    expect(readMonoGroupPrefs("two")).toEqual({ shown: true, collapsed: true });
  });

  it("keeps the other setting when one changes", () => {
    setMonoGroupPrefs("one", { collapsed: true });
    setMonoGroupPrefs("one", { shown: false });
    expect(readMonoGroupPrefs("one")).toEqual({ shown: false, collapsed: true });
  });

  it("falls back to the defaults for anything unreadable", () => {
    expect(parsePrefs("{")).toEqual(DEFAULT_PREFS);
    expect(parsePrefs(JSON.stringify({ shown: "no", collapsed: 1 }))).toEqual(DEFAULT_PREFS);
    expect(parsePrefs(JSON.stringify({ shown: false }))).toEqual({ shown: false, collapsed: false });
  });
});
