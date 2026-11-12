import { execFile } from "node:child_process";
import { stat, mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import type {
  GithubWorkItem,
  GithubWorkItemComment,
  GithubWorkItemThread,
} from "../src/features/inbox/model/githubTasks";
import type { GithubPrCheckState } from "../src/features/inbox/model/githubPrChecks";

const exec = promisify(execFile);
const MAX_DIFF_BYTES = 2 * 1024 * 1024;

/** Only the existing project-scoped Inbox operations, never caller-supplied argv. */
export const GITHUB_INBOX_COMMANDS = [
  "git_github_repo",
  "git_github_repositories",
  "git_github_work_items",
  "git_github_work_item",
  "git_github_work_item_details",
  "git_github_work_item_thread",
  "git_github_work_item_comment",
  "git_github_pr_action",
  "git_github_pr_diff",
  "git_github_pr_checks",
  "git_github_check_details",
] as const;
export type GithubInboxCommand = (typeof GITHUB_INBOX_COMMANDS)[number];

const avatar = (login = "") =>
  login.trim()
    ? `https://avatars.githubusercontent.com/${encodeURIComponent(login.trim())}?s=64`
    : "";
const text = (value: unknown) =>
  typeof value === "string" ? value.trim() : "";
function repoSlug(value: unknown): string {
  const repo = text(value);
  if (
    !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repo) ||
    repo.split("/").some((p) => p === "." || p === "..")
  )
    throw new Error("Invalid GitHub repository");
  return repo;
}
function itemKind(value: unknown): "issue" | "pr" {
  if (value !== "issue" && value !== "pr")
    throw new Error("Unknown GitHub task kind");
  return value;
}
function itemNumber(value: unknown): string {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value <= 0)
    throw new Error("Invalid GitHub item number");
  return String(value);
}
const itemFields = (kind: string) =>
  "number,title,url,state,createdAt,updatedAt,labels,assignees," +
  (kind === "pr" ? "isDraft" : "stateReason");
type ItemRow = Omit<GithubWorkItem, "repo" | "draft" | "kind"> & {
  isDraft?: boolean;
};
function workItem(
  row: ItemRow,
  repo: string,
  kind: "issue" | "pr",
): GithubWorkItem {
  return {
    kind,
    repo,
    number: row.number,
    title: row.title,
    url: row.url,
    state: row.state.toLowerCase(),
    stateReason: row.stateReason?.toLowerCase() ?? "",
    createdAt: row.createdAt ?? "",
    updatedAt: row.updatedAt ?? "",
    draft: row.isDraft ?? false,
    labels: (row.labels ?? []).map((label) => ({
      name: label.name,
      color: label.color ?? "",
    })),
    assignees: (row.assignees ?? []).map(({ login }) => ({
      login,
      avatarUrl: avatar(login),
    })),
  };
}

type ExecError = Error & { code?: string; stderr?: string; stdout?: string };
async function ghRaw(
  cwd: string,
  args: string[],
  allowEmpty = false,
): Promise<string> {
  // ENOENT from cwd and executable lookup otherwise looks identical to Node.
  const metadata = await stat(cwd).catch((error: Error) => {
    throw new Error(
      `Cannot access GitHub working directory '${cwd}': ${error.message}`,
    );
  });
  if (!metadata.isDirectory())
    throw new Error(`GitHub working directory '${cwd}' is not a directory.`);
  let output: string;
  try {
    output = (
      await exec("gh", args, {
        cwd,
        timeout: 30_000,
        maxBuffer: MAX_DIFF_BYTES,
        encoding: "utf8",
        env: {
          ...process.env,
          GH_PROMPT_DISABLED: "1",
          GIT_TERMINAL_PROMPT: "0",
          GH_PAGER: "cat",
          GIT_PAGER: "cat",
        },
      })
    ).stdout.trim();
  } catch (reason) {
    const error = reason as ExecError;
    if (error.code === "ENOENT") {
      // Recheck for a directory removed between stat and spawn.
      await stat(cwd).catch(() => {
        throw new Error(`Cannot access GitHub working directory '${cwd}'.`);
      });
      throw new Error("GitHub CLI (`gh`) is not installed on this machine.");
    }
    throw new Error(
      text(error.stderr) ||
        text(error.stdout) ||
        `Could not run GitHub CLI in '${cwd}': ${error.message}`,
    );
  }
  if (!output && !allowEmpty) throw new Error("gh returned no output");
  return output;
}

