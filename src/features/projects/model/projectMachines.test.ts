// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import {
  autoLinkIn,
  autoLinkProjects,
  forgetIn,
  homeIn,
  linkIn,
  locationsIn,
  loadProjectMachines,
  normalizeGitRemoteUrl,
  projectLocations,
  unlinkIn,
  unlinkProjectLocation,
  type ProjectMachines,
} from "./projectMachines";
import { loadRecents } from "./recents";

const MAC = "/Users/me/code/app";
const MINI = "remote://env-mini/Users/me/app";
const BOX = "remote://env-box/home/me/app";
const BOX_OTHER = "remote://env-box/home/me/app-copy";

const state = (partial: Partial<ProjectMachines> = {}): ProjectMachines => ({
  links: {},
  separate: [],
  identities: {},
  ...partial,
});

afterEach(() => localStorage.clear());

describe("normalizeGitRemoteUrl", () => {
  it.each([
    ["git@github.com:T3Tools/T3Code.git", "github.com/t3tools/t3code"],
    ["https://github.com/T3Tools/T3Code.git", "github.com/t3tools/t3code"],
    ["https://user@github.com/a/b/", "github.com/a/b"],
    ["ssh://git@github.com:22/a/b.git", "github.com/a/b"],
    ["git://example.com/a/b", "example.com/a/b"],
    [
      "ssh://git@gitlab.example.com:2222/group/sub/repo.git",
      "gitlab.example.com/group/sub/repo",
    ],
    ["git@ssh.dev.azure.com:v3/Org/Proj/Repo", "dev.azure.com/org/proj/_git/repo"],
    [
      "https://Org@dev.azure.com/Org/Proj/_git/Repo",
      "dev.azure.com/org/proj/_git/repo",
    ],
    [
      "https://org.visualstudio.com/Proj/_git/Repo",
      "dev.azure.com/org/proj/_git/repo",
    ],
    ["/srv/git/app.git", "file:/srv/git/app"],
    ["C:\\repos\\app", "file:c:/repos/app"],
    ["", ""],
  ])("%s", (input, output) => {
    expect(normalizeGitRemoteUrl(input)).toBe(output);
  });
});

describe("links", () => {
  it("lists the home first, then this computer, then remote folders", () => {
    let next = linkIn(state(), BOX, MINI)!;
    next = linkIn(next, BOX, MAC)!;
    expect(locationsIn(next, MINI)).toEqual([BOX, MAC, MINI]);
    expect(homeIn(next, MAC)).toBe(BOX);
  });

  it("keeps one folder per machine", () => {
    const next = linkIn(state(), MAC, BOX)!;
    expect(linkIn(next, MAC, BOX_OTHER)).toBeUndefined();
    expect(linkIn(state(), MAC, MAC)).toBeUndefined();
  });

  it("moves a home's members when it joins another project", () => {
    const boxProject = linkIn(state(), BOX, MINI)!;
    const next = linkIn(boxProject, MAC, BOX)!;
    expect(locationsIn(next, MINI)).toEqual([MAC, BOX, MINI]);
    expect(Object.values(next.links).every((home) => home === MAC)).toBe(true);
  });

  it("links into the target's home when given a member", () => {
    const next = linkIn(linkIn(state(), MAC, MINI)!, MINI, BOX)!;
    expect(homeIn(next, BOX)).toBe(MAC);
  });

  it("promotes the first member when the home is unlinked", () => {
    const linked = linkIn(linkIn(state(), MAC, MINI)!, MAC, BOX)!;
    const next = unlinkIn(linked, MAC);
    expect(homeIn(next, MAC)).toBe(MAC);
    expect(locationsIn(next, BOX)).toEqual([BOX, MINI]);
    expect(next.separate).toEqual([MAC]);
  });

  it("forgets a member without marking it separate", () => {
    const linked = linkIn(state({ identities: { [MINI]: "x" } }), MAC, MINI)!;
    const next = forgetIn(unlinkIn(linked, MINI), MINI);
    expect(locationsIn(next, MAC)).toEqual([MAC]);
    expect(next.separate).toEqual([]);
    expect(next.identities).toEqual({});
  });

  it("brings an unlinked folder back to the rail", () => {
    localStorage.setItem(
      "monocode.projectMachines.v1",
      JSON.stringify(linkIn(state(), MAC, MINI)),
    );
    unlinkProjectLocation(MINI);
    expect(projectLocations(MAC)).toEqual([MAC]);
    expect(loadRecents().map((item) => item.path)).toContain(MINI);
  });
});

describe("autoLinkIn", () => {
  const repo = "github.com/me/app";

  it("joins folders of one repository on different machines", () => {
    const next = autoLinkIn(
      state({ identities: { [MAC]: repo, [MINI]: repo, [BOX]: "github.com/me/other" } }),
      [
        { path: MINI, openedAt: 1 },
        { path: MAC, openedAt: 2 },
        { path: BOX, openedAt: 3 },
      ],
    );
    expect(locationsIn(next, MINI)).toEqual([MAC, MINI]);
    expect(locationsIn(next, BOX)).toEqual([BOX]);
  });

  it("keeps the most recent folder when one machine has two clones", () => {
    const next = autoLinkIn(
      state({ identities: { [MAC]: repo, [BOX]: repo, [BOX_OTHER]: repo } }),
      [
        { path: MAC, openedAt: 1 },
        { path: BOX, openedAt: 2 },
        { path: BOX_OTHER, openedAt: 3 },
      ],
    );
    expect(locationsIn(next, MAC)).toEqual([MAC, BOX_OTHER]);
    expect(homeIn(next, BOX)).toBe(BOX);
  });

  it("leaves unlinked folders and folders without a remote alone", () => {
    const next = autoLinkIn(
      state({
        identities: { [MAC]: repo, [MINI]: repo, [BOX]: "" },
        separate: [MINI],
      }),
      [
        { path: MAC, openedAt: 1 },
        { path: MINI, openedAt: 2 },
        { path: BOX, openedAt: 3 },
      ],
    );
    expect(next.links).toEqual({});
  });

  it("stores identities and reports a new link", () => {
    const linked = autoLinkProjects(
      [
        { path: MAC, openedAt: 1 },
        { path: MINI, openedAt: 2 },
      ],
      {
        [MAC]: "git@github.com:me/app.git",
        [MINI]: "https://github.com/me/app",
      },
    );
    expect(linked).toBe(true);
    expect(loadProjectMachines().identities[MAC]).toBe(repo);
    expect(projectLocations(MINI)).toEqual([MAC, MINI]);
  });
});
