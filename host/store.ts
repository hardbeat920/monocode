import { DatabaseSync } from "node:sqlite";
import { lstatSync } from "node:fs";
import { randomUUID, createHash, randomBytes } from "node:crypto";
import { dirname, join, relative, isAbsolute } from "node:path";
import type {
  CommandReceipt,
  HostProject,
  HostSession,
  HostSessionSummary,
  RemoteProvider,
  SessionSync,
} from "../src/features/connections/model/protocol";
import type { LinkedWorkItem } from "../src/features/sessions/model/session";
import { sessionNeedsInput } from "../src/features/sessions/model/session";

const CACHED_SESSIONS = 32;

export class HostStore {
  readonly db: DatabaseSync;
  readonly environmentId: string;
  readonly attachmentDir: string;
  readonly isolationDir: string;
  // This process is the only session writer, so recently used snapshots are
  // served from memory instead of re-parsing whole transcripts. Callers must
  // treat returned values as immutable.
  private cache = new Map<string, HostSession>();

  constructor(path: string) {
    this.attachmentDir = join(dirname(path), "attachments");
    this.isolationDir = join(dirname(path), "isolation");
    this.db = new DatabaseSync(path);
    this.db
      .exec(`PRAGMA busy_timeout=5000; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
      CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY, cwd TEXT NOT NULL UNIQUE, name TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id), snapshot TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS receipts (id TEXT PRIMARY KEY, signature TEXT NOT NULL, receipt TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS events (session_id TEXT NOT NULL REFERENCES sessions(id), revision INTEGER NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(session_id, revision));
      CREATE TABLE IF NOT EXISTS devices (id TEXT PRIMARY KEY, name TEXT NOT NULL, hash TEXT NOT NULL UNIQUE);
      CREATE TABLE IF NOT EXISTS workspace_removals (path TEXT PRIMARY KEY, identity TEXT NOT NULL, sessions_json TEXT NOT NULL);`);
    const columns = this.db.prepare("PRAGMA table_info(sessions)").all();
    if (!columns.some((column) => column.name === "summary"))
      this.db.exec("ALTER TABLE sessions ADD COLUMN summary TEXT");
    this.db
      .prepare("INSERT OR IGNORE INTO metadata VALUES ('environmentId', ?)")
      .run(randomUUID());
    this.environmentId = String(
      this.db
        .prepare("SELECT value FROM metadata WHERE key='environmentId'")
        .get()!.value,
    );
    for (const row of this.db.prepare("SELECT path FROM workspace_removals").all()) {
      try {
        this.finishCowRemoval(String(row.path), true);
      } catch (error) {
        console.error("Workspace removal recovery will retry on restart:", error);
      }
    }
  }

  transaction<T>(fn: () => T): T {
    this.db.exec("BEGIN IMMEDIATE");
    try {
      const value = fn();
      this.db.exec("COMMIT");
      return value;
    } catch (error) {
      this.cache.clear();
      try {
        this.db.exec("ROLLBACK");
      } catch (rollbackError) {
        console.error("Could not roll back host transaction:", rollbackError);
      }
      throw error;
    }
  }

  project(id: string): HostProject {
    const row = this.db.prepare("SELECT * FROM projects WHERE id=?").get(id);
    if (!row) throw new Error("Project is not registered on this machine");
    return row as unknown as HostProject;
  }

  projects(): HostProject[] {
    return this.db
      .prepare("SELECT * FROM projects ORDER BY name")
      .all() as unknown as HostProject[];
  }

  addProject(cwd: string, name: string): HostProject {
    this.db
      .prepare("INSERT OR IGNORE INTO projects VALUES (?, ?, ?)")
      .run(randomUUID(), cwd, name);
    return this.db
      .prepare("SELECT * FROM projects WHERE cwd=?")
      .get(cwd) as unknown as HostProject;
  }

  private remember(value: HostSession): HostSession {
    this.cache.delete(value.session.id);
    this.cache.set(value.session.id, value);
    if (this.cache.size > CACHED_SESSIONS)
      this.cache.delete(this.cache.keys().next().value!);
    return value;
  }

