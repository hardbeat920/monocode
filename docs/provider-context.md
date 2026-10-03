# Continue a conversation with another provider

Choose another provider and model in the composer, then send a message. MonoCode keeps the session and working copy. A request submitted during an active local turn waits for that turn to finish and keeps its selected provider and model. The remote client applies a provider change after its current turn settles when the host advertises `sessionProviderSwitchV1`.

MonoCode saves native conversation bindings by provider, account, and working directory. A supported return to an earlier provider resumes its native conversation with the missing interval of shared history. A missing saved transcript boundary requires a fresh conversation with the surviving history. Changing a model within one provider keeps the existing continuation behavior.

## What shared history contains

The exporter preserves complete selected user and assistant messages, recorded command and tool outcomes, file activity, plans, task lists, and error notices. It records each item's original provider and model when available. Historical tool records are evidence. Importing them never executes a tool call.

The default allowance is 16,000 bytes, with a 64,000-byte ceiling. Known model capacity and native occupancy reduce that allowance after reserves for the fully prepared current request, attachment content and paths, and further work. Request preparation includes approved plans and MonoCode instructions before the exporter checks capacity or changes provider processes. The exporter keeps items whole, orders them chronologically, and records omissions. Oversized history can be read from a durable snapshot through the target provider's file tools. Transfer details show the delivery mode and selected and omitted item counts.

Private reasoning, unsent drafts, internal prompts, and unfinished activity stay outside the portable history. Historical attachments travel as references to durable saved copies when available. Their bytes are not inserted as historical image messages. Missing or oversized assets carry an unavailable reason. The current request and its attachments still use the provider's ordinary input path.

Codex imports historical user and assistant messages through `thread/inject_items` when its installed app-server supports that method. A method-not-found response uses attributed text instead. Other providers receive attributed history with a separate current-request section.

## Recovery and persistence

Picker intent, provider bindings, and transfer receipts persist separately from the transcript. A provider startup ID does not prove that it accepted a user request. MonoCode saves native import completion before submitting the current turn. It commits the accepted transfer only after the provider acknowledges that turn or supplies delivery evidence.

Claude confirms the resumed native ID before sending input. It gives each user request a unique input ID and records acceptance after the input write succeeds and Claude echoes that ID. Startup output and manual compaction do not accept a user request. The [CLI replay flag](https://code.claude.com/docs/en/cli-reference) provides this acknowledgment.

An unconfirmed import or acceptance can leave the target native conversation ambiguous. MonoCode retains the source, discards the uncertain target identity, and keeps the unaccepted request as a draft. Retry that draft to send its original request once into a fresh target conversation. A failed native resume uses eligible full portable history rather than an empty conversation.

The remote host owns history export, attachment snapshots, provider processes, and delivery receipts. Provider-switch commands include an expected session revision and use the existing command deduplication and persistent outbox. Older hosts retain their model-only configuration behavior. Switching providers does not move a session to another host.

A remote provider choice made during an active turn waits in the open client until the host becomes idle. Closing the client before it submits that change loses the choice. Once the host accepts it, the saved picker intent survives reconnects and host restarts. Local queued messages retain the existing in-memory queue behavior.

## Implementation

`providerContext.ts` defines bindings and delivery state. `portableContext.ts` exports and budgets history. `contextTransfer.ts` shares native and attributed-text delivery rules between the desktop registry and the Node host. `contextAssets.ts` and the owning storage implementation preserve historical asset references. The existing composer and handoff row display transfer state.

The SQLite session record stores a versioned provider-context envelope. Old records remain readable without it. Context snapshots and assets live under the owning application's data directory and are removed when the session is deleted.
