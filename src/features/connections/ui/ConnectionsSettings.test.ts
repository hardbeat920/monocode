// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { ConnectionsSettings } from "./ConnectionsSettings";
import type { RemoteMachine, SshSetup } from "../model/protocol";

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

it("removes the saved connection without issuing a remote stop", async () => {
  machines = [machine];
  await render();
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Remove Home Mac"]')!
      .click(),
  );
  expect(invoke).toHaveBeenCalledWith("remote_disconnect", {
    machineId: "machine",
  });
  expect(
    vi
      .mocked(invoke)
      .mock.calls.some(([, params]) =>
        JSON.stringify(params ?? {}).includes('"stop"'),
      ),
  ).toBe(false);
});
