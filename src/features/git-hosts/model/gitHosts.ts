import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { slash } from "../../../shared/lib/paths";
import { remoteRequest, useRemoteMachines } from "../../connections/model/connections";
import type { RemoteMachine } from "../../connections/model/protocol";
import { rememberRemoteProject } from "../../connections/model/remoteProjects";
import {
  GIT_HOST_IDS,
  type CheckoutPlan,
  type GitHostId,
  type GitHostRepo,
  type GitHostStatus,
  type HostCheckoutJob,
} from "./types";

export const OPEN_CLONE_REPO_EVENT = "monocode:open-clone-repo";
export type CloneTarget = "local" | "remote";
export type CloneRepoRequest = { provider: GitHostId; target: CloneTarget };

export const openCloneRepo = (request: CloneRepoRequest) =>
  window.dispatchEvent(new CustomEvent(OPEN_CLONE_REPO_EVENT, { detail: request }));

/** Where a clone runs: this computer, or a connected machine. Each provider
 * call goes through one of these, so the dialog does not care which. */
export type CloneLocation = {
  statuses(): Promise<GitHostStatus[]>;
  repos(provider: GitHostId): Promise<GitHostRepo[]>;
  plan(provider: GitHostId, slug: string, parent: string): Promise<CheckoutPlan>;
  /** Opens an existing checkout or clones one, resolving to the project's
   * rail key. Aborting stops waiting; a clone already running finishes. */
  checkout(provider: GitHostId, slug: string, parent: string, signal: AbortSignal): Promise<string>;
};

const slashPlan = (plan: CheckoutPlan): CheckoutPlan => ({ ...plan, path: slash(plan.path) });

export const localCloneLocation: CloneLocation = {
  statuses: () => invoke<GitHostStatus[]>("git_host_statuses"),
  repos: (provider) => invoke<GitHostRepo[]>("git_host_repos", { provider }),
  plan: (provider, slug, parent) =>
    invoke<CheckoutPlan>("git_host_checkout_plan", { provider, slug, parent }).then(slashPlan),
  checkout: (provider, slug, parent) =>
    invoke<CheckoutPlan>("git_host_checkout", { provider, slug, parent }).then(
      (plan) => slashPlan(plan).path,
    ),
};

export const localDefaultCloneParent = () =>
  invoke<string>("git_host_default_parent").then(slash);

const CHECKOUT_POLL_MS = 1_000;

const aborted = () => new DOMException("Cancelled", "AbortError");

const wait = (ms: number, signal: AbortSignal) =>
  new Promise<void>((resolve, reject) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener(
      "abort",
      () => {
        clearTimeout(timer);
        reject(aborted());
      },
      { once: true },
    );
  });

/** A connected machine. Its checkout is a job the machine runs; the project
 * is only added to the rail once it finishes and nobody cancelled. */
export function remoteCloneLocation(machine: Pick<RemoteMachine, "id" | "environmentId">): CloneLocation {
  return {
    statuses: () => remoteRequest<GitHostStatus[]>(machine.id, "gitHosts.status"),
    repos: (provider) => remoteRequest<GitHostRepo[]>(machine.id, "gitHosts.repos", { provider }),
    plan: (provider, slug, parent) =>
      remoteRequest<CheckoutPlan>(machine.id, "gitHosts.checkoutPlan", { provider, slug, parent }),
    checkout: async (provider, slug, parent, signal) => {
      const { jobId } = await remoteRequest<{ jobId: string }>(machine.id, "gitHosts.checkoutStart", {
        provider,
        slug,
        parent,
      });
      for (;;) {
        if (signal.aborted) throw aborted();
        const job = await remoteRequest<HostCheckoutJob>(machine.id, "gitHosts.checkoutStatus", { jobId });
        if (signal.aborted) throw aborted();
        if (job.state === "done") return rememberRemoteProject(machine.environmentId, job.project).key;
        if (job.state === "error") throw new Error(job.error);
        await wait(CHECKOUT_POLL_MS, signal);
      }
    },
  };
}

const REPOS_FRESH_MS = 5 * 60_000;
const repoCache = new Map<string, { at: number; repos: Promise<GitHostRepo[]> }>();

/** Repositories for a location, kept for a few minutes so reopening the
 * dialog is instant. */
