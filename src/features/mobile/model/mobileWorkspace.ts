import { invoke } from "@tauri-apps/api/core";
import {
  gitCommit,
  gitDiffFiles,
  gitDiffIndex,
  gitFileDiff,
  gitPrCreate,
  gitPrStatus,
  gitPush,
  gitStageAll,
  gitStageFile,
  gitSync,
  gitUnstageAll,
  gitUnstageFile,
  notifyGitChanged,
  type FsEntry,
} from "../../../platform/tauri/fs";
import { generateCommitMessage, generatePrContent } from "../../../integrations/harness";
import { remotePrContent } from "../../source-control/ui/GitChangesPanel";
import {
  loadProjectFiles,
  rankProjectFiles,
  resolveOpenablePath,
} from "../../files/model/fileIndex";
import { recordInboxSelfActivity } from "../../inbox/model/inboxSelfActivity";
import { buildUnifiedFile } from "../../source-control/model/unifiedDiff";
import { listWorktrees } from "../../source-control/model/worktrees";
import {
  highlightDiffForMobile,
  highlightForMobile,
  highlightInBackground,
  highlightProgress,
} from "./mobileHighlight";
import {
  isRemoteProjectPath,
  loadRecents,
  loadPinnedProjects,
  sameProjectPath,
} from "../../projects/model/recents";
import { listMonos, monoLook, monoState } from "../../monos/model/mono";
import { displayPath, pathKey, projectName } from "../../../shared/lib/paths";
import {
  getSession,
  listSessionsByProject,
  type SessionSummary,
} from "../../sessions/data/sessionStore";
import {
  HARNESSES,
  HARNESS_TITLE,
  RUNTIME_MODES,
  RUNTIME_MODE_HINT,
  RUNTIME_MODE_LABEL,
  hasPendingApproval,
  newDefaultSession,
  sessionDisplayTitle,
  type HarnessId,
  type RuntimeMode,
  type Session,
} from "../../sessions/model/session";
import {
  isPickerProviderVisible,
  mergeModelSettings,
  modelsFor,
  resolveModel,
} from "../../sessions/model/models";
import { isHarnessAvailable } from "../../../integrations/harness/core/availability";
import { mobileTranscript } from "./mobileTranscript";
import {
  CUSTOM_OPTION_ID,
  buildQuestionReply,
  type UserQuestionPrompt,
  type UserQuestionReply,
} from "../../sessions/model/userQuestion";
import {
  mobileAccountChoice,
  mobileProviderAccounts,
  mobileSessionAccountId,
} from "./mobileProviderAccounts";
import {
  resolveTabGroupColor,
  resolveTabGroupMascot,
  loadTabGroupColors,
  loadTabGroupMascots,
} from "../../workspace/model/tabGroups";

export const mobileProjectId = (path: string) => encodeURIComponent(path);

/**
 * A session the phone starts in `cwd`, as a new desktop tab would start it:
 * the project's or the last chosen provider and model, and the permission mode
 * of the desktop's active session.
 */
export function mobileNewSession(cwd: string, runtimeMode?: RuntimeMode): Session {
  return newDefaultSession(cwd, runtimeMode);
}

