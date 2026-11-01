import { applyMark, needsMe, isSnoozed } from "./model";
import type {
  InboxCheck,
  InboxClient,
  InboxDetail,
  InboxDiff,
  InboxItem,
  InboxList,
} from "./protocol";

// A fixture-backed client with realistic sample items, so the whole Inbox runs
// and can be tested before the desktop serves it. Off unless
// EXPO_PUBLIC_INBOX_FIXTURES=1 is set when the bundle is built; nothing in a
// release turns it on.

const MIN = 60_000;

/** Whether this build was started with the fixture client switched on. */
export const FIXTURES_ENABLED = process.env.EXPO_PUBLIC_INBOX_FIXTURES === "1";

export type FixtureControls = {
  /** Every request fails, as with the desktop out of reach. */
  offline: boolean;
  /** Replies are held this long. */
  latencyMs: number;
  /** The desktop calls its snapshot stale and a provider failed. */
  stale: boolean;
};

type Sample = Omit<InboxItem, "updatedAt" | "projectId"> & {
  /** Minutes ago. */
  ago: number;
  /** Index into the phone's projects, if the item maps to one. */
  project?: number;
  body: string;
  thread?: { author: string; ago: number; body: string; kind?: "comment" | "review" | "commit" }[];
  files?: {
    path: string;
    lines: [kind: "h" | "c" | "a" | "d", text: string][];
  }[];
  checkList?: InboxCheck[];
  decision?: InboxDetail["reviewDecision"];
};

