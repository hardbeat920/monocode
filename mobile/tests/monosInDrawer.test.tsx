import { act, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, expect, it, vi } from "vitest";

import { resetMonosInDrawer, useMonosInDrawer } from "@/features/settings/monosInDrawer";

const store = vi.hoisted(() => new Map<string, string>());
vi.mock("expo-sqlite/kv-store", () => ({
  default: {
    getItemSync: (key: string) => store.get(key) ?? null,
    setItem: async (key: string, value: string) => void store.set(key, value),
  },
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

beforeEach(() => {
  store.clear();
  resetMonosInDrawer();
});

function mount() {
  const seen: { value: boolean; set: (next: boolean) => void } = { value: false, set: () => {} };
  function Probe() {
    const [value, set] = useMonosInDrawer();
    useEffect(() => {
      seen.value = value;
      seen.set = set;
    });
    return null;
  }
  const root = createRoot(document.createElement("div"));
  act(() => root.render(<Probe />));
  return { seen, root };
}

it("starts off, with the Monos on the Sessions pages", () => {
  const { seen, root } = mount();
  expect(seen.value).toBe(false);
  act(() => root.unmount());
});

it("is kept once turned on, and read again at the next launch", () => {
  const { seen, root } = mount();
  act(() => seen.set(true));
  expect(seen.value).toBe(true);
  expect(store.get("layout.monosInDrawer")).toBe("true");
  act(() => root.unmount());
  resetMonosInDrawer();
  const again = mount();
  expect(again.seen.value).toBe(true);
  act(() => again.root.unmount());
});