export async function mobileCatalog(
  live: readonly Session[],
  runtimeMode?: RuntimeMode,
) {
  const roster = listMonos();
  const paths = [
    ...new Set([
      ...loadRecents().map((project) => project.path),
      ...loadPinnedProjects(),
      ...roster.flatMap((mono) => mono.projects),
    ]),
  ].filter((path) => path && path !== "~");
  const colors = loadTabGroupColors();
  const mascots = loadTabGroupMascots();
  const projects = paths.map((path) => {
    const defaults = mobileNewSession(path, runtimeMode);
    return {
      id: mobileProjectId(path),
      path,
      name: projectName(path),
      machine: "Desktop",
      branch: "",
      color: resolveTabGroupColor(path, colors),
      mascot: resolveTabGroupMascot(path, mascots),
      additions: 0,
      deletions: 0,
      defaultHarness: defaults.harness,
      defaultModel: defaults.model,
      defaultModelName: resolveModel(defaults.harness, defaults.model).name,
      defaultRuntimeMode: defaults.runtimeMode,
    };
  });
  const summaries = (
    await Promise.all(
      paths.map((path) => listSessionsByProject(path).catch(() => [])),
    )
  ).flat();
  const sessions = new Map<string, ReturnType<typeof mobileSummary>>();
  for (const summary of summaries) {
    if (
      !summary.archived &&
      !summary.sidebarHidden &&
      !summary.orchestrationLeadId
    )
      sessions.set(summary.id, mobileSummary(summary));
  }
  for (const session of live) {
    if (
      session.inboxAsk ||
      session.orchestrationLeadId ||
      session.sidebarHidden ||
      roster.some((mono) => mono.sessionId === session.id)
    )
      continue;
    if (paths.some((path) => sameProjectPath(path, session.cwd)))
      sessions.set(session.id, {
        id: session.id,
        projectId: mobileProjectId(session.cwd),
        title: sessionDisplayTitle(session.title, session.harness),
        harness: session.harness,
        model: session.model,
        modelName: resolveModel(session.harness, session.model).name,
        branch: session.worktreeRemoved ? "" : (session.branch ?? ""),
        ...mobileSessionWorktree(session),
        runtimeMode: session.runtimeMode,
        updatedAt:
          [...session.blocks].reverse().find((block) => block.startedAt)
            ?.startedAt ?? 0,
        busy: !!session.busy,
        // What it waits on the user for, so the phone can notify of it.
        ...(hasPendingApproval(session.blocks)
          ? { waiting: "approval" as const }
          : session.pendingQuestion
            ? { waiting: "question" as const }
            : {}),
        live: true,
      });
  }
  const monos = await Promise.all(
    roster.map(async (mono) => {
      const session =
        live.find((session) => session.id === mono.sessionId) ??
        (mono.sessionId
          ? await getSession(mono.sessionId).catch(() => undefined)
          : undefined);
      const messages = session ? mobileMessages(session) : [];
      const last = messages[messages.length - 1];
      const lastActiveAt = session
        ? ([...session.blocks].reverse().find((block) => block.startedAt)
            ?.startedAt ?? 0)
        : 0;
      return {
        id: mono.id,
        name: monoLook(mono).name,
        mascot: mono.mascot,
        color: mono.color,
        projectIds: mono.projects.map(mobileProjectId),
        unread: 0,
        status: session ? monoState(session).status : "idle",
        busy: !!session?.busy,
        live: live.some((current) => current.id === mono.sessionId),
        sessionId: mono.sessionId,
        harness: session?.harness,
        model: session?.model,
        lastMessage: last?.text ?? "",
        lastActiveAt,
      };
    }),
  );
  return {
    projects,
    monos,
    sessions: [...sessions.values()].sort((a, b) => b.updatedAt - a.updatedAt),
  };
}
/** The worktree a session works in, so the phone shows its changes and files
 *  rather than the checkout's; none once the worktree is gone. */
function mobileSessionWorktree(session: {
  worktreeCwd?: string;
  worktreeRemoved?: boolean;
}) {
  return session.worktreeCwd && !session.worktreeRemoved
    ? { worktreePath: session.worktreeCwd }
    : {};
}
function mobileSummary(session: SessionSummary) {
  return {
    id: session.id,
    projectId: mobileProjectId(session.cwd),
    title: sessionDisplayTitle(session.title, session.harness),
    harness: session.harness,
    model: session.model,
    modelName: resolveModel(session.harness, session.model).name,
    branch: session.branch ?? "",
    ...mobileSessionWorktree(session),
    runtimeMode: session.runtimeMode,
    updatedAt: session.updatedAt,
    busy: false,
    live: false,
  };
}
export function mobileMessages(session: Session) {
  return session.blocks
    .filter(
      (block) =>
        (block.role === "user" || block.role === "assistant") &&
        !block.internal &&
        !block.draft,
    )
    .slice(-200)
    .map((block) => ({ id: block.id, role: block.role, text: block.text }));
}
/**
 * A conversation as the phone shows it. A session the desktop does not have
 * open is read from disk once into `saved` and reused from there: it cannot
 * change until it opens, and then the live copy wins.
 */
