import type { HarnessId } from "../session";
import { HARNESSES } from "../session";
import { resolveRemoteAgent } from "../connections";
import { isRemotePath, parseRemotePath } from "../remote";
import {
  resolveClaudeBinary,
  resolveCodexBinary,
  resolveCursorBinary,
  resolveFxBinary,
  resolveGrokBinary,
  resolveHermesBinary,
  resolveOmpBinary,
  resolveOpenCodeBinary,
  resolvePiBinary,
} from "./child";
import { isLiveHarness } from "./registry";

export type HarnessAvailability = Record<HarnessId, boolean>;

/**
 * We only ever check whether the binary exists, never whether it is
 * authenticated, so the hint must not blame a login.
 */
const CLI: Record<HarnessId, { name: string; install?: string }> = {
  claude: { name: "Claude Code CLI" },
  codex: { name: "Codex CLI" },
  cursor: { name: "Cursor CLI" },
  grok: {
    name: "Grok Build CLI",
    install: "curl -fsSL https://x.ai/cli/install.sh | bash",
  },
  opencode: { name: "OpenCode CLI" },
  pi: { name: "Pi CLI", install: "npm i -g @earendil-works/pi-coding-agent" },
  omp: { name: "omp CLI", install: "curl -fsSL https://omp.sh/install | sh" },
  fx: { name: "fx CLI", install: "curl -fsSL https://fx.sh/setup.sh | bash" },
  hermes: {
    name: "Hermes Agent CLI",
    install:
      "Install from hermes-agent.nousresearch.com, then run hermes model",
  },
};

let availability: HarnessAvailability = {
  claude: false,
  codex: false,
  cursor: false,
  grok: false,
  opencode: false,
  pi: false,
  omp: false,
  fx: false,
  hermes: false,
};
let version = 0;
let inflight: Promise<void> | null = null;
let probedAt = 0;
const listeners = new Set<() => void>();

/**
 * A probe stats ~100 paths across eight resolvers. The model picker and the
 * providers pane both probe on open, so without a TTL every open pays for it
 * again to learn what it already knows. Installing a CLI mid-session is rare,
 * and `force` covers it.
 */
const PROBE_TTL_MS = 30_000;

function emit() {
  version += 1;
  for (const listener of listeners) listener();
}

export function subscribeHarnessAvailability(onStoreChange: () => void): () => void {
  listeners.add(onStoreChange);
  return () => {
    listeners.delete(onStoreChange);
  };
}

export function getHarnessAvailabilitySnapshot(): number {
  return version;
}

export function hasProbedHarnessAvailability(): boolean {
  return probedAt > 0;
}

export function isHarnessAvailable(id: HarnessId): boolean {
  return availability[id];
}

export function harnessUnavailableHint(id: HarnessId): string {
  const { name, install } = CLI[id];
  const how = install ? ` (\`${install}\`)` : "";
  return `${name} not found${how}. Install it, or restart MonoCode if it is already installed.`;
}

export function probeHarnessAvailability(
  options?: { force?: boolean },
): Promise<void> {
  if (inflight) return inflight;
  if (!options?.force && probedAt > 0 && Date.now() - probedAt < PROBE_TTL_MS) {
    return Promise.resolve();
  }
  inflight = Promise.all(
    HARNESSES.map(async (id) => {
      if (!isLiveHarness(id)) return [id, false] as const;
      if (id === "cursor") {
        try {
          await resolveCursorBinary();
          return [id, true] as const;
        } catch {
          return [id, false] as const;
        }
      }
      if (id === "claude") {
        try {
          await resolveClaudeBinary();
          return [id, true] as const;
        } catch {
          return [id, false] as const;
        }
      }
      if (id === "codex") {
        try {
          await resolveCodexBinary();
          return [id, true] as const;
        } catch {
          return [id, false] as const;
        }
      }
      if (id === "opencode") {
        try {
          await resolveOpenCodeBinary();
          return [id, true] as const;
        } catch {
          return [id, false] as const;
        }
      }
      if (id === "pi") {
        try {
          await resolvePiBinary();
          return [id, true] as const;
        } catch {
          return [id, false] as const;
        }
      }
      if (id === "omp") {
        try {
          await resolveOmpBinary();
          return [id, true] as const;
        } catch {
          return [id, false] as const;
        }
      }
      if (id === "fx") {
        try {
          await resolveFxBinary();
          return [id, true] as const;
        } catch {
          return [id, false] as const;
        }
      }
      if (id === "grok") {
        try {
          await resolveGrokBinary();
          return [id, true] as const;
        } catch {
          return [id, false] as const;
        }
      }
      if (id === "hermes") {
        try {
          await resolveHermesBinary();
          return [id, true] as const;
        } catch {
          return [id, false] as const;
        }
      }
      return [id, false] as const;
    }),
  )
    .then((entries) => {
      const next = { ...availability };
      for (const [id, ok] of entries) next[id] = ok;
      availability = next;
      emit();
    })
    .finally(() => {
      probedAt = Date.now();
      inflight = null;
    });
  return inflight;
}

// ---------------------------------------------------------------------------
// Remote availability: per connection, claude/codex only for now.
// ---------------------------------------------------------------------------

export const REMOTE_HARNESSES: HarnessId[] = ["claude", "codex"];

type RemoteAvailability = Partial<Record<HarnessId, boolean>>;

const remoteAvailability = new Map<string, RemoteAvailability>();
const remoteInflight = new Map<string, Promise<void>>();
const remoteProbedAt = new Map<string, number>();
const remoteListeners = new Set<() => void>();
let remoteVersion = 0;

function emitRemote() {
  remoteVersion += 1;
  for (const listener of remoteListeners) listener();
}

export function subscribeRemoteHarnessAvailability(
  onStoreChange: () => void,
): () => void {
  remoteListeners.add(onStoreChange);
  return () => {
    remoteListeners.delete(onStoreChange);
  };
}

export function getRemoteHarnessAvailabilitySnapshot(): number {
  return remoteVersion;
}

export function isRemoteHarnessAvailable(
  connectionId: string,
  id: HarnessId,
): boolean {
  return remoteAvailability.get(connectionId)?.[id] ?? false;
}

export function probeRemoteHarnessAvailability(
  connectionId: string,
  options?: { force?: boolean },
): Promise<void> {
  const existing = remoteInflight.get(connectionId);
  if (existing) return existing;
  const probedAt = remoteProbedAt.get(connectionId) ?? 0;
  if (!options?.force && probedAt > 0 && Date.now() - probedAt < PROBE_TTL_MS) {
    return Promise.resolve();
  }
  const probe = Promise.all(
    REMOTE_HARNESSES.map(async (id) => {
      try {
        await resolveRemoteAgent(connectionId, id);
        return [id, true] as const;
      } catch {
        return [id, false] as const;
      }
    }),
  )
    .then((entries) => {
      const next: RemoteAvailability = {};
      for (const [id, ok] of entries) next[id] = ok;
      remoteAvailability.set(connectionId, next);
      emitRemote();
    })
    .finally(() => {
      remoteProbedAt.set(connectionId, Date.now());
      remoteInflight.delete(connectionId);
    });
  remoteInflight.set(connectionId, probe);
  return probe;
}

/** Availability for a cwd: remote projects probe per connection. */
export function isHarnessAvailableFor(id: HarnessId, cwd: string): boolean {
  if (!isRemotePath(cwd)) return isHarnessAvailable(id);
  const parsed = parseRemotePath(cwd);
  if (!parsed) return false;
  if (!REMOTE_HARNESSES.includes(id)) return false;
  return isRemoteHarnessAvailable(parsed.connectionId, id);
}
