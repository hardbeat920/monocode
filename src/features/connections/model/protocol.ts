import type { Block, Session, RuntimeMode } from "../../sessions/model/session";
import type { UserQuestionReply } from "../../sessions/model/userQuestion";
import type { AgentModel } from "../../sessions/model/models";
import type { LinkedWorkItem } from "../../sessions/model/session";

export const HOST_PROTOCOL_VERSION = 1;
export const SESSION_PROVIDER_SWITCH_CAPABILITY = "sessionProviderSwitchV1";
export const SESSION_PROVIDER_INSPECTION_CAPABILITY = "sessionProviderInspectionV1";
export const REMOTE_PROVIDERS = [
  "codex",
  "claude",
  "cursor",
  "grok",
  "opencode",
  "pi",
  "omp",
  "fx",
  "hermes",
  "antigravity",
] as const;
export type RemoteProvider = (typeof REMOTE_PROVIDERS)[number];
export type HostDescriptor = {
  protocolVersion: number;
  environmentId: string;
  name: string;
  providers: RemoteProvider[];
  capabilities: string[];
  platform?: "win32" | "darwin" | "linux";
};
export type HostProject = { id: string; cwd: string; name: string };
export type HostDirectory = {
  path: string;
  parent: string | null;
  entries: { name: string; path: string }[];
};
export type HostModelCatalog = {
  models: Partial<Record<RemoteProvider, AgentModel[]>>;
  errors: Partial<Record<RemoteProvider, string>>;
};
export type HostWorktree = {
  path: string;
  branch: string | null;
  head: string;
  isMain: boolean;
  missing: boolean;
};
export type HostSession = {
  session: Session;
  projectId: string;
  revision: number;
  runId?: string;
  status: "idle" | "running" | "interrupted";
  /** Missing from snapshots written before creation time was stored. */
  createdAt?: number;
  updatedAt: number;
  archived?: boolean;
  pinned?: boolean;
  /** Temporary branch created by the composer for automatic first-turn naming. */
  autoWorktreeBranch?: string;
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
  cwd?: string;
  model?: string;
  runtimeMode?: RuntimeMode;
  providerSessionId?: string | null;
  createdAt?: number;
  linkedWorkItem?: LinkedWorkItem;
  needsInput?: boolean;
  branch?: string;
  worktreeCwd?: string;
  repo?: string;
  draft?: boolean;
};

export type RemoteAttachment = {
  id: string;
  name: string;
  mimeType: string;
  kind: "image" | "audio" | "file";
  size: number;
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

/** A sync too large for one response. Its serialized JSON is read in bounded
 * pieces with `sessions.syncChunk`, so every piece describes one revision. */
export type SessionSyncTransfer = {
  kind: "chunked";
  transfer: string;
  /** UTF-16 length of the serialized `SessionSync`. */
  length: number;
};
export type SessionSyncChunk = { data: string };
export type SessionSyncResponse = SessionSync | SessionSyncTransfer;

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
      worktreeCwd?: string;
      autoWorktreeBranch?: string;
      harness: RemoteProvider;
      model: string;
      modelSettings?: Record<string, string>;
      runtimeMode: RuntimeMode;
    }
  | {
      type: "configure";
      commandId: string;
      sessionId: string;
      model: string;
      modelSettings: Record<string, string>;
      runtimeMode: RuntimeMode;
    }
  | {
      type: "switchProvider";
      commandId: string;
      sessionId: string;
      expectedRevision: number;
      harness: RemoteProvider;
      model: string;
      modelSettings: Record<string, string>;
      runtimeMode: RuntimeMode;
    }
  | {
      type: "confirmProviderInspection";
      commandId: string;
      sessionId: string;
      expectedRevision: number;
    }
  | { type: "compact"; commandId: string; sessionId: string }
  | {
      type: "send";
      commandId: string;
      sessionId: string;
      text: string;
      attachments?: RemoteAttachment[];
      intent?: "default" | "plan" | "build";
      draftBlockId?: string;
      planBlockId?: string;
    }
  | {
      type: "draft";
      commandId: string;
      sessionId: string;
      text: string;
      attachments?: RemoteAttachment[];
    }
  | {
      type: "removeDraft";
      commandId: string;
      sessionId: string;
      draftBlockId: string;
    }
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
  return (
    typeof value === "string" &&
    REMOTE_PROVIDERS.some((provider) => provider === value)
  );
}

export function hostSupportsProviderSwitch(host?: HostDescriptor): boolean {
  return !!host?.capabilities?.includes(SESSION_PROVIDER_SWITCH_CAPABILITY);
}

export function hostSupportsProviderInspection(host?: HostDescriptor): boolean {
  return host?.protocolVersion === HOST_PROTOCOL_VERSION && host.capabilities.includes(SESSION_PROVIDER_INSPECTION_CAPABILITY);
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
