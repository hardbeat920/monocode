import { afterEach, describe, expect, it, vi } from "vitest";
import { mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { AddressInfo } from "node:net";
import { HostEngine } from "./engine";
import { HostStore } from "./store";
import { createHostServer } from "./server";
import type { SendTurnInput } from "../src/integrations/harness/core/types";

const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
});

async function setup() {
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
  const server = createHostServer(engine, ["codex"]);
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
    directory,
    store,
    project,
    call,
    first,
    second,
    send,
    turn: () => turn!,
    finish: () => finish(),
  };
}

describe("remote host API", () => {
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

  it("reads host files while rejecting traversal and symlink escapes", async () => {
    const s = await setup();
    writeFileSync(join(s.directory, "hello.txt"), "from host");
    expect(
      await s.call("files.read", {
        projectId: s.project.id,
        path: "hello.txt",
      }),
    ).toEqual({ status: 200, value: { result: "from host" } });
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
});
