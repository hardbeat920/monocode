// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { QuickLaunch } from "../../quick-composer/model/quickComposer";
import {
  newSession,
  type Attachment,
  type HarnessId,
  type Session,
} from "../../sessions/model/session";
import {
  FIGMA_SESSION_MODEL_EVENT,
  requestFigmaSessionModel,
  type FigmaGeneration,
  type FigmaPreviewWorkspace,
  type FigmaSessionModelRequest,
} from "../model/figma";
import { figmaActivityFor, reportFigmaActivity } from "../model/figmaActivity";
import type { FigmaModelChoice } from "../model/figmaModels";
import {
  applyFigmaSessionModel,
  deliverFigmaGeneration,
  figmaLaunchId,
} from "./useFigmaLaunches";

const generation: FigmaGeneration = {
  id: "1727790000000-abcdef12",
  previewBytes: 2048,
  source: {
    nodeId: "12:34",
    name: "Card",
    type: "FRAME",
    width: 320,
    height: 200,
  },
  document: {
    id: "0:0",
    name: "App",
    fileKey: null,
    pageId: "0:1",
    pageName: "Screens",
  },
  diagnostics: [],
};

const choice: FigmaModelChoice = {
  harness: "codex",
  model: "gpt-5",
  modelSettings: { effort: "high" },
};

function session(cwd: string, changes: Partial<Session> = {}): Session {
  return {
    ...newSession("claude", cwd, "opus"),
    title: "Checkout",
    ...changes,
  };
}

function previewIn(cwd: string): FigmaPreviewWorkspace {
  return {
    directory: `${cwd}/.monocode/figma/${generation.id}`,
    relativeDirectory: `.monocode/figma/${generation.id}`,
    previewPath: `${cwd}/.monocode/figma/${generation.id}/design/preview.png`,
  };
}

function workspace(selected: Session | null, accepted = true) {
  return {
    launch: vi.fn<(launch: QuickLaunch, id: string) => Promise<void>>(
      async () => {},
    ),
    submit: vi.fn<
      (sessionId: string, text: string, attachments: Attachment[]) => boolean
    >(() => accepted),
    prepare: vi.fn<
      (generationId: string, cwd: string) => Promise<FigmaPreviewWorkspace>
    >(async (_id, cwd) => previewIn(cwd)),
    selected,
  };
}

beforeEach(() => {
  const storage = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
    removeItem: (key: string) => storage.delete(key),
  });
});

afterEach(() => {
  reportFigmaActivity(null);
  vi.unstubAllGlobals();
});

it("generates in the selected session of the project", async () => {
  const selected = session("/work/app");
  const target = workspace(selected);
  await deliverFigmaGeneration(generation, "/work/app", choice, target);
  expect(target.launch).not.toHaveBeenCalled();
  expect(target.prepare).toHaveBeenCalledWith(generation.id, "/work/app");
  expect(target.submit).toHaveBeenCalledTimes(1);
  const [sessionId, prompt, attachments] = target.submit.mock.calls[0];
  expect(sessionId).toBe(selected.id);
  expect(prompt).toContain("pixel-perfect (1:1)");
  expect(prompt).toContain(
    `Write the component files into \`.monocode/figma/${generation.id}\``,
  );
  expect(prompt).toContain("Do not paste their code in the chat");
  expect(attachments.map((attachment) => attachment.path)).toEqual([
    previewIn("/work/app").previewPath,
  ]);
  expect(figmaActivityFor("/work/app")).toEqual({
    status: "started",
    cwd: "/work/app",
    generation,
    choice: {
      harness: "claude",
      model: selected.model,
      modelSettings: selected.modelSettings,
    },
    target: { kind: "session", sessionId: selected.id, title: "Checkout" },
  });
});

it("prepares the preview in the selected session's worktree", async () => {
  const target = workspace(
    session("/work/app", { worktreeCwd: "/work/app-worktrees/figma" }),
  );
  await deliverFigmaGeneration(generation, "/work/app", choice, target);
  expect(target.prepare).toHaveBeenCalledWith(
    generation.id,
    "/work/app-worktrees/figma",
  );
  expect(target.submit.mock.calls[0][2][0]?.path).toBe(
    previewIn("/work/app-worktrees/figma").previewPath,
  );
});

