import { expect, it, vi } from "vitest";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { readFileSync, existsSync } from "node:fs";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { HostChildBackend, mergeOpenCodeConfig } from "./child-backend";

it("merges scoped policy without losing inherited providers or agent prompts", () => {
  const merged = JSON.parse(
    mergeOpenCodeConfig(
      JSON.stringify({
        provider: { local: { options: { baseURL: "http://localhost:8000" } } },
        agent: {
          custom: { prompt: "Review", permission: { edit: { "*": "allow" } } },
        },
      }),
      JSON.stringify({ agent: { custom: { permission: { edit: "deny" } } } }),
    ),
  );
  expect(merged.provider.local.options.baseURL).toBe("http://localhost:8000");
  expect(merged.agent.custom).toEqual({
    prompt: "Review",
    permission: { edit: "deny" },
  });
});

it("merges inherited JSONC without changing quoted content and rejects JSONC policy", () => {
  const merged = JSON.parse(
    mergeOpenCodeConfig(
      `{
    // Keep URL and comment markers inside strings.
    "provider": {"local": {"options": {"baseURL": "https://example.com/a//b"},},},
    "agent": {"custom": {"prompt": "Quoted \\"/* text */\\""},},
  }`,
      '{"agent":{"custom":{"permission":{"edit":"deny"}}}}',
    ),
  );
  expect(merged.provider.local.options.baseURL).toBe(
    "https://example.com/a//b",
  );
  expect(merged.agent.custom.prompt).toBe('Quoted "/* text */"');
  expect(merged.agent.custom.permission.edit).toBe("deny");
  expect(() => mergeOpenCodeConfig("{}", "{/* policy */}")).toThrow();
});

it("expands inline env fragments and file content once in the spawn directory", () => {
  const cwd = mkdtempSync(join(tmpdir(), "monocode-inline-config-"));
  vi.stubEnv(
    "MONOCODE_TEST_INLINE_OPTIONS",
    '{"baseURL":"http://localhost:9000/"}',
  );
  vi.stubEnv("MONOCODE_TEST_INLINE_DESCRIPTION", "fixture-description");
  vi.stubEnv("MONOCODE_TEST_INLINE_LITERAL", "{env:SECOND}");
  writeFileSync(
    join(cwd, "prompt.txt"),
    '  Review "quoted"\nKeep {env:SECOND} and {file:missing.txt} literal.  ',
  );
  try {
    const merged = mergeOpenCodeConfig(
      `{
      // {file:absent-comment.txt}
      "provider": {"local": {"options": {env:MONOCODE_TEST_INLINE_OPTIONS}}},
      "agent": {"custom": {"prompt": "{file:prompt.txt}", "description": "{env:MONOCODE_TEST_INLINE_DESCRIPTION}"}},
      "username": "{env:MONOCODE_TEST_INLINE_LITERAL}",
    }`,
      '{"agent":{"custom":{"permission":{"edit":"deny"}}}}',
      cwd,
    );
    const value = JSON.parse(merged);
    expect(value.provider.local.options.baseURL).toBe("http://localhost:9000/");
    expect(value.agent.custom.description).toBe("fixture-description");
    expect(value.agent.custom.prompt).toBe(
      'Review "quoted"\nKeep {env:SECOND} and {file:missing.txt} literal.',
    );
    expect(value.username).toBe("{env:SECOND}");
    expect(merged).not.toContain("{env:");
    expect(merged).not.toContain("{file:");
    expect(value.agent.custom.permission.edit).toBe("deny");
  } finally {
    vi.unstubAllEnvs();
    rmSync(cwd, { recursive: true, force: true });
  }
});

it("expands against the final child environment including removed auth values", () => {
  vi.stubEnv("OPENCODE_SERVER_PASSWORD", "synthetic-parent-password");
  vi.stubEnv("MONOCODE_TEST_CHILD_VALUE", "parent-value");
  try {
    const value = JSON.parse(
      mergeOpenCodeConfig(
        '{"agent":{"custom":{"prompt":"{env:MONOCODE_TEST_CHILD_VALUE}","description":"{env:OPENCODE_SERVER_PASSWORD}"}}}',
        '{"agent":{"custom":{"permission":{"edit":"deny"}}}}',
        process.cwd(),
        { MONOCODE_TEST_CHILD_VALUE: "child-value" },
      ),
    );
    expect(value.agent.custom.prompt).toBe("child-value");
    expect(value.agent.custom.description).toBe("");
  } finally {
    vi.unstubAllEnvs();
  }
});

