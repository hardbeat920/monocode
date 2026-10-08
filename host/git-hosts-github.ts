import { execFile } from "node:child_process";
import { dirname } from "node:path";
import { promisify } from "node:util";
import type { GitHostRepo } from "../src/features/git-hosts/model/types";
import type { GitHostProvider } from "./git-hosts";

const exec = promisify(execFile);

/** Pages of 100 repositories listed, most recently pushed first. Anything
 * older can still be typed as `owner/name`. */
const REPO_PAGES = 3;
const SEARCH_RESULTS = 20;
const CLONE_TIMEOUT_MS = 15 * 60_000;
const HOST = "github.com";

const gh = (args: string[], options: { cwd?: string; timeout?: number } = {}) =>
  exec("gh", args, {
    cwd: options.cwd,
    timeout: options.timeout ?? 30_000,
    maxBuffer: 8 * 1024 * 1024,
    encoding: "utf8",
    env: { ...process.env, GH_PROMPT_DISABLED: "1", GIT_TERMINAL_PROMPT: "0", GH_PAGER: "cat" },
  });

const ghError = (error: unknown) => {
  const stderr = (error as { stderr?: string }).stderr?.trim();
  if ((error as { code?: string }).code === "ENOENT")
    return new Error("GitHub CLI (`gh`) is not installed on this machine.");
  return new Error(stderr || (error instanceof Error ? error.message : String(error)));
};

/** GitHub through the GitHub CLI, which owns sign-in and clone credentials. */
export const github: GitHostProvider = {
  id: "github",
  domain: HOST,

  async status() {
    try {
      await gh(["auth", "status", "--active", "--hostname", HOST], { timeout: 10_000 });
      return { provider: "github", installed: true, authenticated: true };
    } catch (error) {
      const installed = (error as { code?: string }).code !== "ENOENT";
      return { provider: "github", installed, authenticated: false };
    }
  },

  async repos() {
    const viewer = await viewerLogin();
    const repos: GitHostRepo[] = [];
    for (let page = 1; page <= REPO_PAGES; page++) {
      const { stdout } = await gh([
        "api",
        // Pinned to github.com so `GH_HOST` cannot point it at another server.
        "--hostname",
        HOST,
        `user/repos?affiliation=owner,collaborator,organization_member&sort=pushed&per_page=100&page=${page}`,
        "--jq",
        ".[] | {slug: .full_name, description, private, pushedAt: .pushed_at}",
      ]).catch((error: unknown) => {
        throw ghError(error);
      });
      const listed = parseGithubRepos(stdout, viewer);
      repos.push(...listed);
      if (listed.length < 100) break;
    }
    return repos;
  },

  async search(query) {
    const trimmed = query.trim();
    if (!trimmed) return [];
    const viewer = await viewerLogin();
    // Forks are excluded from search by default; GitHub repositories
    // started this way are common enough to be worth finding too.
    const { stdout } = await gh([
      "api",
      "--hostname",
      HOST,
      "--method",
      "GET",
      "search/repositories",
      "-f",
      `q=${trimmed} fork:true`,
      "-f",
      `per_page=${SEARCH_RESULTS}`,
      "-f",
      "sort=stars",
      "-f",
      "order=desc",
      "--jq",
      ".items[] | {slug: .full_name, description, private, pushedAt: .pushed_at}",
    ]).catch((error: unknown) => {
      throw ghError(error);
    });
    return parseGithubRepos(stdout, viewer);
  },

  async cloneInto(slug, dest) {
    await gh(["repo", "clone", `${HOST}/${slug}`, dest], {
      cwd: dirname(dest),
      timeout: CLONE_TIMEOUT_MS,
    }).catch((error: unknown) => {
      throw ghError(error);
    });
  },
};

/** The signed-in account's own login, so each repository can be told apart
 * from one owned by an organization or another collaborator. */
async function viewerLogin(): Promise<string> {
  const { stdout } = await gh(["api", "--hostname", HOST, "user", "--jq", ".login"]).catch(
    (error: unknown) => {
      throw ghError(error);
    },
  );
  return stdout.trim();
}

export function parseGithubRepos(lines: string, viewer: string): GitHostRepo[] {
  return lines
    .split("\n")
    .filter((line) => line.trim())
    .map((line) => {
      const repo = JSON.parse(line) as {
        slug: string;
        description: string | null;
        private: boolean;
        pushedAt: string | null;
      };
      return {
        provider: "github",
        slug: repo.slug,
        description: repo.description?.trim() || undefined,
        private: repo.private,
        pushedAt: repo.pushedAt ?? undefined,
        mine: repo.slug.split("/")[0]?.toLowerCase() === viewer.toLowerCase(),
      };
    });
}
