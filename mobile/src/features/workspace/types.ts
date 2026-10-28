// Trimmed-down versions of the desktop shapes (RecentProject, Mono, SessionSummary).
// These will be filled from the paired desktop once sync exists.

import type { MascotStatus } from "@/features/mascots/PixelMascot";

// The desktop's HarnessId (src/features/sessions/model/session.ts).
export type Harness =
  | "claude"
  | "codex"
  | "cursor"
  | "grok"
  | "opencode"
  | "pi"
  | "omp"
  | "fx"
  | "hermes"
  | "antigravity"
  | "devin";

export type Project = {
  id: string;
  name: string;
  path: string;
  machine: string;
  branch: string;
  /** Group color, as on the desktop rail. */
  color: string;
  /** Explicit mascot pick; otherwise hashed from the path. */
  mascot?: string;
  /** Uncommitted changes in the working tree. */
  changes?: ChangedFile[];
  additions: number;
  deletions: number;
  busy?: boolean;
  defaultHarness?: Harness;
  defaultModel?: string;
  /** The default model as the desktop names it. */
  defaultModelName?: string;
  /** The permission mode a new desktop tab would start with. */
  defaultRuntimeMode?: RuntimeMode;
};

// Trimmed from the desktop's GitChangedFile (src/platform/tauri/fs.ts) and
// UnifiedLine (src/features/source-control/model/unifiedDiff.ts).
export type ChangeStatus = "modified" | "added" | "deleted" | "untracked";

export type DiffLine = {
  kind: "add" | "del" | "context" | "hunk";
  text: string;
  oldNumber: number | null;
  newNumber: number | null;
  /** Its syntax colours as runs of [text, index into the file's palette]. */
  runs?: [string, number][];
};

export type ChangedFile = {
  /** Path relative to the project root. */
  relative: string;
  status: ChangeStatus;
  additions: number;
  deletions: number;
  /** Whether some or all of it is staged, and whether some is not; absent from an older desktop. */
  staged?: boolean;
  unstaged?: boolean;
  /** Unified diff, hunk headers included. */
  lines: DiffLine[];
  /** The colours its lines' runs index, when the desktop could highlight it. */
  palette?: string[];
};

/** An entry in one of a project's folders, as the Files page lists it. */
export type FileEntry = { name: string; isDir: boolean; ignored: boolean };

/**
 * A file as the desktop sends it for reading. With `highlighted`, each line
 * comes as runs of [text, index into `palette`] in the desktop's syntax colours.
 */
export type FileContents = {
  /** Where the desktop found the file, relative to the root; unset from an
   *  older desktop. */
  path?: string;
  text: string;
  truncated: boolean;
  highlighted?: { palette: string[]; lines: [string, number][][] };
  /** For a file too large to highlight at once: the desktop's background
   *  highlight, whose colours come in a chunk at a time. */
  highlightJob?: string;
};

/** One of a project's other working copies, named by its branch. */
export type Worktree = { path: string; name: string };

export type Mono = {
  id: string;
  name: string;
  mascot: string;
  color: string;
  projectIds: string[];
  unread: number;
  status: MascotStatus;
  /** Latest line in the mono's conversation, for its drawer row. */
  lastMessage: string;
  lastActiveAt: number;
  harness?: Harness;
  model?: string;
};

// The desktop's RuntimeMode (src/features/sessions/model/session.ts).
export type RuntimeMode = "supervised" | "auto-accept-edits" | "auto" | "full-access";

export type SessionSummary = {
  id: string;
  projectId: string;
  monoId?: string;
  title: string;
  harness: Harness;
  model: string;
  /** The model as the desktop names it, e.g. "Opus 4.1" for "claude:opus". */
  modelName?: string;
  branch: string;
  /** The worktree it works in, when not the project's checkout; absent from an older desktop. */
  worktreePath?: string;
  /** Its permission mode, as the desktop's access picker sets it. */
  runtimeMode?: RuntimeMode;
  updatedAt: number;
  busy?: boolean;
  /** What it waits on the user for; absent from an older desktop. */
  waiting?: "approval" | "question";
};

export type Message = {
  id: string;
  role: "user" | "assistant";
  text: string;
};

// The desktop's MobileTurn (src/features/mobile/model/mobileTranscript.ts): a
// transcript the desktop has already grouped and folded as AgentTranscript does.
export type WorkKind = "research" | "edit" | "run" | "agent" | "other" | "think" | "note";

export type WorkStep = { id: string; label: string; failed?: boolean };

export type WorkPhase = { id: string; kind: WorkKind; title: string; steps: WorkStep[] };

/** What an opened fold holds, in order: the agent's notes between its work. */
export type FoldEntry =
  | { type: "prose"; id: string; text: string }
  | ({ type: "phase" } & WorkPhase);

export type TurnItem =
  | {
      type: "user";
      id: string;
      text: string;
      at?: number;
      reaction?: string;
      emojiOnly?: boolean;
      attachments?: number;
      compact?: boolean;
    }
  | { type: "text"; id: string; text: string; streaming?: boolean }
  | {
      type: "header";
      id: string;
      live: boolean;
      label?: string;
      waiting?: string;
      startedAt?: number;
      durationMs?: number;
      model?: string;
      harness?: Harness;
      entries: FoldEntry[];
    }
  | { type: "work"; id: string; live: boolean; phases: WorkPhase[] }
  | { type: "notice"; id: string; text: string; error?: boolean };

export type Turn = {
  id: string;
  stampAt?: number;
  items: TurnItem[];
  completedAt?: number;
  footer?: boolean;
  /** What the footer's copy button copies: the turn's replies, as on desktop. */
  copyText?: string;
  /** Token accounting for the footer's metrics button; Monos leave it out. */
  metrics?: TurnMetrics;
  durationMs?: number;
};

// The desktop's TurnMetrics (src/features/sessions/model/session.ts).
export type TurnMetrics = {
  inputTokens?: number;
  outputTokens?: number;
  cacheReadTokens?: number;
  cacheWriteTokens?: number;
  cacheHitPercent?: number;
};
