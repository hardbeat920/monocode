import { existsSync, readFileSync } from "node:fs";
import { request } from "node:http";
import { join } from "node:path";

/** Written by `serve` to `<data dir>/running.json`, readable only by the
 * owner. `secret` authorizes local lifecycle requests. */
export type RunningHost = { pid: number; port: number; secret: string };

export type HostStatus = {
  /** Absent for hosts older than 0.5, which report nothing. */
  version?: string;
  pid?: number;
  port?: number;
  runningTurns?: number;
  providers?: string[];
  network?: { enabled: boolean; bind: string; error?: string };
};

export function readRunning(directory: string): RunningHost | undefined {
  const path = join(directory, "running.json");
  if (!existsSync(path)) return;
  try {
    return JSON.parse(readFileSync(path, "utf8")) as RunningHost;
  } catch {
    return;
  }
}

/** Asks the running host to report its status, stop, or reload its network
 * settings. Throws when no host answers on the recorded port. Each request
 * opens its own connection: a pooled one may belong to a host that stopped. */
export function lifecycle(
  state: RunningHost,
  action: "status" | "stop" | "network",
): Promise<HostStatus> {
  return new Promise((resolve, reject) => {
    const call = request(
      {
        host: "127.0.0.1",
        port: state.port,
        path: "/lifecycle",
        method: "POST",
        agent: false,
        timeout: 5_000,
        headers: {
          Authorization: `Bearer ${state.secret}`,
          "Content-Type": "application/json",
        },
      },
      (response) => {
        let text = "";
        response.setEncoding("utf8");
        response.on("data", (chunk) => (text += chunk));
        response.on("end", () => {
          if (response.statusCode !== 200) {
            reject(new Error("Could not verify the running host"));
            return;
          }
          try {
            resolve(JSON.parse(text) as HostStatus);
          } catch {
            resolve({});
          }
        });
      },
    );
    call.on("timeout", () => call.destroy(new Error("The host did not answer")));
    call.on("error", reject);
    call.end(JSON.stringify({ action }));
  });
}

/** The running host's status, or undefined when none answers. */
export async function runningStatus(
  directory: string,
): Promise<(HostStatus & { state: RunningHost }) | undefined> {
  const state = readRunning(directory);
  if (!state) return;
  try {
    return { ...(await lifecycle(state, "status")), state };
  } catch {
    return;
  }
}

/** Compares `a.b.c` versions; prerelease suffixes sort before the release. */
export function compareVersions(a: string, b: string): number {
  const parse = (value: string) => {
    const [core, pre = ""] = value.split("-", 2);
    return { parts: core.split(".").map((part) => Number(part) || 0), pre };
  };
  const left = parse(a);
  const right = parse(b);
  for (let i = 0; i < 3; i++) {
    const delta = (left.parts[i] ?? 0) - (right.parts[i] ?? 0);
    if (delta) return Math.sign(delta);
  }
  if (left.pre === right.pre) return 0;
  if (!left.pre) return 1;
  if (!right.pre) return -1;
  return left.pre < right.pre ? -1 : 1;
}
