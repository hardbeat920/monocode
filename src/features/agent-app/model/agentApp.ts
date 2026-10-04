import { isHarnessAvailable } from "../../../integrations/harness/core/availability";
import { looksLikeProject } from "../../projects/model/recents";
import {
  mergeModelSettings,
  modelEffortSetting,
  modelsFor,
  preferredModelId,
  resolveModel,
} from "../../sessions/model/models";
import {
  HARNESSES,
  RUNTIME_MODE_HINT,
  RUNTIME_MODE_LABEL,
  RUNTIME_MODES,
  type HarnessId,
  type Session,
} from "../../sessions/model/session";
import {
  loadSessionFolders,
  placeSessionInFolder,
  saveSessionFolders,
} from "../../sessions/model/sessionFolders";
import {
  normalizeNoteTags,
  noteTitle,
  type Note,
  type NoteUpsert,
} from "../../notes";
import type { QuickLaunch } from "../../quick-composer/model/quickComposer";
import type { Worktree, Worktrees } from "../../source-control/model/worktrees";
import { pathKey } from "../../../shared/lib/paths";
import type { SplitDir } from "../../workspace/model/layout";
import { consumeOperatorCommand } from "../../sessions/model/operatorCommand";
import { sessionConversationPage } from "./sessionConversation";
import type { LinkedPeer } from "../../sessions/model/sessionLinks";

export type AppSessionListing = {
  id: string;
  title: string;
  harness: HarnessId;
  model: string;
  busy: boolean;
  hasDraft: boolean;
};

export type AppSessionPlacement = {
  direction: SplitDir;
  besideSessionId: string;
};

export type AgentAppHost = {
  start(
    launch: QuickLaunch,
    id: string,
    placement?: AppSessionPlacement,
  ): Promise<void>;
  sessions(cwd: string): Promise<AppSessionListing[]>;
  session(id: string): Promise<Session | null>;
  send(
    id: string,
    prompt: string,
    requestId: string,
  ): Promise<{ alreadySubmitted: boolean }>;
  draft(
    id: string,
    prompt: string,
    requestId: string,
  ): Promise<{ alreadySaved: boolean; draft: boolean }>;
  worktrees(cwd: string): Promise<Worktrees>;
  createWorktree(
    cwd: string,
    branch: string,
    base: string,
    existing: boolean,
  ): Promise<Worktree>;
  notes(): Promise<Note[]>;
  note(id: string): Promise<Note | null>;
  saveNote(note: NoteUpsert): Promise<Note>;
  /** Sessions the user linked to this one. */
  links?(id: string): Promise<LinkedPeer[]>;
  /** A linked session, from any project. */
  linkedSession?(id: string): Promise<Session | null>;
  /** Deliver a message now, or after the peer's current turn ends. */
  sendLinked?(
    from: Session,
    to: string,
    message: string,
    requestId: string,
  ): Promise<{ queued: boolean; alreadySent: boolean }>;
};

/**
 * What the calling turn may do. "full" comes from /operator. "limited" is
 * every other thread: listing and starting sessions when the setting allows
 * it, and talking to linked sessions.
 */
export type AgentAppAccess = {
  scope: "full" | "limited";
  openSessions: boolean;
  /** Limited scope only: started sessions wait as drafts for the user. */
  reviewOpenedSessions?: boolean;
};

const FULL_ACCESS: AgentAppAccess = { scope: "full", openSessions: true };

const OPEN_SESSION_ACTIONS = new Set(["sessions.list", "sessions.start"]);
const LINK_ACTIONS = new Set(["links.read", "links.send"]);

function checkAccess(action: string, access: AgentAppAccess) {
  if (access.scope === "full" || LINK_ACTIONS.has(action)) return;
  if (OPEN_SESSION_ACTIONS.has(action)) {
    if (access.openSessions) return;
    throw new Error(
      "Opening sessions from agents is turned off in MonoCode settings. Ask the user to turn on Let agents open sessions, or to use /operator.",
    );
  }
  throw new Error(
    `${action} needs full app access. Ask the user to use /operator in this thread.`,
  );
}