  private find(id: string): HostSession | undefined {
    const cached = this.cache.get(id);
    if (cached) return this.remember(cached);
    const row = this.db
      .prepare("SELECT snapshot FROM sessions WHERE id=?")
      .get(id);
    return row
      ? this.remember(JSON.parse(String(row.snapshot)) as HostSession)
      : undefined;
  }

  session(id: string): HostSession {
    const value = this.find(id);
    if (!value) throw new Error("Session not found on this machine");
    return value;
  }

  summaries(projectId: string): HostSessionSummary[] {
    return this.db
      .prepare("SELECT id, summary FROM sessions WHERE project_id=?")
      .all(projectId)
      .map((row) => {
        const cached = row.summary
          ? (JSON.parse(String(row.summary)) as HostSessionSummary)
          : undefined;
        if (
          cached?.model &&
          cached.needsInput !== undefined &&
          cached.providerSessionId !== undefined
        )
          return cached;
        const fresh = summary(this.session(String(row.id)));
        this.db.prepare("UPDATE sessions SET summary=? WHERE id=?").run(
          JSON.stringify(fresh),
          String(row.id),
        );
        return fresh;
      })
      .sort((a, b) => b.updatedAt - a.updatedAt);
  }

  sync(id: string, revision?: number): SessionSync {
    const value = this.session(id);
    const { blockRevisions, ...snapshot } = value;
    if (revision === value.revision) return { kind: "unchanged", revision };
    if (revision === undefined || revision > value.revision || !blockRevisions)
      return { kind: "snapshot", value: snapshot };
    const {
      session: { blocks, ...session },
      ...rest
    } = snapshot;
    return {
      kind: "delta",
      base: revision,
      value: { ...rest, session },
      blockIds: blocks.map((block) => block.id),
      blocks: blocks.filter(
        (block) => (blockRevisions[block.id] ?? value.revision) > revision,
      ),
    };
  }

  sessions(projectId?: string): HostSession[] {
    const rows = projectId
      ? this.db
          .prepare("SELECT snapshot FROM sessions WHERE project_id=?")
          .all(projectId)
      : this.db.prepare("SELECT snapshot FROM sessions").all();
    return rows
      .map((row) => JSON.parse(String(row.snapshot)) as HostSession)
      .sort((a, b) => b.updatedAt - a.updatedAt);
  }

  /** Returns the saved value, stamped with per-block change revisions. */
  save(input: HostSession, event: unknown): HostSession {
    const previous = this.find(input.session.id);
    const value = {
      ...input,
      // Older snapshots have no creation time. Preserve their last recorded
      // timestamp when they are first written by this version of the host.
      createdAt:
        input.createdAt ?? previous?.createdAt ?? previous?.updatedAt ?? input.updatedAt,
      blockRevisions: blockRevisions(previous, input),
    };
    this.db
      .prepare(
        "INSERT INTO sessions (id, project_id, snapshot, summary) VALUES (?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET snapshot=excluded.snapshot, summary=excluded.summary",
      )
      .run(
        value.session.id,
        value.projectId,
        JSON.stringify(value),
        JSON.stringify(summary(value)),
      );
    this.db
      .prepare("INSERT INTO events VALUES (?, ?, ?)")
      .run(value.session.id, value.revision, JSON.stringify(event));
    this.db
      .prepare("DELETE FROM events WHERE session_id=? AND revision<?")
      .run(value.session.id, value.revision - 2_000);
    return this.remember(value);
  }

  updateSession(
    id: string,
    patch: { title?: string; archived?: boolean; pinned?: boolean; linkedWorkItem?: LinkedWorkItem | null },
  ): HostSessionSummary {
    return this.transaction(() => {
      const current = this.session(id);
      if (patch.title !== undefined && (!patch.title.trim() || patch.title.length > 200))
        throw new Error("Invalid session title");
      const next = this.save(
        {
          ...current,
          revision: current.revision + 1,
          archived: patch.archived ?? current.archived,
          pinned: patch.pinned ?? current.pinned,
          session: {
            ...current.session,
            ...(patch.title === undefined ? {} : { title: patch.title.trim() }),
            ...(patch.linkedWorkItem === undefined
              ? {}
              : { linkedWorkItem: patch.linkedWorkItem ?? undefined }),
          },
        },
        { type: "session.metadata", patch },
      );
      return summary(next);
    });
  }

