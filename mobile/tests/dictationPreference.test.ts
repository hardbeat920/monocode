import { beforeEach, expect, it, vi } from "vitest";

const stored = new Map<string, string>();
let failWrites = false;
vi.mock("expo-sqlite/kv-store", () => ({
  default: {
    getItemSync: (key: string) => stored.get(key) ?? null,
    setItem: async (key: string, value: string) => {
      if (failWrites) throw new Error("disk full");
      stored.set(key, value);
    },
  },
}));

beforeEach(() => {
  stored.clear();
  failWrites = false;
  vi.resetModules();
});

const load = () => import("@/features/settings/dictation");

it("starts on Apple Speech in the phone's language, with Parakeet TDT v3 for Parakeet", async () => {
  const { getDictationPreference } = await load();
  expect(getDictationPreference()).toEqual({ engine: "apple", language: null, parakeetModel: "v3" });
});

it("keeps the picked engine and language across launches", async () => {
  const first = await load();
  first.setDictationPreference({ language: "ru-RU" });
  first.setDictationPreference({ engine: "parakeet" });
  vi.resetModules();
  const relaunched = await load();
  expect(relaunched.getDictationPreference()).toEqual({ engine: "parakeet", language: "ru-RU", parakeetModel: "v3" });
});

it("goes back to Automatic when the language is cleared", async () => {
  const { getDictationPreference, setDictationPreference } = await load();
  setDictationPreference({ language: "uk-UA" });
  setDictationPreference({ language: null });
  expect(getDictationPreference().language).toBeNull();
});

it("reads unknown or damaged values as the default", async () => {
  const { parseDictationPreference } = await load();
  expect(parseDictationPreference("{")).toEqual({ engine: "apple", language: null, parakeetModel: "v3" });
  expect(parseDictationPreference('{"engine":"whisper","language":"","parakeetModel":"tiny"}')).toEqual({
    engine: "apple",
    language: null,
    parakeetModel: "v3",
  });
  expect(parseDictationPreference('{"engine":"parakeet","language":"uk-UA","parakeetModel":"v2"}')).toEqual({
    engine: "parakeet",
    language: "uk-UA",
    parakeetModel: "v2",
  });
});

it("keeps a pick for this launch and reports a failed save", async () => {
  const { getDictationPreference, setDictationPreference } = await load();
  failWrites = true;
  await expect(setDictationPreference({ language: "ru-RU" })).rejects.toThrow("disk full");
  expect(getDictationPreference().language).toBe("ru-RU");
});

it("writes picks in the order they were made", async () => {
  const { setDictationPreference } = await load();
  await Promise.all([
    setDictationPreference({ language: "uk-UA" }),
    setDictationPreference({ language: "ru-RU" }),
  ]);
  expect(JSON.parse(stored.get("dictation.preference")!)).toMatchObject({ language: "ru-RU" });
});
