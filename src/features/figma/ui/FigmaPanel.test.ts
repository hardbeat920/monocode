// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { OPEN_CONNECTIONS_EVENT } from "../../connections/model/connections";
import {
  FIGMA_LAUNCH_EVENT,
  type FigmaBridgeStatus,
  type FigmaGeneration,
  type FigmaLaunchRequest,
} from "../model/figma";
import { reportFigmaActivity } from "../model/figmaActivity";
import {
  clearFigmaModelPick,
  pickFigmaModel,
  resolveFigmaModel,
  saveFigmaDefaultModel,
} from "../model/figmaModels";
import { setFigmaSessionTarget } from "../model/figmaTarget";
import { FigmaPanel } from "./FigmaPanel";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async () => () => {},
}));

const PREVIEW = "data:image/png;base64,iVBORw0KGgo=";

let root: Root;
let container: HTMLDivElement;
let status: FigmaBridgeStatus;

const generation: FigmaGeneration = {
  id: "1727790000000-abcdef12",
  previewBytes: 2048,
  source: {
    nodeId: "1:2",
    name: "Primary",
    type: "COMPONENT",
    width: 120,
    height: 40,
  },
  document: {
    id: "0:0",
    name: "Design system",
    fileKey: null,
    pageId: "0:1",
    pageName: "Buttons",
  },
  diagnostics: [],
};

function connectedStatus(
  source: FigmaGeneration["source"] | null,
  selectionCount = source ? 1 : 0,
): FigmaBridgeStatus {
  return {
    enabled: true,
    listening: true,
    port: 3056,
    error: null,
    pluginDirectory: "/data/figma-plugin",
    pluginInstalled: true,
    connections: [
      {
        id: "connection-1",
        connectedAt: 1,
        selection: { selectionCount, document: generation.document, source },
      },
    ],
  };
}

async function render(cwd = "/work/app") {
  await act(async () => {
    root.render(createElement(FigmaPanel, { cwd }));
  });
}

function modelTrigger(): HTMLButtonElement {
  const trigger = container.querySelector<HTMLButtonElement>(
    "button[aria-keyshortcuts]",
  );
  if (!trigger) throw new Error("Missing model picker");
  return trigger;
}