async function linkedPeer(
  source: Session,
  id: string,
  host: AgentAppHost,
): Promise<LinkedPeer> {
  if (id === source.id)
    throw new Error("Use the current conversation to continue this session");
  const peers = (await host.links?.(source.id)) ?? [];
  const peer = peers.find((entry) => entry.id === id);
  if (!peer)
    throw new Error(
      "That session is not linked to this one. Only the user can link sessions.",
    );
  return peer;
}

const FIELDS = new Map<string, readonly string[]>([
  ["models.list", []],
  ["sessions.list", []],
  ["sessions.read", ["sessionId", "before", "limit", "maxChars"]],
  ["sessions.send", ["sessionId", "prompt"]],
  ["sessions.draft", ["sessionId", "prompt"]],
  [
    "sessions.start",
    [
      "prompt",
      "draft",
      "harness",
      "model",
      "modelSettings",
      "effort",
      "runtimeMode",
      "reveal",
      "workspaceMode",
      "worktreeBase",
      "worktreeCwd",
      "placement",
      "besideSessionId",
    ],
  ],
  ["worktrees.list", []],
  ["worktrees.create", ["branch", "base", "existing"]],
  ["folders.list", []],
  ["folders.move", ["sessionId", "folderId", "newFolderName"]],
  ["notes.list", ["limit", "offset"]],
  ["notes.read", ["id"]],
  ["notes.write", ["id", "title", "body", "tags"]],
  ["links.read", ["sessionId", "before", "limit", "maxChars"]],
  ["links.send", ["sessionId", "message"]],
]);

function fields(action: string, input: Record<string, unknown>) {
  const allowed = FIELDS.get(action);
  if (!allowed) throw new Error(`Unknown app action: ${action}`);
  const unknown = Object.keys(input).filter((key) => !allowed.includes(key));
  if (unknown.length)
    throw new Error(`Unknown ${action} fields: ${unknown.join(", ")}`);
}

function requiredString(value: unknown, name: string, max = 30_000): string {
  if (typeof value !== "string" || !value.trim() || value.length > max)
    throw new Error(
      `${name} must be a non-empty string under ${max} characters`,
    );
  return value.trim();
}

function agentPrompt(value: unknown): string {
  const prompt = requiredString(value, "prompt", 240_000);
  if (consumeOperatorCommand(prompt).matched)
    throw new Error("App calls cannot enable /operator in another session");
  return prompt;
}

function optionalString(
  value: unknown,
  name: string,
  max = 512,
): string | undefined {
  return value === undefined ? undefined : requiredString(value, name, max);
}

function noteBody(value: unknown): string {
  if (typeof value !== "string" || value.length > 240_000)
    throw new Error("body must be a string under 240000 characters");
  return value.replace(/\r\n?/g, "\n");
}

function noteTags(value: unknown): string[] {
  if (
    !Array.isArray(value) ||
    value.length > 20 ||
    value.some((tag) => typeof tag !== "string" || tag.length > 48)
  )
    throw new Error(
      "tags must be an array of at most 20 strings under 48 characters each",
    );
  return normalizeNoteTags(value as string[]);
}

function requireProject(source: Session): string {
  if (!looksLikeProject(source.cwd))
    throw new Error("Choose a project folder in this session first");
  return source.cwd;
}

async function projectSession(
  source: Session,
  id: string,
  host: AgentAppHost,
): Promise<Session> {
  const cwd = requireProject(source);
  if (!(await host.sessions(cwd)).some((session) => session.id === id))
    throw new Error("Session was not found in this project");
  const target = await host.session(id);
  if (!target) throw new Error("Session was not found in this project");
  return target;
}

