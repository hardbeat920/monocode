// @vitest-environment node

import { readFileSync } from "node:fs";
import { expect, it, vi } from "vitest";

vi.mock("expo", () => ({ requireOptionalNativeModule: () => null }));
vi.mock("expo-sqlite/kv-store", () => ({ default: {} }));
vi.mock("expo-notifications", () => ({}));
vi.mock("expo-task-manager", () => ({}));
vi.mock("react-native", () => ({ Platform: { OS: "web" } }));

const { activitySessions, pushedActivity } = await import(
  "@/features/liveActivity/liveActivity"
);

const session = (id: string, fields: object) =>
  ({ id, projectId: "p", title: id, harness: "claude", model: "", branch: "", updatedAt: 0, ...fields }) as never;

it("keeps both copies of the activity attributes identical", () => {
  // ActivityKit pairs the app's activity with the widget by this type.
  const code = (path: string) =>
    readFileSync(new URL(path, import.meta.url), "utf8")
      .split("\n")
      .filter((line) => !line.trimStart().startsWith("//"))
      .join("\n");
  expect(code("../targets/live-activity/MonocodeActivityAttributes.swift")).toBe(
    code("../modules/monocode-activity/ios/MonocodeActivityAttributes.swift"),
  );
});

it("shows working and waiting sessions and the ones that just finished, as the desktop does", () => {
  const before = [session("a", { busy: true }), session("b", { busy: true }), session("c", {})];
  const after = [
    session("a", {
      busy: true,
      waiting: "approval",
      modelName: "Opus 5.5",
      request: { id: "r1", label: "Run cargo test", count: 1 },
    }),
    session("b", {}),
    session("c", {}),
  ];
  const shown = activitySessions(before, after, [{ id: "p", name: "monocode", path: "/code/monocode", color: "#4fd1c5" }]);
  expect(shown.map((s) => [s.id, s.status])).toEqual([["a", "approval"], ["b", "done"]]);
  expect(shown[0]).toMatchObject({
    project: "monocode",
    model: "Opus 5.5",
    request: { id: "r1", label: "Run cargo test", count: 1 },
  });
  expect(shown[1].request).toBeUndefined();
  // The first list from the desktop says nothing finished.
  expect(activitySessions(null, after, []).map((s) => s.id)).toEqual(["a"]);
});

it("finds the sessions in a push however Expo nests its data", () => {
  const sessions = [{ id: "a", status: "working" }];
  expect(pushedActivity({ body: { activity: sessions } })).toEqual(sessions);
  expect(pushedActivity({ aps: {}, body: JSON.stringify({ activity: sessions }) })).toEqual(sessions);
  expect(pushedActivity({ data: { other: 1 } })).toBeNull();
});
