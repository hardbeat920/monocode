import { useSyncExternalStore } from "react";
import { X } from "./icons";
import { ModelSettings } from "./ModelSettings";
import {
  CHAT_COMMANDS,
  commandModel,
  commandsLocked,
  type CommandPanel,
} from "../lib/agentCommands";
import { getModelSnapshot, modelsFor, subscribeModels } from "../lib/models";
import { contextTooltip } from "../lib/contextUsage";
import {
  RUNTIME_MODES,
  RUNTIME_MODE_LABEL,
  RUNTIME_MODE_HINT,
  sessionWorkCwd,
  type RuntimeMode,
  type Session,
} from "../lib/session";

type Props = {
  panel: CommandPanel;
  session: Session;
  onClose: () => void;
  onModelChange: (model: string) => void;
  onModelSettingsChange: (settings: Record<string, string>) => void;
  onRuntimeModeChange: (mode: RuntimeMode) => void;
};

/** A small response/control panel in the existing composer. It never replaces
 * the transcript or changes the identity of the conversation. */
export function AgentCommandPanel({
  panel,
  session,
  onClose,
  onModelChange,
  onModelSettingsChange,
  onRuntimeModeChange,
}: Props) {
  useSyncExternalStore(subscribeModels, getModelSnapshot, getModelSnapshot);
  const model = commandModel(session);
  const context = session.context ? contextTooltip(session.context) : null;
  const locked = commandsLocked(session);
  return (
    <section
      aria-label={`Agent command /${panel.command}`}
      className="mb-2 max-h-[40vh] overflow-y-auto rounded-lg border border-content/15 bg-content/5 px-3 py-2 text-xs"
    >
      <div className="mb-2 flex items-center gap-2">
        <strong className="font-mono">/{panel.command}</strong>
        <span className="flex-1 text-content/50">
          {session.harness === "codex" ? "Codex" : "Claude Code"} · current chat
        </span>
        <button
          type="button"
          aria-label="Dismiss command result"
          onClick={onClose}
          className="rounded p-1 text-content/60 hover:bg-content/10"
        >
          <X className="size-3.5" />
        </button>
      </div>
      {panel.message ? (
        <p
          role={panel.error ? "alert" : "status"}
          className={panel.error ? "text-red-400" : "text-content/70"}
        >
          {panel.message}
        </p>
      ) : null}
      {panel.kind === "status" || panel.kind === "context" ? (
        <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1.5">
          {panel.kind === "status" ? (
            <>
              <dt className="text-content/50">Model</dt>
              <dd className="break-all">
                {model?.name || session.model || "Not selected"}
              </dd>
              <dt className="text-content/50">Access</dt>
              <dd>{RUNTIME_MODE_LABEL[session.runtimeMode]}</dd>
              <dt className="text-content/50">Project</dt>
              <dd className="break-all">{sessionWorkCwd(session)}</dd>
              <dt className="text-content/50">Session</dt>
              <dd className="break-all">
                {session.providerSessionId || "Starts with the first message"}
              </dd>
              <dt className="text-content/50">State</dt>
              <dd>
                {session.busy ? "Working" : "Idle"}
                {session.queuedMessages?.length
                  ? ` · ${session.queuedMessages.length} queued`
                  : ""}
              </dd>
            </>
          ) : null}
          <dt className="text-content/50">Context</dt>
          <dd>
            {context
              ? `${context.headline} · ${context.detail}`
              : "Not reported by the agent yet"}
          </dd>
        </dl>
      ) : null}
      {panel.kind === "model" ? (
        <label className="flex flex-col gap-2">
          Model for this chat
          <select
            aria-label="Chat model"
            disabled={locked}
            value={session.model}
            onChange={(event) => onModelChange(event.target.value)}
            className="rounded border border-content/15 bg-background-base px-2 py-1.5 text-content disabled:opacity-50"
          >
            {!model ? (
              <option value={session.model}>
                {session.model || "Select a model"}
              </option>
            ) : null}
            {modelsFor(session.harness).map((entry) => (
              <option key={entry.id} value={entry.id}>
                {entry.name}
              </option>
            ))}
          </select>
          {!modelsFor(session.harness).length ? (
            <span className="text-content/50">
              No models have been reported by this agent yet.
            </span>
          ) : null}
        </label>
      ) : null}
      {panel.kind === "permissions" ? (
        <div
          className="flex flex-wrap gap-1.5"
          role="group"
          aria-label="Chat permissions"
        >
          {RUNTIME_MODES.map((mode) => (
            <button
              key={mode}
              type="button"
              disabled={locked}
              aria-pressed={session.runtimeMode === mode}
              title={RUNTIME_MODE_HINT[mode]}
              onClick={() => onRuntimeModeChange(mode)}
              className={`rounded px-2 py-1.5 disabled:opacity-50 ${session.runtimeMode === mode ? "bg-accent/15 text-accent" : "bg-content/5 hover:bg-content/10"}`}
            >
              {RUNTIME_MODE_LABEL[mode]}
            </button>
          ))}
        </div>
      ) : null}
      {panel.kind === "settings" ? (
        <fieldset
          disabled={locked}
          className="flex flex-wrap gap-1.5 disabled:opacity-50"
        >
          <ModelSettings
            harness={session.harness}
            model={session.model}
            values={session.modelSettings}
            onChange={(settings) => {
              if (!locked) onModelSettingsChange(settings);
            }}
          />
        </fieldset>
      ) : null}
      {["model", "permissions", "settings"].includes(panel.kind) && locked ? (
        <p className="mt-2 text-content/50">
          Finish the current turn and queued messages to change settings.
        </p>
      ) : null}
      {panel.kind === "help" ? (
        <>
          <p className="mb-2 text-content/60">
            These commands act on this chat. Use /agent:name when a skill has
            the same name. Other CLI commands are marked as unsupported in chat.
          </p>
          <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1.5">
            {Object.entries(CHAT_COMMANDS).map(([name, description]) => (
              <div key={name} className="contents">
                <dt className="font-mono">/{name}</dt>
                <dd className="text-content/65">{description}</dd>
              </div>
            ))}
          </dl>
        </>
      ) : null}
    </section>
  );
}
