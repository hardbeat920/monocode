/** Hosting services a project can be cloned from. Each one has a provider on
 * this computer (`src-tauri/src/git_hosts`) and on connected machines
 * (`host/git-hosts`); the UI only knows them through this id. */
export const GIT_HOST_IDS = ["github"] as const;
export type GitHostId = (typeof GIT_HOST_IDS)[number];

export const isGitHostId = (value: unknown): value is GitHostId =>
  typeof value === "string" && (GIT_HOST_IDS as readonly string[]).includes(value);

export type GitHostStatus = {
  provider: GitHostId;
  /** The provider's CLI is on PATH. */
  installed: boolean;
  /** Signed in, so its repositories can be listed and cloned. */
  authenticated: boolean;
};

export type GitHostRepo = {
  provider: GitHostId;
  /** `owner/name`, the form the provider's CLI clones from. */
  slug: string;
  description?: string;
  private: boolean;
  pushedAt?: string;
};

/** Where a checkout ends up before anything is cloned. */
export type CheckoutPlan = {
  path: string;
  /** A folder with the repository's name already tracks it, so it is opened as is. */
  reuse: boolean;
};

export type CheckoutResult = CheckoutPlan;

/** Remote checkouts outlive one RPC (30s), so the host runs them as jobs. */
export type HostCheckoutJob =
  | { state: "running" }
  | { state: "done"; reused: boolean; project: { id: string; cwd: string; name: string } }
  | { state: "error"; error: string };
