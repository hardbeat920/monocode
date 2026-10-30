import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useEmptyBackground } from "@/features/background/useEmptyBackground";

const mock = vi.hoisted(() => ({
  storage: new Map<string, string>(),
  light: false,
  render: vi.fn(
    async (
      _uri: string,
      key: string,
      effect: string,
      light: boolean,
      empty: boolean,
    ) => `${key}.${effect}.${light}.${empty}.png`,
  ),
}));
vi.mock("expo-sqlite/kv-store", () => ({
  default: {
    getItemSync: (key: string) => mock.storage.get(key),
    setItemSync: (key: string, value: string) => mock.storage.set(key, value),
  },
}));
vi.mock("expo-file-system", () => ({
  File: class {
    uri: string;
    exists = false;
    constructor(_dir: unknown, name: string) {
      this.uri = name;
    }
  },
}));
vi.mock("expo-image-picker", () => ({ launchImageLibraryAsync: vi.fn() }));
vi.mock("@/ui/theme", () => ({
  useScheme: () => (mock.light ? "light" : "dark"),
}));
vi.mock("@/features/background/render", () => ({
  BACKGROUNDS: {},
  renderEffect: mock.render,
  forgetRenders: vi.fn(),
  prepareImage: vi.fn(),
}));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root | undefined;
let controls: ReturnType<typeof useEmptyBackground>;
let mirror: ReturnType<typeof useEmptyBackground>;
function Probe({ empty = true, project = "one" }) {
  controls = useEmptyBackground(project, empty);
  mirror = useEmptyBackground(project, empty);
  return null;
}
async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
}
afterEach(async () => {
  await act(async () => root?.unmount());
  root = undefined;
  mock.storage.clear();
  mock.light = false;
  mock.render.mockClear();
});
describe("background preferences and rendered display", () => {
  it("persists edits across remounts, updates other mounted consumers and selects the actual renderer", async () => {
    mock.storage.set(
      "background:one",
      JSON.stringify({ image: "saved.jpg", opacity: 0.45 }),
    );
    root = createRoot(document.createElement("div"));
    await act(async () => root!.render(<Probe />));
    await settle();
    expect(controls.background?.source).toMatchObject({
      uri: expect.stringContaining("dither.false.true"),
    });
    await act(async () =>
      controls.update({
        effect: "ascii",
        scope: "all",
        opacity: 0.6,
        sessionOpacity: 0.12,
      }),
    );
    await settle();
    expect(mirror.settings).toEqual(controls.settings);
    expect(controls.background?.source).toMatchObject({
      uri: expect.stringContaining("ascii.false.true"),
    });
    await act(async () => root!.render(<Probe empty={false} />));
    await settle();
    expect(controls.background?.opacity).toBe(0.12);
    expect(controls.background?.source).toMatchObject({
      uri: expect.stringContaining("ascii.false.false"),
    });
    await act(async () => controls.update({ scope: "empty" }));
    expect(controls.background).toBeNull();
    await act(async () => root!.unmount());
    root = createRoot(document.createElement("div"));
    await act(async () => root!.render(<Probe />));
    await settle();
    expect(controls.settings).toMatchObject({
      image: "saved.jpg",
      effect: "ascii",
      scope: "empty",
      opacity: 0.6,
      sessionOpacity: 0.12,
    });
    expect(controls.background?.opacity).toBe(0.6);
  });
  it("separates projects and scheme variants and clears the rendered photo on removal", async () => {
    mock.storage.set(
      "background:one",
      JSON.stringify({ image: "one.jpg", effect: "halftone" }),
    );
    mock.storage.set(
      "background:two",
      JSON.stringify({ image: "two.jpg", effect: "scanlines" }),
    );
    root = createRoot(document.createElement("div"));
    await act(async () => root!.render(<Probe />));
    await settle();
    mock.light = true;
    await act(async () => root!.render(<Probe project="two" />));
    await settle();
    expect(controls.background?.source).toMatchObject({
      uri: expect.stringContaining("two.jpg.scanlines.true.true"),
    });
    await act(async () => controls.removeImage());
    expect(controls.background).toBeNull();
    expect(JSON.parse(mock.storage.get("background:one")!).image).toBe(
      "one.jpg",
    );
    expect(JSON.parse(mock.storage.get("background:two")!).image).toBeNull();
  });
});
