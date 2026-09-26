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
  it("steers a real active turn and waits for its replacement response", async () => {
    const cwd = mkdtempSync(join(tmpdir(), "monocode-copilot-steer-"));
    const sessionId = "copilot-real-steer";
    const events: HarnessEvent[] = [];
    let steering: Promise<void> | undefined;
    let steeringError: unknown;
    try {
      await copilot.sendCopilotTurn({
        sessionId,
        cwd,
        model: "copilot:auto",
        runtimeMode: "supervised",
        text: "Use the shell tool to run exactly sleep 8. Do not read or write any files. After the command finishes, reply exactly ORIGINAL_ONLY.",
        onEvent: (event) => {
          events.push(event);
          if (!steering && (event.type === "approval.requested" || event.type === "message.delta")) {
            steering = copilot.steerCopilotTurn({
              sessionId,
              cwd,
              model: "copilot:auto",
              text: "Change your final reply to exactly STEERING_ACCEPTED. Do not run any additional tools.",
            }).catch((error: unknown) => { steeringError = error; });
          }
          if (event.type === "approval.requested") {
            queueMicrotask(() => copilot.respondCopilotApproval(sessionId, event.requestId, "deny"));
          }
        },
      });
      expect(steering).toBeDefined();
      expect(events.flatMap((event) => event.type === "message.delta" ? [event.text] : []).join("")).toContain("STEERING_ACCEPTED");
      expect(events.filter((event) => event.type === "message.completed")).toHaveLength(1);
      expect(events.filter((event) => event.type === "reasoning.completed")).toHaveLength(1);
      expect(events.some((event) => event.type === "session.error")).toBe(false);
      await steering;
      expect(steeringError).toBeUndefined();
    } finally {
      await copilot.forgetCopilotSession(sessionId);
      rmSync(cwd, { recursive: true, force: true });
    }
  }, 120_000);

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
