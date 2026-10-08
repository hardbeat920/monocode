import {
  browserMcpLaunch,
  type BrowserMcpLaunch,
} from "../../../platform/tauri/browser";
import { loadAgentBrowser } from "../../settings/model/displayPrefs";

/** MCP server name agents see; tools appear as browser_* under it. */
export const BROWSER_MCP_NAME = "monocode_browser";

/** Context supplied each turn, including when an older provider thread resumes. */
export const BROWSER_AGENT_CONTEXT = `<monocode_browser>
You are running inside MonoCode. This chat's embedded, built-in, in-app browser is provided by the monocode_browser MCP server. For requests to open a page in your embedded browser, search a website, click links, read pages, take screenshots, or evaluate JavaScript there, use its browser_* tools. Discover those tools if they are deferred. Start with browser_open for a new page or browser_tabs for existing pages; use browser_read, browser_type, browser_click, browser_eval, and browser_screenshot as needed.
MonoCode's browser is separate from Computer Use's iab browser. An unavailable iab or an empty Computer Use browser inventory does not mean MonoCode's browser is unavailable. Try the monocode_browser tools before reporting that it is unavailable. Users do not need to name tools. Honor an explicit request to use another browser. The browser can run while its panel is hidden.
</monocode_browser>`;

/** Session credential variables the server reads; set on every provider child. */
const CREDENTIAL_ENV = ["MONOCODE_APP_ENDPOINT", "MONOCODE_APP_TOKEN"];

/**
 * How this session's provider should start the browser MCP server, or null
 * when agents may not use the browser. Call after the turn is authorized:
 * that is when the session's credential exists.
 */
export async function browserMcpFor(
  sessionId: string,
): Promise<BrowserMcpLaunch | null> {
  // What `isTauri()` reads, without importing it: provider tests mock that
  // module without it.
  const tauri = !!(globalThis as { isTauri?: boolean }).isTauri;
  if (!tauri || !loadAgentBrowser()) return null;
  return browserMcpLaunch(sessionId).catch(() => null);
}

/**
 * Claude Code `--mcp-config`. The provider child already carries the session
 * credential and passes its environment to stdio servers, so it stays out of
 * the arguments.
 */
export function claudeBrowserMcpConfig(launch: BrowserMcpLaunch): string {
  return JSON.stringify({
    mcpServers: {
      [BROWSER_MCP_NAME]: {
        type: "stdio",
        command: launch.command,
        args: launch.args,
      },
    },
  });
}

/**
 * Codex `-c` overrides. Codex gives MCP servers a filtered environment, so
 * name the credential variables to forward instead of writing their values.
 */
export function codexBrowserMcpArgs(launch: BrowserMcpLaunch): string[] {
  const key = `mcp_servers.${BROWSER_MCP_NAME}`;
  // JSON strings and arrays are valid TOML values.
  return [
    "-c",
    `${key}.command=${JSON.stringify(launch.command)}`,
    "-c",
    `${key}.args=${JSON.stringify(launch.args)}`,
    "-c",
    `${key}.env_vars=${JSON.stringify(CREDENTIAL_ENV)}`,
  ];
}

/** ACP `mcpServers` for session/new and session/load. */
export function acpBrowserMcpServers(launch: BrowserMcpLaunch | null): Array<{
  name: string;
  command: string;
  args: string[];
  env: Array<{ name: string; value: string }>;
}> {
  if (!launch) return [];
  return [
    {
      name: BROWSER_MCP_NAME,
      command: launch.command,
      args: launch.args,
      env: Object.entries(launch.env).map(([name, value]) => ({ name, value })),
    },
  ];
}

/** ACP `mcpServers` for a session, empty when agents may not use the browser. */
export async function acpBrowserMcpServersFor(sessionId: string) {
  return acpBrowserMcpServers(await browserMcpFor(sessionId));
}
