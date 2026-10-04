// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useStateRef } from "./useStateRef";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function mount() {
  let api!: ReturnType<typeof useStateRef<string>>;
  function Probe() {
    api = useStateRef(() => "/alpha");
    return createElement("span", null, api[0]);
  }
  return {
    render: () => act(async () => root.render(createElement(Probe))),
    get: () => api,
  };
}

it("exposes a set value through the ref before the next render", async () => {
  const probe = mount();
  await probe.render();
  const [, set, ref] = probe.get();

  let seen: string | undefined;
  await act(async () => {
    set("/beta");
    seen = ref.current;
  });

  expect(seen).toBe("/beta");
  expect(container.textContent).toBe("/beta");
});

it("lets a same-tick follow-up override an earlier switch", async () => {
  const probe = mount();
  await probe.render();
  const [, set, ref] = probe.get();

  // An opener switches to /beta, then a follow to the tab's filed project
  // (/alpha) must not be skipped as "already there".
  await act(async () => {
    set("/beta");
    if (ref.current !== "/alpha") set("/alpha");
  });

  expect(container.textContent).toBe("/alpha");
  expect(probe.get()[2].current).toBe("/alpha");
});
