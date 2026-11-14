# Agent access with /operator

## Enable access

Type `/operator` at the start of a composer message to enable MonoCode access in that thread. For example, `/operator start two Codex sessions: one to inspect the API and one to review the UI`, or `/operator list my notes`. The slash picker also offers this command. The transcript shows only the request text in a translucent amber bubble; MonoCode removes the command from the request sent to the agent and supplies the local `app` CLI path and instructions on that turn. Later turns in the same thread can use the CLI without repeating `/operator`; other threads receive no CLI instructions or app access. The CLI can act only during an active agent turn. The agent can run the shown `app --help` command for the exact JSON input fields.

## CLI actions

Sessions started by an ordinary Operator remain visible in project history.
Monos use the same project actions, with their **Show started sessions** setting
controlling whether their launches appear in the user's history. This is
independent of navigation: `reveal` defaults to `false`, so a background launch
can stay visible in history without switching the user's current conversation.

- `models.list` shows available providers, models, settings, and permission modes.
- `sessions.start` creates a session in the current project with a prompt. Set `reveal: true` to open its tab, or `placement: "right"` / `placement: "down"` to split the calling session's pane; `besideSessionId` selects another visible session pane in the project. Reuse the returned session ID as the next `besideSessionId` to build nested layouts. By default it submits the prompt; set `draft: true` to save it unsent without starting an agent turn. It accepts a provider, model, effort or other model settings, permission mode, and current checkout or new worktree choice. Set `worktreeCwd` to a path from `worktrees.list` for a specific existing checkout. Use `worktrees.create` to create a worktree on a named new or existing local branch, then pass its path as `worktreeCwd`. Omit `runtimeMode` to inherit the calling session's permission mode, or set it explicitly to override. It returns the new session ID as soon as the pane and prompt are accepted, so the agent can move it into a folder immediately.
- `sessions.list` shows project sessions and their archived status. `sessions.read` returns up to three recent user/assistant exchanges, with a cursor for older exchanges and a per-message character cap. `sessions.send` submits a follow-up to an idle session, while `sessions.draft` saves an unsent message for the user to review. `folders.list` and `folders.move` organize project sessions in sidebar folders, including a new folder.
- `sessions.stop`, `sessions.archive` and `sessions.delete` take a `sessionId` to manage another session in the project. Monos can select an assigned project with `project`. Stop cancels the current turn and pauses queued messages; archive stops and saves the conversation for later restoration; delete stops and permanently removes the conversation. Open files, terminals and worktrees are kept. These actions cannot target the caller, Mono chats, habit runs or orchestration workers. Reuse the same request ID when retrying a call.
- When a Mono successfully stops, archives or deletes a session it is monitoring, its pending completion report for that session is dismissed, including any queued report. The Mono confirms the action in its current reply. Reports for other sessions and other Monos are kept. Rejected launches or follow-ups return a CLI error without a later completion report.
- `notes.list` returns titles and short previews; `notes.read` returns one full note by ID.
- `notes.list` accepts `limit` and `offset`. `notes.write` creates a note from its
  `body`, with optional title and tags; pass an existing note `id` to update just
  its title, body or tags while preserving the other fields.

Retry an uncertain mutation with the same request ID and identical input.
Unknown fields and invalid options produce errors instead of being ignored.

## Native Operator completion reports

In the native app, an ordinary Operator chat can set `notifyOnComplete: true`
on `sessions.start` or `sessions.send`, then end its turn. MonoCode resumes that
chat with a completion report when its subscribed work finishes. Child sessions
do not need Operator access.

The optional `notifyGroup` field controls when reports arrive:

| Input | Report timing |
| --- | --- |
| No label | One report after all subscribed workers from that caller turn finish. |
| A different label per worker | One report for each worker as it finishes. |
| The same label for several workers | One report after that group finishes, including workers added in later turns before delivery. |

Labels belong to the calling session. They are trimmed and must contain 1 to 64
characters. A label requires `notifyOnComplete: true`. Reusing a label after its
report was delivered starts a fresh group. Monos reject `notifyGroup`; their
existing completion behaviour is unchanged.

For example, launch the implementation with `notifyGroup: "implementation"`
and the review with `notifyGroup: "review"` to receive separate reports. Use
`notifyGroup: "release-check"` on both to wait for both. Each call also needs
`notifyOnComplete: true` and its usual launch or send fields.

Reports include completed, failed, stopped or removed outcomes, and list other
reports still pending. Completion does not establish that the work passed
verification. The receiving assistant must inspect and verify the result.

Delivery waits while the parent is busy, has a draft or attachments, has queued
work, or cannot use its selected provider account. Archiving or deleting the
parent drops its subscriptions. When the caller itself stops, archives or
deletes a monitored worker through the CLI, that worker's subscription is
dismissed, since the caller can confirm the action in its current reply.

An empty chat can still report a terminal failure or cancellation. A fresh empty
chat is not treated as completed. Sends to a session whose worktree is marked
removed fail before accepting the prompt or adding a completion subscription.

If saved subscriptions cannot be read, the app shows the load error and disables
subscription changes. It preserves the stored records rather than treating the
failure as an empty snapshot. Follow the [recovery steps](#recovering-from-a-subscription-load-error)
below before using completion subscriptions again.

Subscriptions survive app restarts. Unfinished workers are reported as stopped;
known terminal outcomes are retained. MonoCode saves delivery before dispatch to
prevent duplicate automatic follow-ups after a restart. A crash between those
steps can lose that report; delivery is at most once, rather than guaranteed.
Project boundaries and the restriction against managing the caller remain in
force.

### Recovering from a subscription load error

1. Quit all MonoCode instances using the affected profile. Locate its data
   directory using [Data and configuration](native/README.md#data-and-configuration),
   accounting for any custom database path. Back up the entire directory,
   including any SQLite `-wal` and `-shm` files that remain after quitting.
2. Identify the affected storage for the recovery request. Desktop-compatible
   profiles save `operator-completions:<session-id>` records in the `settings`
   table of `monocode.db.gpui.sqlite3`, beside `monocode.db`. Standalone development
   profiles save those records in the `native_settings` table of `monocode.db`.
   Do not delete either database or edit records as a general repair step.
3. [Open a bug report](https://github.com/hardbeat920/monocode/issues/new?template=bug_report.yml)
   with the app version, operating system and exact load error. Ask for recovery
   instructions for your profile. Keep the backup private, since it includes
   conversations and settings.
4. Follow the maintainer's repair instructions, then restart MonoCode so it can
   load the repaired subscriptions before accepting new completion subscriptions.

Repairing records can preserve pending reports. If recovery requires resetting
or removing subscription records, pending automatic reports for the affected
parent chats are discarded. They are not recreated automatically; inspect the
worker conversations manually for their results. Resetting subscription records
alone does not delete worker conversations or rerun their work.

## Orchestration workers

Orchestration workers keep their existing scoped `control` workflow and do not receive this app access.

Return to the [README](../README.md) for a quick start.
