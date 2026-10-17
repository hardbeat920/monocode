import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import { hostname, homedir } from "node:os";
import { execFile } from "node:child_process";
import { realpath, stat } from "node:fs/promises";
import { promisify } from "node:util";
import {
  HOST_PROTOCOL_VERSION,
  type HostModelCatalog,
  type RemoteProvider,
} from "../src/features/connections/model/protocol";
import { HostEngine } from "./engine";
import { writeAttachmentChunk, readAttachmentChunk } from "./attachments";
import type { LinkedWorkItem } from "../src/features/sessions/model/session";
import { linkedWorkItemsEqual, normalizeLinkedWorkItem } from "../src/features/sessions/model/sessionWorkItem";
import { SyncTransfers } from "./sync-transfer";
import { browseHostDirectories } from "./browse";
import {
  createHostBranch,
  hostBranches,
  switchHostBranch,
} from "./git-branches";
import {
  createHostWorktree,
  hostWorktrees,
  resolveHostWorktreeAsync,
} from "./git-worktrees";
import {
  createHostPath,
  hostFileDiff,
  hostGitAction,
  hostGitIndex,
  indexHostFiles,
  listHostFiles,
  readHostFile,
  searchHostContent,
  searchHostFiles,
  writeHostFile,
} from "./workspace";
import { WorkspaceCommands } from "./workspace-commands";
import { discoverCodexModels } from "../src/integrations/harness/providers/codex/codexCatalog";
import { discoverClaudeModels } from "../src/integrations/harness/providers/claude/claudeCatalog";
import { discoverCursorModels } from "../src/integrations/harness/providers/cursor/cursorCatalog";
import { discoverGrokModels } from "../src/integrations/harness/providers/grok/grokCatalog";
import { discoverOpenCodeModels } from "../src/integrations/harness/providers/opencode/opencodeCatalog";
import { discoverPiModels, discoverOmpModels } from "../src/integrations/harness/providers/pi/piCatalog";
import { discoverFxModels } from "../src/integrations/harness/providers/fx/fxCatalog";
import { discoverHermesModels } from "../src/integrations/harness/providers/hermes/hermesCatalog";
import { discoverAntigravityModels } from "../src/integrations/harness/providers/antigravity/antigravityCatalog";
import { discoverDevinModels } from "../src/integrations/harness/providers/devin/devinCatalog";
import { setHarnessModels, type AgentModel } from "../src/features/sessions/model/models";
import {
  resolveAntigravityBinary,
  resolveClaudeBinary,
  resolveCodexBinary,
  resolveCursorBinary,
  resolveDevinBinary,
  resolveFxBinary,
  resolveGrokBinary,
  resolveHermesBinary,
  resolveOmpBinary,
  resolveOpenCodeBinary,
  resolvePiBinary,
} from "../src/integrations/harness/core/child";

const exec = promisify(execFile);
// Providers also add models server-side, without a CLI update.
const CATALOG_MAX_AGE_MS = 5 * 60_000;
const resolveBinary: Record<RemoteProvider, () => Promise<{ path: string }>> = {
  codex: () => resolveCodexBinary(),
  claude: () => resolveClaudeBinary(),
  cursor: () => resolveCursorBinary(),
  grok: () => resolveGrokBinary(),
  opencode: () => resolveOpenCodeBinary(),
  pi: () => resolvePiBinary(),
  omp: () => resolveOmpBinary(),
  fx: () => resolveFxBinary(),
  hermes: () => resolveHermesBinary(),
  antigravity: () => resolveAntigravityBinary(),
  devin: () => resolveDevinBinary(),
};
// A 1 MiB text file can expand to 6 MiB when JSON escapes control characters.
// Existing files.write sends both the original and replacement contents.
const MAX_BODY = 16 * 1024 * 1024;
const discoverModels: Record<RemoteProvider, (cwd: string) => Promise<AgentModel[]>> = {
  codex: discoverCodexModels,
  claude: discoverClaudeModels,
  cursor: discoverCursorModels,
  grok: discoverGrokModels,
  opencode: discoverOpenCodeModels,
  pi: discoverPiModels,
  omp: discoverOmpModels,
  fx: discoverFxModels,
  hermes: discoverHermesModels,
  antigravity: discoverAntigravityModels,
  devin: discoverDevinModels,
};

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

/** Identifies each installed provider CLI. An update changes its real path or
 * modification time, which invalidates the catalog the old version reported. */
async function providerBinaries(providers: RemoteProvider[]): Promise<string> {
  const binaries = await Promise.all(
    providers.map(async (provider) => {
      try {
        const file = await realpath((await resolveBinary[provider]()).path);
        return `${file}:${(await stat(file)).mtimeMs}`;
      } catch {
        return "";
      }
    }),
  );
  return binaries.join("\n");
}