export async function mobileConversation(
  id: string,
  live: readonly Session[],
  saved?: Map<string, Session>,
  range?: (turnIds: string[]) => [number, number],
) {
  const mono = id.startsWith("mono:")
    ? listMonos().find((mono) => mono.id === id.slice(5))
    : undefined;
  if (id.startsWith("mono:") && !mono)
    throw new Error("Mono is no longer available");
  const sessionId = mono ? mono.sessionId : id;
  const session = sessionId
    ? (live.find((session) => session.id === sessionId) ??
      (await readSaved(sessionId, saved)))
    : undefined;
  return {
    id,
    messages: session ? mobileMessages(session) : [],
    turns: session
      ? mobileTranscript(session, {
          busy: !!session.busy,
          mono: mono ? { name: monoLook(mono).name } : undefined,
          pendingQuestion: !!session.pendingQuestion,
          backgroundTasks: session.backgroundTasks,
          range,
        })
      : [],
    busy: !!session?.busy,
    // What the agent asks, for the phone to answer as the desktop's card does.
    question: session?.pendingQuestion
      ? {
          requestId: session.pendingQuestion.requestId,
          title: session.pendingQuestion.title,
          questions: session.pendingQuestion.questions,
          autoResolveAt: session.pendingQuestion.autoResolveAt,
        }
      : null,
  };
}

/**
 * What the phone's model sheet offers: each provider the desktop's model
 * picker shows, its models with their settings (effort, fast mode and the
 * rest come as each model's own settings), the permission modes, and what a
 * conversation, or a new session in a project, would run with now.
 */
export async function mobileModels(
  params: { projectId?: string; conversationId?: string },
  live: readonly Session[],
  saved?: Map<string, Session>,
  runtimeMode?: RuntimeMode,
) {
  const harnesses = HARNESSES.filter(
    (id) => isHarnessAvailable(id) && isPickerProviderVisible(id),
  );
  let current: Session | undefined;
  if (params.conversationId) {
    const mono = params.conversationId.startsWith("mono:")
      ? listMonos().find((mono) => mono.id === params.conversationId!.slice(5))
      : undefined;
    const sessionId = mono ? mono.sessionId : params.conversationId;
    current = sessionId
      ? (live.find((session) => session.id === sessionId) ??
        (await readSaved(sessionId, saved)) ??
        undefined)
      : undefined;
  }
  if (!current) {
    const path = mobileProjectPaths().find(
      (path) => mobileProjectId(path) === params.projectId,
    );
    if (!path) throw new Error("Project is no longer available");
    current = mobileNewSession(path, runtimeMode);
  }
  const model = resolveModel(current.harness, current.model);
  return {
    harnesses: harnesses.map((id) => ({ id, label: HARNESS_TITLE[id] })),
    models: harnesses.flatMap((harness) =>
      modelsFor(harness).map((entry) => ({
        id: entry.id,
        harness: entry.harness,
        name: entry.name,
        provider: entry.provider?.name,
        settings: entry.settings ?? [],
      })),
    ),
    runtimeModes: RUNTIME_MODES.map((id) => ({
      id,
      label: RUNTIME_MODE_LABEL[id],
      hint: RUNTIME_MODE_HINT[id],
    })),
    ...mobileProviderAccounts(current.cwd),
    current: {
      harness: current.harness,
      model: model.id,
      modelSettings: mergeModelSettings(model, current.modelSettings),
      runtimeMode: current.runtimeMode,
      providerAccountId: mobileSessionAccountId(current),
    },
  };
}

/** A model sheet choice from the phone, keeping only what is well formed. */
export function mobileChoice(params: Record<string, unknown>): {
  harness?: HarnessId;
  model?: string;
  modelSettings?: Record<string, string>;
  runtimeMode?: RuntimeMode;
} {
  const harness = HARNESSES.find((id) => id === params.harness);
  const model =
    typeof params.model === "string" && params.model.length <= 256
      ? params.model
      : undefined;
  const raw = params.modelSettings;
  const modelSettings =
    raw && typeof raw === "object" && !Array.isArray(raw)
      ? Object.fromEntries(
          Object.entries(raw).filter(
            (entry): entry is [string, string] =>
              typeof entry[1] === "string" && entry[1].length <= 64,
          ),
        )
      : undefined;
  const runtimeMode = RUNTIME_MODES.find((mode) => mode === params.runtimeMode);
  return {
    ...(harness && model ? { harness, model } : {}),
    ...(modelSettings ? { modelSettings } : {}),
    ...(runtimeMode ? { runtimeMode } : {}),
  };
}

/**
 * The phone's answers to a question, keeping only the questions and options it
 * asked, and rebuilt as the desktop's card builds them. Undefined when malformed,
 * including answers that leave nothing answered: only an explicit skip skips.
 */
