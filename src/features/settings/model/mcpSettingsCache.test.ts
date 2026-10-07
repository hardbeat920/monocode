import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { McpConnection } from "./mcp";
import {
  clearMcpSettingsCache,
  getCachedMcpSettings,
  loadMcpSettings,
  subscribeMcpSettings,
} from "./mcpSettingsCache";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const configured: McpConnection[] = [
  {
    provider: "claude",
    name: "docs",
    scope: "project",
    configPath: "/repo/.mcp.json",
    transport: "stdio",
  },
];

beforeEach(() => {
  clearMcpSettingsCache();
  invoke.mockReset();
});
afterEach(clearMcpSettingsCache);

it("publishes discovery before slow health and shares both requests across consumers", async () => {
  let resolveHealth!: (output: string) => void;
  invoke.mockImplementation((command: string) =>
    command === "mcp_discover"
      ? Promise.resolve(configured)
      : new Promise((resolve) => {
          resolveHealth = resolve;
        }),
  );
  const onChange = vi.fn();
  const stop = subscribeMcpSettings("/repo", onChange);
  try {
    const [picker, settings] = await Promise.all([
      loadMcpSettings("/repo", false, { claudeHealth: false }),
      loadMcpSettings("/repo"),
    ]);
    expect(picker.servers[0].status).toBe("Configured");
    expect(settings.servers[0].name).toBe("docs");
    expect(invoke).toHaveBeenCalledTimes(2);
    await loadMcpSettings("/repo");
    expect(invoke).toHaveBeenCalledTimes(2);
    resolveHealth(
      "docs: local - Connected\nremote: https://example.com - Needs authentication",
    );
    await vi.waitFor(() =>
      expect(getCachedMcpSettings("/repo")?.servers).toHaveLength(2),
    );
    expect(onChange.mock.calls.at(-1)?.[0].servers[0].status).toBe("Connected");
  } finally {
    stop();
  }
});

it("loads non-Claude pickers without running Claude and lets a later Claude consumer request health", async () => {
  invoke.mockImplementation(async (command: string) =>
    command === "mcp_discover" ? configured : "docs: local - Connected",
  );
  await loadMcpSettings("/repo", false, { claudeHealth: false });
  await loadMcpSettings("/repo", false, { claudeHealth: false });
  expect(invoke).toHaveBeenCalledTimes(1);
  await loadMcpSettings("/repo");
  await vi.waitFor(() =>
    expect(getCachedMcpSettings("/repo")?.servers[0].status).toBe("Connected"),
  );
  expect(invoke).toHaveBeenCalledTimes(2);
});

it("ignores health from a request superseded by refresh", async () => {
  const health: ((output: string) => void)[] = [];
  invoke.mockImplementation((command: string) =>
    command === "mcp_discover"
      ? Promise.resolve(configured)
      : new Promise((resolve) => {
          health.push(resolve);
        }),
  );
  await loadMcpSettings("/repo");
  await loadMcpSettings("/repo", true);
  health[1]("docs: local - Connected");
  await vi.waitFor(() =>
    expect(getCachedMcpSettings("/repo")?.servers[0].status).toBe("Connected"),
  );
  health[0]("docs: local - Failed\nstale: local - Connected");
  await Promise.resolve();
  expect(
    getCachedMcpSettings("/repo")?.servers.map((server) => server.name),
  ).toEqual(["docs"]);
  expect(getCachedMcpSettings("/repo")?.servers[0].status).toBe("Connected");
});

it("keeps configured rows when health fails and preserves disabled status", async () => {
  invoke.mockImplementation((command: string) =>
    command === "mcp_discover"
      ? Promise.resolve([{ ...configured[0], enabled: false }])
      : Promise.reject(new Error("Health unavailable")),
  );
  await loadMcpSettings("/repo");
  await vi.waitFor(() =>
    expect(getCachedMcpSettings("/repo")?.claudeError).toContain(
      "Health unavailable",
    ),
  );
  expect(getCachedMcpSettings("/repo")?.servers[0].status).toBe("Disabled");
  expect(getCachedMcpSettings("/repo")?.error).toBe("");
});
