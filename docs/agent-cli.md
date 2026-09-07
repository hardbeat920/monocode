# Agent commands in chat

For Codex and Claude Code, the `/` picker has remembered **All**, **Agent
commands**, and **Skills** filters. Commands operate on the current MonoCode
conversation. Their results and controls appear above the composer while the
transcript, draft, and attachments stay mounted.

Use `/agent:status` or `/agent:model` to choose a command explicitly when a skill
has the same name. `/cli:name` is accepted as a compatibility alias and has the
same chat behavior. In **All**, skills and MonoCode's `/plan` and `/compact` keep
their existing meaning. **Skills** retains normal skill submission.

| Command | Behavior in MonoCode |
| --- | --- |
| `/status` | Show this chat's model, access mode, project, provider session ID, state, and reported context usage. |
| `/context` | Show the last context reading reported by the agent. |
| `/model [model ID]` | Open a model selector or select an exact ID/name from this agent's catalog. |
| `/permissions [mode]`, `/approvals [mode]` | Show or change this chat's MonoCode access mode. |
| `/compact` | Compact the existing conversation through its provider adapter. |
| `/plan [request]` | Plan the next message, or send the supplied request in plan mode. |
| `/fast [on\|off]` | Show or set fast mode when the current model's catalog exposes it. |
| `/effort [level]`, `/reasoning [level]` | Show or set an effort level advertised by the current model. |
| `/diff` | Open this session's project diff in MonoCode. |
| `/stop` | Stop the current agent turn. |
| `/help` | Show the supported chat commands. |

Access modes use MonoCode's existing names: `supervised`, `auto-accept-edits`,
`auto`, and `full-access`. Settings apply to the same conversation through the
same callbacks as the normal composer controls. Settings changes and compaction
are blocked during an active turn or queued follow-ups; status and stop remain
available.

This is a GUI command integration, not full CLI command parity. The picker also
recognizes documented CLI names, but entries without a chat implementation are
marked **Not supported in chat**. Selecting one shows an error and preserves the
command draft. Unknown names in **Agent commands** receive the same treatment.
No command starts a terminal, forks provider history, or falls back to sending
unsupported command text as a model prompt. `/plan <request>` intentionally
starts a normal planning turn, and `/compact` invokes provider compaction.

`/status` displays MonoCode's current session data, not the terminal's native
status screen. Account rate limits remain in the existing usage footer; unknown
context usage is shown as unavailable rather than zero.

Validate with `npm run check`. The browser regression fixture at
`/tests/agent-commands.html` (serve with `npm run dev`) uses the real SessionPane,
composer, and transcript with mocked IPC. It cannot launch a real agent or PTY.

Protocol and command references:

- [Codex app-server](https://learn.chatgpt.com/docs/app-server)
- [Codex CLI commands](https://learn.chatgpt.com/docs/developer-commands)
- [Claude Agent SDK commands](https://code.claude.com/docs/en/agent-sdk/slash-commands#commands-in-agent-sdk-sessions)
- [Claude Code commands](https://code.claude.com/docs/en/commands)
