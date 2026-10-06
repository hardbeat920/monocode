import { useEffect, useMemo, useState } from "react";
import { OPEN_CONNECTIONS_EVENT } from "../../connections/model/connections";
import { HARNESS_TITLE } from "../../sessions/model/session";
import { ModelPicker } from "../../sessions/ui/ModelPicker";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { useFigmaActivity } from "../hooks/useFigmaActivity";
import { useFigmaBridge } from "../hooks/useFigmaBridge";
import { useFigmaModel } from "../hooks/useFigmaModel";
import { useFigmaSessionTarget } from "../hooks/useFigmaSessionTarget";
import {
  captureFigmaGeneration,
  figmaDisplayName,
  figmaProjectError,
  figmaSourceLabel,
  loadFigmaSelectionPreview,
  requestFigmaLaunch,
  requestFigmaSessionModel,
  type FigmaActivity,
  type FigmaConnection,
  type FigmaSource,
} from "../model/figma";
import { reportFigmaActivity } from "../model/figmaActivity";
import {
  clearFigmaModelPick,
  pickFigmaModel,
  resolveFigmaModel,
  type FigmaModelSource,
} from "../model/figmaModels";

const PREVIEW_DELAY_MS = 250;

const MODEL_SOURCE_LABEL: Record<FigmaModelSource, string> = {
  picked: "Picked here",
  figma: "Figma default",
  project: "Project default",
};

function openConnections() {
  window.dispatchEvent(new Event(OPEN_CONNECTIONS_EVENT));
}

function activityMessage(activity: FigmaActivity): string {
  if (activity.status === "capturing")
    return `Capturing ${figmaDisplayName(activity.source.name)} from Figma…`;
  if (activity.status === "started")
    return activity.target.kind === "session"
      ? `Sent ${figmaDisplayName(activity.generation.source.name)} to ${figmaDisplayName(activity.target.title)}. Its preview files appear in that chat.`
      : `Started ${HARNESS_TITLE[activity.choice.harness]} for ${figmaDisplayName(activity.generation.source.name)}.`;
  return activity.message;
}

function EmptyState({ message }: { message: string }) {
  return (
    <div className="flex flex-col items-start gap-2 px-3 py-3">
      <p className="text-[12px] leading-relaxed text-content/50">{message}</p>
      <SecondaryButton onClick={openConnections}>
        Open Figma settings
      </SecondaryButton>
    </div>
  );
}

