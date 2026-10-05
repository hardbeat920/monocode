# What submit needs from other packages

Submit works around each gap locally. The local traits live in
`submit/hooks.rs` (`SubmitPeers`, filled in with `Submit::set_peers`), with
no-op defaults and the method names below, so moving them into
`runtime::hooks` is mechanical.

## Runtime hooks

1. `OrchestrationHooks`: `submission_error(session_id, managed)`,
   `led_run_status(session_id)` (`orchestrator.run`),
   `run_status_for_session(session_id)` (`orchestrator.forSession`),
   `observe(session_id, event)`, `prompt(session_id, text)`, and a
   `stop_for_session` that returns `None` when the session has no run (Stop
   then cancels the turn itself). The proposal helpers
   `discover_settings`, `planning_prompt`, `repair_prompt`, and
   `complete_proposal` (orchestrationPlan.ts, orchestrationCatalog.ts).
2. `RemoteHooks`: `is_remote(cwd)` (`remoteProjectFor`) and the
   `remoteSessionActions` calls `submit`, `save_draft`, `compact`, `stop`,
   plus `build_plan` (`buildRemotePlan`).
3. A projects hook: `synchronize_project_location`,
   `apply_project_location_change`, `removing_worktree_paths`,
   `create_worktree`, `rename_worktree_branch`.
4. `AttentionHooks`: `dismiss_notices_for_continued_session` (due reminders
   and the linked work item update card) and `announce_finished_later`
   (`Notifier::announce_finished_later`, attention NEEDS item 5).
5. Inbox: `ask_prompt` (`inboxAskPrompt`, which needs instructions/inbox.md)
   and `resolve_linked_work_item`.
6. Files and notes: `apply_file_mentions` (`applyFileMentionsToTurn`) and
   `apply_notes` (`applyNotesToTurn`).
7. History: `draft_session_discarded`, so `onRemoveDraft` can drop a
   draft-only session from the history and linked-session lists.

## Runtime `Sessions`

8. Resolved. `Sessions::clear_save_state` clears the queued save, saved
   fingerprint, and saved user block when `onRemoveDraft` discards a chat.

## Engine globals

9. A shared `Kv`, `HarnessRegistry`, `SharedCatalog`, and harness
   availability. `SubmitConfig` takes them; an `Engine` accessor would let
   every package share one.

## Harness crate

10. `NativeCommandProvider` cannot say whether it has `subscribe` without
    subscribing. `skillCatalogKey` scoped a catalog to the session when it
    did. Submit uses `raw_slash_commands()` as the stand-in (only omp has
    both today). A `fn has_live_updates(&self) -> bool` would fix it.

## Duplicates to merge later

11. `submit::message_queue` copies the three helpers `onSubmit` needs from
    `attention::queue`, because submit cannot depend on the attention
    feature. Attention owns the queue and its dispatch.
12. `workspace::chat_context` copies part of `submit::chat_context`.
13. `submit::ci_repair` ports the pure half of inbox/model/ciRepair.ts
    (`CiRepairRequest`, `compactCiRepairContext`, `buildCiRepairRequest`),
    which handoffs, second opinions, and their tests need. The inbox
    package may port the same file.
14. `submit::pipeline::session_edits` has `temporaryWorktreeBranchName` and
    `namedWorktreeBranch` from worktrees.ts, which the projects package may
    also port.