it("waits for SSE headers and suppresses the closed subscription's end after reopening", async () => {
  let headersSent = false;
  const server = createServer((_request, response) => {
    setTimeout(() => {
      response.writeHead(200, { "Content-Type": "text/event-stream" });
      response.write("data: ready\n\n");
      headersSent = true;
    }, 50);
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("No address");
  const backend = new HostChildBackend();
  const ends: unknown[] = [];
  await backend.listen("harness-sse-end", ({ payload }) => ends.push(payload));
  try {
    const args = {
      sessionId: "same",
      url: `http://127.0.0.1:${address.port}/event`,
    };
    await backend.invoke("harness_sse_open", args);
    expect(headersSent).toBe(true);
    await backend.invoke("harness_sse_close", { sessionId: "same" });
    await backend.invoke("harness_sse_open", args);
    expect(ends).toEqual([]);
  } finally {
    await backend.close();
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});

it.each([false, true])(
  "discards a resolved SSE read after closing its subscription, reopened %s",
  async (reopen) => {
    const controllers: ReadableStreamDefaultController<Uint8Array>[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(
            new ReadableStream<Uint8Array>({
              start(controller) {
                controllers.push(controller);
              },
            }),
            { headers: { "Content-Type": "text/event-stream" } },
          ),
      ),
    );
    const backend = new HostChildBackend();
    const events: string[] = [];
    await backend.listen<{ data: string }>("harness-sse", ({ payload }) =>
      events.push(payload.data),
    );
    const args = { sessionId: "same", url: "http://127.0.0.1:4096/event" };
    try {
      await backend.invoke("harness_sse_open", args);
      controllers[0].enqueue(new TextEncoder().encode("data: obsolete\n\n"));
      const closed = reopen
        ? backend.invoke("harness_sse_open", args)
        : backend.invoke("harness_sse_close", { sessionId: "same" });
      await closed;
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(events).toEqual([]);
      if (reopen) {
        controllers[1].enqueue(new TextEncoder().encode("data: current\n\n"));
        await vi.waitFor(() => expect(events).toEqual(["current"]));
      }
    } finally {
      await backend.close();
      for (const controller of controllers) controller.close();
      vi.unstubAllGlobals();
    }
  },
);

it("stops dispatching buffered SSE frames when an event callback closes the subscription", async () => {
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  vi.stubGlobal(
    "fetch",
    vi.fn(
      async () =>
        new Response(
          new ReadableStream<Uint8Array>({
            start(value) {
              controller = value;
            },
          }),
          { headers: { "Content-Type": "text/event-stream" } },
        ),
    ),
  );
  const backend = new HostChildBackend();
  const events: string[] = [];
  await backend.listen<{ data: string }>("harness-sse", ({ payload }) => {
    events.push(payload.data);
    void backend.invoke("harness_sse_close", { sessionId: "same" });
  });
  try {
    await backend.invoke("harness_sse_open", {
      sessionId: "same",
      url: "http://127.0.0.1:4096/event",
    });
    controller.enqueue(
      new TextEncoder().encode("data: first\n\ndata: obsolete\n\n"),
    );
    await vi.waitFor(() => expect(events).toEqual(["first"]));
  } finally {
    await backend.close();
    controller.close();
    vi.unstubAllGlobals();
  }
});

it("cancels the SSE request and releases its reader after a frame error", async () => {
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  let signal!: AbortSignal;
  const stream = new ReadableStream<Uint8Array>({
    start(value) {
      controller = value;
    },
  });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_url: string, options: RequestInit) => {
      signal = options.signal as AbortSignal;
      return new Response(stream, {
        headers: { "Content-Type": "text/event-stream" },
      });
    }),
  );
  const backend = new HostChildBackend();
  const ends: { error?: string }[] = [];
  await backend.listen<{ error?: string }>("harness-sse-end", ({ payload }) =>
    ends.push(payload),
  );
  try {
    await backend.invoke("harness_sse_open", {
      sessionId: "frame",
      url: "http://127.0.0.1:4096/event",
    });
    controller.enqueue(new Uint8Array(8 * 1024 * 1024 + 1));
    await vi.waitFor(() => expect(ends).toHaveLength(1));
    expect(ends[0].error).toContain("frame is too large");
    expect(signal.aborted).toBe(true);
    expect(stream.locked).toBe(false);
  } finally {
    await backend.close();
    controller.close();
    vi.unstubAllGlobals();
  }
});
import { REMOTE_PROVIDERS } from "../src/features/connections/model/protocol";