  cowWorkspaceSessions(cowId: string, path: string): HostSession[] {
    return this.sessions().filter((value) => {
      const cwd = value.session.worktreeCwd || value.session.cwd;
      const rel = relative(path, cwd);
      return !value.session.worktreeRemoved &&
        (value.session.cowId === cowId || rel === "" || (!rel.startsWith("..") && !isAbsolute(rel)));
    });
  }

  prepareCowRemoval(cowId: string, path: string, projectCwd: string, keepSessions: boolean, expectedRootIdentity: readonly [string, string]): HostSession[] {
    return this.transaction(() => {
      const sessions = this.cowWorkspaceSessions(cowId, path);
      if (sessions.some((value) => value.status === "running"))
        throw new Error("Stop the sessions using this workspace first");
      if (!keepSessions && sessions.length)
        throw new Error("Sessions still use this workspace. Move or delete those sessions first (including archived sessions).");
      const identity = workspaceIdentity(path);
      if (!identity || identity.device !== expectedRootIdentity[0] || identity.inode !== expectedRootIdentity[1])
        throw new Error("Copy-on-write checkout was replaced");
      this.db.prepare("INSERT INTO workspace_removals VALUES (?, ?, ?)").run(
        path,
        JSON.stringify(identity),
        JSON.stringify(sessions),
      );
      for (const value of sessions) {
        this.save({
          ...value,
          revision: value.revision + 1,
          session: {
            ...value.session,
            cwd: projectCwd,
            worktreeCwd: value.session.worktreeCwd || value.session.cwd,
            worktreeRemoved: true,
            cowId: undefined,
            cowSourceCwd: undefined,
            workspaceMode: undefined,
            worktreeBase: undefined,
            branch: undefined,
            providerSessionId: undefined,
            context: undefined,
            pendingSwitch: undefined,
            pendingQuestion: undefined,
            busy: false,
            queueStatus: "paused",
          },
        }, { type: "workspace.removed", path });
      }
      return sessions;
    });
  }

  /** Finish deletion or restore only the original surviving checkout after failure/restart. */
  finishCowRemoval(path: string, failed = false): boolean {
    const row = this.db.prepare("SELECT identity, sessions_json FROM workspace_removals WHERE path=?").get(path);
    if (!row) return false;
    const identity = failed ? workspaceIdentity(path) : null;
    const survives = !!identity && JSON.stringify(identity) === String(row.identity);
    this.transaction(() => {
      if (survives) {
        for (const previous of JSON.parse(String(row.sessions_json)) as HostSession[]) {
          const current = this.find(previous.session.id);
          // A session may have been deleted or reattached while recovery was pending.
          if (!current?.session.worktreeRemoved || current.revision !== previous.revision + 1) continue;
          this.save({ ...previous, revision: current.revision + 1 }, { type: "workspace.removalFailed" });
        }
      }
      this.db.prepare("DELETE FROM workspace_removals WHERE path=?").run(path);
    });
    return !survives;
  }

  deleteSession(id: string): void {
    this.transaction(() => {
      const current = this.session(id);
      if (current.status === "running")
        throw new Error("Stop this session before deleting it");
      this.db.prepare("DELETE FROM events WHERE session_id=?").run(id);
      this.db.prepare("DELETE FROM sessions WHERE id=?").run(id);
      this.cache.delete(id);
    });
  }

  receipt(id: string, signature: string): CommandReceipt | undefined {
    const row = this.db.prepare("SELECT * FROM receipts WHERE id=?").get(id);
    if (!row) return undefined;
    if (row.signature !== signature)
      throw new Error("Command ID was already used with a different payload");
    return JSON.parse(String(row.receipt)) as CommandReceipt;
  }

