import { describe, expect, it } from "vitest";
import {
  consumeInstalledUpdate,
  rememberInstalledUpdate,
  type UpdateNoticeStore,
} from "./updateNotice";

function memoryStore(): UpdateNoticeStore {
  const values = new Map<string, string>();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => void values.delete(key),
  };
}

describe("installed update marker", () => {
  it("is consumed once", () => {
    const store = memoryStore();
    rememberInstalledUpdate("0.1.23", store);
    expect(consumeInstalledUpdate(store)).toEqual({ version: "0.1.23" });
    expect(consumeInstalledUpdate(store)).toBeNull();
  });

  it.each(["", "   "])("does not store blank version %j", (version) => {
    const store = memoryStore();
    rememberInstalledUpdate(version, store);
    expect(consumeInstalledUpdate(store)).toBeNull();
  });

  it.each(["not json", "{}", '{"version":3}', '{"version":""}'])(
    "removes malformed marker %j",
    (value) => {
      const store = memoryStore();
      store.setItem("monocode.installedUpdate", value);
      expect(consumeInstalledUpdate(store)).toBeNull();
      expect(store.getItem("monocode.installedUpdate")).toBeNull();
    },
  );

  it("treats missing storage as no marker", () => {
    expect(consumeInstalledUpdate(memoryStore())).toBeNull();
  });

  it.each(["getItem", "setItem", "removeItem"] as const)(
    "contains %s storage errors",
    (method) => {
      const store = memoryStore();
      store[method] = () => {
        throw new Error("storage unavailable");
      };
      expect(() => rememberInstalledUpdate("0.1.23", store)).not.toThrow();
      expect(() => consumeInstalledUpdate(store)).not.toThrow();
    },
  );
});
