import { expect, it, vi } from "vitest";
import {
  carryModelSettings,
  findRemoteModel,
  remoteModelControls,
  sameModelSettings,
} from "./remoteModels";
import type { AgentModel } from "../../sessions/model/models";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const effort = {
  id: "effort",
  label: "Reasoning",
  kind: "select" as const,
  value: "high",
  options: ["low", "high", "xhigh"].map((value) => ({ value, label: value })),
};
const opus: AgentModel = {
  id: "claude:opus-4-6",
  harness: "claude",
  name: "Opus",
  nativeId: "claude-opus-4-6",
  settings: [effort],
};

it("matches saved models across catalog id schemes, preferring exact ids", () => {
  expect(findRemoteModel([opus], "claude:opus-4.6")).toBe(opus);
  expect(findRemoteModel([opus], "claude-opus-4-6")).toBe(opus);
  const alias = { ...opus, id: "claude:opus-4.6", nativeId: "opus" };
  expect(findRemoteModel([opus, alias], "claude:opus-4.6")).toBe(alias);
  expect(findRemoteModel([opus], "claude:sonnet-4-6")).toBeUndefined();
});

it("uses only catalog settings for a newly chosen model", () => {
  const catalog = { models: { claude: [opus] }, errors: {} };
  expect(
    remoteModelControls(catalog, "claude", opus.id, { context: "1m" }),
  ).toEqual({ model: opus, settings: [effort] });
});

it("describes a Claude model from built-in metadata when the host cannot", () => {
  const controls = remoteModelControls(
    undefined,
    "claude",
    "claude:opus-5-5",
    { effort: "xhigh" },
    "claude:opus-5-5",
  );
  expect(controls.fallback).toBe("no-catalog");
  expect(controls.settings.map((setting) => setting.id)).toEqual([
    "effort",
    "fast",
    "context",
  ]);
});

it("carries compatible choices to another model and compares settings by value", () => {
  expect(
    carryModelSettings([effort], { effort: "xhigh", fast: "true" }),
  ).toEqual({ effort: "xhigh" });
  expect(carryModelSettings([effort], { effort: "max" })).toEqual({
    effort: "high",
  });
  expect(sameModelSettings({ a: "1", b: "2" }, { b: "2", a: "1" })).toBe(true);
  expect(sameModelSettings({ a: "1" }, { a: "1", b: "2" })).toBe(false);
});