/** Two short body paragraphs, with a hard cap independent of Markdown length. */
export function notePreview(body: string): string {
  return body
    .trim()
    .split(/\n\s*\n/)
    .map((paragraph) => paragraph.replace(/\s+/g, " ").trim())
    .filter(Boolean)
    .slice(0, 2)
    .join("\n\n")
    .slice(0, 400);
}

function startLaunch(
  source: Session,
  input: Record<string, unknown>,
): QuickLaunch {
  const cwd = requireProject(source);
  const prompt = agentPrompt(input.prompt);
  const draft = input.draft ?? false;
  if (typeof draft !== "boolean") throw new Error("draft must be a boolean");
  const harness = input.harness ?? source.harness;
  if (!HARNESSES.includes(harness as HarnessId))
    throw new Error("Unknown harness; run models.list for available providers");
  const chosenHarness = harness as HarnessId;
  if (!isHarnessAvailable(chosenHarness))
    throw new Error(`${chosenHarness} is not available in MonoCode`);
  const requestedModel = optionalString(input.model, "model");
  const model = requestedModel
    ? modelsFor(chosenHarness).find((entry) => entry.id === requestedModel)
    : resolveModel(
        chosenHarness,
        chosenHarness === source.harness
          ? source.model
          : preferredModelId(chosenHarness),
      );
  if (!model || model.harness !== chosenHarness)
    throw new Error("Unknown model; run models.list for exact model IDs");
  const rawSettings = input.modelSettings;
  if (
    rawSettings !== undefined &&
    (!rawSettings ||
      typeof rawSettings !== "object" ||
      Array.isArray(rawSettings))
  )
    throw new Error(
      "modelSettings must be an object of setting IDs and values",
    );
  const requestedSettings = {
    ...((rawSettings ?? {}) as Record<string, unknown>),
  };
  if (input.effort !== undefined) {
    const effort = modelEffortSetting(model);
    if (!effort)
      throw new Error(`${model.id} does not expose an effort setting`);
    requestedSettings[effort.id] = requiredString(input.effort, "effort", 128);
  }
  for (const [key, value] of Object.entries(requestedSettings)) {
    const setting = model.settings?.find((entry) => entry.id === key);
    if (
      !setting ||
      typeof value !== "string" ||
      !setting.options.some((option) => option.value === value)
    )
      throw new Error(
        `Invalid model setting ${key}; run models.list for allowed values`,
      );
  }
  const runtimeMode = input.runtimeMode ?? source.runtimeMode;
  if (!RUNTIME_MODES.includes(runtimeMode as Session["runtimeMode"]))
    throw new Error(`runtimeMode must be one of: ${RUNTIME_MODES.join(", ")}`);
  const reveal = input.reveal ?? false;
  if (typeof reveal !== "boolean") throw new Error("reveal must be a boolean");
  const workspaceMode = input.workspaceMode ?? "current";
  if (workspaceMode !== "current" && workspaceMode !== "worktree")
    throw new Error("workspaceMode must be current or worktree");
  const worktreeBase = optionalString(input.worktreeBase, "worktreeBase");
  if (worktreeBase && workspaceMode !== "worktree")
    throw new Error("worktreeBase requires workspaceMode worktree");
  const worktreeCwd = optionalString(input.worktreeCwd, "worktreeCwd");
  if (worktreeCwd && workspaceMode !== "current")
    throw new Error("worktreeCwd requires workspaceMode current");
  return {
    cwd,
    prompt,
    ...(draft ? { draft: true } : {}),
    harness: chosenHarness,
    model: model.id,
    modelSettings: mergeModelSettings(model, {
      ...(chosenHarness === source.harness && model.id === source.model
        ? source.modelSettings
        : {}),
      ...(requestedSettings as Record<string, string>),
    }),
    runtimeMode: runtimeMode as Session["runtimeMode"],
    reveal,
    workspaceMode,
    ...(workspaceMode === "current" && (worktreeCwd || source.worktreeCwd)
      ? { worktreeCwd: worktreeCwd || source.worktreeCwd }
      : {}),
    ...(worktreeBase ? { worktreeBase } : {}),
  };
}

