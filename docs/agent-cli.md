# Agent CLI commands

For Codex and Claude Code, type `/` and choose **All**, **Agent commands**, or
**Skills**. This choice is remembered. **Agent commands** accepts any slash
command, including custom and newly released commands absent from completion.

Use `/cli:status`, `/cli:model`, or `/cli:compact` to explicitly select the
agent's command in any filter. In **All**, existing skills and MonoCode's
`/plan` and `/compact` retain their meaning. In **Skills**, existing skill
submission is unchanged. The **CLI** button opens the native agent directly.

Native commands open an embedded terminal running the installed CLI. Complete
its startup prompts, then click **Send to CLI** to send the prepared command,
or type directly in the terminal. Type `/` there for the agent's own complete
menu. Its menus, custom commands, plugins, login, permissions, model settings
and future commands are handled by the installed agent. The suggestion catalog
contains documented command names; availability can differ by agent version,
platform, account, and enabled features.

The native session is **separate**. If this chat has provider history, Codex
uses `fork <id>` and Claude uses `--resume <id> --fork-session` to copy it. This
prevents two processes from writing the same conversation. Messages and
per-session settings are not synchronized back into the original MonoCode
chat. Project files and the agent account are shared: file edits, sign-out and
global configuration changes still affect other sessions. Use the agent's
`/resume` to reopen native history.
New chats and pending provider switches start a fresh native session. Native
CLI settings apply, including its default model and permission policy.

The regular composer stays mounted so drafts and attachments survive a visit
to CLI. Opening a CLI is blocked during an active turn or queued follow-ups.
**Close CLI · Back to chat** terminates only that embedded process tree. Closing
the MonoCode pane or app also cleans it up. Closing may interrupt work in that
native session; use `/exit` in the agent when you want it to finish gracefully.

Validation: `npm run check:web`, `npm run check:rust`. The browser smoke fixture
at `/tests/agent-cli.html` (serve with `npm run dev`) mocks IPC and cannot start
an agent or model turn. It covers the real composer and native-terminal UI.

Command-name references:

- [Codex commands](https://learn.chatgpt.com/docs/developer-commands)
- [Claude Code commands](https://code.claude.com/docs/en/commands)
