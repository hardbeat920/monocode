import { execFileSync, spawn } from "node:child_process";
import {
  chmodSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { expect, it, vi } from "vitest";
import { HostStore } from "./store";
import { HostEngine } from "./engine";
import { WorkspaceCommands } from "./workspace-commands";
import { hostCow, resolveHostWorkspaceAsync, type HostCow } from "./cow";
import type { HostProvider } from "./providers";

it("keeps healthy project file and Git RPCs usable after another project disappears", async () => {
  const folder = realpathSync.native(mkdtempSync(join(tmpdir(), "monocode-cow-roots-")));
  const healthy = join(folder, "healthy");
  const missing = join(folder, "missing");
  mkdirSync(healthy);
  execFileSync("git", ["init", "-q", "-b", "main"], { cwd: healthy });
  execFileSync("git", ["config", "user.name", "Test"], { cwd: healthy });
  execFileSync("git", ["config", "user.email", "test@example.invalid"], { cwd: healthy });
  writeFileSync(join(healthy, "file.txt"), "healthy\n");
  execFileSync("git", ["add", "."], { cwd: healthy });
  execFileSync("git", ["commit", "-qm", "base"], { cwd: healthy });
  mkdirSync(missing);
  const store = new HostStore(join(folder, "host.db"));
  store.addProject(missing, "Missing");
  store.addProject(healthy, "Healthy");
  rmSync(missing, { recursive: true });
  const commands = new WorkspaceCommands(store, async (_id, action) => action());
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  try {
    expect(await commands.run("read_text_file", { path: join(healthy, "file.txt") })).toBe("healthy\n");
    await commands.run("write_text_file", { path: join(healthy, "file.txt"), original: "healthy\n", content: "edited\n" });
    expect(await commands.run("git_diff_index", { cwd: healthy })).toMatchObject({ branch: "main" });
    const outside = join(folder, "unowned");
    mkdirSync(outside);
    writeFileSync(join(outside, "file.txt"), "unowned\n");
    await expect(commands.run("read_text_file", { path: join(outside, "file.txt") })).rejects.toThrow(/outside/);
  } finally {
    error.mockRestore();
    store.close();
    rmSync(folder, { recursive: true, force: true });
  }
});

it("authorizes native copies asynchronously while another helper holds the registry lock", async () => {
  const folder = realpathSync.native(mkdtempSync(join(tmpdir(), "monocode-cow-async-")));
  const cwd = join(folder, "project");
  mkdirSync(cwd);
  const git = (...args: string[]) => execFileSync("git", args, { cwd, encoding: "utf8" });
  git("init", "-q", "-b", "main");
  git("config", "user.name", "Test");
  git("config", "user.email", "test@example.invalid");
  writeFileSync(join(cwd, "file.txt"), "base\n");
  git("add", ".");
  git("commit", "-qm", "base");
  const store = new HostStore(join(folder, "host.db"));
  const project = store.addProject(cwd, "Project");
  const provider: HostProvider = {
    send: async () => {}, cancel: async () => {}, stop: async () => {},
    bind: () => {}, approve: () => {}, answer: () => {},
  };
  const engine = new HostEngine(store, { codex: provider });
  let lock: ReturnType<typeof spawn> | undefined;
  let listing: Promise<unknown> | undefined;
  try {
    const capability = await hostCow<{ supported: boolean }>(store, "cow_capability", { cwd });
    if (!capability.supported) {
      expect(process.env.MONOCODE_REQUIRE_COW).not.toBe("1");
      return;
    }
    const copy = await hostCow<HostCow>(store, "cow_create", { cwd, sessionId: "async-owner" });
    lock = spawn("python3", ["-c", "import fcntl,sys; f=open(sys.argv[1],'r+'); fcntl.flock(f,fcntl.LOCK_EX); print('locked',flush=True); sys.stdin.read()", join(store.isolationDir, ".lock")]);
    await new Promise<void>((accept, reject) => {
      lock!.stdout!.once("data", () => accept());
      lock!.once("error", reject);
      lock!.once("exit", () => reject(new Error("Lock holder exited before acquiring the lock")));
    });
    listing = hostCow(store, "cow_list", { cwd });
    let listingSettled = false;
    void listing.then(() => { listingSettled = true; }, () => { listingSettled = true; });
    const transaction = vi.spyOn(store, "transaction");
    const creating = engine.commandAsync({ type: "create", commandId: "async-create", projectId: project.id,
      harness: "codex", model: "codex:test", runtimeMode: "supervised", cowId: copy.id });
    expect(transaction).not.toHaveBeenCalled();
    let heartbeat = false;
    await new Promise<void>((accept) => setImmediate(() => { heartbeat = true; accept(); }));
    expect(heartbeat).toBe(true);
    expect(await creating).toMatchObject({ sessionId: "async-owner" });
    expect(listingSettled).toBe(false);
    transaction.mockRestore();
    const commands = new WorkspaceCommands(store, async (_id, action) => action());
    expect(await commands.run("read_text_file", { path: join(copy.path, "file.txt") })).toBe("base\n");
    renameSync(copy.path, `${copy.path}-original`);
    mkdirSync(copy.path);
    mkdirSync(join(copy.path, ".git"));
    writeFileSync(join(copy.path, "file.txt"), "replacement\n");
    await expect(commands.run("read_text_file", { path: join(copy.path, "file.txt") })).rejects.toThrow(/ownership changed/);
    await expect(engine.commandAsync({ type: "send", commandId: "replaced-send", sessionId: "async-owner", text: "work" })).rejects.toThrow(/replaced|ownership|unavailable/);
  } finally {
    if (lock) {
      lock.stdin?.end();
      await new Promise<void>((accept) => lock!.exitCode !== null ? accept() : lock!.once("close", () => accept()));
    }
    await listing?.catch(() => {});
    await engine.close();
    store.close();
    rmSync(folder, { recursive: true, force: true });
  }
}, 30_000);

it("keeps ordinary projects usable when one legacy copy cannot upgrade", async () => {
  const folder = mkdtempSync(join(tmpdir(), "monocode-cow-upgrade-"));
  const cwd = join(folder, "project");
  const ordinary = join(folder, "ordinary");
  mkdirSync(cwd);
  const git = (...args: string[]) =>
    execFileSync("git", args, { cwd, encoding: "utf8" });
  git("init", "-b", "main");
  git("config", "user.name", "Test");
  git("config", "user.email", "test@example.invalid");
  writeFileSync(join(cwd, "file.txt"), "base\n");
  git("add", ".");
  git("commit", "-m", "base");
  git("clone", cwd, ordinary);
  const store = new HostStore(join(folder, "host.db"));
  store.addProject(cwd, "Project");
  store.addProject(ordinary, "Ordinary");
  const commands = new WorkspaceCommands(store, async (_id, action) =>
    action(),
  );
  try {
    const cap = await hostCow<{ supported: boolean }>(store, "cow_capability", {
      cwd,
    });
    if (!cap.supported) {
      expect(process.env.MONOCODE_REQUIRE_COW).not.toBe("1");
      return;
    }
    const legacy = (await commands.run("cow_create", {
      cwd,
      sessionId: "legacy-copy",
    })) as HostCow;
    const receiptPath = join(store.isolationDir, legacy.id, "workspace.json");
    const receipt = JSON.parse(readFileSync(receiptPath, "utf8"));
    receipt.gitConfigVersion = 1;
    receipt.sourceCwd = join(folder, "removed-source");
    writeFileSync(receiptPath, JSON.stringify(receipt));
    const healthy = (await commands.run("cow_create", {
      cwd,
      sessionId: "healthy-copy",
    })) as HostCow;
    expect(await commands.run("cow_list", { cwd })).toEqual([
      expect.objectContaining({ id: healthy.id }),
    ]);
    expect(
      await commands.run("read_text_file", { path: join(cwd, "file.txt") }),
    ).toBe("base\n");
    expect(
      await commands.run("read_text_file", {
        path: join(ordinary, "file.txt"),
      }),
    ).toBe("base\n");
    expect(await commands.run("git_diff_index", { cwd })).toMatchObject({
      branch: "main",
      files: [],
    });
    expect(
      await commands.run("git_diff_index", { cwd: ordinary }),
    ).toMatchObject({ branch: "main", files: [] });
    await expect(
      commands.run("read_text_file", { path: join(legacy.path, "file.txt") }),
    ).rejects.toThrow(/outside/);
    expect(readFileSync(join(legacy.path, "file.txt"), "utf8")).toBe("base\n");
  } finally {
    store.close();
    rmSync(folder, { recursive: true, force: true });
  }
}, 30_000);

it("runs providers in native clones, integrates session edits, and guards ownership and cleanup", async () => {
  const folder = mkdtempSync(join(tmpdir(), "monocode-host-cow-"));
  const cwd = join(folder, "project");
  mkdirSync(cwd);
  const git = (...args: string[]) =>
    execFileSync("git", args, { cwd, encoding: "utf8" });
  git("init", "-b", "main");
  git("config", "user.name", "Test");
  git("config", "user.email", "test@example.com");
  writeFileSync(join(cwd, "file.txt"), "first\nsecond\nthird\n");
  git("add", ".");
  git("commit", "-m", "base");
  writeFileSync(join(cwd, "file.txt"), "first\ninherited\nthird\n");
  const store = new HostStore(join(folder, "host.db"));
  const project = store.addProject(cwd, "Project");
  let executionCwd: string | undefined;
  let finishTurn: () => void = () => {};
  const turn = new Promise<void>((resolve) => {
    finishTurn = resolve;
  });
  const provider: HostProvider = {
    send: async (input) => {
      executionCwd = input.cwd;
      writeFileSync(
        join(input.cwd, "file.txt"),
        "first\ninherited\nthird\nsession\n",
      );
      await turn;
    },
    cancel: async () => {
      finishTurn();
    },
    stop: async () => {},
    bind: () => {},
    approve: () => {},
    answer: () => {},
  };
  const engine = new HostEngine(store, { codex: provider });
  const commands = new WorkspaceCommands(store, (id, action) =>
    engine.withIdleProject(id, action),
  );
  let copy: HostCow | undefined;
  try {
    await expect(
      commands.run("cow_create", { cwd: folder, sessionId: "outside" }),
    ).rejects.toThrow();
    const capability = await hostCow<{ supported: boolean; reason?: string }>(
      store,
      "cow_capability",
      { cwd },
    );
    if (!capability.supported) {
      expect(capability.reason).toBeTruthy();
      expect(process.env.MONOCODE_REQUIRE_COW).not.toBe("1");
      return;
    }
    copy = (await commands.run("cow_create", {
      cwd,
      sessionId: "owner",
    })) as HostCow;
    expect(copy.sessionId).toBe("owner");
    expect(await resolveHostWorkspaceAsync(store, cwd, copy.path)).toBe(
      copy.path,
    );
    expect(
      await commands.run("read_text_file", {
        path: join(copy.path, "file.txt"),
      }),
    ).toBe("first\ninherited\nthird\n");
    const create = {
      type: "create" as const,
      commandId: "cow-owner",
      projectId: project.id,
      harness: "codex" as const,
      model: "codex:test",
      runtimeMode: "supervised" as const,
      cowId: copy.id,
    };
    expect((await engine.commandAsync(create)).sessionId).toBe("owner");
    expect((await engine.commandAsync(create)).sessionId).toBe("owner");
    await expect(engine.commandAsync({ ...create, commandId: "duplicate" })).rejects.toThrow(
      /already belongs/,
    );
    await expect(
      resolveHostWorkspaceAsync(store, cwd, folder),
    ).rejects.toThrow();
    await engine.commandAsync({
      type: "send",
      commandId: "edit",
      sessionId: "owner",
      text: "Edit the isolated checkout",
    });
    await vi.waitFor(() => expect(executionCwd).toBe(copy?.path));
    expect(readFileSync(join(cwd, "file.txt"), "utf8")).toBe(
      "first\ninherited\nthird\n",
    );
    await expect(
      commands.run("cow_remove", { cwd, cowId: copy.id, force: true }),
    ).rejects.toThrow(/running.*sessions?/);
    finishTurn();
    await vi.waitFor(() => expect(store.session("owner").status).toBe("idle"));
    expect(
      await commands.run("cow_status", { cwd: copy.path, cowId: copy.id }),
    ).toMatchObject({
      files: [
        { relative: "file.txt", status: "M", additions: 1, deletions: 0 },
      ],
    });
    expect(
      await commands.run("cow_file_diff", {
        cwd: copy.path,
        cowId: copy.id,
        relative: "file.txt",
      }),
    ).toMatchObject({
      original: "first\ninherited\nthird\n",
      current: "first\ninherited\nthird\nsession\n",
    });
    const integrated = await commands.run("cow_apply", {
      cwd,
      cowId: copy.id,
      toCwd: cwd,
    });
    expect(integrated).toMatchObject({
      files: ["file.txt"],
      alreadyApplied: 0,
    });
    expect(readFileSync(join(cwd, "file.txt"), "utf8")).toBe(
      "first\ninherited\nthird\nsession\n",
    );
    expect(
      await commands.run("cow_apply", { cwd, cowId: copy.id, toCwd: cwd }),
    ).toMatchObject({ alreadyApplied: 1 });
    expect(await commands.run("cow_list", { cwd })).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ id: copy.id, sessionIds: ["owner"] }),
      ]),
    );
    await expect(
      commands.run("cow_remove", { cwd, cowId: copy.id, force: true }),
    ).rejects.toThrow("Sessions still use");
    expect(
      await commands.run("cow_remove", {
        cwd,
        cowId: copy.id,
        force: true,
        keepSessions: true,
      }),
    ).toMatchObject({
      sessionIds: ["owner"],
      projectCwd: realpathSync.native(cwd),
    });
    expect(store.session("owner").session).toMatchObject({
      cowId: undefined,
      worktreeRemoved: true,
    });
    expect(() =>
      engine.command({
        type: "send",
        commandId: "stale",
        sessionId: "owner",
        text: "edit",
      }),
    ).toThrow(/removed/);
  } finally {
    finishTurn();
    await engine.close();
    if (copy)
      await hostCow(store, "cow_remove", {
        cwd,
        cowId: copy.id,
        force: true,
      }).catch(() => {});
    store.close();
    rmSync(folder, { recursive: true, force: true });
  }
}, 30_000);

