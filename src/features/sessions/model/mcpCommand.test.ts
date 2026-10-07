import { describe, expect, it } from "vitest";
import { isMcpCommand } from "./mcpCommand";

describe("/mcp", () => {
  it("matches only a standalone command", () => {
    expect(isMcpCommand(" /MCP ")).toBe(true);
    expect(isMcpCommand("/mcp explain this server")).toBe(false);
    expect(isMcpCommand("please check /mcp")).toBe(false);
  });
});
