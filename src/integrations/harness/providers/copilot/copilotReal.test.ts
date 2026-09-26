import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { createInterface } from "node:readline";
import { describe, expect, it, vi } from "vitest";
import type { HarnessEvent } from "../../core/types";
import { modelsFor } from "../../../../features/sessions/model/models";
import { refreshCopilotCatalog } from "./copilotCatalog";

const BIN = process.env.COPILOT_BIN ?? process.env.PATH?.split(delimiter)
  .map((directory) => join(directory, process.platform === "win32" ? "copilot.exe" : "copilot"))
  .find(existsSync);
const REAL = process.env.COPILOT_REAL === "1" && !!BIN && existsSync(BIN);

const live = vi.hoisted(() => ({
  children: new Map<string, ChildProcess>(),
  listeners: new Map<string, (line: string) => void>(),
  exits: new Map<string, (code: number | null) => void>(),
  errors: new Map<string, (line: string) => void>(),
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => homedir(),
}));

vi.mock("../../core/child", () => ({
  resolveCopilotBinary: async () => ({ path: BIN }),
  spawnChild: async (id: string, command: string, args: string[], cwd: string) => {
    const child = spawn(command, args, { cwd });
    live.children.set(id, child);
    createInterface({ input: child.stdout! }).on("line", (line) => live.listeners.get(id)?.(line));
    createInterface({ input: child.stderr! }).on("line", (line) => live.errors.get(id)?.(line));
    child.on("exit", (code) => live.exits.get(id)?.(code));
  },
  killChild: async (id: string) => {
    live.children.get(id)?.kill("SIGKILL");
    live.children.delete(id);
  },
  writeChild: async (id: string, line: string) => {
    const child = live.children.get(id);
    if (!child) throw new Error(`no child for ${id}`);
    await new Promise<void>((resolve, reject) => child.stdin!.write(`${line}\n`, (error) => error ? reject(error) : resolve()));
  },
  watchChild: (id: string, onLine: (line: string) => void, onExit: (code: number | null) => void, onStderr: (line: string) => void) => {
    live.listeners.set(id, onLine);
    live.exits.set(id, onExit);
    live.errors.set(id, onStderr);
  },
  unwatchChild: (id: string) => {
    live.listeners.delete(id);
    live.exits.delete(id);
    live.errors.delete(id);
  },
}));

const copilot = await import("./copilot");

describe.skipIf(!REAL)("Copilot real ACP endpoint", () => {
  it("writes a requested file through a real turn", async () => {
    const cwd = mkdtempSync(join(tmpdir(), "monocode-copilot-"));
    const events: HarnessEvent[] = [];
    try {
      await copilot.sendCopilotTurn({
        sessionId: "copilot-real",
        cwd,
        model: "copilot:auto",
        runtimeMode: "full-access",
        text: "Create a file named hello.txt in the current directory containing exactly COPILOT_REAL_OK followed by a newline. Do not create other files.",
        attachments: [],
        onEvent: (event) => events.push(event),
      });
      expect(readFileSync(join(cwd, "hello.txt"), "utf8")).toBe("COPILOT_REAL_OK\n");
      expect(events).toContainEqual({ type: "message.completed" });
    } finally {
      await copilot.forgetCopilotSession("copilot-real");
      rmSync(cwd, { recursive: true, force: true });
    }
  }, 240_000);

  it("discovers the model catalog from the real CLI", async () => {
    await refreshCopilotCatalog();
    const models = modelsFor("copilot");
    expect(models.length).toBeGreaterThan(1);
    expect(models.some((model) => model.nativeId === "auto")).toBe(true);
  }, 120_000);
});