export function cachedRepos(
  key: string,
  location: CloneLocation,
  provider: GitHostId,
): Promise<GitHostRepo[]> {
  const cacheKey = `${key}:${provider}`;
  const cached = repoCache.get(cacheKey);
  if (cached && Date.now() - cached.at < REPOS_FRESH_MS) return cached.repos;
  const repos = location.repos(provider);
  repoCache.set(cacheKey, { at: Date.now(), repos });
  repos.catch(() => repoCache.delete(cacheKey));
  return repos;
}

const parentKey = (location: string) => `monocode.clone-parent.v1:${location}`;

/** The folder last cloned into at a location (`local` or a machine's environment id). */
export function rememberedCloneParent(location: string): string | undefined {
  try {
    return localStorage.getItem(parentKey(location)) || undefined;
  } catch {
    return undefined;
  }
}

export function rememberCloneParent(location: string, parent: string) {
  try {
    localStorage.setItem(parentKey(location), parent);
  } catch {
    // Only a default for next time.
  }
}

export type GitHostAvailability = {
  /** Providers signed in on this computer. */
  local: GitHostId[];
  /** Machines where each provider is signed in. */
  remote: Partial<Record<GitHostId, string[]>>;
};

/** Signed-in providers from a status reply; anything malformed counts as none. */
export function signedInProviders(statuses: unknown): GitHostId[] {
  if (!Array.isArray(statuses)) return [];
  return statuses
    .filter(
      (status): status is GitHostStatus =>
        !!status &&
        typeof status === "object" &&
        (status as GitHostStatus).authenticated === true &&
        GIT_HOST_IDS.includes((status as GitHostStatus).provider),
    )
    .map((status) => status.provider);
}

const AVAILABILITY_FRESH_MS = 15_000;
const AVAILABILITY_CHANGE = "monocode:git-host-availability";
let localProviders: GitHostId[] = [];
/** Last answer from each machine. A failed check keeps it, so one dropped
 * request does not hide a machine mid-dialog. */
const machineProviders = new Map<string, GitHostId[]>();
let checkedAt = 0;
let checkedKey: string | undefined;
let inFlight: string | undefined;

function snapshot(): GitHostAvailability {
  const remote: GitHostAvailability["remote"] = {};
  for (const [machine, providers] of machineProviders)
    for (const provider of providers) (remote[provider] ??= []).push(machine);
  return { local: localProviders, remote };
}

const publish = () => window.dispatchEvent(new Event(AVAILABILITY_CHANGE));

/** Checks this computer and each machine independently, publishing each
 * answer as it arrives so a slow machine never holds up the rest. */
function refreshAvailability(machines: readonly RemoteMachine[], force: boolean) {
  const key = machines.map((machine) => machine.id).join("\n");
  if (inFlight === key) return;
  if (!force && checkedKey === key && Date.now() - checkedAt < AVAILABILITY_FRESH_MS) return;
  inFlight = key;
  checkedKey = key;
  checkedAt = Date.now();
  const ids = new Set(machines.map((machine) => machine.id));
  for (const id of machineProviders.keys()) if (!ids.has(id)) machineProviders.delete(id);
  publish();
  const local = localCloneLocation.statuses().then(
    (statuses) => {
      localProviders = signedInProviders(statuses);
      publish();
    },
    () => undefined,
  );
  const remote = machines.map((machine) =>
    remoteCloneLocation(machine).statuses().then(
      (statuses) => {
        // Ignore answers for machines removed while this check ran.
        if (checkedKey !== key) return;
        machineProviders.set(machine.id, signedInProviders(statuses));
        publish();
      },
      () => undefined,
    ),
  );
  void Promise.allSettled([local, ...remote]).then(() => {
    if (inFlight === key) inFlight = undefined;
  });
}

/** Which providers can clone here and on which machines. Checked in the
 * background when first used, when machines change, and on `refresh`;
 * callers render with the last known answer. */
export function useGitHostAvailability(): GitHostAvailability & { refresh(): void } {
  const { machines } = useRemoteMachines();
  const [value, setValue] = useState(snapshot);
  const key = machines.map((machine) => machine.id).join("\n");
  useEffect(() => {
    const update = () => setValue(snapshot());
    update();
    window.addEventListener(AVAILABILITY_CHANGE, update);
    return () => window.removeEventListener(AVAILABILITY_CHANGE, update);
  }, []);
  // A changed machine list is checked right away, even mid-check.
  useEffect(() => refreshAvailability(machines, checkedKey !== key), [key]);
  const refresh = useCallback(() => refreshAvailability(machines, false), [key]);
  return { ...value, refresh };
}