it("resolves every provider and runs only allowed provider commands", async () => {
  const directory = mkdtempSync(join(tmpdir(), "monocode-catalog-test-"));
  const file = join(directory, "provider.cjs");
  writeFileSync(file, "console.log(JSON.stringify(process.argv.slice(2)))");
  const backend = new HostChildBackend(
    Object.fromEntries(REMOTE_PROVIDERS.map((provider) => [provider, file])),
  );
  try {
    for (const provider of REMOTE_PROVIDERS) {
      const resolved = await backend.invoke<{ path: string; args?: string[] }>(
        `harness_resolve_${provider}`,
      );
      expect(resolved.path).toBe(file);
      if (provider === "antigravity")
        expect(resolved.args).toEqual(
          process.platform === "linux" ? ["--uid="] : [],
        );
    }
    const output = await backend.invoke<string>("harness_exec", {
      command: file,
      args: ["models", "--json"],
      binaryProvider: "fx",
      cwd: directory,
    });
    expect(JSON.parse(output)).toEqual(["models", "--json"]);
    const cleanupArgs = [
      "--no-auto-update",
      "sessions",
      "delete",
      "550e8400-e29b-41d4-a716-446655440000",
    ];
    const cleanupOutput = await backend.invoke<string>("harness_exec", {
      command: file,
      args: cleanupArgs,
      binaryProvider: "grok",
      cwd: directory,
    });
    expect(JSON.parse(cleanupOutput)).toEqual(cleanupArgs);
    await expect(
      backend.invoke("harness_exec", {
        command: file,
        args: ["-e", "console.log('unsafe')"],
        binaryProvider: "fx",
      }),
    ).rejects.toThrow("Unsupported headless catalog command");
    for (const args of [
      ["service", "status"],
      ["service", "start"],
      ["service", "get", "password"],
    ]) {
      const serviceOutput = await backend.invoke<string>("harness_exec", {
        command: file,
        args,
        binaryProvider: "opencode",
        cwd: directory,
      });
      expect(JSON.parse(serviceOutput)).toEqual(args);
    }
    for (const [provider, args] of [
      ["fx", ["service", "start"]],
      ["opencode", ["service", "stop"]],
      ["opencode", ["service get", "password"]],
      ["opencode", ["service", "get", "password", "--json"]],
      ["opencode", ["models --json"]],
      ["cursor", cleanupArgs],
      ["grok", ["--no-auto-update", "sessions", "delete", "--all"]],
      ["grok", ["--no-auto-update", "sessions", "delete", "../sessions"]],
      ["grok", [...cleanupArgs, "--all"]],
    ] as const) {
      await expect(
        backend.invoke("harness_exec", {
          command: file,
          args,
          binaryProvider: provider,
        }),
      ).rejects.toThrow("Unsupported headless catalog command");
    }
    writeFileSync(join(directory, "note.txt"), "host-owned transcript");
    expect(
      await backend.invoke("harness_read_text_file", {
        path: join(directory, "note.txt"),
      }),
    ).toBe("host-owned transcript");
    const transcript = "x".repeat(1024 * 1024 + 1);
    writeFileSync(join(directory, "large-transcript.txt"), transcript);
    expect(
      await backend.invoke("harness_read_text_file", {
        path: join(directory, "large-transcript.txt"),
      }),
    ).toBe(transcript);
  } finally {
    await backend.close();
    rmSync(directory, { recursive: true, force: true });
  }
});

