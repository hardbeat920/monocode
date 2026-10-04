import { expect, it } from "vitest";
import {
  mcpContextText,
  mcpPickerServers,
  mcpTagParts,
  newMcpTag,
  taggedMcpServers,
} from "./mcpPicker";
import type { McpConnection } from "../../settings/model/mcp";

const servers: McpConnection[] = [
  {
    provider: "cursor",
    name: "other",
    scope: "user",
    configPath: "/cursor",
    transport: "stdio",
  },
  {
    provider: "claude",
    name: "needs-login",
    scope: "user",
    configPath: "/claude",
    transport: "http",
  },
  {
    provider: "claude",
    name: "docs",
    scope: "project",
    configPath: "/repo/.mcp.json",
    transport: "stdio",
  },
];

it("prioritizes usable servers while retaining authentication and other providers", () => {
  const ranked = mcpPickerServers(
    servers,
    "claude",
    new Map([["needs-login", "Needs authentication"]]),
    "",
  );
  expect(ranked.map((server) => [server.name, server.availability])).toEqual([
    ["docs", "available"],
    ["needs-login", "authentication"],
    ["other", "unavailable"],
  ]);
  expect(
    mcpPickerServers(servers, "claude", new Map(), "cursor").map(
      (server) => server.name,
    ),
  ).toEqual(["other"]);
});

it("adds only selected server names to outgoing context", () => {
  expect(mcpContextText([servers[2]], "Find the docs")).toContain(
    '"docs" (claude)',
  );
  expect(mcpContextText([], "Find the docs")).toBe("Find the docs");
});

it("makes disabled provider entries unselectable even when health says Connected", () => {
  for (const provider of ["claude", "codex", "opencode"] as const) {
    const [server] = mcpPickerServers(
      [{ ...servers[2], provider, enabled: false }],
      provider,
      new Map([["docs", "Connected"]]),
      "",
    );
    expect(server.availability).toBe("unavailable");
    expect(server.detail).toBe("Disabled in provider configuration");
  }
});

it("keeps MCP references inline and only uses tags still in the draft", () => {
  const docs = newMcpTag(servers[2], []);
  const anotherDocs = newMcpTag({ ...servers[2], provider: "cursor" }, [docs]);
  expect(docs.token).toBe("@mcp/docs");
  expect(anotherDocs.token).toBe("@mcp/cursor/docs");
  const text = `Ask ${docs.token} about this, then ${anotherDocs.token}.`;
  expect(
    mcpTagParts(text, [docs, anotherDocs]).filter((part) => part.tag),
  ).toHaveLength(2);
  expect(taggedMcpServers(text, [docs, anotherDocs])).toHaveLength(2);
  expect(taggedMcpServers(`Ask ${docs.token}2 about this`, [docs])).toEqual([]);
  expect(taggedMcpServers("Ask about this", [docs])).toEqual([]);
});