type RunGh = typeof ghRaw;
/** One shared backoff per Host, just as desktop webviews share Rust's backoff. */
export class GithubInbox {
  private backoff: { until: number; error: Error } | undefined;
  constructor(private readonly raw: RunGh = ghRaw) {}

  private async gh(
    cwd: string,
    args: string[],
    allowEmpty = false,
  ): Promise<string> {
    if (this.backoff && Date.now() < this.backoff.until)
      throw this.backoff.error;
    this.backoff = undefined;
    try {
      return await this.raw(cwd, args, allowEmpty);
    } catch (reason) {
      const error =
        reason instanceof Error ? reason : new Error(String(reason));
      const message = error.message.toLowerCase();
      const primary =
        message.includes("api rate limit") && message.includes("exceeded");
      const secondary =
        message.includes("secondary rate limit") ||
        message.includes("abuse detection");
      if ((primary || secondary) && !this.backoff) {
        this.backoff = { until: Date.now() + 60_000, error };
        if (primary && message.includes("graphql")) {
          try {
            const json = JSON.parse(
              await this.raw(cwd, [
                "api",
                "graphql",
                "-f",
                "query=query { rateLimit { remaining resetAt } }",
              ]),
            );
            const rate = json.data?.rateLimit;
            if (typeof rate?.remaining === "number" && rate.remaining >= 0) {
              if (rate.remaining > 0) this.backoff = undefined;
              else {
                const until = Date.parse(rate.resetAt) + 1_000;
                if (Number.isFinite(until))
                  this.backoff =
                    until > Date.now() ? { until, error } : undefined;
              }
            }
          } catch {
            /* Keep the one-minute fallback when reset lookup fails. */
          }
        }
      }
      throw error;
    }
  }

  async run(
    command: GithubInboxCommand,
    cwd: string,
    input: Record<string, unknown>,
  ): Promise<unknown> {
    if (
      command === "git_github_repo" ||
      command === "git_github_repositories"
    ) {
      const view = JSON.parse(
        await this.gh(cwd, [
          "repo",
          "view",
          "--json",
          command === "git_github_repo"
            ? "nameWithOwner"
            : "nameWithOwner,parent",
        ]),
      );
      const repo = repoSlug(view.nameWithOwner);
      if (command === "git_github_repo") return repo;
      const repos = [repo];
      if (view.parent) {
        const parent = repoSlug(
          `${view.parent.owner.login}/${view.parent.name}`,
        );
        if (repo.toLowerCase() !== parent.toLowerCase()) repos.push(parent);
      }
      return repos;
    }
    const repo = repoSlug(input.repo);
    if (command === "git_github_check_details")
      return this.checkDetails(cwd, repo, input.jobId);
    if (command === "git_github_work_items") {
      const kind = itemKind(input.kind);
      const limit =
        typeof input.limit === "number" && Number.isFinite(input.limit)
          ? Math.min(100, Math.max(1, Math.floor(input.limit)))
          : 40;
      const args = [
        kind,
        "list",
        "--state",
        text(input.state).toLowerCase() === "all" ? "all" : "open",
        "--limit",
        String(limit),
        "--repo",
        repo,
        "--json",
        itemFields(kind),
      ];
      if (input.assignedToMe === true) args.push("--assignee", "@me");
      if (text(input.search)) args.push("--search", text(input.search));
      return (JSON.parse(await this.gh(cwd, args)) as ItemRow[]).map((row) =>
        workItem(row, repo, kind),
      );
    }
    const number = itemNumber(input.number);
    if (command === "git_github_pr_checks") {
      const row = JSON.parse(
        await this.gh(cwd, [
          "pr",
          "view",
          number,
          "--repo",
          repo,
          "--json",
          "headRefOid,statusCheckRollup",
        ]),
      ) as ChecksRow;
      if (!text(row.headRefOid))
        throw new Error("Pull request is missing head commit");
      return {
        headOid: row.headRefOid.trim(),
        checks: (row.statusCheckRollup ?? []).map(checkFromRow),
      };
    }
    if (command === "git_github_pr_diff")
      return this.diff(cwd, repo, number, input.fullContext === true);
    if (command === "git_github_pr_action") {
      const actions: Record<string, string[]> = {
        merge: ["merge", "--merge"],
        squash: ["merge", "--squash"],
        rebase: ["merge", "--rebase"],
        draft: ["ready", "--undo"],
        ready: ["ready"],
        close: ["close"],
        reopen: ["reopen"],
      };
      const action = actions[text(input.action)];
      if (!action || !Object.hasOwn(actions, text(input.action)))
        throw new Error("Unknown GitHub pull request action");
      await this.gh(
        cwd,
        ["pr", action[0], number, "--repo", repo, ...action.slice(1)],
        true,
      );
      return this.viewItem(cwd, repo, "pr", number);
    }
    const kind = itemKind(input.kind);
    switch (command) {
      case "git_github_work_item":
        return this.viewItem(cwd, repo, kind, number);
      case "git_github_work_item_details": {
        const row = JSON.parse(
          await this.gh(cwd, [
            kind,
            "view",
            number,
            "--repo",
            repo,
            "--json",
            kind === "pr"
              ? "body,author,baseRefName,headRefName,reviewDecision"
              : "body,author",
          ]),
        );
        return {
          body: row.body ?? "",
          author: row.author?.login ?? "",
          authorAvatarUrl: avatar(row.author?.login),
          baseRefName: row.baseRefName ?? "",
          headRefName: row.headRefName ?? "",
          reviewDecision: row.reviewDecision ?? "",
        };
      }
      case "git_github_work_item_thread": {
        const [owner, name] = repo.split("/");
        const json = await this.gh(cwd, [
          "api",
          "graphql",
          "-f",
          `query=${kind === "pr" ? PR_THREAD_QUERY : ISSUE_THREAD_QUERY}`,
          "-F",
          `owner=${owner}`,
          "-F",
          `name=${name}`,
          "-F",
          `number=${number}`,
        ]);
        return threadFromJson(json, kind);
      }
      case "git_github_work_item_comment":
        return this.comment(
          cwd,
          repo,
          kind,
          number,
          input.body,
          input.inReplyTo,
        );
      default:
        throw new Error("Unsupported workspace command");
    }
  }

