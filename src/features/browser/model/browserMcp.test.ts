import { describe, expect, it } from "vitest";
import {
  acpBrowserMcpServers,
  claudeBrowserMcpConfig,
  codexBrowserMcpArgs,
} from "./browserMcp";
import { buildClaudeSpawnArgs } from "../../../integrations/harness/providers/claude/claudeProtocol";

const launch = {
  command: "/Applications/MonoCode.app/Contents/MacOS/monocode",
  args: ["browser-mcp"],
  env: {
    MONOCODE_APP_ENDPOINT: "127.0.0.1:5000",
    MONOCODE_APP_TOKEN: "secret",
  },
};

describe("browser MCP launch", () => {
  it("gives Claude a stdio server without the credential", () => {
    const config = claudeBrowserMcpConfig(launch);
    expect(JSON.parse(config)).toEqual({
      mcpServers: {
        monocode_browser: {
          type: "stdio",
          command: launch.command,
          args: ["browser-mcp"],
        },
      },
    });
    expect(config).not.toContain("secret");
  });

  it("adds Claude's config only to interactive sessions", () => {
    const config = claudeBrowserMcpConfig(launch);
    const args = buildClaudeSpawnArgs({ mcpConfig: config });
    expect(args).toContain("--mcp-config");
    expect(args).not.toContain("--strict-mcp-config");
    const isolated = buildClaudeSpawnArgs({
      isolated: true,
      mcpConfig: config,
    });
    expect(isolated.filter((arg) => arg === config)).toEqual([]);
  });

  it("names the credential variables Codex should forward", () => {
    const args = codexBrowserMcpArgs({
      ...launch,
      command: "C:\\Program Files\\MonoCode\\monocode.exe",
    });
    expect(args).toEqual([
      "-c",
      'mcp_servers.monocode_browser.command="C:\\\\Program Files\\\\MonoCode\\\\monocode.exe"',
      "-c",
      'mcp_servers.monocode_browser.args=["browser-mcp"]',
      "-c",
      'mcp_servers.monocode_browser.env_vars=["MONOCODE_APP_ENDPOINT","MONOCODE_APP_TOKEN"]',
      "-c",
      "mcp_servers.monocode_browser.required=true",
    ]);
    expect(args.join(" ")).not.toContain("secret");
  });

  it("passes ACP agents the credential over stdio", () => {
    expect(acpBrowserMcpServers(null)).toEqual([]);
    expect(acpBrowserMcpServers(launch)).toEqual([
      {
        name: "monocode_browser",
        command: launch.command,
        args: ["browser-mcp"],
        env: [
          { name: "MONOCODE_APP_ENDPOINT", value: "127.0.0.1:5000" },
          { name: "MONOCODE_APP_TOKEN", value: "secret" },
        ],
      },
    ]);
  });
});
