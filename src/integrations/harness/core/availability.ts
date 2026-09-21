import type { HarnessId } from "../../../features/sessions/model/session";
import { HARNESSES } from "../../../features/sessions/model/session";
import {
  resolveAntigravityBinary,
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
import {
  emitHarnessAvailability,
  harnessAvailabilityProbedAt,
  markHarnessAvailabilityProbed,
  setHarnessAvailability,
  type HarnessAvailability,
} from "./availabilityState";
import { resolveHarnessBinary } from "./runtime";

/** One resolver per live harness, keyed the same way `resolveHarnessBinary`
 * expects — used so a probe honors a runtime binary-path override the same
 * way an actual spawn would, instead of only ever checking PATH. A function
 * rather than a plain lookup object, so each binding is only read inside the
 * probe's own try/catch instead of eagerly at module load. */
function resolverFor(id: HarnessId): (() => Promise<{ path: string }>) | undefined {
  switch (id) {
    case "claude":
      return resolveClaudeBinary;
    case "codex":
      return resolveCodexBinary;
    case "cursor":
      return resolveCursorBinary;
    case "grok":
      return resolveGrokBinary;
    case "opencode":
      return resolveOpenCodeBinary;
    case "pi":
      return resolvePiBinary;
    case "omp":
      return resolveOmpBinary;
    case "fx":
      return resolveFxBinary;
    case "hermes":
      return resolveHermesBinary;
    case "antigravity":
      return resolveAntigravityBinary;
    default:
      return undefined;
  }
}

export type { HarnessAvailability } from "./availabilityState";
export {
  getHarnessAvailabilitySnapshot,
  hasProbedHarnessAvailability,
  isHarnessAvailable,
  subscribeHarnessAvailability,
} from "./availabilityState";

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
  antigravity: { name: "Antigravity ACP server (agy_acp_server.par)" },
};

let inflight: Promise<void> | null = null;

/**
 * A probe stats ~100 paths across the resolvers. The model picker and the
 * providers pane both probe on open, so without a TTL every open pays for it
 * again to learn what it already knows. Installing a CLI mid-session is rare,
 * and `force` covers it.
 */
const PROBE_TTL_MS = 30_000;

export function harnessUnavailableHint(id: HarnessId): string {
  const { name, install } = CLI[id];
  const how = install ? ` (\`${install}\`)` : "";
  return `${name} not found${how}. Install it, or restart MonoCode if it is already installed.`;
}

export function probeHarnessAvailability(
  options?: { force?: boolean },
): Promise<void> {
  if (inflight) return inflight;
  const lastProbe = harnessAvailabilityProbedAt();
  if (!options?.force && lastProbe > 0 && Date.now() - lastProbe < PROBE_TTL_MS) {
    return Promise.resolve();
  }
  inflight = Promise.all(
    HARNESSES.map(async (id) => {
      if (!isLiveHarness(id)) return [id, false] as const;
      try {
        const resolver = resolverFor(id);
        if (!resolver) return [id, false] as const;
        await resolveHarnessBinary(id, resolver);
        return [id, true] as const;
      } catch {
        return [id, false] as const;
      }
    }),
  )
    .then((entries) => {
      const next = {} as HarnessAvailability;
      for (const [id, ok] of entries) next[id] = ok;
      setHarnessAvailability(next);
      emitHarnessAvailability();
    })
    .finally(() => {
      markHarnessAvailabilityProbed();
      inflight = null;
    });
  return inflight;
}
