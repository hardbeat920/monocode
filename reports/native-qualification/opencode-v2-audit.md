# OpenCode 2 compatibility audit

The installed OpenCode 2.0.20 failed the native smoke fixture before a model turn. Catalog discovery returned no models. The [smoke log](live-harness/opencode.log) and [metadata](live-harness/opencode.json) record that failure. Both the retained TypeScript adapter and the Rust adapter use the version 1 commands and protocol. This is a baseline compatibility gap, not evidence that the native port broke an otherwise working version 2 integration.

This audit used the official release source at commit `84c9be93a56304a108f1a22df0c5d62c26d5b6ca`, which the [v2.0.20 release](https://github.com/anomalyco/opencode/releases/tag/v2.0.20) identifies.

## Bounded read-only actions

The audit read the existing smoke evidence, both adapters, and the pinned upstream source. It ran `command -v opencode`, `opencode --version`, and help for the root command, `models`, `serve`, `api`, `acp`, `debug`, and the debug subcommands. The executable resolves to Homebrew's installed 2.0.20 directory.

The audit read the structure of the existing local `opencode.jsonc` without printing configuration values. Its top-level keys are `$schema` and `mcp`. It contains no provider or agent overrides. This does not prove which credentials or models the installed provider can use.

No catalog command, API command, ACP process, model turn, account login, installation, downgrade, configuration write, service change, or permission change ran. In version 2, the CLI's [server connection code](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/cli/src/services/server-connection.ts) can create a background service even for a model listing. Its [debug config command](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/cli/src/commands/handlers/debug/config.ts) also resolves that service, so the audit inspected the file directly instead.

## Confirmed contract differences

| Area | Retained and native version 1 integration | Official version 2.0.20 contract |
| --- | --- | --- |
| Catalog commands | `models --verbose` produces a model identifier followed by JSON. `agent list` supplies agent headers. | `models` prints identifiers only. Structured models and agents come from API operations. |
| Model metadata | Variants are an object keyed by variant identifier. | Variants are an array of objects with `id`. |
| HTTP routes | `/session`, `/event`, and `/session/{id}/prompt_async`. | `/api/session`, `/api/event`, and `/api/session/{id}/prompt`. The pinned OpenAPI has no version 1 `/session` or `/event` route. |
| Working directory | Requests use the `directory` query and `x-opencode-directory` header. | Session creation includes a location. Catalog requests use a location query. |
| Model selection and input | A prompt contains `parts` and a model with `providerID` and `modelID`. | The session selects a model with `providerID`, `id`, and optional `variant`. A prompt contains `text` and optional `files`. |
| Permissions | Rules contain `permission`, `pattern`, and `action`. The reply uses a global permission route and `reply`. | Rules contain `action`, `resource`, and `effect`. The reply is session-scoped and contains `decision`. |
| Questions and events | Question routes and `question.asked`. Transcript handling reads event `properties`, `message.part.*`, and `session.status`. | Forms and form reply routes. Events contain `data`, with `session.text.delta` and `session.execution.*`. |

The [catalog command](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/cli/src/commands/handlers/models.ts), [model schema](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/schema/src/model.ts), [session protocol](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/protocol/src/groups/session.ts), [prompt schema](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/schema/src/prompt-input.ts), [permission protocol](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/protocol/src/groups/permission.ts), and [event schema](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/schema/src/event.ts) define these differences. The [OpenAPI document](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/protocol/openapi.json) has 113 paths.

Version 2's [server startup](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/cli/src/server-process.ts) also requires Basic authentication. It creates a random password if no password environment variable exists and prints that password in ordinary foreground mode. The current Rust client supplies no Authorization header. Any compatible adapter must keep the owned server credential out of transcript and qualification logs.

The existing Rust URL parser already accepts the version 2 `server listening on` line. The client already unwraps `{data}` responses. Neither mechanism needs replacement merely because the provider's major version changed. Removing `--verbose` would still leave the catalog parser without its required metadata and would not repair the turn protocol.

## Recommended scope

Preserve the version 1 adapter. Add major-version dispatch and a separate version 2 HTTP adapter and structured catalog reader. Cover server authentication, model and agent selection, prompt and attachment conversion, approvals, forms, cancellation, persisted resume, revert, and child-session events through pinned protocol fixtures before another paid turn.

ACP is a smaller route for basic text turns. The official [ACP service](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/cli/src/acp/service.ts) supports model and effort options, attachments, load, and resume. Its [event handling](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/cli/src/acp/event.ts) cancels `form.created` rather than forwarding questions to the client. An ACP-only fallback would therefore lose retained question behavior.

Version 2's [managed server startup](https://github.com/anomalyco/opencode/blob/84c9be93a56304a108f1a22df0c5d62c26d5b6ca/packages/server/src/process.ts) resumes suspended sessions only when the managed service lifecycle exists. The foreground and stdio modes do not install that lifecycle. An owned `serve --stdio` process can therefore preserve the user's existing CLI account, configuration, and provider session database without starting a background service or copying credentials. Metadata and protocol qualification will still use owned data and configuration paths. The coordinator subsequently authorized a separate version 2 HTTP implementation. The separate implementation now passes pinned transport fixtures and one real anonymous free-model turn. The [qualification report](opencode-v2/qualification.md) records its implementation, source identity, isolation, proof, and remaining checks.