export async function handleAgentApp(
  source: Session,
  requestId: string,
  action: string,
  input: Record<string, unknown>,
  host: AgentAppHost,
  access: AgentAppAccess = FULL_ACCESS,
): Promise<unknown> {
  fields(action, input);
  checkAccess(action, access);
  switch (action) {
    case "models.list":
      return {
        runtimeModes: RUNTIME_MODES.map((id) => ({
          id,
          label: RUNTIME_MODE_LABEL[id],
          description: RUNTIME_MODE_HINT[id],
        })),
        harnesses: HARNESSES.map((harness) => ({
          id: harness,
          available: isHarnessAvailable(harness),
          models: modelsFor(harness).map((model) => ({
            id: model.id,
            name: model.name,
            settings: model.settings ?? [],
          })),
        })),
      };
    case "sessions.list":
      return {
        cwd: requireProject(source),
        sessions: await host.sessions(source.cwd),
      };
    case "sessions.read": {
      const id = requiredString(input.sessionId, "sessionId", 256);
      const target = await projectSession(source, id, host);
      return sessionConversationPage(target, {
        before: optionalString(input.before, "before", 256),
        limit: input.limit as number | undefined,
        maxChars: input.maxChars as number | undefined,
      });
    }
    case "sessions.send": {
      const id = requiredString(input.sessionId, "sessionId", 256);
      const prompt = agentPrompt(input.prompt);
      if (id === source.id)
        throw new Error(
          "Use the current conversation to continue this session",
        );
      if (!/^[A-Za-z0-9_-]{1,128}$/.test(requestId))
        throw new Error("Invalid request ID");
      await projectSession(source, id, host);
      const result = await host.send(
        id,
        prompt,
        `app-${source.id}-${requestId}`,
      );
      return { sessionId: id, submitted: true, ...result };
    }
    case "sessions.draft": {
      const id = requiredString(input.sessionId, "sessionId", 256);
      const prompt = agentPrompt(input.prompt);
      if (id === source.id)
        throw new Error("Use the composer to save a draft in this session");
      if (!/^[A-Za-z0-9_-]{1,128}$/.test(requestId))
        throw new Error("Invalid request ID");
      await projectSession(source, id, host);
      const result = await host.draft(
        id,
        prompt,
        `app-${source.id}-${requestId}`,
      );
      return { sessionId: id, saved: true, ...result };
    }
    case "sessions.start": {
      if (!/^[A-Za-z0-9_-]{1,128}$/.test(requestId))
        throw new Error(
          "request ID must use letters, digits, underscores or hyphens",
        );
      const launch = startLaunch(source, input);
      // With review on, a session opened without /operator waits for the
      // user to send it.
      const forcedDraft =
        access.scope !== "full" && !!access.reviewOpenedSessions;
      const forcedDraftChanged = forcedDraft && !launch.draft;
      if (forcedDraft) launch.draft = true;
      if (input.worktreeCwd !== undefined) {
        const chosen = (await host.worktrees(launch.cwd)).worktrees.find(
          (tree) =>
            !tree.missing &&
            pathKey(tree.path) === pathKey(launch.worktreeCwd!),
        );
        if (!chosen)
          throw new Error(
            "Worktree is unavailable in this project; run worktrees.list",
          );
        launch.worktreeCwd =
          pathKey(chosen.path) === pathKey(launch.cwd)
            ? undefined
            : chosen.path;
      }
      const placement = input.placement ?? "tab";
      if (placement !== "tab" && placement !== "right" && placement !== "down")
        throw new Error("placement must be tab, right or down");
      if (input.besideSessionId !== undefined && placement === "tab")
        throw new Error("besideSessionId requires placement right or down");
      const besideSessionId =
        placement === "tab"
          ? undefined
          : (optionalString(input.besideSessionId, "besideSessionId", 256) ??
            source.id);
      const id = `app-${source.id}-${requestId}`;
      if (besideSessionId)
        await host.start(launch, id, {
          direction: placement as SplitDir,
          besideSessionId,
        });
      else await host.start(launch, id);
      return {
        id,
        cwd: launch.cwd,
        harness: launch.harness,
        model: launch.model,
        submitted: !launch.draft,
        draft: !!launch.draft,
        ...(forcedDraftChanged
          ? {
              note: "Saved as an unsent draft. The user reviews it and sends it from MonoCode.",
            }
          : {}),
      };
    }
    case "links.read": {
      const id = requiredString(input.sessionId, "sessionId", 256);
      await linkedPeer(source, id, host);
      const target = await host.linkedSession?.(id);
      if (!target) throw new Error("The linked session is no longer available");
      return sessionConversationPage(target, {
        before: optionalString(input.before, "before", 256),
        limit: input.limit as number | undefined,
        maxChars: input.maxChars as number | undefined,
      });
    }
    case "links.send": {
      const id = requiredString(input.sessionId, "sessionId", 256);
      const message = requiredString(input.message, "message", 60_000);
      if (consumeOperatorCommand(message).matched)
        throw new Error("App calls cannot enable /operator in another session");
      if (!/^[A-Za-z0-9_-]{1,128}$/.test(requestId))
        throw new Error("Invalid request ID");
      await linkedPeer(source, id, host);
      if (!host.sendLinked)
        throw new Error("Linked sessions are unavailable in this window");
      const result = await host.sendLinked(
        source,
        id,
        message,
        `link-${source.id}-${requestId}`,
      );
      return { sessionId: id, ...result };
    }
    case "worktrees.list":
      return host.worktrees(requireProject(source));
    case "worktrees.create": {
      const cwd = requireProject(source);
      const branch = requiredString(input.branch, "branch", 400);
      const existing = input.existing ?? false;
      if (typeof existing !== "boolean")
        throw new Error("existing must be a boolean");
      const base = optionalString(input.base, "base", 400);
      if (existing && base)
        throw new Error("base cannot be set for an existing branch");
      return host.createWorktree(cwd, branch, base ?? "HEAD", existing);
    }
    case "folders.list": {
      const cwd = requireProject(source);
      return {
        cwd,
        folders: loadSessionFolders(cwd).map(({ id, name, sessionIds }) => ({
          id,
          name,
          sessionIds,
        })),
      };
    }
    case "folders.move": {
      const cwd = requireProject(source);
      const sessionId = requiredString(input.sessionId, "sessionId", 256);
      const folderId = optionalString(input.folderId, "folderId", 256);
      const newFolderName = optionalString(
        input.newFolderName,
        "newFolderName",
        100,
      );
      if (!!folderId === !!newFolderName)
        throw new Error("Supply exactly one of folderId or newFolderName");
      if (
        !(await host.sessions(cwd)).some((session) => session.id === sessionId)
      )
        throw new Error("Session was not found in this project");
      const folders = loadSessionFolders(cwd);
      if (folderId && !folders.some((folder) => folder.id === folderId))
        throw new Error("Folder was not found in this project");
      const next = placeSessionInFolder(
        folders,
        sessionId,
        folderId
          ? { kind: "existing", folderId }
          : { kind: "new", name: newFolderName! },
      );
      saveSessionFolders(cwd, next);
      const folder = next.find((entry) => entry.sessionIds.includes(sessionId));
      return { sessionId, folderId: folder?.id, folderName: folder?.name };
    }
    case "notes.list": {
      const limit = input.limit ?? 30;
      const offset = input.offset ?? 0;
      if (
        !Number.isInteger(limit) ||
        (limit as number) < 1 ||
        (limit as number) > 100
      )
        throw new Error("limit must be an integer from 1 to 100");
      if (!Number.isInteger(offset) || (offset as number) < 0)
        throw new Error("offset must be a non-negative integer");
      const notes = await host.notes();
      return {
        total: notes.length,
        offset,
        notes: notes
          .slice(offset as number, (offset as number) + (limit as number))
          .map((note) => ({
            id: note.id,
            title: note.title,
            preview: notePreview(note.body),
            tags: note.tags,
            sourceCwd: note.sourceCwd,
          })),
      };
    }
    case "notes.read": {
      const id = requiredString(input.id, "id", 256);
      const note = await host.note(id);
      if (!note) throw new Error("Note was not found");
      return note;
    }
    case "notes.write": {
      const id = optionalString(input.id, "id", 256);
      if (id && !/^[A-Za-z0-9_-]+$/.test(id))
        throw new Error("Invalid note ID");
      const title =
        input.title === undefined
          ? undefined
          : requiredString(input.title, "title", 200);
      const body = input.body === undefined ? undefined : noteBody(input.body);
      const tags = input.tags === undefined ? undefined : noteTags(input.tags);
      if (id) {
        if (title === undefined && body === undefined && tags === undefined)
          throw new Error("Supply title, body or tags to update a note");
        const current = await host.note(id);
        if (!current) throw new Error("Note was not found");
        return host.saveNote({
          id,
          title: title ?? current.title,
          body: body ?? current.body,
          tags: tags ?? current.tags,
        });
      }
      if (body === undefined)
        throw new Error("body is required to create a note");
      if (!/^[A-Za-z0-9_-]{1,128}$/.test(requestId))
        throw new Error("Invalid request ID");
      const createdId = `app-${source.id}-${requestId}`;
      const existing = await host.note(createdId);
      if (existing) {
        if (
          existing.title !== (title ?? noteTitle(body)) ||
          existing.body !== body ||
          JSON.stringify(existing.tags) !== JSON.stringify(tags ?? [])
        )
          throw new Error("Request ID was already used for another note");
        return existing;
      }
      return host.saveNote({
        id: createdId,
        title: title ?? noteTitle(body),
        body,
        tags: tags ?? [],
        sourceSessionId: source.id,
        ...(looksLikeProject(source.cwd) ? { sourceCwd: source.cwd } : {}),
      });
    }
  }
}

