// @vitest-environment happy-dom

import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useProjectBackgroundEffect } from "./useProjectBackgroundEffect";
import { prepareNewThreadBackgroundEffect } from "../../settings/model/newThreadBackgroundEffects";
import type { NewThreadBackgroundEffect } from "../../settings/model/appearance";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => path,
}));
vi.mock("../../settings/model/newThreadBackgroundEffects", () => ({
  prepareNewThreadBackgroundEffect: vi.fn(async () => new Blob(["image"])),
  applyPreparedNewThreadBackground: vi.fn(),
  clearPreparedNewThreadBackground: vi.fn(),
}));

let container: HTMLDivElement;
let root: Root;
const NativeURL = URL;
const createObjectURL = vi.fn(() => "blob:processed");
const revokeObjectURL = vi.fn();

function Probe({ effect }: { effect: NewThreadBackgroundEffect }) {
  const url = useProjectBackgroundEffect("/background.png", effect, 101);
  return createElement("div", { "data-url": url });
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  class TestURL extends NativeURL {
    static createObjectURL = createObjectURL;
    static revokeObjectURL = revokeObjectURL;
  }
  vi.stubGlobal("URL", TestURL);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe("project background effects", () => {
  it("uses the original for None and a pane-local processed URL for Dither", async () => {
    await act(async () =>
      root.render(createElement(Probe, { effect: "none" })),
    );
    expect(container.firstElementChild?.getAttribute("data-url")).toBe(
      "/background.png?v=101",
    );
    expect(prepareNewThreadBackgroundEffect).not.toHaveBeenCalled();

    await act(async () =>
      root.render(createElement(Probe, { effect: "dither" })),
    );
    expect(prepareNewThreadBackgroundEffect).toHaveBeenCalledWith(
      "/background.png?v=101",
      "/background.png?v=101",
      "dither",
      false,
    );
    expect(container.firstElementChild?.getAttribute("data-url")).toBe(
      "blob:processed",
    );

    await act(async () =>
      root.render(createElement(Probe, { effect: "none" })),
    );
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:processed");
  });
});
