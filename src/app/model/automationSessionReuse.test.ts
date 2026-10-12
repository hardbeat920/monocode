// @vitest-environment happy-dom
import { afterEach, expect, it } from "vitest";
import { createMono, saveMonoSessionId } from "../../features/monos/model/mono";
import {
  markHabitRun,
  clearHabitRun,
} from "../../features/monos/model/monoHabits";
import { canReuseAutomationSession } from "./automationSessionReuse";

afterEach(() => {
  clearHabitRun("habit-session");
  localStorage.clear();
});

it("reuses ordinary automation sessions, never Mono or active habit sessions", () => {
  const mono = createMono(["/project"]);
  saveMonoSessionId(mono.id, "mono-session");
  markHabitRun("habit-session", mono.id);

  expect(canReuseAutomationSession("automation-session")).toBe(true);
  expect(canReuseAutomationSession("mono-session")).toBe(false);
  expect(canReuseAutomationSession("habit-session")).toBe(false);
});