export function createHostServer(
  engine: HostEngine,
  providers: RemoteProvider[],
  lifecycle?: (request: IncomingMessage, response: ServerResponse) => void,
) {
  const catalogs = new Map<
    string,
    { binaries: string; probed: number; catalog: Promise<HostModelCatalog> }
  >();
  const transfers = new SyncTransfers();
  const workspace = new WorkspaceCommands(
    engine.store,
    (projectId, action) => engine.withIdleProject(projectId, action),
  );
  const models = async (projectId?: unknown) => {
    const cwd =
      typeof projectId === "string"
        ? engine.store.project(projectId).cwd
        : homedir();
    const binaries = await providerBinaries(providers);
    const cached = catalogs.get(cwd);
    let catalog =
      cached?.binaries === binaries &&
      Date.now() - cached.probed < CATALOG_MAX_AGE_MS
        ? cached.catalog
        : undefined;
    if (!catalog) {
      catalog = (async () => {
        const result: HostModelCatalog = { models: {}, errors: {} };
        await Promise.all(
          providers.map(async (provider) => {
            try {
              const discovered = await discoverModels[provider](cwd);
              result.models[provider] = discovered;
              if (discovered.length) setHarnessModels(provider, discovered);
            } catch (error) {
              result.errors[provider] =
                error instanceof Error ? error.message : String(error);
            }
          }),
        );
        return result;
      })().then(
        (result) => {
          if (
            Object.keys(result.errors).length &&
            catalogs.get(cwd)?.catalog === catalog
          )
            catalogs.delete(cwd);
          return result;
        },
        (error) => {
          if (catalogs.get(cwd)?.catalog === catalog) catalogs.delete(cwd);
          throw error;
        },
      );
      catalogs.set(cwd, { binaries, probed: Date.now(), catalog });
    }
    return catalog;
  };
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
        // Reading a request body yields: a device may have been revoked since
        // the headers arrived. Reject it before dispatching any operation.
        if (!engine.store.authenticated(token)) {
          response.writeHead(401).end(JSON.stringify({
            error: "Device credential is invalid or revoked",
          }));
          return;
        }
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
              // Older clients validate this list against Codex and Claude only.
              providers: providers.filter((provider) =>
                Array.isArray(params.supportedProviders)
                  ? params.supportedProviders.includes(provider)
                  : provider === "codex" || provider === "claude"
              ),
              capabilities: [
                "sessions",
                "projects.browse",
                "models.list",
                "approvals",
                "questions",
                "diff",
                "git.branches",
                "git.switch",
                "git.createBranch",
                "git.worktrees",
                "git.worktreeCreate",
                "files.read",
                "files.list",
                "files.index",
                "workspace.run",
                "files.search",
                "files.searchContent",
                "files.create",
                "files.write",
                "git.index",
                "git.fileDiff",
                "git.action",
                "attachments.upload",
                "attachments.read",
                "sessions.draft",
                "sessions.plan",
              ],
            };
            break;
          case "projects.list":
            result = engine.store.projects();
            break;
          case "projects.browse":
            result = await browseHostDirectories(params.path);
            break;
          case "projects.open":
            result = await engine.openProject(String(params.cwd ?? ""));
            break;
          case "models.list":
            result = await models(params.projectId);
            break;
          case "sessions.list": {
            const projectId = String(params.projectId ?? "");
            const project = engine.store.project(projectId);
            const summaries = engine.store.summaries(projectId);
            const paths = [...new Set(summaries.map((session) => session.cwd ?? project.cwd))];
            const branches = new Map(await Promise.all(paths.map(async (cwd) => {
              const branch = await exec("git", ["symbolic-ref", "--quiet", "--short", "HEAD"], {
                cwd, timeout: 2_000,
              }).then(({ stdout }) => stdout.trim()).catch(() => "");
              return [cwd, branch] as const;
            })));
            result = summaries.map((session) => ({
              ...session,
              repo: project.name,
              branch: branches.get(session.cwd ?? project.cwd) || undefined,
              worktreeCwd: session.cwd && session.cwd !== project.cwd
                ? session.cwd : undefined,
            }));
            break;
          }
          case "sessions.update": {
            const sessionId = String(params.sessionId ?? "");
            const current = engine.store.session(sessionId);
            if (current.projectId !== params.projectId)
              throw new Error("Session does not belong to this project");
            const patch: { title?: string; archived?: boolean; pinned?: boolean; linkedWorkItem?: LinkedWorkItem | null } = {};
            if (params.title !== undefined) {
              if (typeof params.title !== "string") throw new Error("Invalid session title");
              patch.title = params.title;
            }
            if (params.archived !== undefined) {
              if (typeof params.archived !== "boolean") throw new Error("Invalid archive value");
              patch.archived = params.archived;
            }
            if (params.pinned !== undefined) {
              if (typeof params.pinned !== "boolean") throw new Error("Invalid pin value");
              patch.pinned = params.pinned;
            }
            if (params.linkedWorkItem !== undefined) {
              const item = params.linkedWorkItem;
              const parsed = normalizeLinkedWorkItem(item);
              if (item !== null && (
                !parsed || !linkedWorkItemsEqual(parsed, item as LinkedWorkItem)
              )) throw new Error("Invalid linked work item");
              patch.linkedWorkItem = item as LinkedWorkItem | null;
            }
            if (Object.keys(patch).length === 0) throw new Error("No session changes supplied");
            result = engine.updateSession(sessionId, patch);
            break;
          }
          case "sessions.delete": {
            const sessionId = String(params.sessionId ?? "");
            const current = engine.store.session(sessionId);
            if (current.projectId !== params.projectId)
              throw new Error("Session does not belong to this project");
            engine.store.deleteSession(sessionId);
            result = { deleted: true };
            break;
          }
          case "sessions.sync": {
            const sessionId = String(params.sessionId ?? "");
            result = transfers.respond(
              sessionId,
              engine.store.sync(
                sessionId,
                Number.isSafeInteger(params.revision)
                  ? Number(params.revision)
                  : undefined,
              ),
            );
            break;
          }
          case "sessions.syncChunk":
            result = transfers.chunk(
              String(params.sessionId ?? ""),
              String(params.transfer ?? ""),
              Number(params.offset),
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
          case "attachments.upload":
            result = writeAttachmentChunk(engine.store, params);
            break;
          case "attachments.read":
            result = readAttachmentChunk(engine.store, params);
            break;
          case "devices.revokeSelf":
            // Only the caller's own credential. Sessions and other devices
            // are unaffected; the host keeps running.
            result = { revoked: engine.store.revokeToken(token) };
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
              {
                cwd: await resolveHostWorktreeAsync(project.cwd, params.cwd),
                timeout: 10_000,
                maxBuffer: 2 * 1024 * 1024,
              },
            );
            result = diff.stdout;
            break;
          }
          case "git.branches": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            result = await hostBranches(
              await resolveHostWorktreeAsync(project.cwd, params.cwd),
            );
            break;
          }
          case "git.switch": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            const cwd = await resolveHostWorktreeAsync(project.cwd, params.cwd);
            result = await engine.withIdleProject(project.id, () =>
              switchHostBranch(cwd, params.branch, params.remote),
            );
            break;
          }
          case "git.createBranch": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            const cwd = await resolveHostWorktreeAsync(project.cwd, params.cwd);
            result = await engine.withIdleProject(project.id, () =>
              createHostBranch(cwd, params.branch),
            );
            break;
          }
          case "git.worktrees": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            result = await hostWorktrees(project.cwd);
            break;
          }
          case "git.worktreeCreate": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            const cwd = await resolveHostWorktreeAsync(project.cwd, params.cwd);
            result = await engine.withIdleProject(project.id, () =>
              createHostWorktree(
                project.cwd,
                params.branch,
                params.base,
                params.existing,
                cwd,
              ),
            );
            workspace.invalidateRoots();
            break;
          }
          case "files.read": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            result = await readHostFile(
              await resolveHostWorktreeAsync(project.cwd, params.cwd),
              params.path,
            );
            break;
          }
          case "files.list": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            result = await listHostFiles(
              await resolveHostWorktreeAsync(project.cwd, params.cwd),
              params.path,
            );
            break;
          }
          case "files.index": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            result = await indexHostFiles(
              await resolveHostWorktreeAsync(project.cwd, params.cwd),
            );
            break;
          }
          case "workspace.run":
            result = (await workspace.run(params.command, params.args)) ?? null;
            break;
          case "files.search": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            result = await searchHostFiles(
              await resolveHostWorktreeAsync(project.cwd, params.cwd),
              params.query,
            );
            break;
          }
          case "files.searchContent": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            result = await searchHostContent(
              await resolveHostWorktreeAsync(project.cwd, params.cwd),
              params,
            );
            break;
          }
          case "files.create": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            result = await createHostPath(
              await resolveHostWorktreeAsync(project.cwd, params.cwd),
              params.parent,
              params.name,
              params.isDir,
            );
            break;
          }
          case "files.write": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            result =
              (await writeHostFile(
                await resolveHostWorktreeAsync(project.cwd, params.cwd),
                params.path,
                params.expected,
                params.content,
              )) ?? null;
            break;
          }
          case "git.index": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            result = await hostGitIndex(
              await resolveHostWorktreeAsync(project.cwd, params.cwd),
            );
            break;
          }
          case "git.fileDiff": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            result = await hostFileDiff(
              await resolveHostWorktreeAsync(project.cwd, params.cwd),
              params.path,
              params.staged === true,
            );
            break;
          }
          case "git.action": {
            const project = engine.store.project(
              String(params.projectId ?? ""),
            );
            const cwd = await resolveHostWorktreeAsync(project.cwd, params.cwd);
            result =
              (await engine.withIdleProject(project.id, () =>
                hostGitAction(
                  cwd,
                  params.action,
                  params.path,
                  params.message,
                  params.content,
                ),
              )) ?? null;
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
