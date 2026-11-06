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
the Inbox row in its drawer.

It also carries `"inboxActions": true` when the desktop answers `inbox.readAll`,
`inbox.action` and `inbox.comment`. Without it the phone offers no Mark all as
read; an older desktop's `inbox.item` also has no `actions`, so the item sheet
offers no merge, pull request action or comment.

`inboxRev` names what the phone's Inbox would read. While a phone is listing the
workspace the desktop keeps the list fresh and moves this when it changes; the
phone reads `inbox.list` again only then.

Catalog sessions a Mono launched carry `monoId`.

## Methods

### `inbox.list`

`{scope?: "all"|"needsMe"|"done", source?: "github"|"linear"|"jira"|"gitlab"|"azuredevops", projectId?: string, q?: string, authors?: string[], places?: string[], statuses?: ("open"|"draft"|"closed"|"merged")[], kind?: "issue"|"pr", sources?: string[], updatedSince?: number (ms), assigned?: boolean, cursor?: string, limit?: number (default 30, max 100), refresh?: boolean}`

→ `{items: InboxItem[], nextCursor?: string, stale: boolean, fetchedAt: number (ms), providers: {id, connected, error?}[], counts: {unread, needsMe, done}, options: {authors: {name, avatarKey?, itemKey, count}[], me?: number, places: {id, name, kind: "project"|"tracker"|"linearProject", count}[], sources: string[]}}`

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
- The filters are the desktop's filter menu, across every source. An item has
  to pass each one that is set and match any value of a list. `authors` takes
  logins or `@me`, the GitHub login once a fetch has named it. `assigned` is an
  item with the attention reason `assigned`, or one listing that login as an
  assignee. `updatedSince` comes from the phone so "Today" is the phone's day.
- `places` takes what the project picker offers: a phone project id,
  `repo:<label>` for an item with no project (a Linear team, a Jira project),
  or `linear:<id>` for a Linear project (`linear:~none` for none).
- `InboxItem` carries `mine`, `assigned` and `linearProject`, so the phone can
  apply a filter at once while this answer is on its way. An older desktop
  ignores the filters and sends no `options`; the phone then filters what it
  has loaded and offers no Me or Assigned to me.
- `counts` covers the `source`/`projectId` slice and the filters, ignoring
  `scope` and `q`, so tabs can show their numbers: `unread` and `needsMe` count
  items that are not done or snoozed; `done` counts done items. `inbox.readAll`
  takes the same filters.
- `options` is what the filter sheets list: the scope's items in the
  `source`/`projectId` slice, ignoring `q`. Each list follows every filter but
  its own, so with a project picked `authors` holds that project's people, and
  picking one author leaves the others on the sheet. `authors` leaves out the
  signed-in user, whose count is `me`, and stops at 30, most items first.
- `cursor` is opaque. It names the last item of the previous page, so items
  appearing or leaving between pages never repeat or skip one. An invalid
  cursor is an error.

### `inbox.item`

`{key}` → `{item, body (markdown), thread: {id, author, createdAt, body (markdown), kind: "comment"|"review"|"commit", replyTo?}[], reviewDecision?: "approved"|"changesRequested"|"pending", checksSummary?: "passing"|"failing"|"pending", actions: ("comment"|"merge"|"squash"|"rebase"|"ready"|"draft"|"close"|"reopen")[], baseRef?, headRef?}`

Fetches the description and discussion from the provider, so it takes as long as
the provider does (it runs off the UI thread). The thread is in time order;
replies follow as ordinary `comment` entries and a commit's `body` is its
headline. `item.comments` counts the non-commit entries and `item.checks` is
filled for GitHub pull requests. A failed half (details or discussion) comes back
empty; both failing is an error.

`actions` is what the desktop's detail offers now: `comment` on every item, and
on a GitHub pull request `merge`, `squash`, `rebase`, `draft` and `close` while
it's open, `ready` and `close` while it's a draft, `reopen` once it's closed, and
none of them once merged. `review` is on every GitHub pull request, in any state, and says its lines take
the `inbox.review*` methods. `path` and `line` are set on an entry that is a
comment on a line. `replyTo` is set on entries the provider can answer: a
GitHub review thread or a Linear comment. `baseRef` and `headRef` name the
branches where the provider has them.

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

