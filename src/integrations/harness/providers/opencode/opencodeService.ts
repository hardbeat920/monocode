import { execChild } from "../../core/child";

export type OpenCodeV2Service = {
  url: string;
  password: string;
};

export function parseOpenCodeServiceUrl(output: string): string | null {
  for (const line of output.split("\n")) {
    const match = line
      .trim()
      .match(/^(https?:\/\/(?:127\.0\.0\.1|localhost):\d+)\/?$/i);
    if (match?.[1]) return match[1];
  }
  return null;
}

export async function resolveOpenCodeV2ServiceUrl(
  path: string,
  cwd: string,
): Promise<string> {
  const status = await execChild(path, ["service", "status"], cwd).catch(
    () => "",
  );
  const existing = parseOpenCodeServiceUrl(status);
  if (existing) return existing;

  const started = await execChild(path, ["service", "start"], cwd).catch(
    (error: unknown) => {
      throw new Error(
        `Could not start the OpenCode background service: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    },
  );
  const fromStart = parseOpenCodeServiceUrl(started);
  if (fromStart) return fromStart;

  const refreshed = await execChild(path, ["service", "status"], cwd);
  const url = parseOpenCodeServiceUrl(refreshed);
  if (url) return url;
  throw new Error("OpenCode background service did not report a local URL.");
}

export async function resolveOpenCodeV2Service(
  path: string,
  cwd: string,
): Promise<OpenCodeV2Service> {
  const url = await resolveOpenCodeV2ServiceUrl(path, cwd);
  const password = (
    await execChild(path, ["service", "get", "password"], cwd)
  ).trim();
  if (!password) {
    throw new Error("OpenCode background service did not report a password.");
  }
  return { url, password };
}
