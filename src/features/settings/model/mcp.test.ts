import { describe, expect, it } from "vitest";
import { parseClaudeMcpList } from "./mcp";

describe("Claude MCP list", () => {
  it("keeps server names and health while omitting launch details", () => {
    expect(
      parseClaudeMcpList(
        "Checking MCP server health...\nnotion: https://example.com/mcp - ! Needs authentication\nlocal_tools: npx tools - --token secret - ✔ Connected\n",
      ),
    ).toEqual([
      { name: "notion", status: "! Needs authentication" },
      { name: "local_tools", status: "✔ Connected" },
    ]);
  });
});
