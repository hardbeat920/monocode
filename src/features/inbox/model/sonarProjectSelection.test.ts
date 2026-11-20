import { expect, it } from "vitest";
import { sonarProjectSelection } from "./sonarProjectSelection";

const projects = [
  { path: "/tmp/repo", name: "repo" },
  { path: "/tmp/renamed", name: "Checkout alias" },
];
it("prefers remote matches over folder or display names", () => {
  expect(
    sonarProjectSelection(
      projects,
      " REPO ",
      "/tmp/repo",
      new Map([["/tmp/renamed", ["upstream", "repo"]]]),
    ),
  ).toBe("/tmp/renamed");
});
it("prefers the current checkout among matching remotes", () => {
  expect(
    sonarProjectSelection(
      projects,
      "repo",
      "/tmp/renamed",
      new Map(projects.map((project) => [project.path, ["repo"]])),
    ),
  ).toBe("/tmp/renamed");
});
it("falls back to display and folder names when no remote matches", () => {
  expect(
    sonarProjectSelection(projects, "Checkout alias", "/tmp/repo", new Map()),
  ).toBe("/tmp/renamed");
  expect(
    sonarProjectSelection(projects, "repo", "/tmp/renamed", new Map()),
  ).toBe("/tmp/repo");
});
it("preserves the usual default for renamed Sonar repositories and empty projects", () => {
  expect(
    sonarProjectSelection(projects, "old-name", "/tmp/renamed", new Map()),
  ).toBe("/tmp/renamed");
  expect(
    sonarProjectSelection(projects, "old-name", "/tmp/other", new Map()),
  ).toBe("/tmp/repo");
  expect(sonarProjectSelection([], "repo", "/tmp/other", new Map())).toBe(
    "/tmp/other",
  );
});
