# Mobile Inbox protocol

The phone's Inbox is a view of the desktop Inbox. The desktop owns the provider
connections, the cached items and the triage state; the phone asks for slim,
display-ready structures over the existing mobile link.

Transport: the mobile link request/response (`handle_mobile_request` in
`crates/monocode-gpui/src/shell/mobile_host.rs`; the phone calls `request(...)`
in `mobile/src/features/connection/ConnectionProvider.tsx`). All JSON, camelCase.
The host's wire types live in `crates/monocode-core/src/mobile/inbox.rs`; the
handlers in `crates/monocode-gpui/src/shell/mobile_host/inbox.rs`.

## Capability

The workspace catalog carries `"inbox": true` (next to `monoTools`, `push` and
`monoFiles`). A desktop without it does not answer `inbox.*`; the phone hides
the Inbox segment.

## Methods

### `inbox.list`

`{scope?: "all"|"needsMe"|"done", source?: "github"|"linear"|"jira"|"gitlab"|"azuredevops", projectId?: string, q?: string, cursor?: string, limit?: number (default 30, max 100), refresh?: boolean}`

→ `{items: InboxItem[], nextCursor?: string, stale: boolean, fetchedAt: number (ms), providers: {id, connected, error?}[], counts: {unread, needsMe, done}}`

- Serves the cached snapshot immediately and never waits on a provider. If the
  snapshot is older than 30 s the host starts a background refresh through the
  desktop Inbox; the phone's next poll sees the result.
- The list is built from a fetch with default filters over the desktop's
  projects and the Monos' projects, whatever the desktop Inbox's own filters
  (assigned to me, one author, open only, hidden Linear teams or Jira
  projects) are set to. While those filters and projects would not change the
  fetch, the desktop's own snapshot is used and nothing extra runs; otherwise
  the host keeps a second snapshot for the phone and refreshes it the same way.
  Items only that snapshot lists start out read.
- `refresh: true` (pull to refresh, retry) makes the host fetch from the
  providers now, unless a fetch is already running, and answer when it ends
  (after 20 s at most, with whatever the Inbox then holds). A phone that
  doesn't send it gets the cache, as before.
- `stale` is true while the snapshot is older than its polling interval
  (120 s while the desktop Inbox is on screen, 300 s otherwise), was never
  fetched, or a provider failed. `fetchedAt` is 0 until the first fetch ends.
- `providers` lists all five providers in a fixed order. `error` is one short
  line with anything credential-shaped removed.
- `projectId` restricts to items whose checkout maps to that phone project id
  (the catalog's `projects[].id`). An item with no known mapping (a Linear or
  Jira issue) appears only when `projectId` is omitted.
- Scopes, newest first (ties by `key`):
  - `all`: everything not done and not snoozed.
  - `needsMe`: open or draft items that have an attention reason, not done, not
    snoozed.
  - `done`: items marked done.
  - A snoozed item is in no scope until its wake time passes, then it is back
    in `all` (and `needsMe` if it still needs the user).
- `q` matches every word against title, reference, repo, team, author and labels.
- `counts` covers the `source`/`projectId` slice, ignoring `scope` and `q`, so
  tabs can show their numbers: `unread` and `needsMe` count items that are not
  done or snoozed; `done` counts done items.
- `cursor` is opaque. It names the last item of the previous page, so items
  appearing or leaving between pages never repeat or skip one. An invalid
  cursor is an error.

### `inbox.item`

`{key}` → `{item, body (markdown), thread: {id, author, createdAt, body (markdown), kind: "comment"|"review"|"commit"}[], reviewDecision?: "approved"|"changesRequested"|"pending", checksSummary?: "passing"|"failing"|"pending"}`

Fetches the description and discussion from the provider, so it takes as long as
the provider does (it runs off the UI thread). The thread is in time order;
replies follow as ordinary `comment` entries and a commit's `body` is its
headline. `item.comments` counts the non-commit entries and `item.checks` is
filled for GitHub pull requests. A failed half (details or discussion) comes back
empty; both failing is an error.

### `inbox.diff`

`{key, path?}` → `{files: {path, additions, deletions, lines: {kind: "context"|"add"|"del"|"hunk", text, oldNo?, newNo?}[]}[], truncated}`

Without `path`: every file (at most 300) with its first 200 lines. With `path`:
that one file with up to 5000 lines. `truncated` is true when anything was left
out, including a patch the provider cut. A binary file has no lines. Issues have
no diff (error).

### `inbox.checks`

`{key}` → `{checks: {name, state: "passing"|"failing"|"pending"|"skipped", url?, durationMs?}[]}`

GitHub pull requests only; an empty array otherwise. `name` is
`workflow / job` when there is a workflow.

