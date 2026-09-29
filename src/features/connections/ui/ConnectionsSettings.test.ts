// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { ConnectionsSettings } from "./ConnectionsSettings";
import {
  REMOTE_PROVIDERS,
  type RemoteMachine,
  type SshSetup,
} from "../model/protocol";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
let container: HTMLDivElement;
let root: Root;
let state: SshSetup;
let machines: RemoteMachine[];
const machine: RemoteMachine = {
  id: "machine",
  name: "Home Mac",
  environmentId: "env",
  endpoint: "ssh://me@home",
  ssh: { target: "me@home", remotePort: 3774 },
};
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers();
  machines = [];
  state = { id: "setup", message: "Installing host…", done: false };
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "remote_machines") return [...machines];
    if (command === "remote_ssh_begin" || command === "remote_ssh_reconnect")
      return "setup";
    if (command === "remote_ssh_poll") return { ...state };
    if (command === "remote_request")
      return { environmentId: "env", providers: ["codex"] };
    if (command === "remote_disconnect") {
      machines = [];
      return;
    }
    if (command === "remote_ssh_cancel" || command === "remote_ssh_answer")
      return;
    throw new Error(`Unexpected command ${command}`);
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
const button = (name: string) =>
  [...container.querySelectorAll("button")].find(
    (button) => button.textContent?.trim() === name,
  )!;
async function render() {
  await act(async () => root.render(createElement(ConnectionsSettings)));
}
async function fill(selector: string, value: string) {
  const input = container.querySelector<HTMLInputElement>(selector)!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function start() {
  await render();
  await act(async () => button("Add machine").click());
  await fill(
    'input[placeholder="user@my-mac-mini or an SSH alias"]',
    "me@home",
  );
  await act(async () => button("Connect").click());
}
async function poll() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(400);
  });
}

it("starts SSH setup from Settings and makes the machine available after native pairing", async () => {
  await start();
  expect(invoke).toHaveBeenCalledWith("remote_ssh_begin", {
    target: "me@home",
    name: "",
    port: null,
  });
  expect(container.textContent).toContain("Installing host…");
  machines = [machine];
  state = { ...state, done: true, machine };
  await poll();
  expect(container.textContent).toContain("Home Mac is connected");
  expect(container.textContent).toContain("SSH · me@home");
  expect(
    container.querySelector(
      'input[placeholder="user@my-mac-mini or an SSH alias"]',
    ),
  ).toBeNull();
});

it("requires an explicit host trust answer and forwards secrets only to the native prompt", async () => {
  state.prompt = {
    id: "trust",
    message: "Host fingerprint: SHA256:example",
    confirm: true,
  };
  await start();
  expect(invoke).not.toHaveBeenCalledWith(
    "remote_ssh_answer",
    expect.anything(),
  );
  await act(async () => button("Trust host and continue").click());
  expect(invoke).toHaveBeenCalledWith("remote_ssh_answer", {
    jobId: "setup",
    promptId: "trust",
    answer: "yes",
  });
  state = {
    ...state,
    prompt: { id: "password", message: "Password:", confirm: false },
  };
  await poll();
  await fill(
    'input[aria-label="SSH password or passphrase"]',
    "secret-for-this-prompt",
  );
  await act(async () => button("Continue").click());
  expect(invoke).toHaveBeenCalledWith("remote_ssh_answer", {
    jobId: "setup",
    promptId: "password",
    answer: "secret-for-this-prompt",
  });
  expect(
    container.querySelector<HTMLInputElement>(
      'input[aria-label="SSH password or passphrase"]',
    )!.value,
  ).toBe("");
});

it("keeps the SSH address after a failed install and cancels active setup when Settings closes", async () => {
  state = { ...state, done: true, error: "Host package is unavailable" };
  await start();
  expect(container.textContent).toContain("Host package is unavailable");
  expect(
    container.querySelector<HTMLInputElement>(
      'input[placeholder="user@my-mac-mini or an SSH alias"]',
    )!.value,
  ).toBe("me@home");
  state = { id: "setup", message: "Connecting…", done: false };
  await act(async () => button("Connect").click());
  await act(async () => root.unmount());
  root = createRoot(container);
  expect(invoke).toHaveBeenCalledWith("remote_ssh_cancel", { jobId: "setup" });
});

