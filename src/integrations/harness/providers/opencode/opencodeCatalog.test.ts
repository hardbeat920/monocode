import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  modelsFor,
  resetHarnessModelOverlays,
} from "../../../../features/sessions/model/models";
import { execChild } from "../../core/child";
import { refreshOpenCodeCatalog, reloadOpenCodeCatalog } from "./opencodeCatalog";

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home/test",
}));
vi.mock("../../core/child", () => ({
  execChild: vi.fn(),
  resolveOpenCodeBinary: async () => ({ path: "/fixture/opencode" }),
}));

const catalogOutput = vi.fn<() => string>();

function listedIds() {
  return modelsFor("opencode").map((model) => model.nativeId);
}

beforeEach(() => {
  resetHarnessModelOverlays();
  vi.mocked(execChild).mockReset();
  catalogOutput
    .mockReset()
    .mockReturnValue('fixture/model\n{"name":"Fixture Model"}');
  vi.mocked(execChild).mockImplementation(async (_path, args) => {
    if (args[0] === "--version") return "1.19.6";
    if (args[0] === "models") return catalogOutput();
    if (args[0] === "agent") return "";
    throw new Error(`Unexpected OpenCode command: ${args[0]}`);
  });
});

afterEach(() => {
  resetHarnessModelOverlays();
  vi.restoreAllMocks();
});

it("shares one catalog read between background and user refreshes", async () => {
  await Promise.all([refreshOpenCodeCatalog(), reloadOpenCodeCatalog()]);

  expect(catalogOutput).toHaveBeenCalledTimes(1);
  expect(listedIds()).toContain("fixture/model");
});

it("keeps the last catalog on failure and lets the user retry", async () => {
  await reloadOpenCodeCatalog();
  const failure = new Error("catalog command failed");
  catalogOutput.mockImplementationOnce(() => {
    throw failure;
  });
  vi.spyOn(console, "debug").mockImplementation(() => {});

  await Promise.all([
    expect(refreshOpenCodeCatalog()).resolves.toBeUndefined(),
    expect(reloadOpenCodeCatalog()).rejects.toBe(failure),
  ]);
  expect(listedIds()).toContain("fixture/model");

  catalogOutput.mockReturnValue('fixture/added\n{"name":"Added Model"}');
  await reloadOpenCodeCatalog();
  expect(listedIds()).toContain("fixture/added");
  expect(listedIds()).not.toContain("fixture/model");
});

it("does not replace a working catalog with empty CLI output", async () => {
  await reloadOpenCodeCatalog();
  catalogOutput.mockReturnValue("");

  await expect(reloadOpenCodeCatalog()).rejects.toThrow("OpenCode returned no models.");
  expect(listedIds()).toContain("fixture/model");
});
