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

it("adds a standard mcpServers entry to the selected provider", async () => {
  await act(async () =>
    root.render(createElement(McpSettings, { cwd: "/repo" })),
  );
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Add MCP server"]')!
      .click(),
  );
  expect(document.body.textContent).toContain("Add MCP server");
  const provider = document.body.querySelector<HTMLButtonElement>(
    '[aria-label="Provider: Claude Code"]',
  )!;
  await act(async () => provider.click());
  const cursor = [
    ...document.body.querySelectorAll<HTMLButtonElement>('[role="option"]'),
  ].find((button) => button.textContent === "Cursor")!;
  await act(async () => cursor.click());
  const config = document.body.querySelector<HTMLTextAreaElement>("textarea")!;
  const json =
    '{"mcpServers":{"new-server":{"command":"npx","args":["example"]}}}';
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(config, json);
    config.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () =>
    document.body
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  expect(invoke).toHaveBeenCalledWith("mcp_add", {
    cwd: "/repo",
    provider: "cursor",
    name: "",
    config: json,
    scope: "project",
  });
});

it("shows only configured provider chips until the filter button reveals all", async () => {
  await act(async () =>
    root.render(createElement(McpSettings, { cwd: "/repo" })),
  );
  const chips = container.querySelector(
    '[aria-label="Filter MCP servers by provider"]',
  )!;
  expect(chips.textContent).toContain("Codex");
  expect(chips.textContent).not.toContain("Cursor");
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Show all providers"]')!
      .click(),
  );
  expect(chips.textContent).toContain("Cursor");
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>(
        '[aria-label="Show available providers"]',
      )!
      .click(),
  );
  expect(chips.textContent).not.toContain("Cursor");
});

it("ignores discovery from a previous project after cwd changes", async () => {
  let resolveOld!: (connections: unknown[]) => void;
  invoke.mockImplementation((command: string, args: { cwd: string }) => {
    if (command === "claude_mcp_list") return Promise.resolve("");
    if (args.cwd === "/old")
      return new Promise((resolve) => {
        resolveOld = resolve;
      });
    return Promise.resolve([
      {
        provider: "cursor",
        name: "new-project",
        scope: "project",
        configPath: "/new/.cursor/mcp.json",
        transport: "stdio",
      },
    ]);
  });
  await act(async () =>
    root.render(createElement(McpSettings, { cwd: "/old" })),
  );
  await act(async () =>
    root.render(createElement(McpSettings, { cwd: "/new" })),
  );
  expect(container.textContent).toContain("new-project");
  await act(async () =>
    resolveOld([
      {
        provider: "cursor",
        name: "old-project",
        scope: "project",
        configPath: "/old/.cursor/mcp.json",
        transport: "stdio",
      },
    ]),
  );
  expect(container.textContent).toContain("new-project");
  expect(container.textContent).not.toContain("old-project");
});
