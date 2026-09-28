// @vitest-environment happy-dom
import { act, createElement, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { AddRemoteProjectDialog } from "./AddRemoteProjectDialog";
import { remoteProjectFor } from "../model/remoteProjects";
import { isLocalProject, looksLikeProject } from "../../projects/model/recents";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

let root: Root;
let container: HTMLDivElement;
let machines: unknown[];
const opened: string[] = [];

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  opened.length = 0;
  machines = [
    {
      id: "machine",
      name: "Home server",
      endpoint: "ssh://home",
      environmentId: "env",
    },
  ];
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command, input) => {
    if (command === "remote_machines") return machines;
    const { method, params } = input as {
      method: string;
      params: { path?: string; cwd?: string };
    };
    if (method === "projects.browse" && params.path === "/home/me/code/app")
      return {
        path: "/home/me/code/app",
        parent: "/home/me/code",
        entries: [],
      };
    if (method === "projects.browse")
      return params.path === "/home/me/code"
        ? {
            path: "/home/me/code",
            parent: "/home/me",
            entries: [{ name: "app", path: "/home/me/code/app" }],
          }
        : {
            path: "/home/me",
            parent: "/home",
            entries: [{ name: "code", path: "/home/me/code" }],
          };
    if (method === "projects.open")
      return { id: "host-project", cwd: params.cwd, name: "app" };
    throw new Error(`Unexpected ${method}`);
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
  localStorage.clear();
});

async function render() {
  // The app runs in StrictMode, which remounts components in development.
  await act(async () =>
    root.render(
      createElement(
        StrictMode,
        null,
        createElement(AddRemoteProjectDialog, {
          onCancel: vi.fn(),
          onOpen: (key: string) => opened.push(key),
        }),
      ),
    ),
  );
  for (let i = 0; i < 4; i++)
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
}
const button = (text: string) =>
  [...document.body.querySelectorAll("button")].find((candidate) =>
    candidate.textContent?.includes(text),
  )!;

it("browses a machine's folders and adds the chosen one as a project", async () => {
  await render();
  await act(async () => button("code").click());
  await act(async () => button("app").click());
  const path = document.body.querySelector<HTMLInputElement>(
    'input[aria-label="Folder path on the machine"]',
  )!;
  expect(path.value).toBe("/home/me/code/app");
  expect(path.getAttribute("spellcheck")).toBe("false");
  await act(async () => button("Open").click());
  expect(opened).toEqual(["remote://env/home/me/code/app"]);
  expect(remoteProjectFor(opened[0])).toEqual({
    key: "remote://env/home/me/code/app",
    environmentId: "env",
    projectId: "host-project",
    cwd: "/home/me/code/app",
  });
  // It is a rail project, but never a folder on this computer.
  expect(looksLikeProject(opened[0])).toBe(true);
  expect(isLocalProject(opened[0])).toBe(false);
});

it("points to Settings when no machine is connected", async () => {
  machines = [];
  await render();
  expect(document.body.textContent).toContain("No machines are connected yet");
  expect(button("Add a machine")).toBeTruthy();
});
