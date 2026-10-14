// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { InboxProvider } from "./githubTasks";

const KEY = "monocode.inboxConnectors.v1";
const DEFAULTS = {
  github: true,
  gitlab: true,
  azuredevops: true,
  jira: true,
  linear: true,
};
let connectors: typeof import("./inboxConnectors");
let root: Root | undefined;
let container: HTMLDivElement;

beforeEach(async () => {
  vi.resetModules();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  connectors = await import("./inboxConnectors");
  container = document.createElement("div");
  document.body.appendChild(container);
});

afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function mountProbe() {
  function Probe() {
    const enabled = connectors.useInboxConnectors();
    return createElement("span", null, JSON.stringify(enabled));
  }
  root = createRoot(container);
  act(() => root!.render(createElement(Probe)));
}

function visibleConnectors() {
  return JSON.parse(container.textContent ?? "{}");
}

it("defaults all providers to enabled and returns a stable snapshot", () => {
  const snapshot = connectors.loadInboxConnectors();
  expect(snapshot).toEqual(DEFAULTS);
  expect(connectors.loadInboxConnectors()).toBe(snapshot);
  for (const provider of Object.keys(DEFAULTS) as InboxProvider[])
    expect(connectors.isInboxConnectorEnabled(provider)).toBe(true);
});

it("persists independent choices without changing connection credentials", async () => {
  localStorage.setItem("monocode.gitlab.token", "saved-token");
  connectors.setInboxConnectorEnabled("github", false);
  connectors.setInboxConnectorEnabled("jira", false);
  connectors.setInboxConnectorEnabled("github", true);
  const expected = { ...DEFAULTS, jira: false };
  expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual(expected);
  expect(localStorage.getItem("monocode.gitlab.token")).toBe("saved-token");
  vi.resetModules();
  const reloaded = await import("./inboxConnectors");
  expect(reloaded.loadInboxConnectors()).toEqual(expected);
  expect(reloaded.isInboxConnectorEnabled("jira")).toBe(false);
});

it("ignores invalid storage while retaining valid per-provider booleans", () => {
  for (const value of ["invalid json", "null", "[]", "42"] as const) {
    localStorage.setItem(KEY, value);
    expect(connectors.loadInboxConnectors()).toEqual(DEFAULTS);
  }
  localStorage.setItem(
    KEY,
    JSON.stringify({
      github: false,
      gitlab: "false",
      jira: null,
      future: false,
    }),
  );
  expect(connectors.loadInboxConnectors()).toEqual({
    ...DEFAULTS,
    github: false,
  });
});

it("preserves an external provider choice when changing another provider", () => {
  connectors.loadInboxConnectors();
  localStorage.setItem(KEY, JSON.stringify({ gitlab: false }));
  connectors.setInboxConnectorEnabled("github", false);
  expect(connectors.loadInboxConnectors()).toEqual({
    ...DEFAULTS,
    github: false,
    gitlab: false,
  });
});

it("updates mounted consumers immediately after an in-window change", () => {
  mountProbe();
  expect(visibleConnectors()).toEqual(DEFAULTS);
  act(() => connectors.setInboxConnectorEnabled("linear", false));
  expect(visibleConnectors()).toEqual({ ...DEFAULTS, linear: false });
  act(() => connectors.setInboxConnectorEnabled("linear", true));
  expect(visibleConnectors()).toEqual(DEFAULTS);
});

it("synchronizes another window's changes and storage clearing", () => {
  mountProbe();
  act(() => {
    localStorage.setItem(KEY, JSON.stringify({ azuredevops: false }));
    window.dispatchEvent(new StorageEvent("storage", { key: KEY }));
  });
  expect(visibleConnectors()).toEqual({ ...DEFAULTS, azuredevops: false });
  act(() => {
    localStorage.clear();
    window.dispatchEvent(new StorageEvent("storage", { key: null }));
  });
  expect(visibleConnectors()).toEqual(DEFAULTS);
});

it("ignores unrelated storage events and releases subscriptions", () => {
  const listener = vi.fn();
  const unsubscribe = connectors.subscribeInboxConnectors(listener);
  window.dispatchEvent(new StorageEvent("storage", { key: "unrelated" }));
  connectors.setInboxConnectorEnabled("github", true);
  expect(listener).not.toHaveBeenCalled();
  connectors.setInboxConnectorEnabled("github", false);
  expect(listener).toHaveBeenCalledOnce();
  unsubscribe();
  connectors.setInboxConnectorEnabled("github", true);
  window.dispatchEvent(new StorageEvent("storage", { key: KEY }));
  expect(listener).toHaveBeenCalledOnce();
});

it("keeps a failed persistence write effective and accepts a later external change", () => {
  mountProbe();
  const write = vi.spyOn(localStorage, "setItem").mockImplementation(() => {
    throw new Error("Storage full");
  });
  act(() => connectors.setInboxConnectorEnabled("gitlab", false));
  expect(visibleConnectors()).toEqual({ ...DEFAULTS, gitlab: false });
  expect(connectors.isInboxConnectorEnabled("gitlab")).toBe(false);
  write.mockRestore();
  act(() => {
    localStorage.setItem(KEY, JSON.stringify({ jira: false }));
    window.dispatchEvent(new StorageEvent("storage", { key: KEY }));
  });
  expect(visibleConnectors()).toEqual({ ...DEFAULTS, jira: false });
});

it("keeps local changes usable when storage access is unavailable", () => {
  mountProbe();
  vi.spyOn(localStorage, "getItem").mockImplementation(() => {
    throw new Error("Storage unavailable");
  });
  vi.spyOn(localStorage, "setItem").mockImplementation(() => {
    throw new Error("Storage unavailable");
  });
  act(() => connectors.setInboxConnectorEnabled("github", false));
  act(() => connectors.setInboxConnectorEnabled("jira", false));
  expect(visibleConnectors()).toEqual({
    ...DEFAULTS,
    github: false,
    jira: false,
  });
  expect(connectors.isInboxConnectorEnabled("linear")).toBe(true);
});
