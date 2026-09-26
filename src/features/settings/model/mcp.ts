export type McpServer = { name: string; status: string };

/** Claude's list output is for humans; keep only names and health text. */
export function parseClaudeMcpList(output: string): McpServer[] {
  return output.split(/\r?\n/).flatMap((line) => {
    const match = line.match(/^([A-Za-z0-9_-]+):\s+(.+)$/);
    if (!match) return [];
    const detail = match[2];
    const status = detail.match(/(?: - | — )(.+)$/)?.[1] ?? detail;
    return [{ name: match[1], status }];
  });
}