### `inbox.mark`

`{key, action: "seen"|"unseen"|"done"|"undone"|"snooze"|"unsnooze", until?: number (ms; required for snooze)}` → `{item}`

State lives on the desktop (`Seen` and `Triage` in the Inbox's saved view
state, keyed by `Item::key()`) and is shared with the desktop Inbox. `done`
clears a snooze. `until` must be in the future for a snooze to hide the item.

### `inbox.start`

`{key, projectId, mode: "work"|"ask", harness?, model?, text?}` → `{sessionId}`

Builds the prompt from the item with `Item::prompt` (description and discussion
included; `ask` asks to discuss rather than implement; `text` is appended as
extra instructions) and starts a session through the `session.start` path. The
command ID dedupe is the same: retrying with the same command ID and input
returns the same session, and reusing it with different input is an error.

### `inbox.media`

`{key, avatarKey?, index?}` → `{mime, base64}`

Exactly one of `avatarKey` or `index`.

- `avatarKey` is an opaque key from an `author.avatarKey` or thread author the
  host sent. The host resolves it to the address it received from the provider
  and returns a PNG of at most 64 px (https addresses only). A key the host did not hand out (or from
  before it restarted) is an error; call `inbox.list` again.
- `index` is the position of an image in the item's `body`, counting in reading
  order both markdown `![alt](url)` and HTML `<img src="url">`. The host fetches
  it through the allowlisted media adapter, shrinks it past 1600 px, and refuses
  more than 3 MB. SVG is refused.

The phone never sends an address for the host to fetch.

## Types

```
InboxItem = {
  key: string,                 // Item::key(), e.g. "github:acme/app:pr:12"
  source: "github"|"linear"|"jira"|"gitlab"|"azuredevops",
  kind: "issue"|"pr",
  number?: string,             // "#12" or "ENG-42"
  title: string,
  repo: string,                // display label
  projectId?: string,
  author: {name: string, avatarKey?: string},
  status: "open"|"draft"|"closed"|"merged",
  updatedAt: number,           // ms
  unread: boolean,
  attention?: "reviewRequested"|"mentioned"|"assigned"|"ciFailed"|"other",
  attentionLabel?: string,
  done: boolean,
  snoozedUntil?: number,       // ms; only while snoozed
  comments: number,
  checks?: "passing"|"failing"|"pending",
  additions?: number,
  deletions?: number,
  url: string                  // for display and opening in the phone's browser only
}
```

List rows do not carry `comments` (0), `checks`, `additions` or `deletions`: the
providers' list calls don't return them. `inbox.item` fills `comments` and
`checks`.

## Attention

`attention` comes from the provider's reason for the item. For GitHub the host
asks, per checked-out repository, which open pull requests request the signed-in
user's review (`review-requested:<login>`) and which open items mention them
(`mentions:<login>`); items assigned to them come from the listed assignees.
Where several apply the order is `reviewRequested`, `assigned`, `mentioned`.
These searches are best effort: if one fails the rows stay as listed. GitLab and
Azure DevOps to-dos carry their own action (`build_failed` is `ciFailed`).
Anything else a provider reports is `other`. `ciFailed` is only set where the
provider says so; GitHub CI status is not polled for the list.

## Push

Category `inbox`, title the item title, body the reason ("Review requested by
sam", "Mentioned by sam", "Assigned to you", "CI failed"), subtitle the repo,
data `{kind: "inbox", key}`; a tap opens the item. Sent for items that newly
appear and need the user (review requested, mentioned, assigned, CI failed), not
for every update, and not for the first fetch of a provider (its backlog is a
baseline). The phone's per-category toggle (`inbox`, next to `agentInput`,
`agentFinished` and `monoMessages` in `mobile/src/features/settings/notifications.ts`)
is respected. The desktop doesn't send one while its own Inbox is on screen and
the window is in front.

## Source of the list

The list is the desktop Inbox's own snapshot: the same fetch, filters and
polling (every 120 s while the desktop Inbox is on screen, every 300 s in the
background; a phone asking after 120 s also starts one). It holds open items
unless the desktop's status filter asks for closed ones, so `status` of
`closed` and `merged` shows up only then. The background Inbox starts with the
app once Automations have opened, or when a phone first asks.

## Security

- Provider tokens and config (`linear-token`, `*-config.json`, the `gh`
  session) never cross the link. Responses are built from the slim types above
  only; a test serialises every response type and checks for credential-shaped
  keys and values.
- The phone sends item keys, avatar keys and image indexes, never an address.
- Provider errors are cut to one line with credential-shaped words removed.
- Merge, close and other destructive provider actions are out of scope.
