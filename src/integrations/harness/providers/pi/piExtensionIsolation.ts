import { joinPath } from "../../../../shared/lib/paths";
import { execChild } from "../../core/child";
import type { PiFlavor } from "./piFlavor";

/** `buildPiSpawnArgs` inputs that keep a workspace's extensions out of a
 *  probe or throwaway job while the user's own provider extensions stay
 *  loadable. */
export interface IsolatedExtensionArgs {
  noExtensions?: true;
  extensionPaths?: string[];
}

/**
 * Extra `buildPiSpawnArgs` inputs for a child that must not run a workspace's
 * extensions but still resolves models those extensions register.
 *
 * `--no-extensions` only disables extension *discovery* — explicit `-e` paths
 * still load — so naming the user's own extension directory keeps
 * plugin-registered providers (`pi.registerProvider`) in the catalog while a
 * workspace's `.omp/extensions` stays out of reach. Without the allowlist the
 * probe reports an empty catalog and an isolated text job handed such a model
 * id exits with "Model not found".
 */
export async function isolatedExtensionArgs(
  flavor: PiFlavor,
): Promise<IsolatedExtensionArgs> {
  if (flavor.gatesProjectExtensions) return {};
  const root = await userExtensionRoot(flavor);
  return { noExtensions: true, ...(root ? { extensionPaths: [root] } : {}) };
}

/** Memoized per flavor: the CLI's agent directory cannot change while MonoCode
 *  runs, and asking for it costs a process spawn. */
const roots = new WeakMap<PiFlavor, Promise<string | null>>();

function userExtensionRoot(flavor: PiFlavor): Promise<string | null> {
  const cached = roots.get(flavor);
  if (cached) return cached;
  const pending = agentDir(flavor)
    .then((dir) => (dir ? joinPath(dir, "extensions") : null))
    .catch(() => null);
  roots.set(flavor, pending);
  return pending;
}

/**
 * `<binary> config path` reports the active profile's agent directory, so
 * `--profile`, `OMP_PROFILE`, and `PI_CODING_AGENT_DIR` are honored instead of
 * assuming `~/.omp/agent`. A CLI too old to answer yields no allowlist, which
 * falls back to plain discovery-off behavior.
 */
async function agentDir(flavor: PiFlavor): Promise<string> {
  const { path } = await flavor.resolveBinary();
  return (await execChild(path, ["config", "path"], undefined, flavor.id)).trim();
}
