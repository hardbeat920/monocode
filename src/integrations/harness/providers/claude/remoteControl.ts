/**
 * Handing one Claude conversation between MonoCode's headless child and an
 * interactive CLI hosted in a pty.
 *
 * Only one process may own a conversation: the CLI refuses a second one on the
 * same session, and two processes appending to one transcript would interleave
 * their records. So this is a hand-over rather than a second connection, and
 * the ordering is the whole of it — stop the child, measure the transcript,
 * then spawn. Every impure step is a port so that ordering can be asserted
 * without a CLI or a pty.
 */

import type { PtyCommand } from "../../../../platform/tauri/pty";

/**
 * Screen the pty is opened at. Fixed rather than inherited from a UI element,
 * because the screen parser that reads approvals off this pty is written
 * against a known width.
 */
export const REMOTE_CONTROL_COLS = 120;
export const REMOTE_CONTROL_ROWS = 40;

export type HandoverTarget = {
  /** MonoCode's thread id. Keys the headless child and the pty alike. */
  sessionId: string;
  /**
   * The id `--resume` is given. Absent until a first turn binds one, and a
   * thread without one has no conversation to hand over.
   */
  providerSessionId?: string;
  /**
   * Directory the next turn will run in: `sessionWorkCwd(session)`, the
   * worktree checkout rather than the project root. Claude resumes a
   * conversation only when the bound directory matches the one it runs in.
   */
  cwd: string;
  /** `--remote-control <name>` — the project name, so a phone list reads well. */
  name: string;
  /**
   * Overrides the program. A bare name resolves through the same PATH the app
   * uses everywhere else; a path with more than one component is taken as it
   * is, which is how a configured binary path reaches the pty.
   */
  program?: string;
  cols?: number;
  rows?: number;
};

/** The impure edges, injected so the ordering is testable. */
export type HandoverPorts = {
  /**
   * Stops the headless child. Must be `stopClaudeSession`, which keeps the
   * resume binding; `forgetClaudeSession` deletes it and would lose the
   * conversation this hand-over exists to preserve.
   */
  stopSession: (sessionId: string) => Promise<void>;
  /**
   * Bytes in the conversation's transcript. Called once the child is stopped,
   * so the offset is the exact boundary between what MonoCode has already
   * rendered and what the pty-hosted process will append.
   */
  transcriptEnd: (target: HandoverTarget) => Promise<number>;
  spawnPty: (
    id: string,
    cwd: string,
    cols: number,
    rows: number,
    command: PtyCommand,
  ) => Promise<void>;
  killPty: (id: string) => Promise<void>;
};

export type RemoteControlHandle = {
  ptyId: string;
  /**
   * Where the mirror starts reading. Everything before it is already on screen,
   * and `--resume` appends rather than forking, so this offset stays valid for
   * the life of the pty.
   */
  offset: number;
};

/**
 * Derived from the thread rather than stored, so a close works from a fresh
 * process: after a reload nothing holds the handle, but the pty is still
 * addressable — which is what lets the app quitting and a thread being archived
 * kill it.
 */
export function remoteControlPtyId(sessionId: string): string {
  return `remote-control:${sessionId}`;
}

export function remoteControlArgs(
  providerSessionId: string,
  name: string,
): string[] {
  return ["--resume", providerSessionId, "--remote-control", name];
}

/**
 * Stops the headless child and hands its conversation to a pty-hosted CLI.
 *
 * On any failure the thread is left with no process at all, which is the state
 * every thread sits in between turns: the resume binding is untouched, so the
 * next turn starts a headless child on the same conversation.
 */
export async function openRemoteControl(
  target: HandoverTarget,
  ports: HandoverPorts,
): Promise<RemoteControlHandle> {
  // Refused before anything is torn down. A thread with no bound id would
  // otherwise have a working child stopped in order to run a `--resume` with
  // nothing to resume, which loses the session to gain nothing.
  const providerSessionId = target.providerSessionId?.trim();
  if (!providerSessionId) {
    throw new Error(
      "This conversation has not started yet, so there is nothing to hand over",
    );
  }

  const ptyId = remoteControlPtyId(target.sessionId);

  // First, and not as a courtesy: a live child is returned by ensureLive before
  // the resume state is read, so it would go on owning the conversation and
  // appending to the transcript the pty is about to take over.
  await ports.stopSession(target.sessionId);

  // Measured while neither process is running, so no record can land between
  // the measurement and the spawn and be missed by both sides.
  const offset = await ports.transcriptEnd(target);

  try {
    await ports.spawnPty(
      ptyId,
      target.cwd,
      target.cols ?? REMOTE_CONTROL_COLS,
      target.rows ?? REMOTE_CONTROL_ROWS,
      {
        program: target.program ?? "claude",
        args: remoteControlArgs(providerSessionId, target.name),
      },
    );
  } catch (error) {
    // A pty that got far enough to hold the conversation would lock out the
    // headless child that takes over on the next turn, so it is reaped before
    // the failure is reported.
    await ports.killPty(ptyId).catch(() => undefined);
    throw error;
  }

  return { ptyId, offset };
}

/**
 * Takes the conversation back. Nothing is restarted here: the binding is intact,
 * so the next turn spawns a headless child on the same conversation.
 */
export async function closeRemoteControl(
  sessionId: string,
  ports: Pick<HandoverPorts, "killPty">,
): Promise<void> {
  await ports.killPty(remoteControlPtyId(sessionId));
}