/** Instructions for a thread without /operator, added to each of its turns. */
export function limitedAppPrompt(input: {
  cli: string;
  openSessions: boolean;
  reviewOpenedSessions?: boolean;
  peers: readonly LinkedPeer[];
}): string {
  const lines: string[] = [];
  if (input.openSessions) {
    const outcome = input.reviewOpenedSessions
      ? "A started session opens with its prompt saved as an unsent draft for the user to review."
      : 'A started session submits its prompt and starts working right away. Pass "draft":true to save the prompt unsent instead.';
    lines.push(
      `You can open MonoCode sessions for the user with \`${input.cli} sessions.list\` and \`${input.cli} sessions.start --json '{"prompt":"..."}'\`. ${outcome} Start one only when the user asks for it or clearly wants work split into a separate session.`,
    );
  }
  if (input.peers.length) {
    const peers = input.peers
      .map((peer) => `- ${peer.title.trim() || "Untitled session"} (${peer.id})`)
      .join("\n");
    lines.push(
      `The user linked this session to:\n${peers}\nRead one with \`${input.cli} links.read --json '{"sessionId":"..."}'\` and message it with \`${input.cli} links.send --json '{"sessionId":"...","message":"..."}'\`. A busy session gets the message after its current turn. Linked agents can send each other 5 messages before the user has to write again; if a send fails for that reason, tell the user.`,
    );
  }
  if (!lines.length) return "";
  return `<monocode_app>\n${lines.join("\n\n")}\nRun \`${input.cli} --help\` for the JSON fields. The CLI uses a credential already in your environment; never print it. Other app actions need the user to use /operator.\n</monocode_app>`;
}
