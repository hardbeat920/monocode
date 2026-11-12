/** Pure helpers shared by the app and the machine host, so both decide the
 * same way whether a folder already tracks a repository. The desktop
 * mirrors them in `src-tauri/src/git_hosts/mod.rs`. */

/** `host` is lowercased; `path` keeps its case, without slashes or `.git`. */
export type RemoteRepo = { host: string; path: string };

/** Splits a Git remote URL into its host and repository path. Understands
 * `https://`, `ssh://`, `git://` and scp-like `git@host:owner/repo` forms. */
export function parseRemoteUrl(url: string): RemoteRepo | null {
  const value = url.trim();
  let authority: string;
  let path: string;
  const scheme = /^[a-z][a-z0-9+.-]*:\/\/([^/]*)(.*)$/i.exec(value);
  if (scheme) {
    // Host without user or port. Paths stay as written, like `git remote -v`.
    authority = scheme[1].replace(/^.*@/, "").replace(/:.*$/, "");
    path = scheme[2];
  } else {
    // scp-like: [user@]host:path. A bare Windows drive or local path is not a remote.
    const match = /^([^:/]+):(.+)$/.exec(value);
    if (!match) return null;
    authority = match[1].replace(/^.*@/, "");
    if (authority.length < 2) return null;
    path = match[2];
  }
  path = stripGitSuffix(path.replace(/^\/+|\/+$/g, ""));
  if (!authority || !path) return null;
  return { host: authority.toLowerCase(), path };
}

export const stripGitSuffix = (name: string) => name.replace(/\.git$/i, "");

/** Whether a remote URL points at `slug` on `host`, ignoring case. */
export function remoteMatches(url: string, host: string, slug: string): boolean {
  const remote = parseRemoteUrl(url);
  return (
    !!remote &&
    remote.host === host.toLowerCase() &&
    remote.path.toLowerCase() === stripGitSuffix(slug).toLowerCase()
  );
}

const SLUG_PART = /^(?!-)[A-Za-z0-9_.-]{1,100}$/;
const WINDOWS_RESERVED = /^(con|prn|aux|nul|com\d|lpt\d)(\..*)?$/i;

/** The folder a repository clones into, or null when its name cannot be one
 * on every platform (`.`, `..`, a trailing dot, or a Windows device name). */
export function repoFolderName(slug: string): string | null {
  const name = stripGitSuffix(slug.split("/")[1] ?? "");
  if (!name || /^\.+$/.test(name) || name.endsWith(".") || WINDOWS_RESERVED.test(name))
    return null;
  return name;
}

/** `owner/name` when it is safe to pass to a CLI and use as a folder name. */
export function validRepoSlug(value: string): boolean {
  const parts = value.split("/");
  return (
    parts.length === 2 &&
    parts.every((part) => SLUG_PART.test(part) && !/^\.+$/.test(part)) &&
    repoFolderName(value) !== null
  );
}

/** `free`: missing or empty, so it can be cloned into. `match`: a complete
 * checkout of the repository. `taken`: anything else. */
export type CheckoutFolderState = "free" | "match" | "taken";

const MAX_CHECKOUT_CANDIDATES = 100;

/** Picks the folder for a repository: its name, or `name-2`, `name-3`… when
 * that name holds something else. A checkout of the repository at any of
 * them is reused rather than cloned again. */
export async function planCheckoutFolder(
  slug: string,
  inspect: (name: string) => CheckoutFolderState | Promise<CheckoutFolderState>,
): Promise<{ name: string; reuse: boolean }> {
  const base = repoFolderName(slug);
  if (!validRepoSlug(slug) || !base) throw new Error("Enter a repository as owner/name");
  for (let index = 1; index <= MAX_CHECKOUT_CANDIDATES; index++) {
    const name = index === 1 ? base : `${base}-${index}`;
    const state = await inspect(name);
    if (state === "match") return { name, reuse: true };
    if (state === "free") return { name, reuse: false };
  }
  throw new Error(`Too many folders named ${base}; choose another location`);
}