async function openRemove() {
  machines = [machine];
  await render();
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Remove Home Mac"]')!
      .click(),
  );
}

it("offers an explicit host update for an SSH machine missing workspace methods", async () => {
  machines = [machine];
  await render();
  expect(container.textContent).toContain(
    "host update needed for Explorer and Changes",
  );
  expect(container.textContent).toContain("interrupts active agent turns");
  await act(async () => button("Update Host").click());
  expect(invoke).toHaveBeenCalledWith("remote_ssh_reconnect", {
    machineId: machine.id,
    upgrade: true,
  });
});

it("advertises every supported provider when checking a host", async () => {
  machines = [machine];
  await render();
  expect(invoke).toHaveBeenCalledWith("remote_request", {
    machineId: machine.id,
    method: "environment.describe",
    params: { supportedProviders: REMOTE_PROVIDERS },
  });
});
const requested = (method: string) =>
  vi
    .mocked(invoke)
    .mock.calls.some(
      ([command, params]) =>
        command === "remote_request" &&
        (params as { method: string }).method === method,
    );

it("explains removal and removes the saved connection without stopping or revoking", async () => {
  await openRemove();
  expect(invoke).not.toHaveBeenCalledWith("remote_disconnect", {
    machineId: "machine",
  });
  expect(container.textContent).toContain("It does not stop the host");
  expect(container.textContent).toContain(
    "leaves this desktop’s credential valid",
  );
  expect(container.textContent).toContain(
    "~/.monocode-host/bin/monocode-host service uninstall",
  );
  await act(async () => button("Remove from this desktop only").click());
  expect(invoke).toHaveBeenCalledWith("remote_disconnect", {
    machineId: "machine",
  });
  expect(requested("devices.revokeSelf")).toBe(false);
  expect(
    vi
      .mocked(invoke)
      .mock.calls.some(([, params]) =>
        JSON.stringify(params ?? {}).includes('"stop"'),
      ),
  ).toBe(false);
});

it("revokes this desktop's credential before removing the connection", async () => {
  await openRemove();
  await act(async () => button("Revoke access and remove").click());
  const calls = vi
    .mocked(invoke)
    .mock.calls.map(([command, params]) =>
      command === "remote_request"
        ? (params as { method: string }).method
        : command,
    );
  expect(calls.indexOf("devices.revokeSelf")).toBeGreaterThanOrEqual(0);
  expect(calls.indexOf("remote_disconnect")).toBeGreaterThan(
    calls.indexOf("devices.revokeSelf"),
  );
  expect(container.textContent).toContain("access was revoked");
});

it("keeps the connection when the host cannot revoke its credential", async () => {
  const fallback = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockImplementation(async (command, params) => {
    if (
      command === "remote_request" &&
      (params as { method: string }).method === "devices.revokeSelf"
    )
      throw "Machine is unreachable";
    return fallback(command, params);
  });
  await openRemove();
  await act(async () => button("Revoke access and remove").click());
  expect(invoke).not.toHaveBeenCalledWith("remote_disconnect", {
    machineId: "machine",
  });
  expect(container.textContent).toContain("Could not revoke access");
  expect(button("Remove from this desktop only")).toBeTruthy();
});

it("does not spellcheck or autocorrect the machine name", async () => {
  await render();
  await act(async () => button("Add machine").click());
  const name = container.querySelector<HTMLInputElement>(
    'input[placeholder="Optional, e.g. Home Mac mini"]',
  )!;
  expect(name.getAttribute("spellcheck")).toBe("false");
  expect(name.getAttribute("autocorrect")).toBe("off");
  expect(name.getAttribute("autocapitalize")).toBe("off");
  expect(container.textContent).toContain("loginctl enable-linger");
});
