import { useEffect, useRef, useState } from "react";
import { copyText } from "../../../platform/tauri/clipboard";
import { revealPath } from "../../../platform/tauri/fs";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { defaultSessionChoice } from "../../sessions/model/models";
import { ModelPicker } from "../../sessions/ui/ModelPicker";
import { useFigmaBridge } from "../hooks/useFigmaBridge";
import { useFigmaDefaultModel } from "../hooks/useFigmaModel";
import {
  figmaDisplayName,
  figmaSourceLabel,
  installFigmaPlugin,
  resetFigmaPairing,
  setFigmaBridgeEnabled,
  type FigmaBridgeStatus,
} from "../model/figma";
import {
  saveFigmaDefaultModel,
  type FigmaModelChoice,
} from "../model/figmaModels";

function bridgeSummary(status: FigmaBridgeStatus): string {
  if (!status.enabled)
    return `Off. Turn it on so the MonoCode plugin in Figma Desktop can connect on localhost:${status.port}.`;
  if (status.listening)
    return `Listening on localhost:${status.port}. Only the plugin paired with this MonoCode can connect.`;
  return `Not listening on localhost:${status.port}.`;
}

function FigmaDefaultModel({
  onError,
}: {
  onError: (message: string) => void;
}) {
  const stored = useFigmaDefaultModel();
  const project = defaultSessionChoice();
  const shown: FigmaModelChoice = stored ?? {
    harness: project.harness,
    model: project.model,
    modelSettings: {},
  };

  const save = (choice: FigmaModelChoice | null) => {
    try {
      saveFigmaDefaultModel(choice);
    } catch (reason) {
      onError(String(reason instanceof Error ? reason.message : reason));
    }
  };

  return (
    <div className="mt-2 flex flex-col gap-1.5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <span className="text-[13px] font-medium text-content">
          Generation model
        </span>
        {stored ? (
          <SecondaryButton onClick={() => save(null)}>
            Use project default
          </SecondaryButton>
        ) : null}
      </div>
      <p className="text-[12px] leading-relaxed text-content/45">
        {stored
          ? "When a project has no selected session, Figma starts a new one with this agent and model. With a session selected, the component is generated in that session with its own agent and model."
          : "When a project has no selected session, Figma starts a new one with the project's default agent and model from Providers. Pick one here to use it instead. With a session selected, the component is generated in that session."}
      </p>
      <div className="flex items-center">
        <ModelPicker
          harness={shown.harness}
          model={shown.model}
          values={shown.modelSettings}
          onChange={(harness, model) =>
            save({ harness, model, modelSettings: shown.modelSettings })
          }
          onSettingsChange={(modelSettings) =>
            save({ ...shown, modelSettings })
          }
        />
      </div>
    </div>
  );
}

