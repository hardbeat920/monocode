# Subagents

Any chat you start yourself can start subagents on any harness and model, such as a Codex review or a Haiku search. Each one is its own chat, shown under the message that started it.

The lead starts one with `thread_start` and waits for its reply with `thread_wait`. `thread_read` shows what a subagent did, `thread_send` gives it another prompt, `thread_stop` stops it and `thread_list` lists the lead's subagents.

A subagent started with `workspaceMode: worktree` works in its own git worktree. Merging that work back is up to the lead.

`thread_close` archives a finished subagent's chat and takes it off the lead's list and the sidebar. A working subagent has to be stopped first.

`new_thread` is different: it opens a top-level chat that belongs to you, not a subagent of the chat that opened it.

By default, Claude Code's `Agent` tool and Codex's multi-agent feature are turned off in these chats, so every subagent is a MonoCode chat. To let them use their own subagent tools again, turn off **Settings > Providers > Subagents only through MonoCode**.

Return to the [README](../README.md) for a quick start.
