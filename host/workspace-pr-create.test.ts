import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";

type CliStub = {
  calls: string[][];
  options: unknown[];
  stdout?: string;
  fail?: boolean;
};

const { cliStubs } = vi.hoisted(() => ({ cliStubs: new Map<string, CliStub>() }));

/** Intercepts `az`/`gh` spawned through `execFile` so CLI routing is tested
 * without real binaries, shells, or PATH manipulation. Real `git` calls
 * pass through untouched, which keeps these tests portable to Windows. */
vi.mock("node:child_process", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("node:child_process")>();
  const { promisify } = await import("node:util");
  type ExecCallback = (error: Error | null, stdout: string, stderr: string) => void;
  const execFile = (
    file: string,
    args: string[],
    options: unknown,
    callback: ExecCallback,
  ): unknown => {
    const stub = cliStubs.get(
      basename(String(file)).toLowerCase().replace(/\.(cmd|exe|bat)$/, ""),
    );
    if (!stub) {
      return (actual.execFile as (...call: unknown[]) => unknown)(
        file,
        args,
        options,
        callback,
      );
    }
    stub.calls.push([...args]);
    stub.options.push(options);
    if (stub.fail) {
      const error = new Error(`Command failed: ${file} ${args.join(" ")}`);
      callback(error, "", "not logged in");
    } else {
      callback(null, stub.stdout ?? "", "");
    }
    return undefined;
  };
  // `execFile` carries a custom promisify implementation resolving
  // `{ stdout, stderr }`. A plain wrapper would lose it and resolve the raw
  // callback values instead, so reattach equivalent behavior.
  (execFile as unknown as Record<symbol, unknown>)[promisify.custom] = (
    file: string,
    args: string[],
    options: unknown,
  ) =>
    new Promise((resolve, reject) => {
      (execFile as (...call: unknown[]) => unknown)(file, args, options, ((
        error: Error | null,
        stdout: string,
        stderr: string,
      ) => (error ? reject(error) : resolve({ stdout, stderr }))) as unknown);
    });
  return {
    ...actual,
    execFile,
  };
});

import type { HostStore } from "./store";
import {
  azurePrWebUrl,
  parseAzureDevOpsRemote,
  pushRemoteFromUpstream,
  selectPushRemote,
  shouldUseShell,
  WorkspaceCommands,
} from "./workspace-commands";

const dirs: string[] = [];