function generateButton(): HTMLButtonElement {
  const match = [...container.querySelectorAll("button")].find((element) =>
    element.textContent?.includes("Generate component"),
  );
  if (!match) throw new Error("Missing generate button");
  return match;
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const storage = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
    removeItem: (key: string) => storage.delete(key),
  });
  status = connectedStatus(generation.source);
  invoke.mockReset();
  invoke.mockImplementation(async (command: string) => {
    if (command === "figma_bridge_status") return status;
    if (command === "figma_selection_preview") return PREVIEW;
    if (command === "figma_generate") return generation;
    throw new Error(`Unexpected command ${command}`);
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  clearFigmaModelPick("/work/app");
  setFigmaSessionTarget(null);
  reportFigmaActivity(null);
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it("shows the live selection with its rendered preview", async () => {
  await render();
  expect(container.textContent).toContain("Design system / Buttons");
  expect(container.textContent).toContain("COMPONENT · 120 × 40");
  await act(async () => {
    await vi.advanceTimersByTimeAsync(300);
  });
  expect(invoke).toHaveBeenCalledWith("figma_selection_preview", {
    connectionId: "connection-1",
    nodeId: "1:2",
  });
  expect(container.querySelector("img")?.getAttribute("src")).toBe(PREVIEW);
});

it("captures the selection and asks the workspace to launch it", async () => {
  const launches: FigmaLaunchRequest[] = [];
  const onLaunch = (event: Event) =>
    launches.push((event as CustomEvent<FigmaLaunchRequest>).detail);
  window.addEventListener(FIGMA_LAUNCH_EVENT, onLaunch);
  await render();
  await act(async () => generateButton().click());
  window.removeEventListener(FIGMA_LAUNCH_EVENT, onLaunch);
  expect(invoke).toHaveBeenCalledWith("figma_generate", {
    connectionId: "connection-1",
  });
  expect(launches).toEqual([
    {
      generation,
      cwd: "/work/app",
      choice: resolveFigmaModel("/work/app").choice,
    },
  ]);
});

it("shows which agent generates the component and lets the project pick another", async () => {
  await render();
  expect(container.textContent).toContain("Generate with");
  expect(container.textContent).toContain("Project default");
  await act(async () => {
    saveFigmaDefaultModel({
      harness: "codex",
      model: "gpt-5",
      modelSettings: {},
    });
  });
  expect(container.textContent).toContain("Figma default");
  expect(modelTrigger().getAttribute("aria-label")).toMatch(/^Codex/);
  await act(async () => {
    pickFigmaModel("/work/app", {
      harness: "claude",
      model: "opus",
      modelSettings: {},
    });
  });
  expect(container.textContent).toContain("Picked here");
  expect(modelTrigger().getAttribute("aria-label")).toMatch(/^Claude Code/);
  const reset = [...container.querySelectorAll("button")].find(
    (element) => element.textContent === "Reset",
  );
  await act(async () => reset?.click());
  expect(container.textContent).toContain("Figma default");
  const launches: FigmaLaunchRequest[] = [];
  const onLaunch = (event: Event) =>
    launches.push((event as CustomEvent<FigmaLaunchRequest>).detail);
  window.addEventListener(FIGMA_LAUNCH_EVENT, onLaunch);
  await act(async () => generateButton().click());
  window.removeEventListener(FIGMA_LAUNCH_EVENT, onLaunch);
  expect(launches[0]?.choice).toEqual({
    harness: "codex",
    model: "gpt-5",
    modelSettings: {},
  });
});

it("generates with the selected session's agent and changes it like the composer", async () => {
  await render();
  await act(async () => {
    setFigmaSessionTarget({
      sessionId: "session-1",
      cwd: "/work/app",
      workCwd: "/work/app",
      title: "Checkout page",
      harness: "codex",
      model: "gpt-5",
      modelSettings: {},
      busy: false,
    });
  });
  expect(container.textContent).toContain("Checkout page");
  expect(container.textContent).toContain(
    "Generated in the selected session as preview files that git ignores",
  );
  expect(container.textContent).not.toContain("Project default");
  expect(modelTrigger().getAttribute("aria-label")).toMatch(/^Codex/);
  await act(async () => {
    setFigmaSessionTarget({
      sessionId: "session-1",
      cwd: "/work/app",
      workCwd: "/work/app",
      title: "Checkout page",
      harness: "codex",
      model: "gpt-5",
      modelSettings: {},
      busy: true,
    });
  });
  expect(container.textContent).toContain("The component is queued");
});

it("ignores a selected session from another project", async () => {
  await act(async () => {
    setFigmaSessionTarget({
      sessionId: "session-2",
      cwd: "/work/site",
      workCwd: "/work/site",
      title: "Landing",
      harness: "codex",
      model: "gpt-5",
      modelSettings: {},
      busy: false,
    });
  });
  await render();
  expect(container.textContent).not.toContain("Landing");
  expect(container.textContent).toContain("starts a new one");
});

it("tells where the generation went", async () => {
  await render();
  await act(async () => {
    reportFigmaActivity({
      status: "started",
      cwd: "/work/app",
      generation,
      choice: { harness: "codex", model: "gpt-5", modelSettings: {} },
      target: {
        kind: "session",
        sessionId: "session-1",
        title: "Checkout page",
      },
    });
  });
  expect(container.textContent).toContain(
    "Sent Primary to Checkout page. Its preview files appear in that chat.",
  );
  await act(async () => {
    reportFigmaActivity({
      status: "started",
      cwd: "/work/app",
      generation,
      choice: { harness: "codex", model: "gpt-5", modelSettings: {} },
      target: { kind: "new" },
    });
  });
  expect(container.textContent).toContain("Started Codex for Primary.");
});

it("keeps another project's generation out of this project's panel", async () => {
  await render();
  await act(async () => {
    reportFigmaActivity({
      status: "failed",
      cwd: "/work/site",
      message: "The Figma plugin disconnected",
    });
  });
  expect(container.textContent).not.toContain("The Figma plugin disconnected");
  await act(async () => {
    reportFigmaActivity({
      status: "failed",
      cwd: "/work/app",
      message: "The Figma plugin disconnected",
    });
  });
  expect(container.textContent).toContain("The Figma plugin disconnected");
});

it("does not generate without exactly one selected layer", async () => {
  status = connectedStatus(null, 3);
  await render();
  expect(container.textContent).toContain("3 layers are selected");
  expect(generateButton().disabled).toBe(true);
});

it("keeps generation off for remote projects", async () => {
  await render("remote://host/home/me/app");
  expect(generateButton().disabled).toBe(true);
  expect(container.textContent).toContain("on this computer");
  expect(container.textContent).not.toContain("Generate with");
});

it("sends people to Settings while the bridge is off", async () => {
  status = { ...connectedStatus(null), enabled: false, connections: [] };
  const opened = vi.fn();
  window.addEventListener(OPEN_CONNECTIONS_EVENT, opened);
  await render();
  const settings = [...container.querySelectorAll("button")].find(
    (element) => element.textContent === "Open Figma settings",
  );
  await act(async () => settings?.click());
  window.removeEventListener(OPEN_CONNECTIONS_EVENT, opened);
  expect(container.textContent).toContain("The Figma bridge is off");
  expect(opened).toHaveBeenCalledTimes(1);
});