function SelectionPreview({
  connectionId,
  source,
}: {
  connectionId: string;
  source: FigmaSource;
}) {
  const [preview, setPreview] = useState<{ key: string; url: string } | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [request, setRequest] = useState(0);
  const key = `${connectionId}:${source.nodeId}:${source.width}x${source.height}:${request}`;

  useEffect(() => {
    let cancelled = false;
    setError(null);
    const timer = window.setTimeout(() => {
      void loadFigmaSelectionPreview(connectionId, source.nodeId)
        .then((url) => {
          if (!cancelled) setPreview({ key, url });
        })
        .catch((reason: unknown) => {
          if (!cancelled)
            setError(String(reason instanceof Error ? reason.message : reason));
        });
    }, PREVIEW_DELAY_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [connectionId, key, source.nodeId]);

  const current = preview?.key === key ? preview.url : null;

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex min-h-24 items-center justify-center overflow-hidden rounded-lg border border-content/10 bg-content/3 p-2">
        {current ? (
          <img
            src={current}
            alt={`Preview of ${figmaDisplayName(source.name)}`}
            className="max-h-64 max-w-full object-contain"
            draggable={false}
          />
        ) : (
          <span className="text-[12px] text-content/40">
            {error ? "Preview unavailable" : "Loading preview…"}
          </span>
        )}
      </div>
      {error ? (
        <div className="flex items-center justify-between gap-2">
          <p role="alert" className="min-w-0 text-[11px] text-red-400/90">
            {error}
          </p>
          <SecondaryButton onClick={() => setRequest((value) => value + 1)}>
            Retry
          </SecondaryButton>
        </div>
      ) : null}
    </div>
  );
}

function ConnectionView({
  connection,
  cwd,
}: {
  connection: FigmaConnection;
  cwd: string;
}) {
  const activity = useFigmaActivity(cwd);
  const model = useFigmaModel(cwd);
  const target = useFigmaSessionTarget(cwd);
  const [generating, setGenerating] = useState(false);
  const selection = connection.selection;
  const source = selection?.source ?? null;
  const projectError = figmaProjectError(cwd);

  const generate = async () => {
    if (!source || generating || projectError) return;
    setGenerating(true);
    reportFigmaActivity({ status: "capturing", cwd, source });
    try {
      const generation = await captureFigmaGeneration(connection.id);
      requestFigmaLaunch({
        generation,
        cwd,
        choice: resolveFigmaModel(cwd).choice,
      });
    } catch (reason) {
      reportFigmaActivity({
        status: "failed",
        cwd,
        message: String(reason instanceof Error ? reason.message : reason),
      });
    } finally {
      setGenerating(false);
    }
  };

  return (
    <div className="flex flex-col gap-3 px-3 py-3">
      {selection ? (
        <p className="truncate text-[12px] text-content/50">
          {figmaDisplayName(selection.document.name)} /{" "}
          {figmaDisplayName(selection.document.pageName)}
        </p>
      ) : null}
      {source ? (
        <>
          <div className="flex min-w-0 flex-col">
            <span className="truncate text-[13px] font-medium text-content">
              {figmaDisplayName(source.name)}
            </span>
            <span className="text-[12px] text-content/45">
              {figmaSourceLabel(source)}
            </span>
          </div>
          <SelectionPreview connectionId={connection.id} source={source} />
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[12px]">
            <dt className="text-content/45">Layer</dt>
            <dd className="truncate text-content/70">{source.nodeId}</dd>
            <dt className="text-content/45">Type</dt>
            <dd className="text-content/70">{source.type}</dd>
            <dt className="text-content/45">Size</dt>
            <dd className="text-content/70">
              {Math.round(source.width)} × {Math.round(source.height)}
            </dd>
          </dl>
        </>
      ) : (
        <p className="text-[12px] leading-relaxed text-content/50">
          {selection && selection.selectionCount > 1
            ? `${selection.selectionCount} layers are selected. Select exactly one layer to inspect and generate it.`
            : "Select a layer in Figma to see it here."}
        </p>
      )}
      {projectError ? null : target ? (
        <div className="flex flex-col gap-1.5">
          <span className="text-[12px] text-content/45">Generate with</span>
          <div className="flex min-w-0 items-center gap-2">
            <ModelPicker
              harness={target.harness}
              model={target.model}
              values={target.modelSettings}
              project={cwd}
              onChange={(harness, next) =>
                requestFigmaSessionModel({
                  sessionId: target.sessionId,
                  kind: "model",
                  harness,
                  model: next,
                })
              }
              onSettingsChange={(modelSettings) =>
                requestFigmaSessionModel({
                  sessionId: target.sessionId,
                  kind: "settings",
                  modelSettings,
                })
              }
            />
            <span
              className="min-w-0 truncate text-[11px] text-content/40"
              title={target.title}
            >
              {figmaDisplayName(target.title)}
            </span>
          </div>
          <p className="text-[11px] leading-relaxed text-content/40">
            {target.busy
              ? "The selected session is busy. The component is queued and generated after its current turn."
              : "Generated in the selected session as preview files that git ignores. Review them in its chat, then ask to implement the component."}
          </p>
        </div>
      ) : (
        <div className="flex flex-col gap-1.5">
          <span className="text-[12px] text-content/45">Generate with</span>
          <div className="flex min-w-0 items-center gap-2">
            <ModelPicker
              harness={model.choice.harness}
              model={model.choice.model}
              values={model.choice.modelSettings}
              project={cwd}
              onChange={(harness, next) =>
                pickFigmaModel(cwd, {
                  harness,
                  model: next,
                  modelSettings: model.choice.modelSettings,
                })
              }
              onSettingsChange={(modelSettings) =>
                pickFigmaModel(cwd, { ...model.choice, modelSettings })
              }
            />
            <span className="min-w-0 truncate text-[11px] text-content/40">
              {MODEL_SOURCE_LABEL[model.source]}
            </span>
            {model.source === "picked" ? (
              <button
                type="button"
                onClick={() => clearFigmaModelPick(cwd)}
                className="shrink-0 text-[11px] text-content/55 hover:text-content"
              >
                Reset
              </button>
            ) : null}
          </div>
          <p className="text-[11px] leading-relaxed text-content/40">
            No session is selected in this project, so the component starts a
            new one.
          </p>
        </div>
      )}
      <button
        type="button"
        disabled={!source || generating || projectError !== null}
        onClick={() => void generate()}
        className="h-8 rounded-md bg-selection px-3 text-[12px] font-medium text-content hover:bg-selection-hover disabled:cursor-default disabled:opacity-40"
      >
        {generating ? "Capturing…" : "Generate component"}
      </button>
      {projectError ? (
        <p className="text-[12px] text-content/45">{projectError}</p>
      ) : null}
      {activity ? (
        <p
          role={activity.status === "failed" ? "alert" : "status"}
          className={`text-[12px] leading-relaxed ${
            activity.status === "failed" ? "text-red-400/90" : "text-content/50"
          }`}
        >
          {activityMessage(activity)}
        </p>
      ) : null}
    </div>
  );
}

export function FigmaPanel({ cwd }: { cwd: string }) {
  const { status, error } = useFigmaBridge();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const connections = useMemo(() => status?.connections ?? [], [status]);
  const connection =
    connections.find((entry) => entry.id === selectedId) ??
    connections[connections.length - 1] ??
    null;

  if (error) {
    return (
      <p role="alert" className="px-3 py-2 text-[12px] text-red-400/90">
        {error}
      </p>
    );
  }
  if (!status) {
    return (
      <p className="px-3 py-2 text-[12px] text-content/50">
        Checking the Figma bridge…
      </p>
    );
  }
  if (!status.enabled) {
    return (
      <EmptyState message="The Figma bridge is off. Turn it on and install the plugin to see your Figma selection here." />
    );
  }
  if (status.error) {
    return <EmptyState message={status.error} />;
  }
  if (!connection) {
    return (
      <EmptyState
        message={
          status.pluginInstalled
            ? "Run MonoCode Figma Bridge in Figma Desktop to connect a file."
            : "Install the Figma plugin to connect Figma Desktop."
        }
      />
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      {connections.length > 1 ? (
        <div className="flex flex-wrap gap-1 border-b border-stroke px-2 py-1.5">
          {connections.map((entry) => (
            <button
              key={entry.id}
              type="button"
              aria-pressed={entry.id === connection.id}
              onClick={() => setSelectedId(entry.id)}
              className={`max-w-full truncate rounded-md px-2 py-0.5 text-[12px] ${
                entry.id === connection.id
                  ? "bg-content/10 text-content"
                  : "text-content/55 hover:bg-content/5"
              }`}
            >
              {entry.selection
                ? figmaDisplayName(entry.selection.document.name)
                : "Figma file"}
            </button>
          ))}
        </div>
      ) : null}
      <ConnectionView key={connection.id} connection={connection} cwd={cwd} />
    </div>
  );
}
