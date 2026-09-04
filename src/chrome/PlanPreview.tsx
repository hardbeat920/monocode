import { AiIdea, CircleDashed, PanelRight, Play } from "./icons";
import { planSummary, planTitle } from "../lib/plan";
import type { HarnessId, PlanBlockMeta, PlanBuildTarget } from "../lib/session";
import { BuildTargetButton } from "./SecondOpinionButton";

type Props = {
  text: string;
  streaming?: boolean;
  busy?: boolean;
  plan?: PlanBlockMeta;
  harness?: HarnessId;
  model?: string;
  onOpen?: () => void;
  onBuild?: (target?: PlanBuildTarget) => void;
};

export function PlanPreview({
  text,
  streaming,
  busy,
  plan,
  harness,
  model,
  onOpen,
  onBuild,
}: Props) {
  const title = planTitle(text);
  const summary = planSummary(text);
  const buildDisabled =
    busy ||
    streaming ||
    !text.trim() ||
    plan?.status === "streaming" ||
    plan?.status === "building" ||
    plan?.status === "built";
  const buildLabel =
    plan?.status === "building"
      ? "Building…"
      : plan?.status === "built"
        ? "Built"
        : "Build";

  return (
    <div className="mb-2 overflow-hidden rounded-[12px] border border-content/10 bg-content/7">
      <div className="flex items-start gap-2.5 px-3 py-2.5">
        {streaming ? (
          <CircleDashed
            className="mt-0.5 size-4 shrink-0 text-content/40"
            strokeWidth={1.75}
          />
        ) : (
          <AiIdea
            className="mt-0.5 size-4 shrink-0 text-content/40"
            strokeWidth={1.75}
          />
        )}
        <div className="min-w-0 flex-1">
          {onOpen ? (
            <button
              type="button"
              className="block w-full truncate text-left font-sans text-[13px] font-medium text-content/90 hover:text-yellow-100"
              title={title}
              onClick={onOpen}
            >
              {title}
            </button>
          ) : (
            <span
              className="block truncate font-sans text-[13px] font-medium text-content/90"
              title={title}
            >
              {title}
            </span>
          )}
          {summary ? (
            <p className="mt-0.5 line-clamp-3 font-sans text-[12px] leading-4.5 text-content/50">
              {summary}
            </p>
          ) : null}
          {onOpen || onBuild ? (
            <div className="mt-2 flex items-center justify-end gap-1.5">
              {onOpen ? (
                <button
                  type="button"
                  title="Open in pane"
                  aria-label="Open plan in pane"
                  className="flex h-6 shrink-0 items-center gap-1 rounded-md bg-content/8 px-2 font-sans text-[11px] text-content/70 hover:bg-content/12 hover:text-content"
                  onClick={onOpen}
                >
                  <PanelRight className="size-3" strokeWidth={1.75} />
                  Open
                </button>
              ) : null}
              {onBuild ? (
                <div className="flex items-center font-sans">
                  <button
                    type="button"
                    title="Build this plan"
                    disabled={buildDisabled}
                    className={`flex h-6 shrink-0 items-center gap-1 bg-content px-2 font-sans text-[11px] text-background-base hover:bg-content/90 disabled:cursor-not-allowed disabled:opacity-40 ${
                      harness ? "rounded-l-md" : "rounded-md"
                    }`}
                    onClick={() => onBuild()}
                  >
                    <Play className="size-3" strokeWidth={1.75} />
                    {buildLabel}
                  </button>
                  {harness ? (
                    <BuildTargetButton
                      from={harness}
                      model={model}
                      disabled={buildDisabled}
                      onPick={(targetHarness, targetModel) =>
                        onBuild({
                          harness: targetHarness,
                          model: targetModel,
                        })
                      }
                    />
                  ) : null}
                </div>
              ) : null}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