export function mobileQuestionReply(
  value: unknown,
  prompt: UserQuestionPrompt,
): UserQuestionReply | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return;
  const reply = value as Record<string, unknown>;
  if (reply.kind === "skipped") return { kind: "skipped" };
  if (reply.kind !== "answered") return;
  const answers: Record<string, string[]> = {};
  const custom: Record<string, string> = {};
  const record = (field: unknown) =>
    field && typeof field === "object" && !Array.isArray(field)
      ? (field as Record<string, unknown>)
      : {};
  for (const question of prompt.questions) {
    const selected = record(reply.answers)[question.id];
    if (Array.isArray(selected))
      answers[question.id] = selected.filter(
        (id): id is string =>
          (id === CUSTOM_OPTION_ID && question.allowCustom) ||
          question.options.some((option) => option.id === id),
      );
    const text = record(reply.custom)[question.id];
    if (typeof text === "string" && text.length <= 10_000)
      custom[question.id] = text;
  }
  const rebuilt = buildQuestionReply(prompt.questions, answers, custom);
  return rebuilt.kind === "answered" ? rebuilt : undefined;
}

/** A new session from the phone, set to the model sheet's choice if it made one. */
export function applyMobileChoice(
  session: Session,
  params: Record<string, unknown>,
): Session {
  const choice = mobileChoice(params);
  const model = choice.harness
    ? resolveModel(choice.harness, choice.model)
    : resolveModel(session.harness, session.model);
  return {
    ...session,
    harness: model.harness,
    model: model.id,
    providerAccountId: mobileAccountChoice(model.harness, session.cwd, params),
    modelSettings: mergeModelSettings(
      model,
      choice.modelSettings ?? session.modelSettings,
    ),
    runtimeMode: choice.runtimeMode ?? session.runtimeMode,
  };
}

/** The other working copies a phone can start a session in, as the desktop
 *  composer's workspace picker lists them. */
export async function mobileProjectWorktrees(projectId: string) {
  const cwd = mobileProjectPaths().find((path) => mobileProjectId(path) === projectId);
  if (!cwd) throw new Error("Project is no longer available");
  const { worktrees } = await listWorktrees(cwd);
  return {
    worktrees: worktrees
      .filter((tree) => !tree.isMain && !tree.missing)
      .map((tree) => ({ path: tree.path, name: tree.branch ?? projectName(tree.path) })),
  };
}

/**
 * Where a phone's new session works: a new worktree off HEAD, made on the
 * first send as automations do, or one of the project's existing worktrees,
 * checked against its list rather than trusted. Else the project's checkout.
 */
export async function mobileWorkspaceChoice(
  cwd: string,
  params: Record<string, unknown>,
): Promise<Partial<Session>> {
  if (params.workspaceMode === "worktree")
    return { workspaceMode: "worktree", worktreeBase: "HEAD" };
  if (typeof params.worktreePath !== "string") return {};
  const tree = await mobileWorktree(cwd, params.worktreePath);
  return { worktreeCwd: tree.path, branch: tree.branch ?? undefined };
}

/** One of the project's other worktrees, by a path the phone named. */
async function mobileWorktree(cwd: string, path: string) {
  const wanted = pathKey(path);
  const tree = (await listWorktrees(cwd)).worktrees.find(
    (tree) => !tree.isMain && !tree.missing && pathKey(tree.path) === wanted,
  );
  if (!tree) throw new Error("That worktree is no longer available");
  return tree;
}

function mobileProjectPaths() {
  return [
    ...loadRecents().map((project) => project.path),
    ...loadPinnedProjects(),
    ...listMonos().flatMap((mono) => mono.projects),
  ];
}

async function readSaved(sessionId: string, saved?: Map<string, Session>) {
  const cached = saved?.get(sessionId);
  if (cached) return cached;
  const started = performance.now();
  const session = await getSession(sessionId);
  console.info(
    `[mobile] read ${sessionId} from disk: ${Math.round(performance.now() - started)}ms, ` +
      `${session?.blocks.length ?? 0} blocks`,
  );
  if (session) saved?.set(sessionId, session);
  return session;
}

/** The project's checkout, or one of its worktrees when the phone names it. */
async function mobileRoot(projectId: string, worktreePath?: string) {
  const cwd = mobileProjectPaths().find((path) => mobileProjectId(path) === projectId);
  if (!cwd) throw new Error("Project is no longer available");
  return worktreePath ? (await mobileWorktree(cwd, worktreePath)).path : cwd;
}

