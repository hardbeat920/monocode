// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  saveRecentModelChoice,
  loadRecentModelChoices,
  setHarnessModels,
  resetHarnessModelOverlays,
} from "./models";
import {
  createTurnModelUsage,
  readUsedModels,
  recordUsedModel,
  reconcileUsedModel,
  type UsedModel,
} from "./usedModels";

const item = (
  model: string,
  usedAt: number,
  extra: Partial<UsedModel> = {},
): UsedModel => ({
  observationId: `turn-${usedAt}`,
  harness: "pi",
  model,
  identitySource: "requested",
  usedAt,
  ...extra,
});
beforeEach(() => {
  localStorage.clear();
  vi.useFakeTimers();
  vi.setSystemTime(100);
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
  resetHarnessModelOverlays();
});

describe("actual-use model recency", () => {
  it("does not migrate picker history or record selection, drafts, acceptance/status, or empty deltas", () => {
    setHarnessModels("pi", [
      { id: "pi:selected", harness: "pi", name: "Selected" },
    ]);
    saveRecentModelChoice("pi", "pi:selected");
    const selected = loadRecentModelChoices();
    const observe = createTurnModelUsage("pi", "pi:selected");
    observe({ type: "session.started" });
    observe({ type: "turn.started", providerTurnId: "accepted" });
    observe({ type: "status", text: "Accepted" });
    observe({ type: "message.delta", text: "" });
    observe({ type: "reasoning.delta", text: "" });
    observe({ type: "session.error", message: "Rejected" });
    expect(readUsedModels()).toEqual([]);
    expect(loadRecentModelChoices()).toEqual(selected);
    expect(localStorage.getItem("monocode.usedModels")).toBeNull();
  });
  it.each(["message.delta", "reasoning.delta", "tool.started"] as const)(
    "records once on the first qualifying %s",
    (type) => {
      const observe = createTurnModelUsage("pi", "pi:requested");
      observe(
        type === "tool.started"
          ? { type, callId: "call", title: "Shell" }
          : { type, text: "activity" },
      );
      const first = readUsedModels();
      expect(first).toMatchObject([
        { model: "pi:requested", identitySource: "requested", usedAt: 100 },
      ]);
      vi.setSystemTime(200);
      observe({ type: "message.delta", text: "more" });
      observe({ type: "tool.started", callId: "second", title: "Shell" });
      expect(readUsedModels()).toEqual(first);
    },
  );
  it("retains first-activity time when model identity is initially unavailable, without inventing an ID", () => {
    const observe = createTurnModelUsage("codex", "");
    observe({ type: "reasoning.delta", text: "first activity" });
    expect(readUsedModels()).toEqual([]);
    vi.setSystemTime(200);
    observe({ type: "session.configChanged", model: "codex:reported" });
    expect(readUsedModels()).toMatchObject([
      {
        harness: "codex",
        model: "codex:reported",
        identitySource: "reported",
        usedAt: 100,
      },
    ]);
    const first = readUsedModels();
    observe({ type: "message.delta", text: "later" });
    expect(readUsedModels()).toEqual(first);
  });
  it("uses reported identity received before activity and ignores subagent model identities", () => {
    const observe = createTurnModelUsage("pi", "pi:default");
    observe({ type: "session.configChanged", model: "pi:reported" });
    expect(readUsedModels()).toEqual([]);
    observe({
      type: "tool.started",
      callId: "agent",
      title: "Subagent",
      agentModel: "other-model",
    });
    expect(readUsedModels()).toMatchObject([
      { model: "pi:reported", identitySource: "reported" },
    ]);
  });
  it("reconciles late reported identity on the same observation without timestamp or recency movement", () => {
    const observe = createTurnModelUsage("pi", "pi:default");
    observe({ type: "message.delta", text: "first" });
    const first = readUsedModels()[0];
    recordUsedModel(item("pi:newer", 200));
    vi.setSystemTime(300);
    observe({ type: "session.configChanged", model: "pi:actual" });
    expect(readUsedModels()).toEqual([
      item("pi:newer", 200),
      { ...first, model: "pi:actual", identitySource: "reported" },
    ]);
    observe({ type: "session.configChanged", model: "pi:actual" });
    expect(readUsedModels()).toHaveLength(2);
  });
  it("collapses corrected identity into an existing pair while keeping its newest observation", () => {
    recordUsedModel(item("pi:default", 100));
    recordUsedModel(item("pi:actual", 200));
    reconcileUsedModel("turn-100", "pi:actual");
    expect(readUsedModels()).toEqual([item("pi:actual", 200)]);
    recordUsedModel(item("pi:default", 300));
    reconcileUsedModel("turn-300", "pi:actual");
    expect(readUsedModels()).toEqual([
      item("pi:actual", 300, { identitySource: "reported" }),
    ]);
  });
  it("keeps ten distinct harness/model pairs in MRU order without changing picker history", () => {
    for (let n = 1; n <= 11; n++) recordUsedModel(item(`pi:${n}`, n));
    expect(readUsedModels().map((entry) => entry.model)).toEqual([
      "pi:11",
      "pi:10",
      "pi:9",
      "pi:8",
      "pi:7",
      "pi:6",
      "pi:5",
      "pi:4",
      "pi:3",
      "pi:2",
    ]);
    recordUsedModel(item("pi:5", 12));
    expect(readUsedModels()).toHaveLength(10);
    expect(readUsedModels()[0]).toEqual(item("pi:5", 12));
    recordUsedModel(item("pi:5", 13, { harness: "codex" }));
    expect(
      readUsedModels()
        .slice(0, 2)
        .map((entry) => entry.harness),
    ).toEqual(["codex", "pi"]);
    expect(localStorage.getItem("monocode.recentModels")).toBeNull();
    reconcileUsedModel("turn-1", "pi:evicted");
    expect(readUsedModels().some((entry) => entry.model === "pi:evicted")).toBe(
      false,
    );
  });
  it("never lets a superseded observation correct newer evidence for the same pair", () => {
    recordUsedModel(item("pi:default", 100));
    recordUsedModel(item("pi:default", 200));
    reconcileUsedModel("turn-100", "pi:old-reported");
    expect(readUsedModels()).toEqual([item("pi:default", 200)]);
  });
  it("validates persisted rows and strips unknown fields, with no fake historical migration", () => {
    localStorage.setItem(
      "monocode.usedModels",
      JSON.stringify([
        item("pi:valid", 3, { identitySource: "reported" }),
        { ...item("pi:unknown-field", 2), extra: "discard" },
        { ...item("pi:bad", 5), harness: "unknown" },
        { ...item("pi:bad", 6), identitySource: "confirmed" },
        { ...item("pi:bad", 7), usedAt: "today" },
        { ...item("pi:bad", 8), observationId: "" },
        { ...item("", 9) },
      ]),
    );
    expect(readUsedModels()).toEqual([
      item("pi:valid", 3, { identitySource: "reported" }),
      item("pi:unknown-field", 2),
    ]);
    localStorage.setItem("monocode.usedModels", "bad JSON");
    expect(readUsedModels()).toEqual([]);
  });
  it("never throws into a model turn when storage reads or writes fail", () => {
    const debug = vi.spyOn(console, "debug").mockImplementation(() => {});
    vi.spyOn(localStorage, "getItem").mockImplementation(() => {
      throw new Error("read blocked");
    });
    vi.spyOn(localStorage, "setItem").mockImplementation(() => {
      throw new Error("quota exceeded");
    });
    const observe = createTurnModelUsage("pi", "pi:requested");
    expect(() =>
      observe({ type: "message.delta", text: "activity" }),
    ).not.toThrow();
    expect(() =>
      observe({ type: "session.configChanged", model: "pi:actual" }),
    ).not.toThrow();
    expect(readUsedModels()).toEqual([]);
    expect(debug).toHaveBeenCalledWith(
      "[monocode] used models save",
      expect.any(Error),
    );
  });
});
