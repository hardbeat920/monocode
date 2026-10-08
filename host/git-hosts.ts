import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { lstat, readdir, stat } from "node:fs/promises";
import { isAbsolute, join, resolve } from "node:path";
import { promisify } from "node:util";
import type { HostProject } from "../src/features/connections/model/protocol";
import {
  planCheckoutFolder,
  remoteMatches,
  validRepoSlug,
  type CheckoutFolderState,
} from "../src/features/git-hosts/model/remoteUrl";
import {
  isGitHostId,
  type CheckoutPlan,
  type GitHostId,
  type GitHostRepo,
  type GitHostStatus,
  type HostCheckoutJob,
} from "../src/features/git-hosts/model/types";
import { github } from "./git-hosts-github";

const exec = promisify(execFile);

/** A Git hosting service this machine can clone from. Mirrors `GitHost` in
 * `src-tauri/src/git_hosts`. */
export interface GitHostProvider {
  id: GitHostId;
  /** The host name its remotes point at, used to recognize existing checkouts. */
  domain: string;
  status(): Promise<GitHostStatus>;
  repos(): Promise<GitHostRepo[]>;
  /** Clones `slug` into `dest`, which is missing or empty. */
  cloneInto(slug: string, dest: string): Promise<void>;
}

export const GIT_HOST_PROVIDERS: Record<GitHostId, GitHostProvider> = { github };

export function gitHostProvider(
  id: unknown,
  providers: Record<GitHostId, GitHostProvider> = GIT_HOST_PROVIDERS,
): GitHostProvider {
  if (!isGitHostId(id)) throw new Error("Unknown Git host");
  return providers[id];
}

export function gitHostStatuses(
  providers: Record<GitHostId, GitHostProvider> = GIT_HOST_PROVIDERS,
): Promise<GitHostStatus[]> {
  return Promise.all(Object.values(providers).map((provider) => provider.status()));
}

const git = (path: string, args: string[]) =>
  exec("git", ["-C", path, ...args], {
    timeout: 10_000,
    encoding: "utf8",
    env: { ...process.env, GIT_OPTIONAL_LOCKS: "0", GIT_TERMINAL_PROMPT: "0" },
  }).then(({ stdout }) => stdout);

async function folderState(path: string, domain: string, slug: string): Promise<CheckoutFolderState> {
  const info = await lstat(path).catch(() => null);
  if (!info) return "free";
  if (!info.isDirectory()) return "taken";
  const entries = await readdir(path).catch(() => null);
  if (!entries) return "taken";
  if (!entries.length) return "free";
  // Only the checkout's own root counts, not a folder inside another repository.
  if (!(await stat(join(path, ".git")).catch(() => null))) return "taken";
  const remotes = await git(path, ["remote", "-v"]).catch(() => "");
  const tracks = remotes.split("\n").some((line) => {
    const url = line.split(/\s+/)[1];
    return !!url && remoteMatches(url, domain, slug);
  });
  if (!tracks) return "taken";
  // An interrupted clone has the remote but no commit checked out.
  const head = await git(path, ["rev-parse", "--verify", "-q", "HEAD"]).catch(() => null);
  return head ? "match" : "taken";
}

/** Folders being cloned into. A clone writes its remote before fetching, so
 * without this a second request could open, or clone beside, a half-written one. */
const cloning = new Set<string>();

/** Where `slug` would be checked out under `parent`. Matches `plan_checkout`
 * in `src-tauri/src/git_hosts/mod.rs`. */
export async function planHostCheckout(
  provider: GitHostProvider,
  slug: unknown,
  parent: unknown,
): Promise<CheckoutPlan> {
  if (typeof slug !== "string" || !validRepoSlug(slug))
    throw new Error("Enter a repository as owner/name");
  if (typeof parent !== "string" || parent.includes("\0") || !isAbsolute(parent))
    throw new Error("Choose an absolute folder to clone into");
  const root = resolve(parent);
  if (!(await stat(root).catch(() => null))?.isDirectory())
    throw new Error(`${root} is not a folder`);
  const { name, reuse } = await planCheckoutFolder(slug, (name) => {
    const path = join(root, name);
    if (cloning.has(path)) throw new Error(`Already cloning into ${path}`);
    return folderState(path, provider.domain, slug);
  });
  return { path: join(root, name), reuse };
}

/** Opens an existing checkout of `slug` under `parent`, or clones it there. */
export async function hostCheckout(
  provider: GitHostProvider,
  slug: unknown,
  parent: unknown,
): Promise<CheckoutPlan> {
  const plan = await planHostCheckout(provider, slug, parent);
  if (plan.reuse) return plan;
  if (cloning.has(plan.path)) throw new Error(`Already cloning into ${plan.path}`);
  cloning.add(plan.path);
  try {
    await provider.cloneInto(slug as string, plan.path);
  } finally {
    cloning.delete(plan.path);
  }
  return plan;
}

const FINISHED_JOB_TTL_MS = 10 * 60_000;

/** Clones run longer than one RPC may take, so they are started here and
 * polled until the project is ready. */
export class GitHostCheckouts {
  private readonly jobs = new Map<string, { key: string; job: HostCheckoutJob }>();

  constructor(
    private readonly openProject: (cwd: string) => Promise<HostProject>,
    private readonly providers: Record<GitHostId, GitHostProvider> = GIT_HOST_PROVIDERS,
  ) {}

  start(params: Record<string, unknown>): { jobId: string } {
    const provider = gitHostProvider(params.provider, this.providers);
    const parent = typeof params.parent === "string" ? resolve(params.parent) : params.parent;
    const key = JSON.stringify([provider.id, params.slug, parent]);
    // A second click on the same repository follows the clone already running.
    for (const [jobId, entry] of this.jobs)
      if (entry.key === key && entry.job.state === "running") return { jobId };
    const jobId = randomUUID();
    const entry: { key: string; job: HostCheckoutJob } = { key, job: { state: "running" } };
    this.jobs.set(jobId, entry);
    void hostCheckout(provider, params.slug, params.parent)
      .then(async ({ path, reuse }) => {
        entry.job = { state: "done", reused: reuse, project: await this.openProject(path) };
      })
      .catch((error: unknown) => {
        entry.job = { state: "error", error: error instanceof Error ? error.message : String(error) };
      })
      .finally(() => {
        setTimeout(() => this.jobs.delete(jobId), FINISHED_JOB_TTL_MS).unref?.();
      });
    return { jobId };
  }

  status(jobId: unknown): HostCheckoutJob {
    const entry = typeof jobId === "string" ? this.jobs.get(jobId) : undefined;
    if (!entry) throw new Error("This clone is no longer running; try again");
    return entry.job;
  }
}