  private async viewItem(
    cwd: string,
    repo: string,
    kind: "issue" | "pr",
    number: string,
  ) {
    return workItem(
      JSON.parse(
        await this.gh(cwd, [
          kind,
          "view",
          number,
          "--repo",
          repo,
          "--json",
          itemFields(kind),
        ]),
      ),
      repo,
      kind,
    );
  }

  private async comment(
    cwd: string,
    repo: string,
    kind: string,
    number: string,
    body: unknown,
    reply: unknown,
  ) {
    const content = text(body),
      threadId = text(reply);
    if (!content) throw new Error("Comment cannot be empty");
    if (threadId && !/^[A-Za-z0-9_=-]{1,255}$/.test(threadId))
      throw new Error("Invalid review thread");
    const dir = await mkdtemp(join(tmpdir(), "monocode-comment-"));
    const path = join(dir, "body.md");
    try {
      await writeFile(path, content, { mode: 0o600 });
      if (threadId) {
        const response = JSON.parse(
          await this.gh(cwd, [
            "api",
            "graphql",
            "-f",
            `query=${REPLY_MUTATION}`,
            "-F",
            `threadId=${threadId}`,
            "-F",
            `body=@${path}`,
          ]),
        );
        if (response.errors?.[0]?.message)
          throw new Error(response.errors[0].message);
        return commentUrl(
          response.data?.addPullRequestReviewThreadReply?.comment?.url ?? "",
        );
      }
      return commentUrl(
        await this.gh(cwd, [
          kind,
          "comment",
          number,
          "--repo",
          repo,
          "--body-file",
          path,
        ]),
      );
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  }

  private async diff(cwd: string, repo: string, number: string, full: boolean) {
    const meta = JSON.parse(
      await this.gh(cwd, [
        "pr",
        "view",
        number,
        "--repo",
        repo,
        "--json",
        `files,additions,deletions${full ? ",baseRefOid,headRefOid" : ""}`,
      ]),
    ) as DiffRow;
    let patch = "",
      truncated = false;
    try {
      patch = full
        ? await this.fullDiff(cwd, meta.baseRefOid, meta.headRefOid)
        : await this.gh(cwd, ["pr", "diff", number, "--repo", repo], true);
    } catch (error) {
      if (
        (error as ExecError).code === "ERR_CHILD_PROCESS_STDIO_MAXBUFFER" ||
        (error as Error).message.includes("maxBuffer")
      )
        truncated = true;
      else throw error;
    }
    if (Buffer.byteLength(patch) > MAX_DIFF_BYTES) {
      patch = "";
      truncated = true;
    }
    const files = (meta.files ?? []).map(({ path, additions, deletions }) => ({
      path,
      additions,
      deletions,
    }));
    return {
      files,
      patch,
      truncated,
      additions:
        meta.additions || files.reduce((sum, f) => sum + f.additions, 0),
      deletions:
        meta.deletions || files.reduce((sum, f) => sum + f.deletions, 0),
    };
  }

  private async fullDiff(
    cwd: string,
    base: unknown,
    head: unknown,
  ): Promise<string> {
    if (
      ![base, head].every(
        (oid) => typeof oid === "string" && /^[0-9a-f]{40,64}$/i.test(oid),
      )
    )
      throw new Error("Pull request is missing base or head commit");
    const git = (args: string[]) =>
      exec("git", args, {
        cwd,
        timeout: 30_000,
        maxBuffer: MAX_DIFF_BYTES,
        encoding: "utf8",
        env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
      }).then((r) => r.stdout);
    const exists = (args: string[]) =>
      git(args).then(
        () => true,
        () => false,
      );
    let remotePromise: Promise<string | undefined> | undefined;
    const fetchRemote = () =>
      (remotePromise ??= (async () => {
        const remotes = (await git(["remote"]))
          .trim()
          .split(/\r?\n/)
          .filter(Boolean);
        const resolved = await git([
          "config",
          "--get-regexp",
          "remote\\..*\\.gh-resolved",
        ]).catch(() => "");
        let remote = remotes.find((name) =>
          resolved
            .split(/\r?\n/)
            .some(
              (line) => line.split(/\s/)[0] === `remote.${name}.gh-resolved`,
            ),
        );
        if (!remote) {
          const url = await this.gh(cwd, ["repo", "view", "--json", "url"])
            .then((s) => text(JSON.parse(s).url))
            .catch(() => "");
          const normalize = (s: string) =>
            s
              .trim()
              .replace(/^git@github.com:/, "https://github.com/")
              .replace(/\.git$|\/$/g, "")
              .toLowerCase();
          if (url)
            for (const name of remotes) {
              if (
                normalize(await git(["remote", "get-url", name])) ===
                normalize(url)
              ) {
                remote = name;
                break;
              }
            }
        }
        remote ??= remotes.includes("origin") ? "origin" : remotes[0];
        return remote;
      })());
    for (const oid of [base as string, head as string]) {
      if (!(await exists(["cat-file", "-e", `${oid}^{commit}`]))) {
        const remote = await fetchRemote();
        if (remote)
          await git(["fetch", "--no-tags", "--depth", "1", remote, oid]).catch(
            () => "",
          );
      }
      if (!(await exists(["cat-file", "-e", `${oid}^{commit}`])))
        throw new Error(
          `Missing git commit ${oid}. Fetch the pull request refs and try again.`,
        );
    }
    if (!(await exists(["merge-base", base as string, head as string]))) {
      const remote = await fetchRemote();
      if (remote)
        for (const deepen of ["50", "200", "800"]) {
          await git(["fetch", "--no-tags", "--deepen", deepen, remote]).catch(
            () => "",
          );
          if (await exists(["merge-base", base as string, head as string]))
            break;
        }
    }
    if (!(await exists(["merge-base", base as string, head as string])))
      throw new Error(
        `Cannot find a merge base for ${base} and ${head}. Fetch more history and try again.`,
      );
    return git([
      "diff",
      "--no-color",
      "--no-ext-diff",
      "--src-prefix=a/",
      "--dst-prefix=b/",
      "-U999999",
      `${base}...${head}`,
    ]);
  }

  private async checkDetails(cwd: string, repo: string, id: unknown) {
    const jobId = text(id);
    if (!/^[1-9]\d*$/.test(jobId))
      throw new Error("Invalid GitHub repository or job ID");
    const prefix = `repos/${repo}`;
    const api = (endpoint: string) =>
      this.gh(cwd, ["api", "--hostname", "github.com", endpoint]).then((s) =>
        JSON.parse(s),
      );
    const job = (await api(`${prefix}/actions/jobs/${jobId}`)) as JobRow;
    if (String(job.id) !== jobId)
      throw new Error("GitHub returned a different job");
    const details = {
      steps: (job.steps ?? []).map((step) => ({
        name: step.name,
        state: checkState(step.status, step.conclusion),
        startedAt: step.started_at ?? null,
        completedAt: step.completed_at ?? null,
      })),
      annotations: [] as {
        path: string;
        line: number;
        message: string;
        level: string;
      }[],
      notice: null as string | null,
    };
    const checkPrefix = `https://api.github.com/${prefix}/check-runs/`;
    const checkId = job.check_run_url?.startsWith(checkPrefix)
      ? job.check_run_url.slice(checkPrefix.length)
      : "";
    if (!checkId || !/^\d+$/.test(checkId))
      details.notice =
        "Error annotations are unavailable. View the full log on GitHub.";
    else
      try {
        const rows = (await api(
          `${prefix}/check-runs/${checkId}/annotations?per_page=100`,
        )) as AnnotationRow[];
        details.annotations = rows.map((row) => ({
          path: row.path ?? "",
          line: row.start_line ?? 0,
          message: row.message ?? "",
          level: row.annotation_level ?? "",
        }));
        if (rows.length === 100)
          details.notice =
            "Showing the first 100 annotations. View the full log on GitHub for more.";
      } catch {
        details.notice =
          "Could not load error annotations. View the full log on GitHub.";
      }
    return details;
  }
}

function commentUrl(output: string): string {
  const url = output
    .split(/\r?\n/)
    .reverse()
    .map((s) => s.trim())
    .find((s) => /^https?:\/\//.test(s));
  if (!url)
    throw new Error(output.trim() || "GitHub did not return a comment URL");
  return url;
}
type DiffRow = {
  files?: { path: string; additions: number; deletions: number }[];
  additions?: number;
  deletions?: number;
  baseRefOid?: string;
  headRefOid?: string;
};
type CheckRow = {
  __typename?: string;
  name?: string;
  context?: string;
  status?: string;
  conclusion?: string;
  state?: string;
  workflowName?: string;
  detailsUrl?: string;
  targetUrl?: string;
  createdAt?: string;
  startedAt?: string;
  completedAt?: string;
};
type ChecksRow = { headRefOid: string; statusCheckRollup?: CheckRow[] };
type JobRow = {
  id: number;
  check_run_url?: string;
  steps?: {
    name: string;
    status: string;
    conclusion?: string;
    started_at?: string;
    completed_at?: string;
  }[];
};
type AnnotationRow = {
  path?: string;
  start_line?: number;
  message?: string;
  annotation_level?: string;
};
function conclusionState(value = ""): GithubPrCheckState {
  switch (value.trim().toUpperCase()) {
    case "SUCCESS":
      return "pass";
    case "FAILURE":
    case "ERROR":
    case "TIMED_OUT":
    case "ACTION_REQUIRED":
    case "STARTUP_FAILURE":
      return "fail";
    case "QUEUED":
    case "IN_PROGRESS":
    case "PENDING":
    case "WAITING":
    case "REQUESTED":
      return "pending";
    case "NEUTRAL":
    case "SKIPPED":
      return "skipping";
    case "CANCELLED":
      return "cancel";
    default:
      return "unknown";
  }
}
function checkState(status = "", conclusion = ""): GithubPrCheckState {
  const s = status.trim().toUpperCase();
  if (["QUEUED", "IN_PROGRESS", "PENDING", "WAITING", "REQUESTED"].includes(s))
    return "pending";
  return s === "COMPLETED" || !s ? conclusionState(conclusion) : "unknown";
}
function checkFromRow(row: CheckRow) {
  const context = row.__typename
    ? row.__typename.toLowerCase() === "statuscontext"
    : !!row.context;
  return {
    name: (context ? row.context : row.name) ?? "",
    workflow: context ? "" : (row.workflowName ?? ""),
    state: context
      ? conclusionState(row.state)
      : checkState(row.status, row.conclusion),
    url: text(context ? row.targetUrl : row.detailsUrl) || null,
    startedAt: (context ? row.createdAt : row.startedAt) ?? null,
    completedAt: context ? null : (row.completedAt ?? null),
  };
}

type CommentRow = {
  id?: string;
  author?: { login?: string };
  body?: string;
  createdAt?: string;
  url?: string;
  path?: string;
  line?: number;
  originalLine?: number;
  isMinimized?: boolean;
  state?: string;
  submittedAt?: string;
};
type Nodes<T> = { nodes: T[]; totalCount: number };
type ThreadRow = {
  comments?: Nodes<CommentRow>;
  reviews?: Nodes<CommentRow>;
  reviewThreads?: Nodes<{
    id: string;
    path: string;
    isResolved: boolean;
    comments: Nodes<CommentRow>;
  }>;
  commits?: Nodes<{
    commit: {
      oid: string;
      committedDate: string;
      messageHeadline: string;
      url: string;
      author?: { name?: string; user?: { login?: string } };
    };
  }>;
  reviewDecision?: string;
  baseRefName?: string;
  headRefName?: string;
};
function mappedComment(
  row: CommentRow,
  kind: string,
  path = "",
  resolved = false,
): GithubWorkItemComment | null {
  if (row.isMinimized) return null;
  const author = text(row.author?.login),
    createdAt = text(row.createdAt);
  return {
    id: text(row.id) || `${kind}:${author}:${createdAt}`,
    kind,
    author,
    authorAvatarUrl: avatar(author),
    body: row.body ?? "",
    createdAt,
    url: row.url ?? "",
    state: "",
    path: text(row.path) || text(path),
    line: row.line ?? row.originalLine ?? null,
    resolved,
    threadId: "",
    replies: [],
  };
}
function threadFromJson(json: string, kind: string): GithubWorkItemThread {
  const response = JSON.parse(json);
  const row = response.data?.repository?.[
    kind === "pr" ? "pullRequest" : "issue"
  ] as ThreadRow | undefined;
  if (!row)
    throw new Error(response.errors?.[0]?.message || "GitHub item not found");
  const comments: GithubWorkItemComment[] = [];
  for (const comment of row.comments?.nodes ?? []) {
    const mapped = mappedComment(comment, "comment");
    if (mapped) comments.push(mapped);
  }
  for (const review of row.reviews?.nodes ?? []) {
    const state = text(review.state).toUpperCase();
    if (
      !state ||
      state === "PENDING" ||
      (state === "COMMENTED" && !text(review.body)) ||
      !text(review.submittedAt)
    )
      continue;
    const mapped = mappedComment(
      { ...review, createdAt: review.submittedAt },
      "review",
    );
    if (mapped) comments.push({ ...mapped, state });
  }
  for (const thread of row.reviewThreads?.nodes ?? []) {
    const mapped = thread.comments.nodes
      .map((c) =>
        mappedComment(c, "review_comment", thread.path, thread.isResolved),
      )
      .filter((c): c is GithubWorkItemComment => c !== null)
      .map((c) => ({ ...c, threadId: thread.id }));
    if (mapped[0]) comments.push({ ...mapped[0], replies: mapped.slice(1) });
  }
  comments.sort(
    (a, b) =>
      a.createdAt.localeCompare(b.createdAt) || a.id.localeCompare(b.id),
  );
  const commits = (row.commits?.nodes ?? [])
    .map(({ commit }) => commit)
    .filter((c) => text(c.oid) && text(c.committedDate))
    .map((c) => ({
      oid: c.oid,
      messageHeadline: c.messageHeadline,
      committedDate: c.committedDate,
      url: c.url,
      author: text(c.author?.user?.login) || c.author?.name || "",
    }));
  const truncated = [
    row.comments,
    row.reviews,
    row.reviewThreads,
    row.commits,
  ].some((n) => n && n.nodes.length < n.totalCount);
  return {
    comments,
    commits,
    truncated,
    reviewDecision: row.reviewDecision ?? "",
    baseRefName: row.baseRefName ?? "",
    headRefName: row.headRefName ?? "",
  };
}

const ISSUE_THREAD_QUERY = `query InboxIssueThread($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) { issue(number: $number) {
    comments(last: 40) { totalCount nodes { id author { login } body createdAt url isMinimized } }
  } }
}`;
const PR_THREAD_QUERY = `query InboxPullRequestThread($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) { pullRequest(number: $number) {
    reviewDecision baseRefName headRefName
    commits(last: 40) { totalCount nodes { commit { oid messageHeadline committedDate url author { name user { login } } } } }
    comments(last: 40) { totalCount nodes { id author { login } body createdAt url isMinimized } }
    reviews(last: 40) { totalCount nodes { id author { login } body state submittedAt url } }
    reviewThreads(last: 20) { totalCount nodes { id isResolved path comments(first: 8) { totalCount nodes { id author { login } body createdAt url path line originalLine isMinimized } } } }
  } }
}`;
const REPLY_MUTATION = `mutation InboxReviewReply($threadId: ID!, $body: String!) {
  addPullRequestReviewThreadReply(input: { pullRequestReviewThreadId: $threadId body: $body }) { comment { url } }
}`;
