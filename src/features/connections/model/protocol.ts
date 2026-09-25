import type { Block, Session, RuntimeMode } from "../../sessions/model/session";
import type { UserQuestionReply } from "../../sessions/model/userQuestion";

export const HOST_PROTOCOL_VERSION = 1;
export type RemoteProvider = "codex" | "claude";
export type HostDescriptor = {
  protocolVersion: number;
  environmentId: string;
  name: string;
  providers: RemoteProvider[];
  capabilities: string[];
  platform?: "win32" | "darwin" | "linux";
};
export type HostProject = { id: string; cwd: string; name: string };
export type HostSession = {
  session: Session;
  projectId: string;
  revision: number;
  runId?: string;
  status: "idle" | "running" | "interrupted";
  updatedAt: number;
  /** Host-only: the revision at which each block last changed. */
  blockRevisions?: Record<string, number>;
};
export type HostSessionSummary = Omit<
  HostSession,
  "session" | "blockRevisions"
> & {
  id: string;
  title: string;
  harness: RemoteProvider;
};

/** `sessions.sync` sends only the blocks that changed after the client's
 * revision, so a long transcript is not re-downloaded on every poll. */
export type SessionSync =
  | { kind: "unchanged"; revision: number }
  | { kind: "snapshot"; value: HostSession }
  | {
      kind: "delta";
      base: number;
      value: Omit<HostSession, "session" | "blockRevisions"> & {
        session: Omit<Session, "blocks">;
      };
      blockIds: string[];
      blocks: Block[];
    };

/** Throws when the delta does not apply to `known`; request a snapshot then. */
export function applySessionSync(
  known: HostSession | undefined,
  sync: SessionSync,
): HostSession {
  if (sync.kind === "snapshot") return sync.value;
  if (
    !known ||
    known.revision !== (sync.kind === "delta" ? sync.base : sync.revision)
  )
    throw new Error("Session sync base does not match");
  if (sync.kind === "unchanged") return known;
  const blocks = new Map(
    known.session.blocks.map((block) => [block.id, block]),
  );
  for (const block of sync.blocks) blocks.set(block.id, block);
  return {
    ...sync.value,
    session: {
      ...sync.value.session,
      blocks: sync.blockIds.map((id) => {
        const block = blocks.get(id);
        if (!block) throw new Error("Session sync is missing a block");
        return block;
      }),
    },
  };
}
export type HostCommand =
  | {
      type: "create";
      commandId: string;
      projectId: string;
      harness: RemoteProvider;
      model: string;
      runtimeMode: RuntimeMode;
    }
  | { type: "send"; commandId: string; sessionId: string; text: string }
  | { type: "cancel"; commandId: string; sessionId: string; runId: string }
  | {
      type: "approve";
      commandId: string;
      sessionId: string;
      runId: string;
      requestId: number;
      decision: "allow" | "deny";
    }
  | {
      type: "answer";
      commandId: string;
      sessionId: string;
      runId: string;
      requestId: number;
      reply: UserQuestionReply;
    };
export type CommandReceipt = {
  commandId: string;
  sessionId: string;
  revision: number;
};

/** Credentials never leave the desktop's native connection store. */
export type RemoteMachine = {
  id: string;
  name: string;
  endpoint: string;
  environmentId: string;
  ssh?: { target: string; port?: number | null; remotePort: number } | null;
};

export type SshSetup = {
  id: string;
  message: string;
  prompt?: { id: string; message: string; confirm: boolean } | null;
  done: boolean;
  error?: string | null;
  machine?: RemoteMachine | null;
};

export function isRemoteProvider(value: unknown): value is RemoteProvider {
  return value === "codex" || value === "claude";
}

export function requireHostDescriptor(value: HostDescriptor): HostDescriptor {
  if (
    value?.protocolVersion !== HOST_PROTOCOL_VERSION ||
    typeof value.environmentId !== "string" ||
    !value.environmentId ||
    !Array.isArray(value.providers) ||
    !value.providers.every(isRemoteProvider)
  ) {
    throw new Error("This machine is running an incompatible MonoCode Host");
  }
  return value;
}
