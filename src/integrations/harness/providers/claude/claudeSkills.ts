import { homeDir } from "../../../../platform/tauri/fs";
import {
  killChild,
  resolveClaudeBinary,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "../../core/child";
import type { NativeCommand } from "../../core/nativeCommands";
import {
  buildClaudeSpawnArgs,
  buildControlRequest,
  KNOWN_TERMINAL_ONLY_COMMANDS,
  nativeCommandsFromControlResponse,
  parseJsonLine,
} from "./claudeProtocol";

const PROBE_ID_PREFIX = "monocode-claude-commands-probe";
const INIT_REQUEST_ID = "monocode_commands_init";
const DISCOVERY_TIMEOUT_MS = 15_000;

/**
 * Cold-start command discovery: no live session exists yet (e.g. the
 * composer's `/` picker opened before the first turn), so a disposable
 * `claude` process is asked the same `initialize` handshake a live session
 * sends, and exits once it answers. No turn runs, so this costs nothing.
 */
export async function discoverClaudeCommands(cwd: string): Promise<NativeCommand[]> {
  const { path } = await resolveClaudeBinary();
  const probeCwd = cwd.trim() || (await homeDir());
  // Unique per call: watchChild keys handlers by id, so two probes racing on
  // a shared id would replace each other's handler and cross-deliver (or
  // lose) their responses.
  const PROBE_ID = `${PROBE_ID_PREFIX}-${crypto.randomUUID()}`;

  let resolveCommands: ((commands: NativeCommand[]) => void) | null = null;
  let rejectCommands: ((error: Error) => void) | null = null;
  const pending = new Promise<NativeCommand[]>((resolve, reject) => {
    resolveCommands = resolve;
    rejectCommands = reject;
  });

  const stop = async () => {
    unwatchChild(PROBE_ID);
    await killChild(PROBE_ID).catch(() => undefined);
  };

  watchChild(
    PROBE_ID,
    (line) => {
      const rec = parseJsonLine(line);
      if (!rec) return;
      const commands = nativeCommandsFromControlResponse(rec, INIT_REQUEST_ID);
      // No turn runs in this probe, so `system/init` never arrives and the
      // CLI's own terminal-only list is never reported — fall back to the
      // known-stable set instead of publishing TUI-only commands as usable.
      if (commands) {
        resolveCommands?.(
          commands.filter(
            (command) => !KNOWN_TERMINAL_ONLY_COMMANDS.has(command.name),
          ),
        );
      }
    },
    () => rejectCommands?.(new Error("Claude Code command probe exited")),
  );

  try {
    await spawnChild(
      PROBE_ID,
      path,
      buildClaudeSpawnArgs({ isolated: true, sessionId: crypto.randomUUID() }),
      probeCwd,
      undefined,
      "claude",
    );
    await writeChild(
      PROBE_ID,
      JSON.stringify(buildControlRequest(INIT_REQUEST_ID, { subtype: "initialize" })),
    );
    return await withTimeout(DISCOVERY_TIMEOUT_MS, pending, () => void stop());
  } finally {
    await stop();
  }
}

function withTimeout<T>(
  ms: number,
  promise: Promise<T>,
  onTimeout: () => void,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      onTimeout();
      reject(new Error("Claude Code command probe timed out"));
    }, ms);
    promise.then(resolve, reject).finally(() => clearTimeout(timer));
  });
}
