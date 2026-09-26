// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { McpSettings } from "./McpSettings";

const invoke = vi.fn();
const ask = vi.fn(async () => true);
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: (...args: unknown[]) => ask(...args),
}));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  invoke.mockReset();
  invoke.mockImplementation(async (command: string) =>
    command === "mcp_discover"
      ? [
          {
            provider: "claude",
            name: "sentry",
            scope: "user",
            configPath: "/home/.claude.json",
            transport: "http",
          },
          {
            provider: "codex",
            name: "docs",
            scope: "user",
            configPath: "/home/.codex/config.toml",
            transport: "http",
          },
        ]
      : command === "claude_mcp_list"
        ? "sentry: https://mcp.example.com - ! Needs authentication"
        : undefined,
  );
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it("lists servers and routes sign in through Claude MCP", async () => {
  await act(async () =>
    root.render(createElement(McpSettings, { cwd: "/repo" })),
  );
  expect(container.textContent).toContain("sentry");
  expect(container.textContent).toContain("Needs authentication");
  expect(container.textContent).toContain("Codex");
  const signIn = [...container.querySelectorAll("button")].find(
    (button) => button.textContent === "Sign in",
  )!;
  await act(async () => signIn.click());
  expect(invoke).toHaveBeenCalledWith("mcp_provider_login", {
    cwd: "/repo",
    provider: "claude",
    name: "sentry",
  });
});

it("filters connections by provider", async () => {
  await act(async () =>
    root.render(createElement(McpSettings, { cwd: "/repo" })),
  );
  const codex = [...container.querySelectorAll("button")].find((button) =>
    button.textContent?.startsWith("Codex"),
  )!;
  await act(async () => codex.click());
  expect(container.textContent).toContain("docs");
  expect(container.textContent).not.toContain("sentry");
  const signIn = [...container.querySelectorAll("button")].find(
    (button) => button.textContent === "Sign in",
  )!;
  await act(async () => signIn.click());
  expect(invoke).toHaveBeenCalledWith("mcp_provider_login", {
    cwd: "/repo",
    provider: "codex",
    name: "docs",
  });
});

it("adds a server with its selected scope", async () => {
  await act(async () =>
    root.render(createElement(McpSettings, { cwd: "/repo" })),
  );
  const name = container.querySelector<HTMLInputElement>("input")!;
  const config = container.querySelector<HTMLTextAreaElement>("textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(name, "new-server");
    name.dispatchEvent(new Event("input", { bubbles: true }));
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(config, '{"type":"http","url":"https://example.com/mcp"}');
    config.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () =>
    container
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  expect(invoke).toHaveBeenCalledWith("claude_mcp_add", {
    cwd: "/repo",
    name: "new-server",
    config: '{"type":"http","url":"https://example.com/mcp"}',
    scope: "local",
  });
});