it("bridges OpenCode HTTP and event streams on loopback", async () => {
  const server = createServer((request, response) => {
    if (request.url === "/event") {
      response.writeHead(200, { "Content-Type": "text/event-stream" });
      response.write('data: {"type":"ready"}\n\n');
    } else {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end('{"ok":true}');
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string")
    throw new Error("No server address");
  const backend = new HostChildBackend();
  const events: string[] = [];
  const unlisten = await backend.listen<{ data: string }>(
    "harness-sse",
    ({ payload }) => events.push(payload.data),
  );
  try {
    const base = `http://127.0.0.1:${address.port}`;
    expect(
      await backend.invoke("harness_http", { url: base, method: "GET" }),
    ).toEqual({ status: 200, body: '{"ok":true}' });
    await backend.invoke("harness_sse_open", {
      sessionId: "fixture",
      url: `${base}/event`,
    });
    await vi.waitFor(() => expect(events).toEqual(['{"type":"ready"}']));
    await backend.invoke("harness_sse_close", { sessionId: "fixture" });
    await expect(
      backend.invoke("harness_http", {
        url: "https://example.com/",
        method: "GET",
      }),
    ).rejects.toThrow("localhost");
  } finally {
    unlisten();
    await backend.close();
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});

it("runs the resolved Claude version fallback in headless mode", async () => {
  const backend = new HostChildBackend({ claude: process.execPath });
  try {
    const version = await backend.invoke<string>("harness_exec", {
      command: process.execPath,
      args: ["--version"],
      binaryProvider: "claude",
      cwd: process.cwd(),
    });
    expect(version.trim()).toBe(process.version);
    await expect(
      backend.invoke("harness_exec", {
        command: process.execPath,
        args: ["-e", "console.log('unsafe')"],
        binaryProvider: "claude",
      }),
    ).rejects.toThrow("Unsupported headless catalog command");
  } finally {
    await backend.close();
  }
});

it.each([false, true])(
  "stops a provider tree (ignores SIGTERM: %s)",
  async (stubborn) => {
    const directory = mkdtempSync(join(tmpdir(), "monocode-provider-tree-"));
    const file = join(directory, "provider.cjs");
    writeFileSync(
      file,
      `const { spawn } = require('node:child_process');
const child = spawn(process.execPath, ['-e', ${JSON.stringify(`${stubborn ? "process.on('SIGTERM', () => {});" : ""} console.log('ready'); setInterval(() => {}, 1000)`)}], { stdio: ['ignore', 'pipe', 'ignore'] });
child.stdout.once('data', () => console.log(JSON.stringify({ child: child.pid })));
setInterval(() => {}, 1000);
`,
    );
    const backend = new HostChildBackend();
    let descendant: number | undefined;
    const stopListening = await backend.listen<{ line: string }>(
      "harness-stdout",
      ({ payload }) => {
        descendant = JSON.parse(payload.line).child;
      },
    );
    try {
      await backend.invoke("harness_spawn", {
        sessionId: "tree",
        command: file,
        args: [],
        cwd: directory,
      });
      await vi.waitFor(() => expect(descendant).toBeTruthy());
      await backend.kill("tree");
      await vi.waitFor(
        () => expect(() => process.kill(descendant!, 0)).toThrow(),
        { timeout: 5000 },
      );
    } finally {
      stopListening();
      await backend.close();
      if (descendant) {
        try {
          process.kill(descendant, "SIGKILL");
        } catch {
          /* gone */
        }
      }
      rmSync(directory, { recursive: true, force: true });
    }
  },
  15_000,
);

it("stops a provider tree when its host pipe closes unexpectedly", async () => {
  const directory = mkdtempSync(join(tmpdir(), "monocode-provider-crash-"));
  const treeFile = join(directory, "tree.json");
  const providerFile = join(directory, "provider.cjs");
  writeFileSync(
    providerFile,
    `const { spawn } = require('node:child_process');
const { writeFileSync } = require('node:fs');
const descendant = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
writeFileSync(${JSON.stringify(treeFile)}, JSON.stringify({ provider: process.pid, descendant: descendant.pid }));
setInterval(() => {}, 1000);
`,
  );
  const guard = spawn(
    process.execPath,
    [resolve("build/host/provider-guard.mjs"), process.execPath, providerFile],
    {
      cwd: directory,
      stdio: ["pipe", "ignore", "ignore", "pipe"],
      detached: process.platform !== "win32",
      windowsHide: true,
    },
  );
  let guardClosed = false;
  guard.once("close", () => {
    guardClosed = true;
  });
  let tree: { provider: number; descendant: number } | undefined;
  try {
    await vi.waitFor(() => expect(existsSync(treeFile)).toBe(true));
    tree = JSON.parse(readFileSync(treeFile, "utf8"));
    guard.stdio[3]?.destroy();
    await vi.waitFor(
      () => {
        expect(() => process.kill(tree!.provider, 0)).toThrow();
        expect(() => process.kill(tree!.descendant, 0)).toThrow();
      },
      { timeout: 5_000 },
    );
    // The guard's cwd keeps this directory locked on Windows until it exits.
    await vi.waitFor(() => expect(guardClosed).toBe(true), { timeout: 5_000 });
  } finally {
    guard.stdio[3]?.destroy();
    guard.kill("SIGKILL");
    for (const pid of [tree?.provider, tree?.descendant]) {
      if (pid)
        try {
          process.kill(pid, "SIGKILL");
        } catch {
          /* gone */
        }
    }
    await vi.waitFor(() => expect(guardClosed).toBe(true), { timeout: 5_000 });
    rmSync(directory, {
      recursive: true,
      force: true,
      maxRetries: 10,
      retryDelay: 100,
    });
  }
}, 10_000);
