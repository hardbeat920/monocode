// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  addMonoProject,
  createMono,
  findMono,
  listMonos,
  markMonoOpened,
  monoLook,
  monoProjectsPhrase,
  monoWorksOn,
  removeMono,
  removeMonoProject,
  reorderMonos,
  projectLandingMono,
  saveMonoSessionId,
} from "./mono";
import { monoBackgroundKey, monoChatBackground } from "./monoBackground";
import { saveProjectChatBackgroundSettings } from "../../projects/model/projectChatBackground";

beforeEach(() => {
  const stored = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
    removeItem: (key: string) => stored.delete(key),
  });
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it("gives each new Mono a mascot and color the others are not using", () => {
  const first = createMono();
  const second = createMono();
  expect(second.mascot).not.toBe(first.mascot);
  expect(second.color).not.toBe(first.color);
  expect(monoLook(first).name).toMatch(/^Mono[A-Z]/);
  expect(first.projects).toEqual([]);
});

it("adds each project once and takes one away by its path", () => {
  const { id } = createMono(["/code/app"]);
  addMonoProject(id, "/code/site");
  addMonoProject(id, "/code/app/");
  expect(findMono(id)?.projects).toEqual(["/code/app", "/code/site"]);
  expect(monoWorksOn(findMono(id)!, "/code/site/")).toBe(true);
  removeMonoProject(id, "/code/app/");
  expect(findMono(id)?.projects).toEqual(["/code/site"]);
});

it("lands on the most recently opened used Mono assigned to a project", () => {
  const first = createMono(["/code/app"]);
  const second = createMono(["/code/app", "/code/site"]);
  const unused = createMono(["/code/app"]);
  saveMonoSessionId(first.id, "first-session");
  saveMonoSessionId(second.id, "second-session");
  expect(projectLandingMono("/code/app", second.id)?.id).toBe(second.id);
  vi.spyOn(Date, "now").mockReturnValueOnce(10).mockReturnValueOnce(20);
  markMonoOpened(first.id);
  markMonoOpened(second.id);

  expect(projectLandingMono("/code/app/")?.id).toBe(second.id);
  expect(projectLandingMono("/code/site")?.id).toBe(second.id);
  expect(projectLandingMono("/code/unused")).toBeUndefined();
  expect(projectLandingMono("/code/app")?.id).not.toBe(unused.id);
});

it("keeps the rail's order and forgets a removed Mono", () => {
  const a = createMono();
  const b = createMono();
  const c = createMono();
  reorderMonos([c.id, a.id, b.id]);
  expect(listMonos().map((mono) => mono.id)).toEqual([c.id, a.id, b.id]);
  removeMono(a.id);
  expect(listMonos().map((mono) => mono.id)).toEqual([c.id, b.id]);
});

it("names its projects the way a sentence would", () => {
  const project = (name: string) => ({ path: `/code/${name}`, name });
  expect(monoProjectsPhrase([])).toBe("");
  expect(monoProjectsPhrase([project("app")])).toBe("app");
  expect(
    monoProjectsPhrase([project("app"), project("site"), project("api")]),
  ).toBe("app, site and api");
});

it("forgets its background along with the Mono", () => {
  const mono = createMono();
  saveProjectChatBackgroundSettings(monoBackgroundKey(mono.id), {
    path: "/bg/mono.png",
    emptyOpacity: 0.24,
    sessionOpacity: 0.24,
    scope: "all",
    effect: "gradient-blur",
  });
  expect(monoChatBackground(mono.id)?.path).toBe("/bg/mono.png");
  removeMono(mono.id);
  expect(monoChatBackground(mono.id)).toBeNull();
});