const SAMPLES: Sample[] = [
  {
    key: "github:monocode/monocode#1284",
    source: "github",
    kind: "pr",
    number: "#1284",
    title: "Stream tool output in the transcript",
    repo: "monocode/monocode",
    project: 0,
    author: { name: "maren-k" },
    status: "open",
    ago: 12,
    unread: true,
    attention: "reviewRequested",
    attentionLabel: "Review requested by maren-k",
    done: false,
    comments: 4,
    checks: "passing",
    additions: 312,
    deletions: 48,
    url: "https://github.com/monocode/monocode/pull/1284",
    body: "Tool output used to appear only when a call finished. This streams it into the turn as it arrives.\n\n- Chunks are coalesced per animation frame\n- Output beyond 40 KB folds behind `Show all`\n",
    decision: "pending",
    thread: [
      { author: "dani-r", ago: 95, body: "Does the coalescing hold up when two tools stream at once?" },
      { author: "maren-k", ago: 80, body: "Yes: each call has its own buffer. Added a test for it." },
      { author: "dani-r", ago: 40, kind: "review", body: "Looks good to me apart from the fold threshold." },
    ],
    files: [
      {
        path: "src/transcript/stream.ts",
        lines: [
          ["h", "@@ -41,6 +41,9 @@ export function createStream("],
          ["c", "  const buf: Chunk[] = []"],
          ["d", "  let flushed = false"],
          ["a", "  let raf = 0"],
          ["a", "  const flush = () => { raf = 0; emit(buf.splice(0)) }"],
          ["c", "  return { push, end }"],
        ],
      },
      {
        path: "src/transcript/fold.ts",
        lines: [
          ["h", "@@ -8,3 +8,4 @@ const LIMIT = 40_000"],
          ["c", "export function shouldFold(text: string) {"],
          ["d", "  return text.length > 20_000"],
          ["a", "  return text.length > LIMIT"],
          ["c", "}"],
        ],
      },
    ],
    checkList: [
      { name: "typecheck", state: "passing", durationMs: 41_000 },
      { name: "lint", state: "passing", durationMs: 18_000 },
      { name: "unit (node 24)", state: "passing", durationMs: 124_000 },
      { name: "build-ios", state: "passing", durationMs: 372_000 },
    ],
  },
  {
    key: "linear:MON-482",
    source: "linear",
    kind: "issue",
    number: "MON-482",
    title: "Composer drops text after dictation ends",
    repo: "Monocode · Mobile",
    project: 0,
    author: { name: "tomas" },
    status: "open",
    ago: 38,
    unread: true,
    attention: "assigned",
    attentionLabel: "Assigned to you",
    done: false,
    comments: 2,
    url: "https://linear.app/monocode/issue/MON-482",
    body: "When dictation ends while the keyboard is dismissing, the last words are lost.\n\n**Steps**\n\n1. Hold the mic\n2. Speak a long sentence\n3. Release while the keyboard animates away\n",
    thread: [{ author: "tomas", ago: 600, body: "Reproduced on iOS 26.1, not on Android." }],
  },
  {
    key: "github:monocode/monocode#1279",
    source: "github",
    kind: "pr",
    number: "#1279",
    title: "Fix reconnect backoff after sleep",
    repo: "monocode/monocode",
    project: 0,
    author: { name: "you" },
    status: "open",
    ago: 61,
    unread: true,
    attention: "ciFailed",
    attentionLabel: "2 checks failing",
    done: false,
    comments: 1,
    checks: "failing",
    additions: 57,
    deletions: 9,
    url: "https://github.com/monocode/monocode/pull/1279",
    body: "The backoff kept growing while the phone slept, so the first retry after waking waited a full minute.",
    decision: "pending",
    files: [
      {
        path: "src/connection/backoff.ts",
        lines: [
          ["h", "@@ -12,4 +12,8 @@ export class Backoff {"],
          ["c", "  next(): number {"],
          ["c", "    this.step = Math.min(this.step * 2, MAX)"],
          ["a", "    return this.step"],
          ["a", "  }"],
          ["a", "  reset() { this.step = MIN }"],
        ],
      },
    ],
    checkList: [
      { name: "build-ios", state: "failing", durationMs: 400_000 },
      { name: "e2e-reconnect", state: "failing", durationMs: 191_000 },
      { name: "typecheck", state: "passing", durationMs: 39_000 },
      { name: "lint", state: "passing", durationMs: 17_000 },
    ],
  },
  {
    key: "jira:BILL-217",
    source: "jira",
    kind: "issue",
    number: "BILL-217",
    title: "Refund webhook retries twice on 502",
    repo: "Billing",
    project: 1,
    author: { name: "Priya N." },
    status: "open",
    ago: 130,
    unread: false,
    attention: "mentioned",
    attentionLabel: "Mentioned you",
    done: false,
    comments: 6,
    url: "https://acme.atlassian.net/browse/BILL-217",
    body: "Our handler acknowledges after the retry window, so the provider sends the event again.",
  },
  {
    key: "azuredevops:Acme/Billing!412",
    source: "azuredevops",
    kind: "pr",
    number: "!412",
    title: "Move invoice export to a queue",
    repo: "Acme/Billing",
    project: 1,
    author: { name: "Lee Park" },
    status: "open",
    ago: 1500,
    unread: false,
    attention: "reviewRequested",
    attentionLabel: "Review requested by Lee Park",
    done: false,
    comments: 3,
    additions: 204,
    deletions: 88,
    url: "https://dev.azure.com/acme/Billing/_git/billing/pullrequest/412",
    body: "Exports run in a worker, so the request returns at once with a job id.",
  },
  {
    key: "github:monocode/monocode#1262",
    source: "github",
    kind: "pr",
    number: "#1262",
    title: "Refactor mobile link host",
    repo: "monocode/monocode",
    project: 0,
    author: { name: "you" },
    status: "merged",
    ago: 3000,
    unread: false,
    attention: "other",
    attentionLabel: "Merged",
    done: false,
    comments: 5,
    checks: "passing",
    additions: 410,
    deletions: 380,
    url: "https://github.com/monocode/monocode/pull/1262",
    body: "Splits the host into request, watch and push modules.",
    checkList: [{ name: "unit (node 24)", state: "passing", durationMs: 130_000 }],
  },
  {
    key: "github:monocode/monocode#1271",
    source: "github",
    kind: "issue",
    number: "#1271",
    title: "Crash when attaching 10+ images",
    repo: "monocode/monocode",
    project: 0,
    author: { name: "maren-k" },
    status: "open",
    ago: 4400,
    unread: false,
    attention: "mentioned",
    attentionLabel: "Mentioned you",
    done: true,
    comments: 3,
    url: "https://github.com/monocode/monocode/issues/1271",
    body: "Attaching more than ten photos at once crashes the composer on older phones.",
  },
  {
    key: "gitlab:acme/web-app!91",
    source: "gitlab",
    kind: "pr",
    number: "!91",
    title: "Lazy-load the settings routes",
    repo: "acme/web-app",
    project: 2,
    author: { name: "tomas" },
    status: "open",
    ago: 9000,
    unread: false,
    attention: "other",
    done: false,
    comments: 0,
    additions: 120,
    deletions: 44,
    url: "https://gitlab.com/acme/web-app/-/merge_requests/91",
    body: "Moves the settings screens behind dynamic imports.",
  },
];

/** The sample items as of `now`, mapped onto the phone's `projectIds`. */
export function sampleItems(now: number, projectIds: readonly string[]): InboxItem[] {
  return SAMPLES.map(({ ago, project, body: _b, thread: _t, files: _f, checkList: _c, decision: _d, ...item }) => ({
    ...item,
    updatedAt: now - ago * MIN,
    ...(project !== undefined && projectIds[project] ? { projectId: projectIds[project] } : {}),
  }));
}

