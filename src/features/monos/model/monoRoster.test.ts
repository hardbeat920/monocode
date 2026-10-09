// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  addMonoProject,
  createMono,
  finishMonoPlanBuild,
  findMono,
  listMonos,
  monoLook,
  monoProjectsPhrase,
  monoWorksOn,
  removeMono,
  removeMonoProject,
  reorderMonos,
  saveMonoPlanMode,
  saveMonoSessionId,
} from "./mono";
import { submitAfterProjectSync } from "../../../app/model/submissionAcceptance";
import { monoBackgroundKey, monoChatBackground } from "./monoBackground";
import { saveProjectChatBackgroundSettings } from "../../projects/model/projectChatBackground";
import { monoSubmissionIntent } from "../../sessions/model/plan";

beforeEach(() => {
  const stored = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
    removeItem: (key: string) => stored.delete(key),
  });
});
afterEach(() => vi.unstubAllGlobals());

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

it("persists Plan mode with the Mono across reloads", () => {
  const { id } = createMono();
  saveMonoPlanMode(id, true);
  expect(findMono(id)?.planMode).toBe(true);
  expect(listMonos()[0].planMode).toBe(true);
  saveMonoSessionId(id, "rotated-session");
  expect(findMono(id)?.planMode).toBe(true);
  expect(findMono(id)?.sessionId).toBe("rotated-session");
  saveMonoPlanMode(id, false);
  expect(findMono(id)?.planMode).toBe(false);
});

it("leaves Plan mode on when Build is rejected and exits after acceptance", () => {
  const { id } = createMono();
  saveMonoPlanMode(id, true);
  const revision = findMono(id)!.planModeRevision ?? 0;
  finishMonoPlanBuild(id, revision, false);
  expect(findMono(id)?.planMode).toBe(true);
  finishMonoPlanBuild(id, revision, true);
  expect(findMono(id)?.planMode).toBe(false);
});

it("keeps a newer Plan mode choice after deferred Build preparation", async () => {
  const { id } = createMono();
  saveMonoPlanMode(id, true);
  const revision = findMono(id)!.planModeRevision ?? 0;
  let finishSync!: (location: {
    path: string;
    identity: string;
    moved: boolean;
  }) => void;
  const submit = vi.fn(() => {
    finishMonoPlanBuild(id, revision, true);
    return true;
  });
  const accepted = submitAfterProjectSync({
    cwd: "/repo",
    sync: new Promise((resolve) => (finishSync = resolve)),
    applyLocationChange: vi.fn(async () => {}),
    submit,
    onError: vi.fn(),
  });

  saveMonoPlanMode(id, false);
  saveMonoPlanMode(id, true);
  finishSync({ path: "/repo", identity: "repo", moved: false });

  await expect(accepted).resolves.toBe(true);
  expect(submit).toHaveBeenCalledOnce();
  expect(findMono(id)?.planMode).toBe(true);
});

it("keeps Plan mode when queued work rejects an explicit Build", () => {
  const { id } = createMono();
  saveMonoPlanMode(id, true);
  const revision = findMono(id)!.planModeRevision ?? 0;
  const intent = monoSubmissionIntent(true, "build", {
    approvedPlanBuild: true,
    hasApprovedPlan: true,
    canStartBuild: false,
  });
  expect(intent).toBeNull();
  finishMonoPlanBuild(id, revision, intent === "build");
  expect(findMono(id)?.planMode).toBe(true);

  // submitSession returns false for a disconnected-provider Build attempt.
  finishMonoPlanBuild(id, revision, false);
  expect(findMono(id)?.planMode).toBe(true);
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
