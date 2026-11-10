import { describe, expect, it } from "vitest";
import { projectForRoute } from "@/features/workspace/routing";

describe("desktop project routes", () => {
  it.each([
    "/Users/test/code/project",
    "/Users/test/My Projects/日本語",
    "/Users/test/code/100% done",
    "/Users/test/code/literal%2Fname",
    "C:\\Users\\test\\My Projects",
  ])("finds %s using either its canonical ID or decoded route param", (path) => {
    const project = { id: encodeURIComponent(path), path };
    expect(projectForRoute([project], project.id)).toBe(project);
    expect(projectForRoute([project], path)).toBe(project);
    // RPCs must continue using the desktop's canonical ID after route lookup.
    expect(projectForRoute([project], path)?.id).toBe(encodeURIComponent(path));
  });

  it("rejects unavailable routes and prefers an exact catalog ID", () => {
    const projects = [{ id: "a", path: "b" }, { id: "b", path: "c" }];
    expect(projectForRoute(projects, "b")).toBe(projects[1]);
    expect(projectForRoute(projects, "missing")).toBeUndefined();
    expect(projectForRoute(projects, undefined)).toBeUndefined();
  });
});
