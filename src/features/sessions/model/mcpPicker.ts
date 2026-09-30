import type { HarnessId } from "./session";
import {
  MCP_PROVIDER_LABELS,
  type McpConnection,
} from "../../settings/model/mcp";

export type McpPickerServer = McpConnection & {
  availability: "available" | "authentication" | "unavailable";
  detail: string;
};

export type McpTag = { server: McpConnection; token: string };

export function newMcpTag(server: McpConnection, existing: McpTag[]): McpTag {
  const used = new Set(existing.map((tag) => tag.token));
  const base = `@mcp/${server.name}`;
  const candidates = [
    base,
    `@mcp/${server.provider}/${server.name}`,
    `@mcp/${server.provider}/${server.scope}/${server.name}`,
  ];
  let token = candidates.find((candidate) => !used.has(candidate));
  if (!token) {
    let suffix = 2;
    while (used.has(`${candidates[2]}-${suffix}`)) suffix += 1;
    token = `${candidates[2]}-${suffix}`;
  }
  return { server, token };
}

export function mcpTagParts(
  text: string,
  tags: McpTag[],
): { text: string; tag?: McpTag }[] {
  if (!text || tags.length === 0) return text ? [{ text }] : [];
  const parts: { text: string; tag?: McpTag }[] = [];
  let cursor = 0;
  while (cursor < text.length) {
    let hit: { start: number; tag: McpTag } | undefined;
    for (const tag of tags) {
      let start = text.indexOf(tag.token, cursor);
      while (start >= 0) {
        const end = start + tag.token.length;
        const before = start === 0 || /[\s([{]/.test(text[start - 1]);
        const after = end === text.length || /[\s)\]}.!?;,]/.test(text[end]);
        if (before && after) break;
        start = text.indexOf(tag.token, start + 1);
      }
      if (
        start >= 0 &&
        (!hit ||
          start < hit.start ||
          (start === hit.start && tag.token.length > hit.tag.token.length))
      ) {
        hit = { start, tag };
      }
    }
    if (!hit) break;
    if (hit.start > cursor) parts.push({ text: text.slice(cursor, hit.start) });
    parts.push({ text: hit.tag.token, tag: hit.tag });
    cursor = hit.start + hit.tag.token.length;
  }
  if (cursor < text.length) parts.push({ text: text.slice(cursor) });
  return parts;
}

export function taggedMcpServers(
  text: string,
  tags: McpTag[],
): McpConnection[] {
  const present = new Set(
    mcpTagParts(text, tags).flatMap((part) =>
      part.tag ? [part.tag.token] : [],
    ),
  );
  return tags.filter((tag) => present.has(tag.token)).map((tag) => tag.server);
}

export function mcpPickerServers(
  connections: McpConnection[],
  harness: HarnessId,
  claudeStatus: ReadonlyMap<string, string>,
  query: string,
): McpPickerServer[] {
  const search = query.trim().toLowerCase();
  const priority = { available: 0, authentication: 1, unavailable: 2 };
  return connections
    .filter((server) =>
      `${server.name} ${MCP_PROVIDER_LABELS[server.provider]} ${server.scope}`
        .toLowerCase()
        .includes(search),
    )
    .map((server) => {
      const status =
        server.provider === "claude"
          ? claudeStatus.get(server.name)
          : undefined;
      const matches = server.provider === harness;
      const authentication =
        matches &&
        status != null &&
        /auth|sign.?in|log.?in|unauthorized/i.test(status);
      const failed =
        matches &&
        status != null &&
        /failed|error|offline|unreachable|disconnected/i.test(status);
      return {
        ...server,
        availability:
          !matches || failed
            ? ("unavailable" as const)
            : authentication
              ? ("authentication" as const)
              : ("available" as const),
        detail: !matches
          ? "Different provider"
          : failed
            ? "Connection unavailable"
            : "Configured for this provider",
      };
    })
    .sort(
      (a, b) =>
        priority[a.availability] - priority[b.availability] ||
        a.name.localeCompare(b.name) ||
        a.provider.localeCompare(b.provider),
    );
}

export function mcpContextText(servers: McpConnection[], text: string): string {
  if (servers.length === 0) return text;
  const names = servers
    .map((server) => `${JSON.stringify(server.name)} (${server.provider})`)
    .join(", ");
  return `MCP context: Use the configured server${servers.length === 1 ? "" : "s"} ${names} when relevant to this request.\n\n${text}`;
}