/**
 * The fixture client. State lives in memory for the life of the app, so
 * marks stick as they do against the desktop.
 */
export function createFixtureClient(options: {
  projectIds: readonly string[];
  now?: () => number;
  controls?: Partial<FixtureControls>;
}): InboxClient & { controls: FixtureControls } {
  const now = options.now ?? Date.now;
  const controls: FixtureControls = {
    offline: false,
    latencyMs: 350,
    stale: false,
    ...options.controls,
  };
  let items = sampleItems(now(), options.projectIds);
  const sample = (key: string) => SAMPLES.find((entry) => entry.key === key);

  const gate = async <T>(run: () => T): Promise<T> => {
    if (controls.latencyMs > 0) await new Promise((r) => setTimeout(r, controls.latencyMs));
    if (controls.offline) throw new Error("The desktop is unreachable");
    return run();
  };
  const find = (key: string) => {
    const item = items.find((entry) => entry.key === key);
    if (!item) throw new Error("No such item");
    return item;
  };

  return {
    controls,
    list: (params) =>
      gate((): InboxList => {
        const scope = params.scope ?? "all";
        const at = now();
        const q = params.q?.trim().toLowerCase();
        const matching = items
          .filter((item) => !params.projectId || item.projectId === params.projectId)
          .filter((item) => !params.source || item.source === params.source)
          .filter((item) =>
            scope === "done"
              ? item.done
              : !item.done && !isSnoozed(item, at) && (scope === "all" || needsMe(item)),
          )
          .filter((item) => !q || `${item.title} ${item.repo}`.toLowerCase().includes(q))
          .sort((a, b) => b.updatedAt - a.updatedAt);
        const limit = params.limit ?? 30;
        const start = params.cursor ? Number(params.cursor) : 0;
        const page = matching.slice(start, start + limit);
        const open = items.filter((item) => !item.done && !isSnoozed(item, at));
        return {
          items: page,
          nextCursor: start + limit < matching.length ? String(start + limit) : undefined,
          stale: controls.stale,
          fetchedAt: at,
          providers: [
            { id: "github", connected: true },
            { id: "linear", connected: true },
            controls.stale
              ? { id: "jira", connected: false, error: "Sign-in expired" }
              : { id: "jira", connected: true },
          ],
          counts: {
            unread: open.filter((item) => item.unread).length,
            needsMe: open.filter(needsMe).length,
            done: items.filter((item) => item.done).length,
          },
        };
      }),
    item: (key) =>
      gate((): InboxDetail => {
        const item = find(key);
        const data = sample(key);
        return {
          item,
          body: data?.body ?? "",
          thread: (data?.thread ?? []).map((entry, index) => ({
            id: `${key}#${index}`,
            author: { name: entry.author },
            createdAt: now() - entry.ago * MIN,
            body: entry.body,
            kind: entry.kind ?? "comment",
          })),
          ...(data?.decision ? { reviewDecision: data.decision } : {}),
          ...(item.checks ? { checksSummary: item.checks } : {}),
        };
      }),
    diff: (key, path) =>
      gate((): InboxDiff => {
        const files = (sample(key)?.files ?? [])
          .filter((file) => !path || file.path === path)
          .map((file) => {
            let oldNo = 41;
            let newNo = 41;
            const lines = file.lines.map(([kind, text]) => {
              if (kind === "h") return { kind: "hunk" as const, text };
              if (kind === "a") return { kind: "add" as const, text, newNo: newNo++ };
              if (kind === "d") return { kind: "del" as const, text, oldNo: oldNo++ };
              return { kind: "context" as const, text, oldNo: oldNo++, newNo: newNo++ };
            });
            return {
              path: file.path,
              additions: lines.filter((line) => line.kind === "add").length,
              deletions: lines.filter((line) => line.kind === "del").length,
              lines,
            };
          });
        return { files, truncated: false };
      }),
    checks: (key) => gate(() => sample(key)?.checkList ?? []),
    mark: (key, action, until) =>
      gate(() => {
        const next = applyMark(find(key), action, until);
        items = items.map((item) => (item.key === key ? next : item));
        return next;
      }),
    start: (params) =>
      gate(() => {
        find(params.key);
        return { sessionId: `fixture-session-${now().toString(36)}` };
      }),
    // The sample people have no avatars; rows fall back to initials.
    media: () =>
      gate((): never => {
        throw new Error("No media in fixtures");
      }),
  };
}
