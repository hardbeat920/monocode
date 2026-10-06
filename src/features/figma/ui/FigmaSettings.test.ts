// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { loadFigmaPanelEnabled } from "../../settings/model/settings";
import type { FigmaBridgeStatus } from "../model/figma";
import {
  loadFigmaDefaultModel,
  saveFigmaDefaultModel,
} from "../model/figmaModels";
import { FigmaSettings } from "./FigmaSettings";

const invoke = vi.hoisted(() => vi.fn());
const bridgeListeners = vi.hoisted(
  () => [] as Array<(event: { payload: unknown }) => void>,
);
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (
    _event: string,
    handler: (event: { payload: unknown }) => void,
  ) => {
    bridgeListeners.push(handler);
    return () => {};
  },
}));

let root: Root;
let container: HTMLDivElement;
let status: FigmaBridgeStatus;

function baseStatus(): FigmaBridgeStatus {
  return {
    enabled: false,
    listening: false,
    port: 3056,
    error: null,
    pluginDirectory: "/data/figma-plugin",
    pluginInstalled: false,
    connections: [],
  };
}

async function render() {
  await act(async () => {
    root.render(createElement(FigmaSettings));
  });
}

function button(label: string): HTMLButtonElement {
  const match = [...container.querySelectorAll("button")].find(
    (element) => element.textContent === label,
  );
  if (!match) throw new Error(`Missing button ${label}`);
  return match;
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const storage = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
    removeItem: (key: string) => storage.delete(key),
  });
  bridgeListeners.length = 0;
  status = baseStatus();
  invoke.mockReset();
  invoke.mockImplementation(
    async (command: string, args?: { enabled?: boolean }) => {
      if (command === "figma_bridge_status") return { ...status };
      if (command === "figma_bridge_set_enabled") {
        status = {
          ...status,
          enabled: args?.enabled === true,
          listening: args?.enabled === true,
        };
        return { ...status };
      }
      if (command === "figma_plugin_install") {
        status = { ...status, pluginInstalled: true };
        return { ...status };
      }
      if (command === "reveal_path") return undefined;
      throw new Error(`Unexpected command ${command}`);
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it("turns the bridge on and shows the Figma sidebar panel", async () => {
  await render();
  expect(container.textContent).toContain("Off. Turn it on");
  expect(loadFigmaPanelEnabled()).toBe(false);
  await act(async () => button("Turn on").click());
  expect(invoke).toHaveBeenCalledWith("figma_bridge_set_enabled", {
    enabled: true,
  });
  expect(container.textContent).toContain("Listening on localhost:3056");
  expect(loadFigmaPanelEnabled()).toBe(true);
  expect(button("Install plugin")).toBeTruthy();
});

it("installs the plugin and explains how to import it", async () => {
  status = { ...baseStatus(), enabled: true, listening: true };
  await render();
  await act(async () => button("Install plugin").click());
  expect(invoke).toHaveBeenCalledWith("figma_plugin_install");
  expect(container.textContent).toContain("Import plugin from manifest");
  expect(container.textContent).toContain("/data/figma-plugin/manifest.json");
  expect(button("Reinstall plugin")).toBeTruthy();
  expect(
    invoke.mock.calls.filter(([command]) => command === "figma_bridge_status"),
  ).toHaveLength(1);
});

it("reveals the manifest to import and reports a failed copy", async () => {
  status = {
    ...baseStatus(),
    enabled: true,
    listening: true,
    pluginInstalled: true,
  };
  vi.stubGlobal("navigator", {
    clipboard: {
      writeText: async () => {
        throw new Error("Clipboard access was denied");
      },
    },
  });
  Object.defineProperty(document, "execCommand", {
    configurable: true,
    value: () => false,
  });
  await render();
  await act(async () => button("Reveal folder").click());
  expect(invoke).toHaveBeenCalledWith("reveal_path", {
    path: "/data/figma-plugin/manifest.json",
  });
  await act(async () => button("Copy path").click());
  Reflect.deleteProperty(document, "execCommand");
  expect(container.querySelector('[role="alert"]')?.textContent).toBe(
    "copy failed",
  );
  expect(button("Copy path")).toBeTruthy();
});

it("follows live bridge updates and lists connected files", async () => {
  status = {
    ...baseStatus(),
    enabled: true,
    listening: true,
    pluginInstalled: true,
  };
  await render();
  expect(container.textContent).toContain("No Figma file is connected");
  await act(async () => {
    for (const listener of bridgeListeners)
      listener({
        payload: {
          ...status,
          connections: [
            {
              id: "connection-1",
              connectedAt: 1,
              selection: {
                selectionCount: 1,
                document: {
                  id: "0:0",
                  name: "Design system",
                  fileKey: null,
                  pageId: "0:1",
                  pageName: "Buttons",
                },
                source: {
                  nodeId: "1:2",
                  name: "Primary",
                  type: "COMPONENT",
                  width: 120,
                  height: 40,
                },
              },
            },
          ],
        },
      });
  });
  expect(container.textContent).toContain("Design system / Buttons");
  expect(container.textContent).toContain("Primary · COMPONENT · 120 × 40");
});

it("sets a Figma default model and returns to the project default", async () => {
  status = {
    ...baseStatus(),
    enabled: true,
    listening: true,
    pluginInstalled: true,
  };
  await render();
  expect(container.textContent).toContain("Generation model");
  expect(container.textContent).toContain(
    "the project's default agent and model from Providers",
  );
  expect(
    [...container.querySelectorAll("button")].some(
      (element) => element.textContent === "Use project default",
    ),
  ).toBe(false);
  await act(async () => {
    saveFigmaDefaultModel({
      harness: "codex",
      model: "gpt-5",
      modelSettings: {},
    });
  });
  expect(container.textContent).toContain(
    "Figma starts a new one with this agent and model",
  );
  expect(
    container
      .querySelector("button[aria-keyshortcuts]")
      ?.getAttribute("aria-label"),
  ).toMatch(/^Codex/);
  await act(async () => button("Use project default").click());
  expect(loadFigmaDefaultModel()).toBeNull();
  expect(container.textContent).toContain(
    "the project's default agent and model from Providers",
  );
});

it("surfaces a bridge that could not listen", async () => {
  status = {
    ...baseStatus(),
    enabled: true,
    error: "Port 3056 is already in use by another app",
  };
  await render();
  expect(container.querySelector('[role="alert"]')?.textContent).toBe(
    "Port 3056 is already in use by another app",
  );
});
