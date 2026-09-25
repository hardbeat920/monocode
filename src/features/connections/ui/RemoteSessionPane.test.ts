// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { RemoteSessionPane } from "./RemoteSessionPane";
import {
  clearPendingRemoteCommand,
  pendingRemoteCommand,
  rememberSession,
  rememberWorkspace,
  saveRemoteDraft,
  savePendingRemoteCommand,
} from "../model/connections";
import type {
  HostCommand,
  HostSession,
  RemoteMachine,
} from "../model/protocol";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../sessions/ui/AgentTranscript", () => ({
  AgentTranscript: () => createElement("div", null, "Host transcript"),
}));

const machine: RemoteMachine = {
  id: "machine",
  name: "Home server",
  endpoint: "http://127.0.0.1:3774",
  environmentId: "env",
};
const snapshot: HostSession = {
  projectId: "project",
  revision: 1,
  status: "idle",
  updatedAt: 0,
  session: {
    id: "session",
    harness: "codex",
    model: "codex:test",
    modelSettings: {},
    runtimeMode: "supervised",
    cwd: "/host/repo",
    title: "Host session",
    blocks: [],
  },
};
let root: Root;
let container: HTMLDivElement;
let failSend: boolean;
let hostIdentity: string;
let commands: HostCommand[];

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  failSend = true;
  hostIdentity = "env";
  commands = [];
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  rememberWorkspace("/laptop/repo", "env", {
    id: "project",
    name: "Repo",
    cwd: "/host/repo",
  });
  rememberSession("/laptop/repo", "env", "session");
  saveRemoteDraft("/laptop/repo", "env:session", "Do the work");
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command, input) => {
    expect(command).toBe("remote_request");
    const { method, params, machineId } = input as {
      method: string;
      params: HostCommand;
      machineId: string;
    };
    expect(machineId).toBe("machine");
    if (method === "environment.describe")
      return {
        protocolVersion: 1,
        environmentId: hostIdentity,
        providers: ["codex"],
        capabilities: ["sessions"],
      };
    if (method === "sessions.list")
      return [{ id: "session", title: "Host session", status: "idle" }];
    if (method === "sessions.sync") return { kind: "snapshot", value: snapshot };
    if (method === "commands.dispatch") {
      commands.push(params);
      if (failSend) throw new Error("Machine is unreachable");
      return { commandId: params.commandId, sessionId: "session", revision: 2 };
    }
    throw new Error(`Unexpected method ${method}`);
  });
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  localStorage.clear();
});

async function render() {
  await act(async () => {
    root.render(
      createElement(RemoteSessionPane, {
        machine,
        project: "/laptop/repo",
        machinePicker: "Machine picker",
      }),
    );
  });
}

it("retains a draft and the original command ID across UI closure and an ambiguous send", async () => {
  await render();
  const form = container.querySelector("textarea")!.form!;
  await act(async () => {
    form.dispatchEvent(
      new Event("submit", { bubbles: true, cancelable: true }),
    );
  });
  expect(commands).toHaveLength(1);
  expect(pendingRemoteCommand("/laptop/repo", "env")?.commandId).toBe(
    commands[0].commandId,
  );
  expect(container.querySelector("textarea")!.value).toBe("Do the work");
  await act(async () => root.unmount());
  root = createRoot(container);
  failSend = false;
  await render();
  const retry = [...container.querySelectorAll("button")].find(
    (button) => button.textContent === "Retry request",
  )!;
  await act(async () => retry.click());
  expect(commands).toHaveLength(2);
  expect(commands[1]).toEqual(commands[0]);
  expect(pendingRemoteCommand("/laptop/repo", "env")).toBeUndefined();
  expect(container.querySelector("textarea")!.value).toBe("");
});

it("never enables execution against a host whose identity changed", async () => {
  hostIdentity = "replacement-host";
  await render();
  expect(container.textContent).toContain("Host identity changed");
  expect(commands).toHaveLength(0);
  expect(
    vi
      .mocked(invoke)
      .mock.calls.every(([command]) => command === "remote_request"),
  ).toBe(true);
});

it("does not send cancellation when the remote view unmounts", async () => {
  await render();
  await act(async () => root.unmount());
  root = createRoot(container);
  expect(commands).toHaveLength(0);
});

it("does not dispatch when it cannot retain a request for safe retry", async () => {
  await render();
  const storage = localStorage;
  vi.stubGlobal("localStorage", {
    getItem: storage.getItem.bind(storage),
    setItem: () => {
      throw new Error("Storage quota exceeded");
    },
  });
  await act(async () => {
    container
      .querySelector("textarea")!
      .form!.dispatchEvent(
        new Event("submit", { bubbles: true, cancelable: true }),
      );
  });
  expect(commands).toHaveLength(0);
  expect(container.textContent).toContain("Cannot save your request locally");
  expect(container.querySelector("textarea")!.value).toBe("Do the work");
});

it("keeps another pane's uncertain request when a late receipt arrives", () => {
  const command: HostCommand = {
    type: "send",
    commandId: "first",
    sessionId: "session",
    text: "First",
  };
  savePendingRemoteCommand("/laptop/repo", "env", command);
  savePendingRemoteCommand("/laptop/repo", "env", {
    ...command,
    commandId: "second",
    text: "Second",
  });
  clearPendingRemoteCommand("/laptop/repo", "env", "first");
  expect(pendingRemoteCommand("/laptop/repo", "env")?.commandId).toBe("second");
});