afterEach(() => {
  cliStubs.clear();
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function stubCli(name: string, stub: Omit<CliStub, "calls" | "options">): CliStub {
  const entry: CliStub = { ...stub, calls: [], options: [] };
  cliStubs.set(name, entry);
  return entry;
}

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

function commands(cwd: string): WorkspaceCommands {
  const store = {
    projects: () => [{ id: "p1", cwd, name: "repo" }],
  } as unknown as HostStore;
  return new WorkspaceCommands(store, async (_id, action) => action());
}

async function createPr(cwd: string): Promise<unknown> {
  return commands(cwd).run("git_pr_create", {
    cwd,
    title: "Add login",
    body: "Details",
    base: "main",
    head: "feature",
  });
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

it("selects the push remote from pushRemote/pushDefault config", () => {
  const remotes = ["origin", "azure"];
  expect(selectPushRemote({ branchPushRemote: "azure", pushDefault: "", remotes })).toBe("azure");
  expect(selectPushRemote({ branchPushRemote: "", pushDefault: "azure", remotes })).toBe("azure");
  expect(selectPushRemote({ branchPushRemote: "", pushDefault: "", remotes })).toBe("origin");
  expect(selectPushRemote({ branchPushRemote: "", pushDefault: "", remotes: ["azure"] })).toBe("azure");
  // Unknown configured names are ignored instead of breaking PR routing.
  expect(selectPushRemote({ branchPushRemote: "missing", pushDefault: "", remotes })).toBe("origin");
  expect(selectPushRemote({ branchPushRemote: "", pushDefault: "missing", remotes })).toBe("origin");
  expect(selectPushRemote({ branchPushRemote: "", pushDefault: "", remotes: [] })).toBeNull();
});

it("requires a shell on Windows to launch the az.cmd wrapper", () => {
  expect(shouldUseShell("win32")).toBe(true);
  expect(shouldUseShell("linux")).toBe(false);
  expect(shouldUseShell("darwin")).toBe(false);
});

it("builds canonical Azure pull-request URLs", () => {
  expect(
    azurePrWebUrl(
      { organizationUrl: "https://dev.azure.com/acme", project: "shop", repo: "web" },
      12,
    ),
  ).toBe("https://dev.azure.com/acme/shop/_git/web/pullrequest/12");
});

it("encodes spaces and reserved characters in PR URLs", () => {
  expect(
    azurePrWebUrl(
      { organizationUrl: "https://dev.azure.com/acme/", project: "My Project", repo: "weird%2Fname" },
      3,
    ),
  ).toBe("https://dev.azure.com/acme/My%20Project/_git/weird%252Fname/pullrequest/3");
});

it("creates Azure PRs through az on the push remote", async () => {
  const cwd = initRepo({ origin: AZURE_URL });
  const az = stubCli("az", { stdout: '{"pullRequestId": 12}' });
  const url = await createPr(cwd);
  expect(url).toBe("https://dev.azure.com/acme/shop/_git/web/pullrequest/12");
  expect(az.calls[0]).toEqual(
    expect.arrayContaining(["--repository", "web", "--source-branch", "feature"]),
  );
});

it("uses the upstream remote, not origin, for Azure detection", async () => {
  const cwd = initRepo({ origin: GITHUB_URL, azure: AZURE_URL });
  trackUpstream(cwd, "azure");
  const az = stubCli("az", { stdout: '{"pullRequestId": 7}' });
  const url = await createPr(cwd);
  expect(url).toBe("https://dev.azure.com/acme/shop/_git/web/pullrequest/7");
  expect(az.calls[0]).toEqual(expect.arrayContaining(["--project", "shop"]));
});

it("uses the push URL when fetch and push URLs differ", async () => {
  const cwd = initRepo({ origin: GITHUB_URL });
  execFileSync("git", ["config", "remote.origin.pushurl", AZURE_URL], { cwd });
  const az = stubCli("az", { stdout: '{"pullRequestId": 5}' });
  const url = await createPr(cwd);
  expect(url).toBe("https://dev.azure.com/acme/shop/_git/web/pullrequest/5");
  expect(az.calls).toHaveLength(1);
});

it("falls back to gh when the push remote is GitHub", async () => {
  const cwd = initRepo({ origin: GITHUB_URL, azure: AZURE_URL });
  trackUpstream(cwd, "origin");
  const az = stubCli("az", { stdout: '{"pullRequestId": 1}' });
  stubCli("gh", { stdout: "https://github.com/acme/web/pull/42" });
  const url = await createPr(cwd);
  expect(url).toBe("https://github.com/acme/web/pull/42");
  expect(az.calls).toHaveLength(0);
});

it("falls back to gh when az fails", async () => {
  const cwd = initRepo({ origin: AZURE_URL });
  const az = stubCli("az", { fail: true });
  stubCli("gh", { stdout: "https://github.com/acme/web/pull/9" });
  const url = await createPr(cwd);
  expect(url).toBe("https://github.com/acme/web/pull/9");
  expect(az.calls).toHaveLength(1);
});

it("forwards exec options (incl. Windows shell flag) to the az process", async () => {
  const cwd = initRepo({ origin: AZURE_URL });
  const az = stubCli("az", { stdout: '{"pullRequestId": 12}' });
  await createPr(cwd);
  expect(az.calls).toHaveLength(1);
  const options = az.options[0] as Record<string, unknown>;
  // gitRoot resolves symlinks (e.g. /var -> /private/var on macOS), so only
  // assert the shell flag plus that a cwd was passed through.
  expect(options.shell).toBe(shouldUseShell());
  expect(typeof options.cwd).toBe("string");
});

it("honors branch.pushRemote over origin when no upstream is set", async () => {
  const cwd = initRepo({ origin: GITHUB_URL, azure: AZURE_URL });
  execFileSync("git", ["config", "branch.feature.pushRemote", "azure"], { cwd });
  const az = stubCli("az", { stdout: '{"pullRequestId": 11}' });
  const url = await createPr(cwd);
  expect(url).toBe("https://dev.azure.com/acme/shop/_git/web/pullrequest/11");
  expect(az.calls).toHaveLength(1);
});

it("honors remote.pushDefault when no upstream is set", async () => {
  const cwd = initRepo({ origin: GITHUB_URL, azure: AZURE_URL });
  execFileSync("git", ["config", "remote.pushDefault", "azure"], { cwd });
  const az = stubCli("az", { stdout: '{"pullRequestId": 13}' });
  const url = await createPr(cwd);
  expect(url).toBe("https://dev.azure.com/acme/shop/_git/web/pullrequest/13");
  expect(az.calls).toHaveLength(1);
});
