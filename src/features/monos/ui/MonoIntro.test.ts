// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { PROJECT_MASCOTS } from "../../projects/model/projectMascots";
import { MonoIntroPopover, monoIntroCrowd } from "./MonoIntro";

let root: Root;
let container: HTMLElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

const look = { mascot: "cat", color: "hsl(142 55% 50%)" };

function button(label: string) {
  return [...document.body.querySelectorAll("button")].find(
    (entry) => entry.textContent === label,
  )!;
}

it("brings every other mascot to line up behind the user's mono", () => {
  const crowd = monoIntroCrowd(look);
  const names = [look.mascot, ...crowd.map((mascot) => mascot.name)];

  expect(new Set(names).size).toBe(PROJECT_MASCOTS.length);
  expect(crowd.map((mascot) => mascot.color)).not.toContain(look.color);
});

it("creates the first mono or puts it off", () => {
  const onCreate = vi.fn();
  const onLater = vi.fn();
  act(() =>
    root.render(
      createElement(MonoIntroPopover, {
        anchor: container,
        look,
        onCreate,
        onLater,
      }),
    ),
  );

  expect(document.body.textContent).toContain("Meet Monos");
  expect(
    document.body.querySelectorAll("[data-mono-intro-stage] svg"),
  ).toHaveLength(PROJECT_MASCOTS.length);

  act(() => button("Create your mono").click());
  expect(onCreate).toHaveBeenCalledOnce();
  act(() => button("Not now").click());
  expect(onLater).toHaveBeenCalledOnce();

  // Only a choice puts it away.
  act(() => {
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    document.body.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true }),
    );
  });
  expect(document.body.textContent).toContain("Meet Monos");
});
