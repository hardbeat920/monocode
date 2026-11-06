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
  /** The Linear project, or `~none`, on a Linear issue. */
  linearProject?: string;
  author: InboxPerson;
  /** The signed-in user wrote it. An older desktop leaves this out. */
  mine?: boolean;
  /** It is assigned to the signed-in user. An older desktop leaves this out. */
  assigned?: boolean;
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
  /** Logins, or `@me`. These filters are as `inbox.list` documents them. */
  authors?: readonly string[];
  /** What `placesOf` gives for an item. */
  places?: readonly string[];
  statuses?: readonly InboxStatus[];
  kind?: InboxItem["kind"];
  sources?: readonly InboxSource[];
  updatedSince?: number;
  assigned?: boolean;
  cursor?: string;
  limit?: number;
  /** Ask the desktop to fetch from its providers now and answer when it has. */
  refresh?: boolean;
};

export type InboxAuthorOption = {
  name: string;
  avatarKey?: string;
  /** An item of theirs, which the avatar is fetched through. */
  itemKey: string;
  count: number;
};

export type InboxPlaceOption = {
  id: string;
  name: string;
  kind: "project" | "tracker" | "linearProject";
  count: number;
};

/** What the filter sheets offer: the scope's items, before any filter. */
export type InboxOptions = {
  authors: InboxAuthorOption[];
  /** The signed-in user's items; unset until a provider has named them. */
  me?: number;
  places: InboxPlaceOption[];
  sources: InboxSource[];
};

export type InboxList = {
  items: InboxItem[];
  nextCursor?: string;
  /** True while the snapshot is older than its polling interval or a provider failed. */
  stale: boolean;
  fetchedAt: number;
  providers: InboxProviderState[];
  counts: { unread: number; needsMe: number; done: number };
  /** An older desktop leaves this out and filters nothing itself. */
  options?: InboxOptions;
};

export type InboxThreadEntry = {
  id: string;
  author: InboxPerson;
  createdAt: number;
  body: string;
  kind: "comment" | "review" | "commit";
  /** What `comment` takes to answer this entry; only where the provider has replies. */
  replyTo?: string;
  /** The file and line a review comment is on. */
  path?: string;
  line?: number;
};

export type InboxDetail = {
  item: InboxItem;
  body: string;
  thread: InboxThreadEntry[];
  reviewDecision?: "approved" | "changesRequested" | "pending";
  checksSummary?: InboxChecks;
  /** What can be done to the item now; an older desktop leaves it out and offers nothing. */
  actions?: InboxItemAction[];
  baseRef?: string;
  headRef?: string;
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

/** What the desktop's pull request buttons do. */
export type InboxAction =
  | "merge"
  | "squash"
  | "rebase"
  | "ready"
  | "draft"
  | "close"
  | "reopen";

export type InboxItemAction = InboxAction | "comment" | "review";

/** Which side of a diff a line comment is on: a deleted line, or an added or unchanged one. */
export type InboxReviewSide = "left" | "right";

export type InboxReviewLine = {
  path: string;
  line: number;
  side: InboxReviewSide;
};

/** A line comment waiting to go out with the review. The desktop's diff shares the list. */
export type InboxReviewComment = InboxReviewLine & { id: number; text: string };

export type InboxActionResult = {
  item: InboxItem;
  actions: InboxItemAction[];
  /** A merge queue or auto-merge has it; the pull request is still open. */
  queued: boolean;
};

export type InboxStartParams = {
  key: string;
  projectId: string;
  mode: "work" | "ask";
  harness?: string;
  model?: string;
  text?: string;
};

export type InboxMedia = { mime: string; base64: string };

/** An avatar, or an image by its place in the item's body or in one comment. */
export type InboxMediaTarget =
  | { avatarKey: string }
  | { index: number }
  | { commentId: string; commentIndex: number };

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
  readAll: (params: InboxListParams) => Promise<{ marked: number }>;
  act: (key: string, action: InboxAction) => Promise<InboxActionResult>;
  comment: (key: string, body: string, replyTo?: string) => Promise<void>;
  review: (key: string) => Promise<InboxReviewComment[]>;
  reviewAdd: (
    key: string,
    line: InboxReviewLine,
    text: string,
  ) => Promise<InboxReviewComment[]>;
  reviewRemove: (key: string, id: number) => Promise<InboxReviewComment[]>;
  /** Posts every pending comment as one review. What is left pending. */
  reviewSubmit: (key: string) => Promise<InboxReviewComment[]>;
  start: (params: InboxStartParams) => Promise<{ sessionId: string }>;
  media: (key: string, target: InboxMediaTarget) => Promise<InboxMedia>;
};

type InboxReviewList = { comments: InboxReviewComment[] };

/** The default page size, as the desktop's. */
export const PAGE_SIZE = 30;

/** Typed requests over the phone's link. `makeId` makes a command id (nativeLink's). */
export function createInboxClient(
  transport: InboxTransport,
  makeId: () => string,
): InboxClient {
  // A command whose reply was lost keeps its id for the retry, so the desktop's
  // dedupe starts one session, merges once and posts one comment.
  const unanswered = new Map<string, string>();
  const once = async <T,>(method: string, params: object): Promise<T> => {
    const body = withoutUndefined(params);
    const signature = JSON.stringify([method, body]);
    let id = unanswered.get(signature);
    if (!id) {
      id = makeId();
      unanswered.set(signature, id);
    }
    const result = await transport<T>(method, body, id);
    unanswered.delete(signature);
    return result;
  };
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
    readAll: (params) => transport<{ marked: number }>("inbox.readAll", withoutUndefined(params)),
    act: (key, action) => once<InboxActionResult>("inbox.action", { key, action }),
    comment: async (key, body, replyTo) => {
      await once<object>("inbox.comment", { key, body, replyTo });
    },
    review: async (key) =>
      (await transport<InboxReviewList>("inbox.review", { key })).comments,
    reviewAdd: async (key, line, text) =>
      (await once<InboxReviewList>("inbox.reviewAdd", { key, ...line, text })).comments,
    reviewRemove: async (key, id) =>
      (await transport<InboxReviewList>("inbox.reviewRemove", { key, id })).comments,
    reviewSubmit: async (key) =>
      (await once<InboxReviewList>("inbox.reviewSubmit", { key })).comments,
    start: (params) => once<{ sessionId: string }>("inbox.start", params),
    media: (key, target) => transport<InboxMedia>("inbox.media", { key, ...target }),
  };
}

function withoutUndefined<T extends object>(value: T): T {
  return Object.fromEntries(
    Object.entries(value).filter(([, entry]) => entry !== undefined),
  ) as T;
}
