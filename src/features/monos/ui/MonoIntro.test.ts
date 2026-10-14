// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { PROJECT_MASCOTS } from "../../projects/model/projectMascots";
import { MonoIntroCard, monoIntroCrowd } from "./MonoIntro";

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

it("creates the first mono or hides monos", () => {
  const onCreate = vi.fn();
  const onHide = vi.fn();
  act(() =>
    root.render(
      createElement(MonoIntroCard, {
        look,
        onCreate,
        onHide,
      }),
    ),
  );

  expect(document.body.textContent).toContain("Meet Monos");
  expect(
    document.body.querySelectorAll("[data-mono-intro-stage] svg"),
  ).toHaveLength(PROJECT_MASCOTS.length);

  act(() => button("Create mono").click());
  expect(onCreate).toHaveBeenCalledOnce();
  act(() =>
    document.body.querySelector<HTMLElement>('[role="switch"]')!.click(),
  );
  expect(onHide).toHaveBeenCalledOnce();
});