/** One folder of a working copy, by its path relative to the root. */
export async function mobileProjectFiles(
  projectId: string,
  relative: string,
  worktreePath?: string,
) {
  const root = await mobileRoot(projectId, worktreePath);
  const entries = await invoke<FsEntry[]>("mobile_list_dir", { root, relative });
  return {
    entries: entries.map((entry) => ({
      name: entry.name,
      isDir: entry.isDir,
      ignored: entry.ignored,
    })),
  };
}

/** Files whose paths match a phone's search, best first, from the same index
 *  and ranking as the desktop's quick open. */
export async function mobileProjectSearch(
  projectId: string,
  query: string,
  worktreePath?: string,
) {
  const root = await mobileRoot(projectId, worktreePath);
  if (!query.trim()) return { results: [] };
  // Files only: a folder has nothing for the phone to open.
  const files = (await loadProjectFiles(root)).filter((file) => !file.isDir);
  return {
    results: rankProjectFiles(files, query, [], 50).map((file) => ({
      path: file.relative,
      name: file.name,
    })),
  };
}

/** Enough of a file to read on a phone, at most this many characters: tens of
 *  thousands of lines, well inside a reply's 8MB. Its colours, being larger,
 *  come separately a chunk at a time. */
const MOBILE_FILE_CHARS = 1024 * 1024;

/** One text file of a working copy, by its path relative to the root or as a
 *  transcript names it, with the path it was found at and its syntax colours
 *  when the desktop can highlight it. A large file comes
 *  plain with a background highlight's ID, for the phone to collect its
 *  colours from `mobileFileHighlight` as they are ready. */
export async function mobileProjectFile(
  projectId: string,
  relative: string,
  worktreePath?: string,
) {
  const root = await mobileRoot(projectId, worktreePath);
  let path = relative;
  let full: string;
  try {
    full = await invoke<string>("mobile_read_text_file", { root, relative });
  } catch (error) {
    // A file named in a transcript may be shortened (`Composer.tsx`) or carry
    // a line (`app.ts:12`); find it as the desktop's transcript links do.
    const resolved = await resolveOpenablePath(root, relative).catch(() => undefined);
    if (!resolved || resolved === relative) throw error;
    full = await invoke<string>("mobile_read_text_file", { root, relative: resolved });
    path = displayPath(resolved, root);
  }
  const text = full.slice(0, MOBILE_FILE_CHARS);
  const highlighted = await highlightForMobile(path, text);
  const highlightJob = highlighted ? undefined : highlightInBackground(path, text);
  return {
    path,
    text,
    truncated: full.length > MOBILE_FILE_CHARS,
    ...(highlighted ? { highlighted } : {}),
    ...(highlightJob ? { highlightJob } : {}),
  };
}

/** The colours a background highlight has finished, from line `from` on. */
export function mobileFileHighlight(job: string, from: number) {
  return highlightProgress(job, Math.max(0, from));
}

/** The uncommitted changes in the project's checkout, or in one of its
 *  worktrees when the phone names it. */
export async function mobileProjectChanges(projectId: string, worktreePath?: string) {
  const path = await mobileRoot(projectId, worktreePath);
  const index = await gitDiffFiles(path);
  let budget = 1_000_000;
  const files = [];
  for (const file of index.files) {
    let lines: (ReturnType<typeof buildUnifiedFile>["lines"][number] & {
      runs?: [string, number][];
    })[] = [];
    let palette: string[] | undefined;
    if (budget > 0 && files.length < 30) {
      const diff = await gitFileDiff(
        path,
        file.relative,
        file.unstaged ? "unstaged" : "staged",
      ).catch(() => null);
      if (
        diff &&
        !diff.binary &&
        !diff.tooLarge &&
        diff.original.length + diff.current.length <= budget
      ) {
        budget -= diff.original.length + diff.current.length;
        lines = buildUnifiedFile(diff.original, diff.current).lines;
        // Its syntax colours too; the diff is still worth sending without them.
        const highlighted = await highlightDiffForMobile(
          file.relative,
          diff.original,
          diff.current,
          lines,
        );
        if (highlighted) ({ lines, palette } = highlighted);
      }
    }
    files.push({
      relative: file.relative,
      status: ["added", "deleted", "untracked"].includes(file.status)
        ? file.status
        : "modified",
      additions: file.additions,
      deletions: file.deletions,
      staged: file.staged,
      unstaged: file.unstaged,
      lines,
      ...(palette ? { palette } : {}),
    });
  }
  // Echoes the worktree, so the phone can tell these changes are the ones it asked for.
  return { branch: index.branch ?? "", files, ...(worktreePath ? { worktreePath } : {}) };
}

