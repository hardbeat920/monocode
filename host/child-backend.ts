import {
  execFile,
  spawn,
  type ChildProcessWithoutNullStreams,
} from "node:child_process";
import { EventEmitter } from "node:events";
import { promisify } from "node:util";
import { join } from "node:path";
import type { ChildBackend } from "../src/integrations/harness/core/child";
import { providerLaunch, resolveProvider } from "./process";

const exec = promisify(execFile);

/** Native process implementation for the headless execution proof. */
export class HostChildBackend implements ChildBackend {
  private events = new EventEmitter();
  private children = new Map<string, ChildProcessWithoutNullStreams>();
  private closing = false;

  constructor(
    private readonly binaries: Partial<Record<"codex" | "claude", string>> = {},
  ) {
    this.events.setMaxListeners(0);
  }

  async resolve(provider: "codex" | "claude"): Promise<string> {
    if (this.binaries[provider]) return this.binaries[provider]!;
    return resolveProvider(provider);
  }

  async listen<T>(
    event: string,
    handler: (event: { payload: T }) => void,
  ): Promise<() => void> {
    this.events.on(event, handler);
    return () => {
      this.events.off(event, handler);
    };
  }

  private emit(event: string, payload: unknown): void {
    this.events.emit(event, { payload });
  }

  async invoke<T>(
    command: string,
    args: Record<string, unknown> = {},
  ): Promise<T> {
    const id = String(args.sessionId ?? "");
    switch (command) {
      case "harness_resolve_codex":
        return { path: await this.resolve("codex") } as T;
      case "harness_resolve_claude":
        return { path: await this.resolve("claude") } as T;
      case "harness_spawn":
        return (await this.start(id, args)) as T;
      case "harness_write": {
        const child = this.children.get(id);
        if (!child || child.stdin.destroyed)
          throw new Error("Provider process is not running");
        await new Promise<void>((resolve, reject) => {
          const timer = setTimeout(
            () => reject(new Error("Provider stdin write timed out")),
            15_000,
          );
          child.stdin.write(`${String(args.line)}\n`, (error) => {
            clearTimeout(timer);
            if (error) reject(error);
            else resolve();
          });
        });
        return undefined as T;
      }
      case "harness_kill":
        await this.kill(id);
        return undefined as T;
      case "harness_kill_all":
        await this.close();
        return undefined as T;
      default:
        throw new Error(`Unsupported headless process operation: ${command}`);
    }
  }

  private async start(
    id: string,
    args: Record<string, unknown>,
  ): Promise<number> {
    if (this.closing) throw new Error("Host is stopping");
    await this.kill(id);
    if (this.closing) throw new Error("Host is stopping");
    const account = args.account as { id?: string } | undefined;
    if (account?.id && account.id !== "default")
      throw new Error(
        "Named provider accounts are not supported by this host yet",
      );
    const launch = await providerLaunch(
      String(args.command),
      args.args as string[],
    );
    if (this.closing) throw new Error("Host is stopping");
    const child = spawn(launch.command, launch.args, {
      cwd: String(args.cwd),
      stdio: "pipe",
      detached: process.platform !== "win32",
      windowsHide: true,
      env: { ...process.env, MONOCODE_HOST: "1" },
    });
    this.children.set(id, child);
    child.stdin.on("error", () => {
      /* write callbacks report failures */
    });
    this.lines(child, id, "stdout");
    this.lines(child, id, "stderr");
    child.on("close", (code) => {
      if (this.children.get(id) === child) this.children.delete(id);
      this.emit("harness-exit", { sessionId: id, code, pid: child.pid });
    });
    await new Promise<void>((resolve, reject) => {
      child.once("spawn", resolve);
      child.once("error", reject);
    });
    return child.pid!;
  }

  private lines(
    child: ChildProcessWithoutNullStreams,
    id: string,
    stream: "stdout" | "stderr",
  ): void {
    let buffer = "";
    child[stream].setEncoding("utf8");
    child[stream].on("data", (data: string) => {
      buffer += data;
      let index: number;
      while ((index = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, index).replace(/\r$/, "");
        buffer = buffer.slice(index + 1);
        if (line.length > 8 * 1024 * 1024) {
          void this.kill(id);
          return;
        }
        this.emit(`harness-${stream}`, { sessionId: id, line });
      }
      if (buffer.length > 8 * 1024 * 1024) {
        buffer = "";
        void this.kill(id);
      }
    });
    child[stream].on("end", () => {
      if (buffer)
        this.emit(`harness-${stream}`, { sessionId: id, line: buffer });
    });
  }

  private signal(
    child: ChildProcessWithoutNullStreams,
    signal: NodeJS.Signals,
  ): void {
    try {
      if (process.platform === "win32") child.kill(signal);
      else if (child.pid) process.kill(-child.pid, signal);
    } catch {
      /* already exited */
    }
  }

  async kill(id: string): Promise<void> {
    const child = this.children.get(id);
    if (!child) return;
    this.children.delete(id);
    if (process.platform === "win32") {
      // Kill descendants while the leader still exists. Closing stdin first
      // can let the leader exit, making its remaining children untraceable.
      if (child.pid && child.exitCode === null && child.signalCode === null) {
        try {
          await exec(
            join(
              process.env.SystemRoot ?? "C:\\Windows",
              "System32",
              "taskkill.exe",
            ),
            ["/PID", String(child.pid), "/T", "/F"],
            { windowsHide: true, timeout: 10_000 },
          );
        } catch {
          child.kill();
        }
      }
      child.stdin.destroy();
      if (child.exitCode !== null || child.signalCode !== null) return;
      await new Promise<void>((resolve) => {
        const timer = setTimeout(resolve, 1_000);
        child.once("close", () => {
          clearTimeout(timer);
          resolve();
        });
      });
      return;
    }
    child.stdin.destroy();
    this.signal(child, "SIGTERM");
    await new Promise<void>((resolve) => {
      const timer = setTimeout(() => {
        this.signal(child, "SIGKILL");
        resolve();
      }, 1_000);
      child.once("close", () => {
        clearTimeout(timer);
        resolve();
      });
    });
  }

  async close(): Promise<void> {
    this.closing = true;
    await Promise.all([...this.children.keys()].map((id) => this.kill(id)));
  }
}
