// @vitest-environment happy-dom
import { expect, it } from "vitest";
import { remotePath, remoteProjectFor } from "./remoteProjects";

it("finds a saved UNC project through its corrected remote path", () => {
  const legacyKey = "remote://env/server/share/repo";
  const project = {
    key: legacyKey,
    environmentId: "env",
    projectId: "project",
    cwd: "\\\\server\\share\\repo",
  };
  localStorage.setItem("monocode.remote-projects.v2", JSON.stringify({ [legacyKey]: project }));
  expect(remoteProjectFor(remotePath("env", project.cwd))).toEqual(project);
  localStorage.removeItem("monocode.remote-projects.v2");
});