it("reports a preview folder it could not prepare", async () => {
  const target = workspace(session("/work/app"));
  target.prepare.mockRejectedValueOnce(
    new Error("Open a project folder to generate the component in"),
  );
  await deliverFigmaGeneration(generation, "/work/app", choice, target);
  expect(target.submit).not.toHaveBeenCalled();
  expect(figmaActivityFor("/work/app")).toEqual({
    status: "failed",
    cwd: "/work/app",
    message: "Open a project folder to generate the component in",
  });
});

it("hands a busy selected session the component to queue", async () => {
  const target = workspace(session("/work/app", { busy: true }));
  await deliverFigmaGeneration(generation, "/work/app", choice, target);
  expect(target.submit).toHaveBeenCalledTimes(1);
  expect(target.launch).not.toHaveBeenCalled();
});

it("starts a new session when the project has no selected session", async () => {
  const target = workspace(null);
  await deliverFigmaGeneration(generation, "/work/app", choice, target);
  expect(target.submit).not.toHaveBeenCalled();
  expect(target.prepare).toHaveBeenCalledWith(generation.id, "/work/app");
  expect(target.launch).toHaveBeenCalledTimes(1);
  const [launch, id] = target.launch.mock.calls[0];
  expect(id).toBe(figmaLaunchId(generation));
  expect(launch).toMatchObject({
    cwd: "/work/app",
    harness: "codex",
    model: "gpt-5",
    modelSettings: { effort: "high" },
    reveal: true,
  });
  expect(figmaActivityFor("/work/app")).toEqual({
    status: "started",
    cwd: "/work/app",
    generation,
    choice,
    target: { kind: "new" },
  });
});

it("ignores a selected session from another project or one it cannot use", async () => {
  for (const selected of [
    session("/work/site"),
    session("/work/app", { orchestrationLeadId: "lead" }),
    session("/work/app", { worktreeRemoved: true }),
    session("/work/app", { workspaceMode: "worktree" }),
  ]) {
    const target = workspace(selected);
    await deliverFigmaGeneration(generation, "/work/app", choice, target);
    expect(target.submit).not.toHaveBeenCalled();
    expect(target.launch).toHaveBeenCalledTimes(1);
  }
});

it("reports a selected session that refuses the component", async () => {
  const target = workspace(session("/work/app"), false);
  await deliverFigmaGeneration(generation, "/work/app", choice, target);
  expect(figmaActivityFor("/work/app")).toEqual({
    status: "failed",
    cwd: "/work/app",
    message:
      "Checkout cannot take the component right now. Wait for its turn to finish or select another session.",
  });
});

it("refuses to generate outside a local project", async () => {
  const target = workspace(session("/work/app"));
  await deliverFigmaGeneration(generation, "remote://host/app", choice, target);
  expect(figmaActivityFor("remote://host/app")?.status).toBe("failed");
  await deliverFigmaGeneration(generation, "~", choice, target);
  expect(figmaActivityFor("~")?.status).toBe("failed");
  expect(figmaActivityFor("/work/app")).toBeNull();
  expect(target.prepare).not.toHaveBeenCalled();
  expect(target.submit).not.toHaveBeenCalled();
  expect(target.launch).not.toHaveBeenCalled();
});

it("sends the panel's model change to the workspace", () => {
  const received: FigmaSessionModelRequest[] = [];
  const onModel = (event: Event) =>
    received.push((event as CustomEvent<FigmaSessionModelRequest>).detail);
  window.addEventListener(FIGMA_SESSION_MODEL_EVENT, onModel);
  requestFigmaSessionModel({
    sessionId: "s1",
    kind: "model",
    harness: "codex",
    model: "gpt-5",
  });
  window.removeEventListener(FIGMA_SESSION_MODEL_EVENT, onModel);
  expect(received).toEqual([
    { sessionId: "s1", kind: "model", harness: "codex", model: "gpt-5" },
  ]);
});

it("changes the selected session's model the same way the composer does", () => {
  const changeModel =
    vi.fn<(sessionId: string, harness: HarnessId, model: string) => void>();
  const changeModelSettings =
    vi.fn<(sessionId: string, modelSettings: Record<string, string>) => void>();
  applyFigmaSessionModel(
    { sessionId: "s1", kind: "model", harness: "codex", model: "gpt-5" },
    { changeModel, changeModelSettings },
  );
  applyFigmaSessionModel(
    { sessionId: "s1", kind: "settings", modelSettings: { effort: "low" } },
    { changeModel, changeModelSettings },
  );
  expect(changeModel).toHaveBeenCalledWith("s1", "codex", "gpt-5");
  expect(changeModelSettings).toHaveBeenCalledWith("s1", { effort: "low" });
});
