# Agent access

Every chat can reach the running app through the MonoCode MCP server. These tools work on the chat's own project and aren't available on remote projects. A read-only or planning thread can only use the ones that read.

## App and panels

- `app_state` shows the project, the active thread, visible panes and which panels are open.
- `app_panel` opens or closes the sidebar, right panel, terminal, settings, inbox, notes or automations.
- `app_actions` lists the actions `app_run` can perform, the same ones the keymap can.
- `app_settings` reads the settings agents may change and their options, or changes some of them. Binary paths, hooks, keybindings, default access and email masking stay with you.

## Threads

- `app_thread_list` and `app_thread_read` find and read other threads in the project. Each read is recorded, keeping the last 20 per thread.
- `app_thread_open` shows a thread as a tab or a split. `app_thread_close` closes its pane or tab without archiving it.
- `app_thread_files` shows which files other active threads changed through edit tools.
- `app_thread_compose` adds a message to a thread's composer for you to review and send. It never replaces text already there.
- `app_thread_set` changes a thread's model, effort or access from its next turn. Raising access shows a dialog, and nothing changes unless you allow it. These changes don't affect your defaults for new chats.
- `app_thread_manage` stops or archives another regular thread.

Subagents follow their lead, so their model, effort and composer can't be changed.

## Project

- `app_worktrees` lists git worktrees or creates one.
- `app_folders` lists sidebar folders or moves a thread into one.
- `app_notes` lists, reads or writes notes.

## Monos

Monos keep their own project actions, including starting, messaging and deleting sessions, through the `app` CLI. Run `app help` in a Mono's chat for the list. Their **Show started sessions** setting controls whether their launches appear in your history.

Return to the [README](../README.md) for a quick start.
