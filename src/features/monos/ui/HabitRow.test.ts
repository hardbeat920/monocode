// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { setHabitRunning, type Habit } from "../model/monoHabits";
import { RunningFor, useHabitRunning } from "./HabitPage";

let root: Root;
let container: HTMLElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  vi.unstubAllGlobals();
});

const habit = { id: "h1", name: "PR check" } as Habit;

function Probe() {
  const since = useHabitRunning(habit.id);
  return since == null
    ? createElement("p", null, "idle")
    : createElement("p", null, "running ", createElement(RunningFor, { since }));
}

it("shows a habit as running while its run is in flight, and idle after", () => {
  act(() => root.render(createElement(Probe)));
  expect(container.textContent).toBe("idle");
  act(() => setHabitRunning(habit.id, Date.now() - 72_000));
  expect(container.textContent).toBe("running 1m 12s");
  act(() => setHabitRunning(habit.id));
  expect(container.textContent).toBe("idle");
});
