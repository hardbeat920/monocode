import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import { hostname } from "node:os";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { realpath, readFile } from "node:fs/promises";
import { relative, resolve, isAbsolute } from "node:path";
import {
  HOST_PROTOCOL_VERSION,
  type RemoteProvider,
} from "../src/features/connections/model/protocol";
import { HostEngine } from "./engine";

const exec = promisify(execFile);
const MAX_BODY = 512 * 1024;

async function body(
  request: IncomingMessage,
): Promise<Record<string, unknown>> {
  let size = 0;
  const chunks: Buffer[] = [];
  for await (const chunk of request) {
    size += chunk.length;
    if (size > MAX_BODY) throw new Error("Request is too large");
    chunks.push(chunk);
  }
  const value: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("Invalid request");
  return value as Record<string, unknown>;
}

export function createHostServer(
  engine: HostEngine,
  providers: RemoteProvider[],
  lifecycle?: (request: IncomingMessage, response: ServerResponse) => void,
) {
  return createServer(
    { requestTimeout: 20_000, headersTimeout: 10_000, maxHeaderSize: 8192 },
    async (request, response) => {
      if (request.url === "/lifecycle" && lifecycle) {
        lifecycle(request, response);
        return;
      }
      response.setHeader("Content-Type", "application/json");
      response.setHeader("Cache-Control", "no-store");
      response.setHeader("X-Content-Type-Options", "nosniff");
      try {
        // Desktop native HTTP supplies credentials. This endpoint intentionally
        // accepts no browser origin and provides no permissive CORS escape hatch.
        if (
          request.headers.origin ||
          request.method !== "POST" ||
          request.url !== "/rpc"
        ) {
          response
            .writeHead(403)
            .end(
              JSON.stringify({ error: "Unsupported request origin or route" }),
            );
          return;
        }
        const token = request.headers.authorization?.match(
          /^Bearer ([A-Za-z0-9_-]{43})$/,
        )?.[1];
        if (!token || !engine.store.authenticated(token)) {
          response.writeHead(401).end(
            JSON.stringify({
              error: "Device credential is invalid or revoked",
            }),
          );
          return;
        }
        const input = await body(request);
        if (input.version !== HOST_PROTOCOL_VERSION)
          throw new Error("Incompatible protocol version");
        if (
          input.method !== "environment.describe" &&
          input.environmentId !== engine.store.environmentId
        )
          throw new Error(
            "Host identity changed; reconnect this machine explicitly",
          );
        const params =
          input.params &&
          typeof input.params === "object" &&
          !Array.isArray(input.params)
            ? (input.params as Record<string, unknown>)
            : {};
        let result: unknown;
        switch (input.method) {
          case "environment.describe":
            result = {
              protocolVersion: HOST_PROTOCOL_VERSION,
              environmentId: engine.store.environmentId,
              name: hostname(),
              platform: process.platform,
              providers,
              capabilities: [
                "sessions",
                "approvals",
                "questions",
                "diff",
                "files.read",
              ],
            };
            break;
          case "projects.list":
            result = engine.store.projects();
            break;
          case "projects.open":
            result = await engine.openProject(String(params.cwd ?? ""));
            break;
          case "sessions.list": {
            const projectId = String(params.projectId ?? "");
            engine.store.project(projectId);
            result = engine.store.summaries(projectId);
            break;
          }
          case "sessions.sync":
            result = engine.store.sync(
              String(params.sessionId ?? ""),
              Number.isSafeInteger(params.revision)
                ? Number(params.revision)
                : undefined,
            );
            break;
          case "sessions.get": {
            const value = engine.store.session(String(params.sessionId ?? ""));
            result = value.revision === params.revision ? null : value;
            break;
          }
          case "events.read": {
            if (!Number.isSafeInteger(params.after) || Number(params.after) < 0)
              throw new Error("Invalid event cursor");
            result = engine.store.events(
              String(params.sessionId ?? ""),
              Number(params.after),
            );
            break;
          }
          case "commands.dispatch":
            result = engine.command(params);
            break;
          case "git.diff": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            const diff = await exec(
              "git",
              [
                "-c",
                "core.pager=cat",
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "HEAD",
                "--",
              ],
              { cwd: project.cwd, timeout: 10_000, maxBuffer: 2 * 1024 * 1024 },
            );
            result = diff.stdout;
            break;
          }
          case "files.read": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            const path = await realpath(
              resolve(project.cwd, String(params.path ?? "")),
            );
            const rel = relative(project.cwd, path);
            if (
              !rel ||
              rel.split(/[\\/]/).includes("..") ||
              isAbsolute(rel) ||
              rel.split(/[\\/]/).some((part) => part.toLowerCase() === ".git")
            )
              throw new Error("File is outside the workspace");
            const { stat } = await import("node:fs/promises");
            if ((await stat(path)).size > 1024 * 1024)
              throw new Error("File is too large to preview");
            result = await readFile(path, "utf8");
            break;
          }
          default:
            throw new Error("Unsupported host method");
        }
        response.end(JSON.stringify({ result }));
      } catch (error) {
        if (!response.destroyed)
          response.writeHead(400).end(
            JSON.stringify({
              error:
                error instanceof Error ? error.message : "Host request failed",
            }),
          );
      }
    },
  );
}
