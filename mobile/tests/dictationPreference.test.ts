import { beforeEach, expect, it, vi } from "vitest";

const stored = new Map<string, string>();
vi.mock("expo-sqlite/kv-store", () => ({
  default: {
    getItemSync: (key: string) => stored.get(key) ?? null,
    setItem: async (key: string, value: string) => {
      stored.set(key, value);
    },
  },
}));

beforeEach(() => {
  stored.clear();
  vi.resetModules();
});

const load = () => import("@/features/settings/dictation");

it("starts on Apple Speech in the phone's language", async () => {
  const { getDictationPreference } = await load();
  expect(getDictationPreference()).toEqual({ engine: "apple", language: null });
});

it("keeps the picked engine and language across launches", async () => {
  const first = await load();
  first.setDictationPreference({ language: "ru-RU" });
  first.setDictationPreference({ engine: "parakeet" });
  vi.resetModules();
  const relaunched = await load();
  expect(relaunched.getDictationPreference()).toEqual({ engine: "parakeet", language: "ru-RU" });
});

it("goes back to Automatic when the language is cleared", async () => {
  const { getDictationPreference, setDictationPreference } = await load();
  setDictationPreference({ language: "uk-UA" });
  setDictationPreference({ language: null });
  expect(getDictationPreference().language).toBeNull();
});

it("reads unknown or damaged values as the default", async () => {
  const { parseDictationPreference } = await load();
  expect(parseDictationPreference("{")).toEqual({ engine: "apple", language: null });
  expect(parseDictationPreference('{"engine":"whisper","language":""}')).toEqual({
    engine: "apple",
    language: null,
  });
  expect(parseDictationPreference('{"engine":"parakeet","language":"uk-UA"}')).toEqual({
    engine: "parakeet",
    language: "uk-UA",
  });
});
