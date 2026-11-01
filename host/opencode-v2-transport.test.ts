import { afterAll, beforeAll, expect, it, vi } from "vitest";
import { mkdtempSync, writeFileSync } from "node:fs";
import { rm } from "node:fs/promises";
import { createServer, type Server, type ServerResponse } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { HostChildBackend } from "./child-backend";
import { HostStore } from "./store";
import { HostEngine } from "./engine";
import { hostProviders } from "./providers";
import {
  acquireHarnessBridge,
  configureChildBackend,
} from "../src/integrations/harness/core/child";

const PASSWORD = "fixture-secret";

// OpenCode 2.x CLI: the chat server is a separately managed background
// service, so the binary only reports its version, URL and password.
const fixture = `const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const args = process.argv.slice(2).join(' ');
if (args === '--version') console.log('opencode v2.0.15');
else if (args === 'service status' || args === 'service start')
  console.log(readFileSync(join(__dirname, 'service-url'), 'utf8'));
else if (args === 'service get password') console.log(${JSON.stringify(PASSWORD)});
else process.exit(2);
`;

let directory: string;
let server: Server;
let store: HostStore;
let engine: HostEngine;
let backend: HostChildBackend;
let release: () => void;
const subscribers = new Set<ServerResponse>();
const unauthorized: string[] = [];

function emit(type: string, data: Record<string, unknown>) {
  for (const subscriber of subscribers)
    subscriber.write(
      `data: ${JSON.stringify({ id: `evt_${type}`, type, data: { sessionID: "ses_v2", ...data } })}\n\n`,
    );
}

beforeAll(async () => {
  directory = mkdtempSync(join(tmpdir(), "monocode-opencode-v2-transport-"));
  server = createServer((request, response) => {
    const expected = `Basic ${Buffer.from(`opencode:${PASSWORD}`).toString("base64")}`;
    if (request.headers.authorization !== expected) {
      unauthorized.push(request.url ?? "");
      response.writeHead(401);
      response.end();
      return;
    }
    const path = new URL(request.url ?? "/", "http://127.0.0.1").pathname;
    if (path === "/api/event") {
      response.writeHead(200, { "Content-Type": "text/event-stream" });
      response.flushHeaders();
      subscribers.add(response);
      request.on("close", () => subscribers.delete(response));
      return;
    }
    const json = (data: unknown) => {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ data }));
    };
    const session = { id: "ses_v2", location: { directory } };
    if (path === "/api/session" || path === "/api/session/ses_v2")
      return json(session);
    if (path === "/api/session/ses_v2/message") return json([]);
    if (path === "/api/session/ses_v2/prompt") {
      response.writeHead(204);
      response.end();
      setTimeout(() => {
        emit("session.execution.started", {});
        emit("session.step.started", {
          assistantMessageID: "msg_1",
          model: { providerID: "openai", id: "fixture-model" },
        });
        emit("session.text.delta", {
          assistantMessageID: "msg_1",
          delta: "Headless OpenCode 2 completed",
        });
        emit("session.step.ended", { assistantMessageID: "msg_1" });
        emit("session.execution.succeeded", {});
      }, 30);
      return;
    }
    response.writeHead(204);
    response.end();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string")
    throw new Error("No server address");
  writeFileSync(
    join(directory, "service-url"),
    `http://127.0.0.1:${address.port}`,
  );
  const binary = join(directory, "opencode.cjs");
  writeFileSync(binary, fixture);
  backend = new HostChildBackend({ opencode: binary });
  configureChildBackend(backend);
  release = await acquireHarnessBridge();
  store = new HostStore(join(directory, "host.db"));
  engine = new HostEngine(store, hostProviders);
});

afterAll(async () => {
  await engine?.close();
  await backend?.close();
  release?.();
  store?.close();
  server?.closeAllConnections();
  await new Promise<void>((resolve) => server?.close(() => resolve()));
  // Windows may retain a directory handle briefly after fixture processes exit.
  if (directory)
    await rm(directory, {
      recursive: true,
      force: true,
      maxRetries: 10,
      retryDelay: 100,
    });
});

it("runs an OpenCode 2.x session through the host service commands and bridge", async () => {
  const project = await engine.openProject(directory);
  const { sessionId } = engine.command({
    type: "create",
    commandId: "create-opencode-v2",
    projectId: project.id,
    harness: "opencode",
    model: "opencode:openai/fixture-model",
    runtimeMode: "supervised",
  });
  engine.command({
    type: "send",
    commandId: "send-opencode-v2",
    sessionId,
    text: "hello",
  });
  await vi.waitFor(
    () => {
      const state = store.session(sessionId).session;
      expect(state.blocks.at(-1)?.text).toContain(
        "Headless OpenCode 2 completed",
      );
      expect(store.session(sessionId).status).toBe("idle");
    },
    { timeout: 8_000 },
  );
  expect(store.session(sessionId).session.providerSessionId).toBe("ses_v2");
  expect(unauthorized).toEqual([]);
});
