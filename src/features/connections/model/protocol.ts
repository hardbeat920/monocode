import type { Block, Session, RuntimeMode } from "../../sessions/model/session";
import type { UserQuestionReply } from "../../sessions/model/userQuestion";
import type { AgentModel } from "../../sessions/model/models";
import type { LinkedWorkItem } from "../../sessions/model/session";

export const HOST_PROTOCOL_VERSION = 1;
/** The npm package a machine runs to host sessions for this desktop. */
export const HOST_PACKAGE = "monocode-host";

/** The command that installs, updates, or pairs a host for this desktop. */
export const hostConnectCommand = (desktopVersion?: string) =>
  `npx ${HOST_PACKAGE}${desktopVersion ? `@${desktopVersion}` : ""} connect`;

/** Compares `a.b.c` versions, ignoring prerelease suffixes. */
export function compareVersions(a: string, b: string): number {
  const parts = (value: string) =>
    value.split("-")[0].split(".").map((part) => Number(part) || 0);
  const left = parts(a);
  const right = parts(b);
  for (let i = 0; i < 3; i++) {
    const delta = (left[i] ?? 0) - (right[i] ?? 0);
    if (delta) return Math.sign(delta);
  }
  return 0;
}

/** Hosts before this desktop's version, or without pushed changes, should
 * be updated. */
export function hostNeedsUpdate(
  host: HostDescriptor,
  desktopVersion?: string,
): boolean {
  if (!host.capabilities?.includes("changes.wait")) return true;
  return !!(
    desktopVersion &&
    host.hostVersion &&
    compareVersions(host.hostVersion, desktopVersion) < 0
  );
}
export type RemoteProvider = "codex" | "claude";
export type HostDescriptor = {
  protocolVersion: number;
  environmentId: string;
  name: string;
  providers: RemoteProvider[];
  capabilities: string[];
  platform?: "win32" | "darwin" | "linux";
  /** The MonoCode Host package version. Hosts before 0.5 omit it. */
  hostVersion?: string;
  /** Network addresses the host listens on, such as `https://10.0.0.5:3774`. */
  endpoints?: string[];
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
/** One session write, as reported by `changes.wait`. */
export type SessionChange = {
  id: string;
  projectId: string;
  revision: number;
  deleted?: boolean;
  /** Whether a turn is running after this write. Lets a desktop notice a
   * finished turn in a tab it is not showing. Absent from older hosts. */
  status?: HostSession["status"];
  busy?: boolean;
};
/** `reset` means the desktop's cursor is from another host run or too old;
 * it reloads what it shows and continues from `cursor`. */
export type SessionChanges = {
  boot: string;
  cursor: number;
  sessions: SessionChange[];
  reset: boolean;
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
  /** How the desktop reaches the host, for display. */
  endpoint: string;
  /** Direct TLS addresses; the desktop pins the host certificate. */
  endpoints?: string[];
  environmentId: string;
  /** A fallback route through an SSH forward to the host's loopback port. */
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
