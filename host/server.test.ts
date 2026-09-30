import { afterEach, describe, expect, it, vi } from "vitest";
import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { AddressInfo } from "node:net";
import { request } from "node:http";
import { HostEngine } from "./engine";
import { HostStore } from "./store";
import { createHostServer } from "./server";
import { HostChildBackend } from "./child-backend";
import { configureChildBackend } from "../src/integrations/harness/core/child";
import type { SendTurnInput } from "../src/integrations/harness/core/types";
import type { RemoteProvider } from "../src/features/connections/model/protocol";

const modelProbe = vi.hoisted(() => vi.fn());
vi.mock("../src/integrations/harness/providers/codex/codexCatalog", () => ({
  discoverCodexModels: modelProbe,
}));
// Catalog tests point the host at a stand-in provider CLI.
const binaries: { codex?: string } = {};
configureChildBackend(new HostChildBackend(binaries));

const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
});

async function setup(providers: RemoteProvider[] = ["codex"]) {
  const directory = mkdtempSync(join(tmpdir(), "monocode-server-test-"));
  const store = new HostStore(join(directory, "host.db"));
  let turn: SendTurnInput | undefined;
  let finish = () => {};
  const send = vi.fn((input: SendTurnInput) => {
    turn = input;
    return new Promise<void>((resolve) => {
      finish = resolve;
    });
  });
  const engine = new HostEngine(store, {
    codex: {
      send,
      stop: async () => finish(),
      cancel: async () => finish(),
      bind: () => {},
      approve: () => {},
      answer: () => {},
    },
  });
  // Follow production's canonicalization, including Windows 8.3 paths such
  // as RUNNER~1 in the CI runner's temporary directory.
  const project = await engine.openProject(directory);
  const server = createHostServer(engine, providers, undefined, {
    endpoints: () => ["https://10.0.0.5:3774"],
  });
  try {
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", resolve);
    });
  } catch (error) {
    store.close();
    rmSync(directory, { recursive: true, force: true });
    throw error;
  }
  const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}/rpc`;
  const first = store.issueDevice("Laptop");
  const second = store.issueDevice("Other computer");
  const pair = async (
    params: unknown,
    method = "pair.exchange",
    remoteAddress?: string,
  ) => {
    // Exercise remote-peer policy over loopback without requiring a LAN peer.
    const listener = (incoming: import("node:http").IncomingMessage) => {
      Object.defineProperty(incoming.socket, "remoteAddress", { value: remoteAddress, configurable: true });
    };
    if (remoteAddress) server.prependOnceListener("request", listener);
    const response = await fetch(url, {
      method: "POST",
      headers: { Connection: "close" },
      body: JSON.stringify({ version: 1, method, params }),
    });
    return {
      status: response.status,
      value: (await response.json()) as { result?: any; error?: string },
    };
  };
  const call = async (
    method: string,
    params: unknown = {},
    token = first.token,
    overrides: Record<string, unknown> = {},
    headers: Record<string, string> = {},
  ) => {
    const response = await fetch(url, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, ...headers },
      body: JSON.stringify({
        version: 1,
        environmentId: store.environmentId,
        method,
        params,
        ...overrides,
      }),
    });
    return {
      status: response.status,
      value: (await response.json()) as { result?: any; error?: string },
    };
  };
  cleanups.push(async () => {
    await engine.close();
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
    store.close();
    rmSync(directory, { recursive: true, force: true });
  });
  return {
    url,
    directory,
    engine,
    store,
    project,
    call,
    pair,
    first,
    second,
    send,
    turn: () => turn!,
    finish: () => finish(),
  };
}

describe("remote host API", () => {
  it("rejects a credential revoked while its request body is arriving", async () => {
    const s = await setup();
    const authenticated = vi.spyOn(s.store, "authenticated");
    const body = JSON.stringify({ version: 1, environmentId: s.store.environmentId,
      method: "commands.dispatch", params: { type: "create", commandId: "revoked-create",
        projectId: s.project.id, harness: "codex", model: "codex:test", runtimeMode: "supervised" } });
    let req: ReturnType<typeof request>;
    const response = new Promise<number | undefined>((resolve, reject) => {
      req = request(s.url, { method: "POST", headers: {
        Authorization: `Bearer ${s.first.token}`, "Content-Length": Buffer.byteLength(body),
      } }, (res) => { res.resume(); res.on("end", () => resolve(res.statusCode)); });
      req.on("error", reject);
      req.write(body.slice(0, 1));
    });
    await vi.waitFor(() => expect(authenticated).toHaveBeenCalledTimes(1));
    s.store.revokeToken(s.first.token);
    req!.end(body.slice(1));
    expect(await response).toBe(401);
    expect(s.store.summaries(s.project.id)).toEqual([]);
  });

  it("uploads an authenticated attachment and sends its host path to the provider", async () => {
    const s = await setup();
    const id = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
    const upload = { id, offset: 0, size: 5, data: Buffer.from("hello").toString("base64") };
    expect((await s.call("attachments.upload", upload, "invalid")).status).toBe(401);
    expect((await s.call("attachments.upload", upload)).value.result).toEqual({ offset: 5 });
    const created = await s.call("commands.dispatch", { type: "create", commandId: "upload-create",
      projectId: s.project.id, harness: "codex", model: "codex:test", runtimeMode: "supervised" });
    const sessionId = created.value.result.sessionId;
    const sent = await s.call("commands.dispatch", { type: "send", commandId: "upload-send",
      sessionId, text: "Read this", attachments: [{ id, name: "notes.txt",
        mimeType: "text/plain", kind: "file", size: 5 }] });
    expect(sent.status).toBe(200);
    await vi.waitFor(() => expect(s.send).toHaveBeenCalledTimes(1));
    expect(s.turn().attachments?.[0].path).toContain(id);
    s.finish();
  });
  it("applies card actions to the owning project and lists their saved state", async () => {
    const s = await setup();
    const create = await s.call("commands.dispatch", {
      type: "create",
      commandId: "card-session",
      projectId: s.project.id,
      harness: "codex",
      model: "codex:test",
      runtimeMode: "supervised",
    });
    const sessionId = create.value.result.sessionId;
    const changed = await s.call("sessions.update", {
      projectId: s.project.id,
      sessionId,
      title: "Codex · Card title",
      pinned: true,
    });
    expect(changed.status).toBe(200);
    expect((await s.call("sessions.list", { projectId: s.project.id })).value.result[0])
      .toMatchObject({ id: sessionId, title: "Codex · Card title", pinned: true, model: "codex:test" });
    expect((await s.call("sessions.update", {
      projectId: "wrong-project", sessionId, archived: true,
    })).status).not.toBe(200);
    expect((await s.call("sessions.delete", {
      projectId: "wrong-project", sessionId,
    })).status).not.toBe(200);
    expect((await s.call("sessions.delete", {
      projectId: s.project.id, sessionId,
    })).status).toBe(200);
    expect((await s.call("sessions.list", { projectId: s.project.id })).value.result).toEqual([]);
  });

  it("lists, creates, and selects registered remote worktrees through RPC", async () => {
    const s = await setup();
    const git = (...args: string[]) =>
      execFileSync("git", args, { cwd: s.project.cwd });
    git("init", "-q");
    git("config", "core.autocrlf", "false");
    git("checkout", "-q", "-b", "main");
    writeFileSync(join(s.project.cwd, ".gitignore"), "host.db*\n");
    writeFileSync(join(s.project.cwd, "file.txt"), "initial\n");
    git("add", ".gitignore", "file.txt");
    git(
      "-c",
      "user.name=Test",
      "-c",
      "user.email=test@example.com",
      "commit",
      "-q",
      "-m",
      "initial",
    );

    const branch = await s.call("git.createBranch", {
      projectId: s.project.id,
      branch: "feature",
    });
    expect(branch.value.result.current).toBe("feature");
    expect(
      (await s.call("git.switch", { projectId: s.project.id, branch: "main" }))
        .value.result.current,
    ).toBe("main");
    // Prime the allowed-root cache before creating a checkout.
    expect((await s.call("workspace.run", { command: "list_dir", args: { path: s.project.cwd } })).status).toBe(200);
    const created = await s.call("git.worktreeCreate", {
      projectId: s.project.id,
      branch: "feature",
      base: "HEAD",
      existing: true,
    });
    expect(created.status).toBe(200);
    const tree = created.value.result;
    cleanups.push(async () =>
      rmSync(join(s.project.cwd, "..", `${s.project.name}-worktrees`), {
        recursive: true,
        force: true,
      }),
    );
    expect(tree.branch).toBe("feature");
    expect((await s.call("workspace.run", { command: "read_text_file", args: {
      path: join(tree.path, "file.txt"),
    } })).value.result).toBe("initial\n");
    expect(
      (await s.call("git.worktrees", { projectId: s.project.id })).value.result
        .worktrees,
    ).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ path: tree.path, branch: "feature" }),
      ]),
    );

    const opened = await s.call("commands.dispatch", {
      type: "create",
      commandId: "in-worktree",
      projectId: s.project.id,
      worktreeCwd: tree.path,
      harness: "codex",
      model: "codex:test",
      runtimeMode: "supervised",
    });
    expect(opened.status).toBe(200);
    expect(s.store.session(opened.value.result.sessionId).session.cwd).toBe(
      tree.path,
    );
    expect((await s.call("sessions.list", { projectId: s.project.id })).value.result[0])
      .toMatchObject({
        id: opened.value.result.sessionId,
        branch: "feature",
        worktreeCwd: tree.path,
        repo: s.project.name,
      });
    expect(
      (
        await s.call("commands.dispatch", {
          type: "create",
          commandId: "outside-worktree",
          projectId: s.project.id,
          worktreeCwd: tmpdir(),
          harness: "codex",
          model: "codex:test",
          runtimeMode: "supervised",
        })
      ).value.error,
    ).toContain("available worktree");

    writeFileSync(join(tree.path, "file.txt"), "changed\n");
    expect(
      (await s.call("git.diff", { projectId: s.project.id, cwd: tree.path }))
        .value.result,
    ).toContain("changed");
  });
  it("retries model discovery after a provider becomes available", async () => {
    const s = await setup();
    modelProbe.mockRejectedValueOnce(new Error("Login required"));
    modelProbe.mockResolvedValueOnce([{ id: "codex:test", name: "Test" }]);
    const first = await s.call("models.list", { projectId: s.project.id });
    expect(first.value.result.errors.codex).toBe("Login required");
    const second = await s.call("models.list", { projectId: s.project.id });
    expect(second.value.result.models.codex).toEqual([
      { id: "codex:test", name: "Test" },
    ]);
    expect(modelProbe).toHaveBeenCalledTimes(2);
  });
  it("advertises newer providers only to desktops that request them", async () => {
    const s = await setup(["codex", "cursor"]);
    expect((await s.call("environment.describe")).value.result.providers)
      .toEqual(["codex"]);
    expect((await s.call("environment.describe", {
      supportedProviders: ["codex", "cursor"],
    })).value.result.providers).toEqual(["codex", "cursor"]);
  });
  it("re-probes models after the provider CLI is updated", async () => {
    const s = await setup();
    cleanups.push(async () => {
      delete binaries.codex;
    });
    writeFileSync(join(s.directory, "codex-1"), "");
    writeFileSync(join(s.directory, "codex-2"), "");
    binaries.codex = join(s.directory, "codex-1");
    modelProbe.mockClear();
    modelProbe.mockResolvedValueOnce([{ id: "codex:old", name: "Old" }]);
    modelProbe.mockResolvedValueOnce([{ id: "codex:new", name: "New" }]);
    const list = async () =>
      (await s.call("models.list", { projectId: s.project.id })).value.result
        .models.codex;
    expect(await list()).toEqual([{ id: "codex:old", name: "Old" }]);
    expect(await list()).toEqual([{ id: "codex:old", name: "Old" }]);
    binaries.codex = join(s.directory, "codex-2");
    expect(await list()).toEqual([{ id: "codex:new", name: "New" }]);
    expect(modelProbe).toHaveBeenCalledTimes(2);
  });
  it("re-probes models once the catalog is five minutes old", async () => {
    const s = await setup();
    modelProbe.mockClear();
    modelProbe.mockResolvedValueOnce([{ id: "codex:old", name: "Old" }]);
    modelProbe.mockResolvedValueOnce([{ id: "codex:new", name: "New" }]);
    const list = async () =>
      (await s.call("models.list", { projectId: s.project.id })).value.result
        .models.codex;
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      expect(await list()).toEqual([{ id: "codex:old", name: "Old" }]);
      vi.setSystemTime(Date.now() + 4 * 60_000);
      expect(await list()).toEqual([{ id: "codex:old", name: "Old" }]);
      vi.setSystemTime(Date.now() + 60_000);
      expect(await list()).toEqual([{ id: "codex:new", name: "New" }]);
    } finally {
      vi.useRealTimers();
    }
    expect(modelProbe).toHaveBeenCalledTimes(2);
  });
  it("lets an authenticated desktop browse host folders without reading files", async () => {
    const s = await setup();
    mkdirSync(join(s.directory, "checkout"));
    writeFileSync(join(s.directory, "private.txt"), "secret");
    const listed = await s.call("projects.browse", { path: s.directory });
    expect(listed.status).toBe(200);
    expect(listed.value.result.entries).toEqual([
      { name: "checkout", path: join(s.directory, "checkout") },
    ]);
    expect(
      (await s.call("projects.browse", { path: s.directory }, "invalid"))
        .status,
    ).toBe(401);
  });
  it("allows a different client to recover work completed while the laptop was disconnected", async () => {
    const s = await setup();
    const create = await s.call("commands.dispatch", {
      type: "create",
      commandId: "create",
      projectId: s.project.id,
      harness: "codex",
      model: "codex:test",
      runtimeMode: "supervised",
    });
    const id = create.value.result.sessionId;
    const command = {
      type: "send",
      commandId: "send",
      sessionId: id,
      text: "Work without this client",
    };
    await s.call("commands.dispatch", command);
    await vi.waitFor(() => expect(s.send).toHaveBeenCalledTimes(1));
    s.turn().onEvent({ type: "message.delta", text: "Finished on the host" });
    s.finish();
    await vi.waitFor(() => expect(s.store.session(id).status).toBe("idle"));
    const recovered = await s.call(
      "sessions.get",
      { sessionId: id },
      s.second.token,
    );
    expect(recovered.value.result.session.blocks.at(-1).text).toBe(
      "Finished on the host",
    );
    await s.call("commands.dispatch", command, s.second.token);
    expect(s.send).toHaveBeenCalledTimes(1);
  });

  it("rejects revoked devices, browser origins, and changed host identities", async () => {
    const s = await setup();
    expect((await s.call("environment.describe", {}, "invalid")).status).toBe(
      401,
    );
    expect(
      (
        await s.call(
          "environment.describe",
          {},
          s.first.token,
          {},
          { Origin: "https://untrusted.example" },
        )
      ).status,
    ).toBe(403);
    expect(
      (
        await s.call("projects.list", {}, s.first.token, {
          environmentId: "different-host",
        })
      ).value.error,
    ).toContain("identity changed");
    s.store.db.prepare("DELETE FROM devices WHERE id=?").run(s.first.id);
    expect((await s.call("environment.describe")).status).toBe(401);
    expect(
      (await s.call("environment.describe", {}, s.second.token)).status,
    ).toBe(200);
  });

  it("exchanges a pairing code for a device credential once", async () => {
    const s = await setup();
    const { code } = s.store.issuePairing();
    const paired = await s.pair({ code, name: "Studio" });
    expect(paired.status).toBe(200);
    expect(paired.value.result).toMatchObject({
      environmentId: s.store.environmentId,
    });
    const token = paired.value.result.token;
    expect(s.store.devices().map((device) => device.name)).toContain("Studio");
    const described = await s.call("environment.describe", {}, token);
    expect(described.value.result).toMatchObject({
      hostVersion: expect.stringMatching(/^\d+\.\d+\.\d+/),
      endpoints: ["https://10.0.0.5:3774"],
      capabilities: expect.arrayContaining(["changes.wait"]),
    });
    expect((await s.pair({ code, name: "Again" })).status).toBe(401);
    // Without a credential, nothing but pairing is reachable.
    expect((await s.pair({}, "projects.list")).status).toBe(401);
  });

  it("limits network pairing failures while keeping SSH pairing available", async () => {
    const s = await setup();
    for (let attempt = 0; attempt < 30; attempt++)
      expect((await s.pair({ code: "x".repeat(43) }, "pair.exchange", "10.0.0.5")).status).toBe(401);
    const { code } = s.store.issuePairing();
    expect((await s.pair({ code }, "pair.exchange", "10.0.0.6")).status).toBe(429);
    // Network lockout must not consume the valid code or block the SSH route.
    expect((await s.pair({ code })).status).toBe(200);
  });

  it("does not count loopback failures toward the network pairing limit", async () => {
    const s = await setup();
    for (const address of ["127.0.0.1", "::1", "::ffff:127.0.0.1"]) {
      for (let attempt = 0; attempt < 31; attempt++)
        expect((await s.pair({ code: "x".repeat(43) }, "pair.exchange", address)).status).toBe(401);
      const { code } = s.store.issuePairing();
      expect((await s.pair({ code }, "pair.exchange", address)).status).toBe(200);
    }
    const { code } = s.store.issuePairing();
    expect((await s.pair({ code }, "pair.exchange", "10.0.0.5")).status).toBe(200);
  });

  it("answers a waiting desktop when a session changes", async () => {
    const s = await setup();
    const first = await s.call("changes.wait", {});
    expect(first.value.result).toMatchObject({ reset: true, sessions: [] });
    const { boot, cursor } = first.value.result;
    const waiting = s.call("changes.wait", { boot, after: cursor });
    const create = await s.call("commands.dispatch", {
      type: "create",
      commandId: "create",
      projectId: s.project.id,
      harness: "codex",
      model: "codex:test",
      runtimeMode: "supervised",
    });
    const changed = (await waiting).value.result;
    expect(changed.reset).toBe(false);
    expect(changed.sessions).toEqual([
      expect.objectContaining({
        id: create.value.result.sessionId,
        projectId: s.project.id,
      }),
    ]);
    const idle = await s.call("changes.wait", {
      boot,
      after: changed.cursor,
      timeoutMs: 20,
    });
    expect(idle.value.result.sessions).toEqual([]);
  });

  it("lets a desktop revoke only its own credential, keeping sessions", async () => {
    const s = await setup();
    const create = await s.call("commands.dispatch", {
      type: "create",
      commandId: "create",
      projectId: s.project.id,
      harness: "codex",
      model: "codex:test",
      runtimeMode: "supervised",
    });
    const id = create.value.result.sessionId;
    expect((await s.call("devices.revokeSelf")).value.result).toEqual({
      revoked: true,
    });
    expect((await s.call("environment.describe")).status).toBe(401);
    const other = await s.call(
      "sessions.sync",
      { sessionId: id },
      s.second.token,
    );
    expect(other.value.result.value.session.id).toBe(id);
  });

  it("reads host files while rejecting traversal and symlink escapes", async () => {
    const s = await setup();
    writeFileSync(join(s.directory, "hello.txt"), "from host");
    expect(
      await s.call("files.read", {
        projectId: s.project.id,
        cwd: s.project.cwd,
        path: "hello.txt",
      }),
    ).toEqual({ status: 200, value: { result: "from host" } });
    expect(
      await s.call("files.write", {
        projectId: s.project.id,
        path: "hello.txt",
        expected: "from host",
        content: "edited",
      }),
    ).toEqual({ status: 200, value: { result: null } });
    expect(
      (
        await s.call("files.read", {
          projectId: s.project.id,
          path: "hello.txt",
        })
      ).value.result,
    ).toBe("edited");
    symlinkSync(
      tmpdir(),
      join(s.directory, "outside"),
      process.platform === "win32" ? "junction" : "dir",
    );
    expect(
      (await s.call("files.read", { projectId: s.project.id, path: "outside" }))
        .value.error,
    ).toContain("outside");
    const sibling = `${s.directory}-outside.txt`;
    writeFileSync(sibling, "must not be exposed");
    try {
      expect(
        (await s.call("files.read", { projectId: s.project.id, path: sibling }))
          .value.error,
      ).toContain("outside");
    } finally {
      rmSync(sibling);
    }
    expect(
      (await s.call("files.read", { projectId: s.project.id, path: ".." }))
        .value.error,
    ).toContain("outside");
  });

  it("answers this app's file commands inside host projects only", async () => {
    const s = await setup();
    const outside = mkdtempSync(join(tmpdir(), "monocode-outside-"));
    cleanups.push(async () => rmSync(outside, { recursive: true, force: true }));
    const checkout = join(s.directory, "checkout");
    mkdirSync(join(checkout, "src"), { recursive: true });
    writeFileSync(join(checkout, "src", "app.ts"), "before\n");
    const project = await s.engine.openProject(checkout);
    const root = project.cwd.replace(/\\/g, "/");
    const run = async (command: string, args: Record<string, unknown>) =>
      (await s.call("workspace.run", { command, args })).value;

    expect((await run("list_dir", { path: root })).result).toEqual([
      { name: "src", path: `${root}/src`, isDir: true, ignored: false },
    ]);
    expect(
      (await run("read_text_file", { path: `${root}/src/app.ts` })).result,
    ).toBe("before\n");
    expect(
      (await run("read_binary_file", { path: `${root}/src/app.ts` })).result,
    ).toBe(Buffer.from("before\n").toString("base64"));
    await run("write_text_file", { path: `${root}/src/app.ts`, content: "after\n" });
    expect(
      (await run("read_text_file", { path: `${root}/src/app.ts` })).result,
    ).toBe("after\n");
    const [stat] = (
      await run("stat_files", { paths: [`${root}/src/app.ts`, `${root}/nope`] })
    ).result;
    expect(stat).toMatchObject({ path: `${root}/src/app.ts` });
    expect(typeof stat.mtimeMs).toBe("number");

    expect(
      (await run("create_path", { parent: root, name: "docs/a.md", isDir: false }))
        .result,
    ).toBe(`${root}/docs/a.md`);
    expect(
      (await run("create_path", { parent: root, name: "docs/a.md", isDir: false }))
        .error,
    ).toContain("already exists");
    expect(
      (await run("rename_path", { path: `${root}/docs/a.md`, name: "b.md" }))
        .result,
    ).toBe(`${root}/docs/b.md`);
    expect(
      (await run("copy_path", { from: `${root}/docs/b.md`, destParent: `${root}/docs` }))
        .result,
    ).toBe(`${root}/docs/b copy.md`);
    expect(
      (await run("move_path", { from: `${root}/docs/b.md`, destParent: `${root}/src` }))
        .result,
    ).toBe(`${root}/src/b.md`);
    await run("delete_path", { path: `${root}/docs` });
    expect(
      (await run("list_project_files", { cwd: root })).result
        .map((file: { relative: string }) => file.relative)
        .sort(),
    ).toEqual(["src/app.ts", "src/b.md"]);
    expect(
      (await s.call("files.index", { projectId: project.id, cwd: root }))
        .value.result,
    ).toEqual(["src/app.ts", "src/b.md"]);

    const git = (...args: string[]) => execFileSync("git", args, { cwd: root });
    git("init", "-q");
    git("config", "user.name", "Host Test");
    git("config", "user.email", "host@example.test");
    git("add", "src");
    git("commit", "-qm", "initial");
    writeFileSync(join(root, "src", "app.ts"), "changed\n");
    expect((await run("search_project", { options: { cwd: root, query: "changed" } })).result.matches)
      .toContainEqual(expect.objectContaining({
        path: `${root}/src/app.ts`, relative: "src/app.ts", line: 1,
      }));
    const gitIndex = (await run("git_diff_index", { cwd: root })).result;
    expect(gitIndex.files).toContainEqual(expect.objectContaining({
      path: "src/app.ts", relative: "src/app.ts", unstaged: true,
    }));
    expect((await run("git_file_diff", { cwd: root, relative: "src/app.ts", staged: false })).result)
      .toMatchObject({ original: "after\n", current: "changed\n" });
    await run("git_stage_file", { cwd: root, relative: "src/app.ts" });
    expect((await run("git_diff_index", { cwd: root })).result.files)
      .toContainEqual(expect.objectContaining({ relative: "src/app.ts", staged: true }));
    const history = (await run("git_history", { cwd: root, limit: 10 })).result;
    expect(history.commits[0])
      .toMatchObject({ subject: "initial", head: true });
    expect(history.commits[0].timestamp).toBeLessThan(10_000_000_000);
    expect((await run("git_commit_files", { cwd: root, sha: history.head })).result)
      .toContainEqual(expect.objectContaining({ relative: "src/app.ts", additions: 1 }));
    expect((await run("git_commit_file_diff", {
      cwd: root, sha: history.head, relative: "src/app.ts",
    })).result).toMatchObject({ original: "", current: "after\n", status: "added" });
    expect((await run("git_worktrees", { cwd: root })).result.worktrees)
      .toContainEqual(expect.objectContaining({ path: project.cwd, isMain: true }));

    for (const path of [outside, `${root}/../outside`, `${root}/.git/config`])
      expect((await run("read_text_file", { path })).error).toBeTruthy();
    expect(
      (await run("write_text_file", { path: join(outside, "x"), content: "x" }))
        .error,
    ).toContain("outside");
    expect((await run("delete_path", { path: root })).error).toBeTruthy();
    expect((await run("rm_rf", { path: root })).error).toContain("Unsupported");
  });

  it("browses and commits changes in the host checkout", async () => {
    const s = await setup();
    const checkout = join(s.directory, "checkout");
    mkdirSync(checkout);
    const project = await s.engine.openProject(checkout);
    const git = (...args: string[]) =>
      execFileSync("git", args, { cwd: checkout });
    git("init", "-q");
    git("config", "user.name", "Host Test");
    git("config", "user.email", "host@example.test");
    mkdirSync(join(checkout, "src"));
    writeFileSync(join(checkout, "src", "app.ts"), "before\n");
    git("add", "--", ".");
    git("commit", "-qm", "initial");
    writeFileSync(join(checkout, "src", "app.ts"), "after\n");
    writeFileSync(join(checkout, "new.ts"), "new\n");

    const root = await s.call("files.list", {
      projectId: project.id,
      path: "",
    });
    expect(root.status).toBe(200);
    expect(root.value.result).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ name: "src", isDir: true }),
        expect.objectContaining({ name: "new.ts", isDir: false }),
      ]),
    );
    expect(
      (
        await s.call("files.search", {
          projectId: project.id,
          query: "app",
        })
      ).value.result,
    ).toEqual([expect.objectContaining({ path: "src/app.ts" })]);
    expect(
      (
        await s.call("files.searchContent", {
          projectId: project.id,
          query: "after",
        })
      ).value.result,
    ).toMatchObject({
      matches: [expect.objectContaining({ relative: "src/app.ts", line: 1 })],
      truncated: false,
    });
    expect(
      (await s.call("files.list", { projectId: project.id, path: "src" })).value
        .result[0].name,
    ).toBe("app.ts");
    expect(
      (await s.call("files.list", { projectId: project.id, path: ".." })).value
        .error,
    ).toContain("outside");
    expect(
      (
        await s.call("files.list", {
          projectId: project.id,
          cwd: s.directory,
          path: "",
        })
      ).value.error,
    ).toContain("worktree");

    const index = await s.call("git.index", { projectId: project.id });
    expect(index.value.result.files).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          relative: "src/app.ts",
          status: "modified",
          unstaged: true,
        }),
        expect.objectContaining({
          relative: "new.ts",
          status: "untracked",
          unstaged: true,
        }),
      ]),
    );
    const diff = await s.call("git.fileDiff", {
      projectId: project.id,
      path: "src/app.ts",
      staged: false,
    });
    expect(diff.value.result).toMatchObject({
      original: "before\n",
      current: "after\n",
    });
    expect(
      (
        await s.call("git.action", {
          projectId: project.id,
          action: "stage",
          path: "../escape",
        })
      ).value.error,
    ).toContain("outside");
    expect(
      (
        await s.call("git.action", {
          projectId: project.id,
          action: "stageContents",
          path: "src/app.ts",
          content: "selected\n",
        })
      ).status,
    ).toBe(200);
    expect(
      (
        await s.call("git.fileDiff", {
          projectId: project.id,
          path: "src/app.ts",
          staged: true,
        })
      ).value.result.current,
    ).toBe("selected\n");
    expect(
      (
        await s.call("git.action", {
          projectId: project.id,
          action: "stageAll",
        })
      ).status,
    ).toBe(200);
    const staged = await s.call("git.index", { projectId: project.id });
    expect(
      staged.value.result.files.every(
        (file: { staged: boolean }) => file.staged,
      ),
    ).toBe(true);
    expect(
      (
        await s.call("git.action", {
          projectId: project.id,
          action: "commit",
          message: "remote commit",
        })
      ).status,
    ).toBe(200);
    expect(
      (await s.call("git.index", { projectId: project.id })).value.result.files,
    ).toEqual([]);
  });
});
