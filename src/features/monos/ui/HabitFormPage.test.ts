// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Habit } from "../model/monoHabits";

const { addHabit, editHabit } = vi.hoisted(() => ({
  addHabit: vi.fn(),
  editHabit: vi.fn(),
}));
vi.mock("../model/monoHabits", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../model/monoHabits")>()),
  addHabit,
  editHabit,
}));

import { HabitFormPage } from "./HabitFormPage";

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
  vi.clearAllMocks();
});

const habit: Habit = {
  id: "h1",
  name: "Morning CI check",
  instructions: "Check CI on main.",
  schedule: {
    scheduleKind: "daily",
    time: "09:00",
    minute: 0,
    dayOfWeek: 1,
  },
  enabled: true,
  createdAt: 0,
  nextRunAt: 0,
};

function type(element: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(
    Object.getPrototypeOf(element),
    "value",
  )!.set!;
  setter.call(element, value);
  element.dispatchEvent(new Event("input", { bubbles: true }));
}

function button(label: string) {
  return [...container.querySelectorAll("button")].find(
    (entry) => entry.textContent === label,
  )!;
}

it("opens filled in with the habit and saves the edit in place", async () => {
  editHabit.mockResolvedValue(habit);
  const onSaved = vi.fn();
  act(() =>
    root.render(
      createElement(HabitFormPage, {
        monoId: "m1",
        habit,
        onBack: () => {},
        onSaved,
      }),
    ),
  );
  expect(container.textContent).toContain("Edit habit");
  const name = container.querySelector<HTMLInputElement>(
    '[aria-label="Name"]',
  )!;
  const instructions = container.querySelector<HTMLTextAreaElement>(
    '[aria-label="What it does"]',
  )!;
  expect(name.value).toBe("Morning CI check");
  expect(instructions.value).toBe("Check CI on main.");

  act(() => type(name, "  CI check  "));
  act(() => type(instructions, "Check CI on main and release branches."));
  await act(async () => button("Save").click());

  expect(addHabit).not.toHaveBeenCalled();
  expect(editHabit).toHaveBeenCalledWith("m1", "h1", {
    name: "CI check",
    instructions: "Check CI on main and release branches.",
    schedule: habit.schedule,
  });
  expect(onSaved).toHaveBeenCalledTimes(1);
});

it("keeps the page open with the reason when saving fails", async () => {
  editHabit.mockRejectedValue(new Error("This habit was removed."));
  const onSaved = vi.fn();
  act(() =>
    root.render(
      createElement(HabitFormPage, {
        monoId: "m1",
        habit,
        onBack: () => {},
        onSaved,
      }),
    ),
  );
  await act(async () => button("Save").click());

  expect(onSaved).not.toHaveBeenCalled();
  expect(container.textContent).toContain("This habit was removed.");
  expect(button("Save").disabled).toBe(false);
});
