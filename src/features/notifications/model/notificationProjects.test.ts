// @vitest-environment happy-dom
import { beforeEach, expect, it } from "vitest";
import {
  inboxNotificationProject,
  knownNotificationProject,
  knownNotificationProjectSelection,
  loadNotificationProjects,
  rememberNotificationProjects,
} from "./notificationProjects";

beforeEach(() => localStorage.clear());

it("derives local notification identity immediately from the normalized path", () => {
  expect(knownNotificationProject("C:/Work/App")).toEqual({
    id: "local:c:/work/app",
    name: "App",
    detail: "C:/Work/App",
    kind: "local",
    paths: ["C:/Work/App"],
  });
  expect(knownNotificationProject("c:\\work\\app")?.id).toBe(
    "local:c:/work/app",
  );
});

it("keeps separate checkout and worktree paths as separate projects", () => {
  const selection = knownNotificationProjectSelection([
    "/work/app",
    "/work/app-review",
  ]);
  expect(selection.projects.map((project) => project.id)).toEqual([
    "local:/work/app",
    "local:/work/app-review",
  ]);
});

it("includes requested paths and provider-only catalog projects", () => {
  rememberNotificationProjects([
    {
      id: "linear:project:roadmap",
      name: "Roadmap",
      detail: "Linear",
      kind: "linear",
      paths: [],
    },
    {
      id: "local:/old/path",
      name: "old",
      detail: "/old/path",
      kind: "local",
      paths: ["/old/path"],
    },
  ]);
  expect(
    knownNotificationProjectSelection(["/work/app"]).projects.map(
      (project) => project.id,
    ),
  ).toEqual(["linear:project:roadmap", "local:/work/app"]);
});

it("uses the same local ID when Inbox enriches a path with repository metadata", () => {
  const project = inboxNotificationProject({
    provider: "github",
    repo: "acme/app",
    url: "https://github.com/acme/app/pull/5",
    projectPath: "/work/app",
  });
  expect(project).toMatchObject({
    id: "local:/work/app",
    name: "acme/app",
    kind: "repository",
  });
  rememberNotificationProjects([project]);
  expect(knownNotificationProject("/work/app")).toEqual(project);
});

it("keeps provider-only repository and Linear identities", () => {
  expect(
    inboxNotificationProject({
      provider: "github",
      repo: "Acme/App",
      url: "git@github.com:Acme/App.git",
    }).id,
  ).toBe("repository:github.com/acme/app");
  expect(
    inboxNotificationProject({
      provider: "linear",
      repo: "PROD",
      url: "https://linear.app/acme/issue/PROD-1",
      projectId: "launch",
      projectName: "Launch",
      teamId: "product",
      teamName: "Product",
    }).id,
  ).toBe("linear:project:launch");
});

it("uses a new catalog version so old Git-derived path mappings are ignored", () => {
  localStorage.setItem(
    "monocode.notificationProjects.v1",
    JSON.stringify([
      {
        id: "repository:github.com/old/app",
        name: "old/app",
        detail: "github.com",
        kind: "repository",
        paths: ["/work/app"],
      },
    ]),
  );
  expect(loadNotificationProjects()).toEqual([]);
  expect(knownNotificationProject("/work/app")?.id).toBe("local:/work/app");
});