  recordReceipt(signature: string, receipt: CommandReceipt): void {
    this.db
      .prepare("INSERT INTO receipts VALUES (?, ?, ?)")
      .run(receipt.commandId, signature, JSON.stringify(receipt));
  }

  events(
    id: string,
    after: number,
  ): { snapshot?: HostSession; events?: unknown[]; revision: number } {
    const snapshot = this.session(id);
    const rows = this.db
      .prepare(
        "SELECT revision, payload FROM events WHERE session_id=? AND revision>? ORDER BY revision",
      )
      .all(id, after);
    if (
      after > snapshot.revision ||
      (after < snapshot.revision && Number(rows[0]?.revision) !== after + 1)
    ) {
      return { snapshot, revision: snapshot.revision };
    }
    return {
      events: rows.map((row) => ({
        revision: row.revision,
        event: JSON.parse(String(row.payload)),
      })),
      revision: snapshot.revision,
    };
  }

  issueDevice(name: string): { id: string; token: string } {
    const id = randomUUID();
    const token = randomBytes(32).toString("base64url");
    this.db
      .prepare("INSERT INTO devices VALUES (?, ?, ?)")
      .run(id, name, this.hash(token));
    return { id, token };
  }

  revokeDevice(id: string): boolean {
    return (
      Number(
        this.db.prepare("DELETE FROM devices WHERE id=?").run(id).changes,
      ) > 0
    );
  }

  /** Lets a desktop revoke only the credential it is using. */
  revokeToken(token: string): boolean {
    return (
      Number(
        this.db
          .prepare("DELETE FROM devices WHERE hash=?")
          .run(this.hash(token)).changes,
      ) > 0
    );
  }

  authenticated(token: string): boolean {
    return !!this.db
      .prepare("SELECT id FROM devices WHERE hash=?")
      .get(this.hash(token));
  }

  private hash(token: string): string {
    return createHash("sha256").update(token).digest("hex");
  }
  close(): void {
    this.db.close();
  }
}

export function summary(value: HostSession): HostSessionSummary {
  return {
    projectId: value.projectId,
    revision: value.revision,
    runId: value.runId,
    status: value.status,
    updatedAt: value.updatedAt,
    id: value.session.id,
    cwd: value.session.cwd,
    cowId: value.session.cowId,
    title: value.session.title,
    harness: value.session.harness as RemoteProvider,
    model: value.session.model,
    runtimeMode: value.session.runtimeMode,
    providerSessionId: value.session.providerSessionId ?? null,
    createdAt: value.createdAt ?? value.updatedAt,
    archived: value.archived,
    pinned: value.pinned,
    linkedWorkItem: value.session.linkedWorkItem,
    needsInput: sessionNeedsInput(value.session),
    draft: value.session.blocks.some((block) => block.role === "user" && block.draft),
  };
}

/** Unchanged blocks keep their previous stamp. Identity is the fast path;
 * values re-read from disk fall back to a structural comparison. */
export function blockRevisions(
  previous: HostSession | undefined,
  next: HostSession,
): Record<string, number> {
  const before = new Map(
    previous?.session.blocks.map((block) => [block.id, block]),
  );
  const revisions: Record<string, number> = {};
  for (const block of next.session.blocks) {
    const old = before.get(block.id);
    const stamp = previous?.blockRevisions?.[block.id];
    revisions[block.id] =
      old &&
      stamp !== undefined &&
      (old === block || JSON.stringify(old) === JSON.stringify(block))
        ? stamp
        : next.revision;
  }
  return revisions;
}

/** Keep replaced roots and Git directories detached; errors other than absence retain the journal. */
function workspaceIdentity(path: string): { device: string; inode: string; gitDevice: string; gitInode: string } | null {
  try {
    const root = lstatSync(path, { bigint: true });
    const git = lstatSync(join(path, ".git"), { bigint: true });
    if (!root.isDirectory() || !git.isDirectory()) return null;
    return { device: String(root.dev), inode: String(root.ino), gitDevice: String(git.dev), gitInode: String(git.ino) };
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  }
}