it("uses ordinary Git commands and preserves local history on clone removal", async () => {
  const folder = mkdtempSync(join(tmpdir(), "monocode-cow-git-"));
  const cwd = join(folder, "project");
  const remote = join(folder, "origin.git");
  mkdirSync(cwd);
  const gitAt = (path: string, ...args: string[]) =>
    execFileSync("git", args, { cwd: path, encoding: "utf8" }).trim();
  const git = (...args: string[]) => gitAt(cwd, ...args);
  git("init", "-b", "main");
  git("config", "user.name", "Test");
  git("config", "user.email", "test@example.com");
  writeFileSync(join(cwd, "file.txt"), "base\n");
  writeFileSync(join(cwd, ".gitignore"), "deps/\n");
  git("add", ".");
  git("commit", "-m", "base");
  git("init", "--bare", remote);
  git("remote", "add", "origin", remote);
  git("push", "-u", "origin", "main");
  git("symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main");
  git("branch", "alternate");
  git("tag", "baseline");
  const originalHead = git("rev-parse", "HEAD");
  writeFileSync(join(cwd, "file.txt"), "inherited\n");
  mkdirSync(join(cwd, "deps"));
  writeFileSync(join(cwd, "deps", "runtime"), "ignored\n");
  const authMarker = join(folder, "ssh-ran");
  const ssh = join(folder, "fixture-ssh");
  writeFileSync(
    ssh,
    `#!/bin/sh\nprintf called >> '${authMarker}'\nfor arg do last="$arg"; done\ncase "$*" in *-G*) exit 0 ;; esac\nexec /bin/sh -c "$last"\n`,
  );
  chmodSync(ssh, 0o755);
  git("config", "core.sshCommand", ssh);
  git("config", "credential.password", "dummy-fixture");
  git("config", "http.extraHeader", "Authorization: dummy-fixture");
  git("remote", "set-url", "origin", `ssh://fixture${remote}`);
  const globalIgnore = join(folder, "global-ignore");
  writeFileSync(globalIgnore, ".DS_Store\n");
  const authInclude = join(folder, "auth-config");
  writeFileSync(
    authInclude,
    `[core]\nexcludesFile = ${globalIgnore}\n[credential]\nusername = fixture-user\nhelper =\nhelper = fixture-helper with spaces\n`,
  );
  const globalConfig = join(folder, "global-config");
  writeFileSync(
    globalConfig,
    `[includeIf "gitdir:${git("rev-parse", "--absolute-git-dir")}"]\npath = ${authInclude}\n`,
  );
  vi.stubEnv("GIT_CONFIG_GLOBAL", globalConfig);
  writeFileSync(join(cwd, ".DS_Store"), "ignored globally\n");
  const hookMarker = join(folder, "hook-ran");
  const hook = join(cwd, ".git", "hooks", "pre-commit");
  writeFileSync(hook, `#!/bin/sh\ntouch '${hookMarker}'\nexit 1\n`);
  chmodSync(hook, 0o755);
  const store = new HostStore(join(folder, "host.db"));
  store.addProject(cwd, "Project");
  const commands = new WorkspaceCommands(store, async (_id, action) =>
    action(),
  );
  let copy: HostCow | undefined;
  try {
    const capability = await hostCow<{ supported: boolean }>(
      store,
      "cow_capability",
      { cwd },
    );
    if (!capability.supported) {
      expect(process.env.MONOCODE_REQUIRE_COW).not.toBe("1");
      return;
    }
    copy = (await commands.run("cow_create", {
      cwd,
      sessionId: "normal-flow",
      base: "alternate",
    })) as HostCow;
    const run = (command: string, args: Record<string, unknown> = {}) =>
      commands.run(command, { cwd: copy!.path, ...args });
    expect(() => readFileSync(authMarker)).toThrow();
    expect(() =>
      gitAt(copy!.path, "config", "--local", "--get", "credential.password"),
    ).toThrow();
    expect(() =>
      gitAt(copy!.path, "config", "--local", "--get", "http.extraHeader"),
    ).toThrow();
    expect(gitAt(copy.path, "config", "--get", "core.sshCommand")).toBe(ssh);
    expect(readFileSync(join(copy.path, ".DS_Store"), "utf8")).toBe("ignored globally\n");
    expect(gitAt(copy.path, "check-ignore", ".DS_Store")).toBe(".DS_Store");
    expect(gitAt(copy.path, "config", "--get", "credential.username")).toBe(
      "fixture-user",
    );
    expect(
      gitAt(copy.path, "config", "--get-all", "credential.helper").split("\n"),
    ).toContain("fixture-helper with spaces");
    expect(copy.branch).toBe("mc/normalfl");
    expect(await run("git_diff_index")).toMatchObject({
      branch: copy.branch,
      defaultBranch: "main",
    });
    expect(gitAt(copy.path, "show-ref", "refs/tags/baseline")).toContain(
      originalHead,
    );
    expect(gitAt(copy.path, "config", "--get", "branch.main.merge")).toBe(
      "refs/heads/main",
    );
    expect(() => gitAt(copy!.path, "rev-parse", "@{upstream}")).toThrow();
    // Simulate an existing pre-auth-fix workspace without touching its files/index/HEAD.
    const receiptPath = join(store.isolationDir, copy.id, "workspace.json");
    const receipt = JSON.parse(readFileSync(receiptPath, "utf8"));
    delete receipt.gitConfigVersion;
    writeFileSync(receiptPath, JSON.stringify(receipt));
    gitAt(copy.path, "config", "--unset-all", "core.sshCommand");
    gitAt(copy.path, "config", "credential.username", "cow-override");
    git("remote", "add", "relative", "../relative.git");
    gitAt(copy.path, "remote", "add", "relative", "../relative.git");
    git("remote", "add", "multiple", "first:repo");
    git("config", "--add", "remote.multiple.url", "second:repo");
    const multi = (await commands.run("cow_create", {
      cwd,
      sessionId: "auth-urls",
    })) as HostCow;
    expect(
      gitAt(multi.path, "config", "--get-all", "remote.multiple.url").split(
        "\n",
      ),
    ).toEqual(["first:repo", "second:repo"]);
    expect(
      gitAt(multi.path, "config", "--get-all", "remote.multiple.pushurl").split(
        "\n",
      ),
    ).toEqual(["first:repo", "second:repo"]);
    await commands.run("cow_remove", { cwd, cowId: multi.id, force: true });
    const beforeUpgradeHead = gitAt(copy.path, "rev-parse", "HEAD");
    const beforeUpgradeIndex = readFileSync(join(copy.path, ".git", "index"));
    await commands.run("cow_list", { cwd });
    expect(gitAt(copy.path, "config", "--get", "core.sshCommand")).toBe(ssh);
    expect(gitAt(copy.path, "config", "--get", "credential.username")).toBe(
      "cow-override",
    );
    expect(resolve(gitAt(copy.path, "remote", "get-url", "relative"))).toBe(
      join(realpathSync.native(cwd), "..", "relative.git"),
    );
    expect(gitAt(copy.path, "rev-parse", "HEAD")).toBe(beforeUpgradeHead);
    expect(readFileSync(join(copy.path, ".git", "index"))).toEqual(
      beforeUpgradeIndex,
    );
    expect(readFileSync(join(copy.path, "file.txt"), "utf8")).toBe(
      "inherited\n",
    );
    expect(JSON.parse(readFileSync(receiptPath, "utf8")).gitConfigVersion).toBe(
      2,
    );
    expect(() => readFileSync(authMarker)).toThrow();
    writeFileSync(join(copy.path, "session.txt"), "session\n");
    await run("git_stage_file", { relative: "session.txt" });
    await run("git_unstage_file", { relative: "session.txt" });
    await run("git_stage_contents", {
      relative: "session.txt",
      contents: "session\n",
    });
    expect(await run("git_locate_files", { paths: [join(copy.path, "session.txt")] })).toEqual([
      { root: copy.path, relative: "session.txt" },
    ]);
    await run("git_commit", { message: "session change", amend: false, paths: ["session.txt"] });
    expect(readFileSync(join(copy.path, "file.txt"), "utf8")).toBe(
      "inherited\n",
    );
    await run("git_stage_all");
    await run("git_unstage_all");
    await run("git_discard_file", { relative: "file.txt" });
    writeFileSync(join(copy.path, "session.txt"), "amended\n");
    await run("git_stage_file", { relative: "session.txt" });
    await run("git_commit", { message: "amended session", amend: true });
    expect(await run("git_head_message")).toContain("amended session");
    const head = gitAt(copy.path, "rev-parse", "HEAD");
    expect(await run("git_history", { limit: 10 })).toMatchObject({ head });
    expect(await run("git_range_context")).toMatchObject({
      base: "main",
      head: copy.branch,
    });
    await run("git_commit_files", { sha: head });
    await run("git_commit_file_diff", { sha: head, relative: "session.txt" });
    await run("git_push");
    expect(readFileSync(authMarker, "utf8")).toContain("called");
    expect(gitAt(remote, "rev-parse", "refs/heads/main")).toBe(originalHead);
    expect(gitAt(remote, "rev-parse", `refs/heads/${copy.branch}`)).toBe(head);
    const peer = join(folder, "peer");
    git("clone", "--branch", copy.branch!, remote, peer);
    gitAt(peer, "config", "user.name", "Test");
    gitAt(peer, "config", "user.email", "test@example.com");
    writeFileSync(join(peer, "remote.txt"), "remote\n");
    gitAt(peer, "add", ".");
    gitAt(peer, "commit", "-m", "remote change");
    gitAt(peer, "push");
    await run("git_pull");
    await run("git_sync");
    expect(readFileSync(join(copy.path, "remote.txt"), "utf8")).toBe(
      "remote\n",
    );
    // Exercise the real gh command routing with a local executable, without creating a remote PR.
    const bin = join(folder, "bin");
    mkdirSync(bin);
    const gh = join(bin, "gh");
    const calls = join(folder, "gh-call.json");
    writeFileSync(
      gh,
      `#!${process.execPath}\nrequire('node:fs').writeFileSync(${JSON.stringify(calls)}, JSON.stringify({cwd:process.cwd(),args:process.argv.slice(2)})); console.log('https://example.invalid/pull/1');\n`,
    );
    chmodSync(gh, 0o755);
    vi.stubEnv("PATH", `${bin}:${process.env.PATH}`);
    await run("git_pr_create", {
      title: "Session",
      body: "Changes",
      base: "main",
      head: copy.branch,
    });
    expect(JSON.parse(readFileSync(calls, "utf8"))).toEqual({
      cwd: copy.path,
      args: [
        "pr",
        "create",
        "--title",
        "Session",
        "--body",
        "Changes",
        "--base",
        "main",
        "--head",
        copy.branch,
      ],
    });
    vi.unstubAllEnvs();
    vi.stubEnv("GIT_CONFIG_GLOBAL", globalConfig);
    await run("git_create_branch", { name: "local-only" });
    writeFileSync(join(copy.path, "local.txt"), "unpublished\n");
    await run("git_stage_all");
    await run("git_commit", { message: "local history", amend: false });
    const localHead = gitAt(copy.path, "rev-parse", "HEAD");
    expect(await commands.run("cow_list", { cwd })).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          branch: "local-only",
          head: localHead,
          dirty: false,
        }),
      ]),
    );
    await run("git_checkout", { name: "main" });
    writeFileSync(join(copy.path, "collision.txt"), "collision\n");
    await run("git_stage_all");
    await run("git_commit", { message: "divergent main", amend: false });
    const divergent = gitAt(copy.path, "rev-parse", "HEAD");
    gitAt(copy.path, "checkout", "--detach", "HEAD");
    writeFileSync(join(copy.path, "detached.txt"), "detached history\n");
    await run("git_stage_all");
    await run("git_commit", { message: "detached history", amend: false });
    const detached = gitAt(copy.path, "rev-parse", "HEAD");
    await commands.run("cow_remove", { cwd, cowId: copy.id });
    expect(git("rev-parse", `refs/heads/mc/kept-${copy.id}/detached`)).toBe(
      detached,
    );
    expect(git("rev-parse", "refs/heads/local-only")).toBe(localHead);
    expect(git("rev-parse", `refs/heads/mc/kept-${copy.id}/main`)).toBe(
      divergent,
    );
    expect(git("rev-parse", "HEAD")).toBe(originalHead);
    expect(readFileSync(join(cwd, "file.txt"), "utf8")).toBe("inherited\n");
    expect(() => readFileSync(hookMarker)).toThrow();
  } finally {
    vi.unstubAllEnvs();
    if (copy)
      await hostCow(store, "cow_remove", {
        cwd,
        cowId: copy.id,
        force: true,
      }).catch(() => {});
    store.close();
    rmSync(folder, { recursive: true, force: true });
  }
}, 60_000);