/** What the phone's commit sheet needs to offer what the desktop's Changes
 *  panel would: whether there is a remote, how the branch stands against it
 *  and the default branch, and its open pull request. */
export async function mobileProjectGit(projectId: string, worktreePath?: string) {
  const cwd = await mobileRoot(projectId, worktreePath);
  const [index, pr] = await Promise.all([
    gitDiffIndex(cwd),
    gitPrStatus(cwd).catch(() => null),
  ]);
  return {
    branch: index.branch ?? "",
    files: index.files.length,
    remote: Boolean(index.remote),
    upstream: Boolean(index.upstream),
    defaultBranch: index.defaultBranch ?? "",
    ahead: index.ahead,
    behind: index.behind,
    aheadOfDefault: index.aheadOfDefault,
    canGenerate: !isRemoteProjectPath(cwd),
    pr: pr?.state === "open" ? { number: pr.number, url: pr.url } : null,
  };
}

/**
 * A Changes page action from the phone, as the desktop's Changes panel runs
 * it: staging, then committing what is staged, pushing and pull requests.
 */
export async function mobileProjectGitAction(
  projectId: string,
  worktreePath: string | undefined,
  params: Record<string, unknown>,
): Promise<{ message?: string; url?: string }> {
  const cwd = await mobileRoot(projectId, worktreePath);
  // The harness picked in the phone's composer writes the commit message and
  // pull request, as the desktop's panel uses its session's.
  const harness = HARNESSES.find((id) => id === params.harness);
  const paths = Array.isArray(params.paths)
    ? params.paths.filter((path): path is string => typeof path === "string")
    : [];
  const recordPush = async () => {
    const pr = await gitPrStatus(cwd).catch(() => null);
    if (pr) recordPrActivity(cwd, pr.number);
  };
  try {
    switch (params.action) {
      case "stage":
      case "unstage": {
        const run = params.action === "stage" ? gitStageFile : gitUnstageFile;
        for (const path of paths) await run(cwd, path);
        return {};
      }
      case "stageAll":
        await gitStageAll(cwd);
        return {};
      case "unstageAll":
        await gitUnstageAll(cwd);
        return {};
      case "generate": {
        if (isRemoteProjectPath(cwd))
          throw new Error("Commit messages can't be generated for remote projects");
        return { message: await generateCommitMessage(cwd, harness) };
      }
      case "commit": {
        const message = typeof params.message === "string" ? params.message : "";
        await gitCommit(cwd, message, false);
        if (params.push !== true && params.createPr !== true) return {};
        await gitPush(cwd);
        await recordPush();
        return params.createPr === true
          ? { url: await createMobilePr(cwd, harness) }
          : {};
      }
      case "sync": {
        const pushes = (await gitDiffIndex(cwd)).ahead > 0;
        await gitSync(cwd);
        if (pushes) await recordPush();
        return {};
      }
      case "pr": {
        if ((await gitDiffIndex(cwd)).ahead > 0) await gitPush(cwd);
        return { url: await createMobilePr(cwd, harness) };
      }
      default:
        throw new Error("Unsupported git action");
    }
  } finally {
    notifyGitChanged();
  }
}

async function createMobilePr(cwd: string, harness?: HarnessId) {
  const content = isRemoteProjectPath(cwd)
    ? await remotePrContent(cwd)
    : await generatePrContent(cwd, harness);
  if (!content) throw new Error("Could not prepare pull request content");
  const url = (
    await gitPrCreate(cwd, content.title, content.body, content.base, content.head)
  ).trim();
  const number = Number(/\/pull\/(\d+)(?:[/?#]|$)/.exec(url)?.[1]);
  if (Number.isInteger(number) && number > 0) recordPrActivity(cwd, number);
  return url;
}

function recordPrActivity(cwd: string, number: number) {
  recordInboxSelfActivity({ provider: "github", kind: "pr", number, projectPath: cwd });
}
