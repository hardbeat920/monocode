import { execFileSync } from "node:child_process";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, expect, it } from "vitest";
import type { HostStore } from "./store";
import {
  azurePrWebUrl,
  parseAzureDevOpsRemote,
  pushRemoteFromUpstream,
  WorkspaceCommands,
} from "./workspace-commands";

const dirs: string[] = [];
let savedPath = "";

beforeEach(() => {
  savedPath = process.env.PATH ?? "";
});

afterEach(() => {
  process.env.PATH = savedPath;
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function tempDir(prefix: string): string {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  dirs.push(dir);
  return dir;
}

function initRepo(remotes: Record<string, string>): string {
  const cwd = tempDir("monocode-host-pr-");
  const git = (...args: string[]) => execFileSync("git", args, { cwd });
  git("init");
  git("checkout", "-b", "feature");
  git("config", "user.name", "Test");
  git("config", "user.email", "test@example.com");
  writeFileSync(join(cwd, "file.txt"), "initial");
  git("add", "file.txt");
  git("commit", "-m", "initial");
  for (const [name, url] of Object.entries(remotes)) git("remote", "add", name, url);
  return cwd;
}

/** Points the branch at a remote-tracking ref without pushing. */
function trackUpstream(cwd: string, remote: string, branch = "feature") {
  const git = (...args: string[]) => execFileSync("git", args, { cwd });
  git("update-ref", `refs/remotes/${remote}/${branch}`, "HEAD");
  git("branch", "--set-upstream-to", `${remote}/${branch}`);
}

function fakeBin(scripts: Record<string, string>): void {
  const bin = tempDir("monocode-host-bin-");
  for (const [name, body] of Object.entries(scripts)) {
    const path = join(bin, name);
    writeFileSync(path, `#!/bin/sh\n${body}\n`);
    chmodSync(path, 0o755);
  }
  process.env.PATH = `${bin}${savedPath ? `:${savedPath}` : ""}`;
}

function commands(cwd: string): WorkspaceCommands {
  const store = {
    projects: () => [{ id: "p1", cwd, name: "repo" }],
  } as unknown as HostStore;
  return new WorkspaceCommands(store, async (_id, action) => action());
}

const AZURE_URL = "https://dev.azure.com/acme/shop/_git/web";
const GITHUB_URL = "https://github.com/acme/web.git";

it("parses hosted, legacy, SSH and on-premises Azure remotes", () => {
  expect(parseAzureDevOpsRemote(`${AZURE_URL}.git`)).toEqual({
    organizationUrl: "https://dev.azure.com/acme",
    project: "shop",
    repo: "web",
  });
  expect(parseAzureDevOpsRemote("https://me@dev.azure.com/acme/shop/_git/web")).toEqual({
    organizationUrl: "https://dev.azure.com/acme",
    project: "shop",
    repo: "web",
  });
  expect(parseAzureDevOpsRemote("https://acme.visualstudio.com/shop/_git/web.git")).toEqual({
    organizationUrl: "https://dev.azure.com/acme",
    project: "shop",
    repo: "web",
  });
  expect(parseAzureDevOpsRemote("git@ssh.dev.azure.com:v3/acme/shop/web")).toEqual({
    organizationUrl: "https://dev.azure.com/acme",
    project: "shop",
    repo: "web",
  });
  expect(parseAzureDevOpsRemote("git@tfs.contoso.com:DefaultCollection/shop/_git/web")).toEqual({
    organizationUrl: "https://tfs.contoso.com/defaultcollection",
    project: "shop",
    repo: "web",
  });
  expect(parseAzureDevOpsRemote("https://tfs.contoso.com/tfs/DefaultCollection/shop/_git/web")).toEqual({
    organizationUrl: "https://tfs.contoso.com/tfs/defaultcollection",
    project: "shop",
    repo: "web",
  });
});

it("rejects non-Azure remotes", () => {
  expect(parseAzureDevOpsRemote(GITHUB_URL)).toBeNull();
  expect(parseAzureDevOpsRemote("git@github.com:acme/web.git")).toBeNull();
  expect(parseAzureDevOpsRemote("")).toBeNull();
  expect(parseAzureDevOpsRemote("not a remote")).toBeNull();
});

it("resolves the push remote from the upstream ref", () => {
  expect(pushRemoteFromUpstream("origin/main")).toBe("origin");
  expect(pushRemoteFromUpstream("azure/feature/nested")).toBe("azure");
  expect(pushRemoteFromUpstream("main")).toBeNull();
  expect(pushRemoteFromUpstream("")).toBeNull();
});

it("builds canonical Azure pull-request URLs", () => {
  expect(
    azurePrWebUrl(
      { organizationUrl: "https://dev.azure.com/acme", project: "shop", repo: "web" },
      12,
    ),
  ).toBe("https://dev.azure.com/acme/shop/_git/web/pullrequest/12");
});

it("creates Azure PRs through az on the push remote", async () => {
  const cwd = initRepo({ origin: AZURE_URL });
  fakeBin({ az: `echo '{"pullRequestId": 12}'` });
  const url = await commands(cwd).run("git_pr_create", {
    cwd,
    title: "Add login",
    body: "Details",
    base: "main",
    head: "feature",
  });
  expect(url).toBe("https://dev.azure.com/acme/shop/_git/web/pullrequest/12");
});

it("uses the upstream remote, not origin, for Azure detection", async () => {
  const cwd = initRepo({ origin: GITHUB_URL, azure: AZURE_URL });
  trackUpstream(cwd, "azure");
  const argsFile = join(tempDir("monocode-host-args-"), "az-args.txt");
  fakeBin({ az: `echo "$@" > "${argsFile}"\necho '{"pullRequestId": 7}'` });
  const url = await commands(cwd).run("git_pr_create", {
    cwd,
    title: "Add login",
    body: "Details",
    base: "main",
    head: "feature",
  });
  expect(url).toBe("https://dev.azure.com/acme/shop/_git/web/pullrequest/7");
  await expect(readFile(argsFile, "utf8")).resolves.toContain("--repository web");
});

it("falls back to gh when the push remote is GitHub", async () => {
  const cwd = initRepo({ origin: GITHUB_URL, azure: AZURE_URL });
  trackUpstream(cwd, "origin");
  fakeBin({ gh: `echo 'https://github.com/acme/web/pull/42'` });
  const url = await commands(cwd).run("git_pr_create", {
    cwd,
    title: "Add login",
    body: "Details",
    base: "main",
    head: "feature",
  });
  expect(url).toBe("https://github.com/acme/web/pull/42");
});

it("falls back to gh when az fails", async () => {
  const cwd = initRepo({ origin: AZURE_URL });
  fakeBin({ az: `echo 'not logged in' >&2\nexit 1`, gh: `echo 'https://github.com/acme/web/pull/9'` });
  const url = await commands(cwd).run("git_pr_create", {
    cwd,
    title: "Add login",
    body: "Details",
    base: "main",
    head: "feature",
  });
  expect(url).toBe("https://github.com/acme/web/pull/9");
});
