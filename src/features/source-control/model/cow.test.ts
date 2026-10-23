import { expect, it, vi } from "vitest";
import { invokeWorkspace } from "../../../platform/tauri/fs";
import { createCowWorkspace } from "./cow";

vi.mock("../../../platform/tauri/fs", () => ({
  invokeWorkspace: vi.fn().mockResolvedValue({ id: "copy", path: "/copies/copy" }),
  notifyGitChanged: vi.fn(),
}));

it("forwards an explicit source base while donor copies retain the native HEAD default", async () => {
  await createCowWorkspace("/repo", "session", "/project", "feature/base");
  expect(invokeWorkspace).toHaveBeenLastCalledWith("cow_create", {
    cwd: "/repo", sessionId: "session", projectCwd: "/project", base: "feature/base",
  });
  await createCowWorkspace("/copies/donor", "child", "/project");
  expect(invokeWorkspace).toHaveBeenLastCalledWith("cow_create", {
    cwd: "/copies/donor", sessionId: "child", projectCwd: "/project",
  });
});
