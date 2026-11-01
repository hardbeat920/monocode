// The Inbox's wire format: the shared contract between the desktop's mobile
// host (`inbox.*` in mobile_host.rs) and this phone. All JSON, camelCase.
// Provider tokens never cross the link; the phone never sends a URL, only an
// item key plus an index or avatar key the desktop resolves.

export type InboxSource = "github" | "linear" | "jira" | "gitlab" | "azuredevops";
export type InboxScope = "all" | "needsMe" | "done";
export type InboxAttention =
  | "reviewRequested"
  | "mentioned"
  | "assigned"
  | "ciFailed"
  | "other";
export type InboxStatus = "open" | "draft" | "closed" | "merged";
export type InboxChecks = "passing" | "failing" | "pending";

export type InboxPerson = { name: string; avatarKey?: string };

export type InboxItem = {
  /** Stable, e.g. "github:owner/repo#12". */
  key: string;
  source: InboxSource;
  kind: "issue" | "pr";
  number?: string;
  title: string;
  /** Display label for the repo or project the item belongs to. */
  repo: string;
  /** The phone project it maps to (catalog `projects[].id`), when known. */
  projectId?: string;
  author: InboxPerson;
  status: InboxStatus;
  updatedAt: number;
  unread: boolean;
  attention?: InboxAttention;
  attentionLabel?: string;
  done: boolean;
  snoozedUntil?: number;
  comments: number;
  checks?: InboxChecks;
  additions?: number;
  deletions?: number;
  /** For display and opening in the phone's browser only. */
  url: string;
};

export type InboxProviderState = { id: string; connected: boolean; error?: string };

export type InboxListParams = {
  scope?: InboxScope;
  source?: InboxSource;
  projectId?: string;
  q?: string;
  cursor?: string;
  limit?: number;
  /** Ask the desktop to fetch from its providers now and answer when it has. */
  refresh?: boolean;
};

export type InboxList = {
  items: InboxItem[];
  nextCursor?: string;
  /** True while the snapshot is older than its polling interval or a provider failed. */
  stale: boolean;
  fetchedAt: number;
  providers: InboxProviderState[];
  counts: { unread: number; needsMe: number; done: number };
};

export type InboxThreadEntry = {
  id: string;
  author: InboxPerson;
  createdAt: number;
  body: string;
  kind: "comment" | "review" | "commit";
};

export type InboxDetail = {
  item: InboxItem;
  body: string;
  thread: InboxThreadEntry[];
  reviewDecision?: "approved" | "changesRequested" | "pending";
  checksSummary?: InboxChecks;
};

export type InboxDiffLine = {
  kind: "context" | "add" | "del" | "hunk";
  text: string;
  oldNo?: number;
  newNo?: number;
};

export type InboxDiffFile = {
  path: string;
  additions: number;
  deletions: number;
  lines: InboxDiffLine[];
};

export type InboxDiff = { files: InboxDiffFile[]; truncated: boolean };

export type InboxCheck = {
  name: string;
  state: "passing" | "failing" | "pending" | "skipped";
  url?: string;
  durationMs?: number;
};

export type InboxMarkAction =
  | "seen"
  | "unseen"
  | "done"
  | "undone"
  | "snooze"
  | "unsnooze";

export type InboxStartParams = {
  key: string;
  projectId: string;
  mode: "work" | "ask";
  harness?: string;
  model?: string;
  text?: string;
};

export type InboxMedia = { mime: string; base64: string };

/** The phone's request function (ConnectionProvider's `command`). */
export type InboxTransport = <T>(
  method: string,
  params?: object,
  id?: string,
) => Promise<T>;

export type InboxClient = {
  list: (params: InboxListParams) => Promise<InboxList>;
  item: (key: string) => Promise<InboxDetail>;
  diff: (key: string, path?: string) => Promise<InboxDiff>;
  checks: (key: string) => Promise<InboxCheck[]>;
  mark: (key: string, action: InboxMarkAction, until?: number) => Promise<InboxItem>;
  start: (params: InboxStartParams) => Promise<{ sessionId: string }>;
  media: (key: string, options?: { avatarKey?: string; index?: number }) => Promise<InboxMedia>;
};

/** The default page size, as the desktop's. */
export const PAGE_SIZE = 30;

/** Typed requests over the phone's link. `makeId` makes a command id (nativeLink's). */
export function createInboxClient(
  transport: InboxTransport,
  makeId: () => string,
): InboxClient {
  // A start whose reply was lost keeps its command id for the retry, so the
  // desktop's dedupe starts one session, not two.
  const starts = new Map<string, string>();
  return {
    list: (params) => transport<InboxList>("inbox.list", withoutUndefined(params)),
    item: (key) => transport<InboxDetail>("inbox.item", { key }),
    diff: (key, path) =>
      transport<InboxDiff>("inbox.diff", withoutUndefined({ key, path })),
    checks: async (key) =>
      (await transport<{ checks: InboxCheck[] }>("inbox.checks", { key })).checks ?? [],
    mark: async (key, action, until) =>
      (
        await transport<{ item: InboxItem }>(
          "inbox.mark",
          withoutUndefined({ key, action, until }),
        )
      ).item,
    start: async (params) => {
      const body = withoutUndefined(params);
      const signature = JSON.stringify(body);
      let id = starts.get(signature);
      if (!id) {
        id = makeId();
        starts.set(signature, id);
      }
      const result = await transport<{ sessionId: string }>("inbox.start", body, id);
      starts.delete(signature);
      return result;
    },
    media: (key, options = {}) =>
      transport<InboxMedia>("inbox.media", withoutUndefined({ key, ...options })),
  };
}

function withoutUndefined<T extends object>(value: T): T {
  return Object.fromEntries(
    Object.entries(value).filter(([, entry]) => entry !== undefined),
  ) as T;
}