### `inbox.readAll`

`{scope?, source?, projectId?, q?}` → `{marked: number}`

Marks read every item that list shows, as `inbox.list` would filter it. A done
or snoozed item is left as it is unless the scope lists it. The read state is
shared with the desktop Inbox.

### `inbox.action`

`{key, action: "merge"|"squash"|"rebase"|"ready"|"draft"|"close"|"reopen"}` → `{item, actions, queued}`

Runs what the desktop's pull request buttons run, with the desktop's own GitHub
sign-in. The host refuses an action `inbox.item` wouldn't list for the item as it
stands, so a stale phone can't merge a draft and no action reaches `gh` for
anything but a GitHub pull request. The phone confirms first, in the desktop's
words.

`queued` is true when a merge returned but the pull request isn't merged yet: a
merge queue or auto-merge has it. A provider's refusal comes back as the error,
cut down as in `inbox.list`. The item stays in its attention state until the next
fetch, and is marked read.

The command ID dedupe is the same as `session.start`. A phone that lost the reply
retries with the same ID and gets the first answer, so nothing merges twice;
while the first is running it hears that the desktop is still working. A refused
command may be retried with the same ID.

### `inbox.comment`

`{key, body, replyTo?}` → `{}`

Posts `body` as the user at the provider, on any item, or as a reply when
`replyTo` is an entry's `replyTo`. An empty body is an error. Same command ID
dedupe as `inbox.action`, so a retry doesn't post twice.

### `inbox.review`

`{key}` → `{comments: PendingComment[]}`

The line comments waiting on a GitHub pull request, the same list the desktop's
diff shows: a comment written on either side is on the other at its next read.
`PendingComment` is `{id, path, line, side: "left"|"right", text}`. A removed
line is `left` at its old number; an added or unchanged line is `right` at its
new number.

### `inbox.reviewAdd`

`{key, path, line, side, text}` → `{comments}`

Adds a comment to the list. Nothing reaches GitHub yet. An empty path or text, a
line of 0 or an unknown side is an error. Same command ID dedupe as
`inbox.action`, so a retry doesn't add it twice.

### `inbox.reviewRemove`

`{key, id}` → `{comments}`

Removes a pending comment. Removing one that's gone changes nothing, so it needs
no command ID.

### `inbox.reviewSubmit`

`{key}` → `{comments}`

Posts the list as one GitHub review with the `COMMENT` event: it never approves
or requests changes. The answer holds what's still pending, which is any comment
added while the review was posting. A second submit during a post, or a submit
with nothing pending, is an error; a failed post keeps every comment. Same
command ID dedupe as `inbox.action`, so a retry can't post the review twice.

All four answer an error for anything but a GitHub pull request.

### `inbox.start`

`{key, projectId, mode: "work"|"ask", harness?, model?, text?}` → `{sessionId}`

Builds the prompt from the item with `Item::prompt` (description and discussion
included; `ask` asks to discuss rather than implement; `text` is appended as
extra instructions) and starts a session through the `session.start` path. The
command ID dedupe is the same: retrying with the same command ID and input
returns the same session, and reusing it with different input is an error.

### `inbox.media`

`{key, avatarKey?, index?, commentId?, commentIndex?}` → `{mime, base64}`

Exactly one of `avatarKey`, `index`, or `commentId` with `commentIndex`.

- `avatarKey` is an opaque key from an `author.avatarKey` or thread author the
  host sent. The host resolves it to the address it received from the provider
  and returns a PNG of at most 64 px (https addresses only). A key the host did not hand out (or from
  before it restarted) is an error; call `inbox.list` again.
- `index` is the position of an image in the item's `body`, counting in reading
  order both markdown `![alt](url)` and HTML `<img src="url">`. The host fetches
  it through the allowlisted media adapter, shrinks it past 1600 px, and refuses
  more than 3 MB. SVG is refused. The count is a plain scan of the text, so an
  image inside a code fence or an HTML comment takes up a number too; the phone
  counts the same way (`mobile/src/features/inbox/bodyImages.ts`).
- `commentId` is a thread entry's `id` and `commentIndex` the position of an
  image in that entry's `body`, counted and fetched as for `index`. These are
  their own fields so that a host without them answers with an error, not with
  an image from the item's body.

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
