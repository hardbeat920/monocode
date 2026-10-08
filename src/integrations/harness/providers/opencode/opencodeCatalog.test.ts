import { beforeEach, expect, it, vi } from "vitest";
import {
  modelsFor,
  resetHarnessModelOverlays,
  resolveModel,
} from "../../../../features/sessions/model/models";
import { localProjectModelSource } from "../../../../features/sessions/ui/modelSource";

const exec = vi.hoisted(() =>
  vi.fn(async (_path: string, args: string[], cwd: string) => {
    if (args[0] === "--version") return "1.14.19";
    if (args[0] === "agent")
      return `${cwd.endsWith("a") ? "project_a" : "project_b"} (primary)\n{}`;
    if (cwd.endsWith("empty")) return "";
    const id = cwd.endsWith("a")
      ? "model-a"
      : cwd.endsWith("b")
        ? "model-b"
        : "home-model";
    return `fixture/${id}\n${JSON.stringify({ id, name: id })}`;
  }),
);
vi.mock("../../core/child", () => ({
  resolveOpenCodeBinary: async () => ({ path: "/fixture/opencode" }),
  execChild: exec,
}));
vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home",
}));
const {
  projectOpenCodeModels,
  refreshProjectOpenCodeCatalog,
  refreshOpenCodeCatalog,
} = await import("./opencodeCatalog");

beforeEach(() => {
  resetHarnessModelOverlays();
  exec.mockClear();
});

it("keeps overlapping project inventory separate from the home catalog", async () => {
  await refreshOpenCodeCatalog();
  await Promise.all([
    refreshProjectOpenCodeCatalog("/project-a"),
    refreshProjectOpenCodeCatalog("/project-b"),
  ]);
  expect(modelsFor("opencode").map((model) => model.nativeId)).toEqual([
    "fixture/home-model",
  ]);
  expect(
    localProjectModelSource("/project-a")
      .modelsFor("opencode")
      .map((model) => model.nativeId),
  ).toEqual(["fixture/model-a"]);
  const sourceB = localProjectModelSource("/project-b");
  expect(sourceB.modelsFor("opencode").map((model) => model.nativeId)).toEqual([
    "fixture/model-b",
  ]);
  expect(sourceB.find("opencode:fixture/model-a")).toBeUndefined();
  expect(
    sourceB
      .resolve("opencode", "opencode:fixture/model-b")
      .settings?.find((setting) => setting.id === "agent")?.value,
  ).toBe("project_b");
  expect(
    resolveModel("opencode", "opencode:fixture/model-a", "/project-a").nativeId,
  ).toBe("fixture/model-a");
  expect(
    resolveModel("opencode", "opencode:fixture/model-a", "/project-b").nativeId,
  ).toBe("fixture/model-b");
  expect(
    exec.mock.calls
      .filter((call) => call[1][0] === "models")
      .map((call) => call[2])
      .sort(),
  ).toEqual(["/home", "/project-a", "/project-b"]);
});

it("coalesces only requests for the same directory", async () => {
  await Promise.all([
    refreshProjectOpenCodeCatalog("/dedupe-a"),
    refreshProjectOpenCodeCatalog("/dedupe-a"),
  ]);
  expect(
    exec.mock.calls.filter((call) => call[1][0] === "models"),
  ).toHaveLength(1);
});

it("preserves an empty enabled-model list for its project", async () => {
  await refreshOpenCodeCatalog();
  await refreshProjectOpenCodeCatalog("/project-empty");
  expect(projectOpenCodeModels("/project-empty")).toEqual([]);
  expect(
    localProjectModelSource("/project-empty").modelsFor("opencode"),
  ).toEqual([]);
  expect(modelsFor("opencode")).toHaveLength(1);
});

it("rejects unsupported OpenCode majors before running legacy inventory commands", async () => {
  exec.mockImplementationOnce(async () => "3.0.0");
  await refreshProjectOpenCodeCatalog("/unsupported");
  expect(projectOpenCodeModels("/unsupported")).toBeUndefined();
  expect(exec).toHaveBeenCalledTimes(1);
});
