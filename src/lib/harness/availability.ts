import type { HarnessId } from "../session";
import { HARNESSES } from "../session";
import {
  resolveAmpBinary,
  resolveClaudeBinary,
  resolveCodexBinary,
  resolveCursorBinary,
  resolveFxBinary,
  resolveOpenCodeBinary,
  resolvePiBinary,
} from "./child";
import { isLiveHarness } from "./registry";

export type HarnessAvailability = Record<HarnessId, boolean>;

const UNAVAILABLE_HINT: Record<HarnessId, string> = {
  claude: "Install Claude Code and run `claude auth login`",
  codex: "Install Codex CLI and run `codex login`",
  cursor: "Install Cursor CLI and run `agent login`",
  opencode: "Install OpenCode and run `opencode auth login`",
  pi: "Install Pi (`npm i -g @earendil-works/pi-coding-agent`) and authenticate",
  fx: "Install fx (`curl -fsSL https://fx.sh/setup.sh | bash`) and run `fx login`",
  amp: "Install Amp (`curl -fsSL https://ampcode.com/install.sh | bash`) and sign in at ampcode.com",
};

let availability: HarnessAvailability = {
  claude: false,
  codex: false,
  cursor: false,
  opencode: false,
  pi: false,
  fx: false,
  amp: false,
};
let version = 0;
let inflight: Promise<void> | null = null;
const listeners = new Set<() => void>();

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

export function isHarnessAvailable(id: HarnessId): boolean {
  return availability[id];
}

export function harnessUnavailableHint(id: HarnessId): string {
  return UNAVAILABLE_HINT[id];
}

export function probeHarnessAvailability(): Promise<void> {
  if (inflight) return inflight;
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
      if (id === "fx") {
        try {
          await resolveFxBinary();
          return [id, true] as const;
        } catch {
          return [id, false] as const;
        }
      }
      if (id === "amp") {
        try {
          await resolveAmpBinary();
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
      inflight = null;
    });
  return inflight;
}
