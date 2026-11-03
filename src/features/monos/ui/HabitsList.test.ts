// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Habit } from "../model/monoHabits";
import { HabitsList, type HabitActions } from "./MonoHabits";

let root: Root;
let container: HTMLElement;
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 9, 12, 12, 0));
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

const at = (hour: number, day = 12) => new Date(2026, 9, day, hour).getTime();
const habit = (id: string, extra: Partial<Habit>): Habit =>
  ({
    id,
    name: id,
    instructions: "",
    schedule: { scheduleKind: "daily", time: "09:00", minute: 0, dayOfWeek: 1 },
    enabled: true,
    createdAt: 0,
    nextRunAt: at(9, 13),
    ...extra,
  }) as Habit;

it("separates what is still to come today from what ran and the rest", () => {
  const actions = {} as HabitActions;
  act(() =>
    root.render(
      createElement(HabitsList, {
        habits: [
          habit("Weekly report", { nextRunAt: at(9, 16) }),
          habit("Evening digest", { nextRunAt: at(18) }),
          habit("Morning digest", { lastRunAt: at(8) }),
          habit("Paused", { enabled: false, nextRunAt: at(9, 14) }),
        ],
        actions,
        onOpen: () => {},
      }),
    ),
  );
  const groups = [...container.querySelectorAll("section")].map((section) => [
    section.getAttribute("aria-label"),
    [...section.querySelectorAll("li")].map(
      (row) =>
        row.textContent?.match(
          /Weekly report|Evening digest|Morning digest|Paused/,
        )?.[0],
    ),
  ]);
  expect(groups).toEqual([
    ["Up next today", ["Evening digest"]],
    ["Done today", ["Morning digest"]],
    ["Later", ["Weekly report", "Paused"]],
  ]);
});