export function FigmaSettings() {
  const { status, setStatus, error, setError } = useFigmaBridge();
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const copiedTimer = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (copiedTimer.current != null) window.clearTimeout(copiedTimer.current);
    },
    [],
  );

  const run = async (action: () => Promise<FigmaBridgeStatus>) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      setStatus(await action());
    } catch (reason) {
      setError(String(reason instanceof Error ? reason.message : reason));
    } finally {
      setBusy(false);
    }
  };

  const manifestPath = status ? `${status.pluginDirectory}/manifest.json` : "";

  const copyManifestPath = () => {
    void copyText(manifestPath).then(
      () => {
        setCopied(true);
        if (copiedTimer.current != null)
          window.clearTimeout(copiedTimer.current);
        copiedTimer.current = window.setTimeout(() => setCopied(false), 1600);
      },
      (reason: unknown) =>
        setError(String(reason instanceof Error ? reason.message : reason)),
    );
  };

  return (
    <div className="px-4 py-3.5">
      {!status ? (
        <p className="text-[12px] text-content/45">
          {error ? null : "Checking the Figma bridge…"}
        </p>
      ) : (
        <div className="flex flex-col gap-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="min-w-0 flex-1 text-[12px] leading-relaxed text-content/65">
              {bridgeSummary(status)}
            </p>
            <SecondaryButton
              disabled={busy}
              onClick={() =>
                void run(() => setFigmaBridgeEnabled(!status.enabled))
              }
            >
              {status.enabled ? "Turn off" : "Turn on"}
            </SecondaryButton>
          </div>
          {status.error ? (
            <p role="alert" className="text-[12px] text-red-400/90">
              {status.error}
            </p>
          ) : null}
          {status.enabled ? (
            <div className="flex flex-col gap-2">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <span className="text-[13px] font-medium text-content">
                  Figma plugin
                </span>
                <SecondaryButton
                  disabled={busy}
                  onClick={() => void run(installFigmaPlugin)}
                >
                  {status.pluginInstalled
                    ? "Reinstall plugin"
                    : "Install plugin"}
                </SecondaryButton>
              </div>
              {status.pluginInstalled ? (
                <>
                  <ol className="list-decimal space-y-1 pl-4 text-[12px] leading-relaxed text-content/45">
                    <li>
                      In Figma Desktop, open Plugins → Development → Import
                      plugin from manifest…
                    </li>
                    <li>Choose the manifest below.</li>
                    <li>
                      Run MonoCode Figma Bridge. It reconnects on its own
                      whenever MonoCode is open.
                    </li>
                  </ol>
                  <div className="flex flex-wrap items-center gap-2">
                    <code className="min-w-0 flex-1 break-all rounded-md bg-content/5 px-2 py-1 text-[11px] text-content/65">
                      {manifestPath}
                    </code>
                    <SecondaryButton onClick={copyManifestPath}>
                      {copied ? "Copied" : "Copy path"}
                    </SecondaryButton>
                    <SecondaryButton
                      onClick={() =>
                        void revealPath(manifestPath).catch((reason: unknown) =>
                          setError(
                            String(
                              reason instanceof Error ? reason.message : reason,
                            ),
                          ),
                        )
                      }
                    >
                      Reveal folder
                    </SecondaryButton>
                  </div>
                </>
              ) : (
                <p className="text-[12px] leading-relaxed text-content/45">
                  Install the plugin to pair Figma Desktop with this MonoCode.
                  The plugin files stay in MonoCode's app data.
                </p>
              )}
              <FigmaDefaultModel onError={setError} />
              <div className="mt-2 flex flex-col gap-1.5">
                <span className="text-[13px] font-medium text-content">
                  Connected files
                </span>
                {status.connections.length === 0 ? (
                  <p className="text-[12px] text-content/45">
                    No Figma file is connected. Run the plugin in a file to
                    connect it.
                  </p>
                ) : (
                  status.connections.map((connection) => (
                    <div
                      key={connection.id}
                      className="flex min-w-0 flex-col text-[12px] text-content/65"
                    >
                      <span className="truncate text-content">
                        {connection.selection
                          ? `${figmaDisplayName(connection.selection.document.name)} / ${figmaDisplayName(connection.selection.document.pageName)}`
                          : "Figma file"}
                      </span>
                      <span className="truncate text-content/45">
                        {connection.selection?.source
                          ? `${figmaDisplayName(connection.selection.source.name)} · ${figmaSourceLabel(connection.selection.source)}`
                          : "No layer selected"}
                      </span>
                    </div>
                  ))
                )}
              </div>
              <div className="mt-2 flex flex-wrap items-center justify-between gap-3">
                <p className="min-w-0 flex-1 text-[12px] leading-relaxed text-content/45">
                  Reset pairing disconnects every plugin and issues a new key.
                  Run the plugin again in Figma to reconnect.
                </p>
                <SecondaryButton
                  danger
                  disabled={busy}
                  onClick={() => void run(resetFigmaPairing)}
                >
                  Reset pairing
                </SecondaryButton>
              </div>
            </div>
          ) : null}
        </div>
      )}
      {error ? (
        <p role="alert" className="mt-3 text-[12px] text-red-400/90">
          {error}
        </p>
      ) : null}
    </div>
  );
}
