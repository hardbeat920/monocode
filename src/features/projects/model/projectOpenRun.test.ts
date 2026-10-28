import { describe, expect, it } from "vitest";
import { newTab } from "../../workspace/model/layout";
import {
  newSession,
  type HarnessId,
  type Session,
} from "../../sessions/model/session";
import { planProjectOpenRun, type ProjectOpenStep } from "./projectOpenRun";

function chat(id: string, cwd: string, harness: HarnessId = "cursor"): Session {
  return {
    ...newSession(harness, cwd),
    id,
    blocks: [{ id: `${id}-user`, role: "user", text: id }],
  };
}

function blank(id: string, cwd: string): Session {
  return { ...newSession("cursor", cwd), id };
}

/** Two projects open, `/beta` active; `/alpha` is the first in the list. */
function workspace(
  sessions: Session[] = [chat("a1", "/alpha", "codex"), chat("b1", "/beta")],
) {
  const tabs = sessions.map((session) => ({
    ...newTab(session.id),
    id: `tab-${session.id}`,
  }));
  return {
    memory: new Map<string, string>(),
    sessions,
    tabs,
    activeTabId: tabs[tabs.length - 1].id,
  };
}

function creates(steps: ProjectOpenStep[]) {
  return steps.filter(
    (step): step is Extract<ProjectOpenStep, { action: "create" }> =>
      step.action === "create",
  );
}

describe("planning a run of folders", () => {
  it("gives every folder its own session and tab, in selection order", () => {
    const state = workspace();
    const steps = planProjectOpenRun({
      ...state,
      paths: ["/one", "/two", "/three"],
    });

    expect(steps.map((step) => [step.action, step.path])).toEqual([
      ["create", "/one"],
      ["create", "/two"],
      ["create", "/three"],
    ]);
    const made = creates(steps);
    expect(made.map((step) => step.session.cwd)).toEqual([
      "/one",
      "/two",
      "/three",
    ]);
    expect(new Set(made.map((step) => step.session.id)).size).toBe(3);
    expect(made.map((step) => step.tab.focusedId)).toEqual(
      made.map((step) => step.session.id),
    );
    // Each tab is anchored to the one created before it, not to the active tab.
    expect(made.map((step) => step.besideTabId)).toEqual([
      "tab-b1",
      made[0].tab.id,
      made[1].tab.id,
    ]);
  });

  it("activates a folder that is already open instead of creating", () => {
    const state = workspace();
    const steps = planProjectOpenRun({
      ...state,
      paths: ["/alpha", "/two"],
    });

    expect(steps[0]).toEqual({
      action: "activate",
      path: "/alpha",
      tabId: "tab-a1",
      paneId: "a1",
    });
    expect(creates(steps)).toHaveLength(1);
  });

  it("activates a folder the run itself just opened", () => {
    const state = workspace();
    const steps = planProjectOpenRun({ ...state, paths: ["/two", "/two"] });

    const made = creates(steps);
    expect(made).toHaveLength(1);
    expect(steps[1]).toEqual({
      action: "activate",
      path: "/two",
      tabId: made[0].tab.id,
      paneId: made[0].session.id,
    });
  });

  it("seeds every new session from the active session, not the first one", () => {
    const state = workspace();
    const steps = planProjectOpenRun({
      ...state,
      paths: ["/one", "/two"],
    });

    expect(creates(steps).map((step) => step.session.harness)).toEqual([
      "cursor",
      "cursor",
    ]);
  });

  it("reuses the active blank session for the first folder only", () => {
    const state = workspace([chat("a1", "/alpha", "codex"), blank("b1", "~")]);
    const steps = planProjectOpenRun({
      ...state,
      paths: ["/one", "/two"],
    });

    expect(steps[0]).toEqual({
      action: "reuse-blank",
      path: "/one",
      sessionId: "b1",
    });
    expect(creates(steps).map((step) => step.session.cwd)).toEqual(["/two"]);
  });

  it("leaves a single folder behaving as it did before the run", () => {
    const state = workspace();

    expect(planProjectOpenRun({ ...state, paths: ["/beta"] })).toEqual([
      { action: "keep", path: "/beta" },
    ]);
    expect(planProjectOpenRun({ ...state, paths: ["/alpha"] })).toEqual([
      { action: "activate", path: "/alpha", tabId: "tab-a1", paneId: "a1" },
    ]);

    const [created, ...rest] = planProjectOpenRun({
      ...state,
      paths: ["/one/"],
    });
    expect(rest).toEqual([]);
    expect(created).toMatchObject({
      action: "create",
      path: "/one",
      besideTabId: "tab-b1",
    });

    const onBlank = workspace([blank("b1", "~")]);
    expect(planProjectOpenRun({ ...onBlank, paths: ["/one"] })).toEqual([
      { action: "reuse-blank", path: "/one", sessionId: "b1" },
    ]);
  });

  it("does nothing when the dialog is dismissed or the path is no project", () => {
    const state = workspace();
    expect(planProjectOpenRun({ ...state, paths: [] })).toEqual([]);
    expect(planProjectOpenRun({ ...state, paths: ["/", "~"] })).toEqual([]);
  });
});
