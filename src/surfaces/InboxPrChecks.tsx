import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useId, useState } from "react";
import {
  AlertCircle,
  CheckCircle,
  ChevronRight,
  CircleDashed,
  CircleHelp,
  CircleX,
  ExternalLink,
  LoaderCircle,
  Minus,
  RefreshCw,
  type IconComponent,
} from "../chrome/icons";
import type { GithubPrChecksView } from "../hooks/useGithubPrChecks";
import {
  checkDuration,
  checkStateLabel,
  countChecks,
  describeCheckCounts,
  fetchGithubCheckDetails,
  githubActionsJobId,
  isHttpUrl,
  sortChecks,
  type GithubPrCheck,
  type GithubPrChecksOverall,
  type GithubPrCheckState,
  type GithubCheckDetails,
} from "../lib/githubPrChecks";

const TAB =
  "relative flex h-9 items-center gap-1.5 text-[12px] leading-none select-none";

function overallMark(overall: GithubPrChecksOverall): {
  Icon: IconComponent;
  className: string;
} {
  switch (overall.kind) {
    case "loading":
      return { Icon: LoaderCircle, className: "animate-spin text-content/45" };
    case "error":
      return { Icon: AlertCircle, className: "text-rose-400/90" };
    case "fail":
      return { Icon: CircleX, className: "text-rose-400/90" };
    case "pending":
      return { Icon: LoaderCircle, className: "animate-spin text-content/55" };
    case "pass":
      return { Icon: CheckCircle, className: "text-emerald-400/90" };
    case "neutral":
      return { Icon: CircleDashed, className: "text-content/45" };
  }
}

function checkMark(state: GithubPrCheckState): {
  Icon: IconComponent;
  className: string;
} {
  switch (state) {
    case "pass":
      return { Icon: CheckCircle, className: "text-emerald-400/90" };
    case "fail":
      return { Icon: CircleX, className: "text-rose-400/90" };
    case "pending":
      return { Icon: LoaderCircle, className: "animate-spin text-content/55" };
    case "cancel":
      return { Icon: Minus, className: "text-content/45" };
    case "unknown":
      return { Icon: CircleHelp, className: "text-content/45" };
    case "skipping":
      return { Icon: CircleDashed, className: "text-content/40" };
  }
}

/** The Checks tab: label plus an overall mark whose name spells out the counts. */
export function PrChecksTab({
  overall,
  selected,
  onSelect,
}: {
  overall: GithubPrChecksOverall;
  selected: boolean;
  onSelect: () => void;
}) {
  const mark = overallMark(overall);
  const label = `Checks: ${overall.description}`;
  const failClass =
    mark.className.split(" ").find((entry) => entry.startsWith("text-")) ?? "";
  return (
    <button
      type="button"
      role="tab"
      aria-selected={selected}
      aria-label={label}
      title={label}
      onClick={onSelect}
      className={`${TAB} ${selected ? "text-content" : "text-content/50 hover:text-content"}`}
    >
      <span className="leading-none">Checks</span>
      <mark.Icon
        className={`size-3.5 shrink-0 ${mark.className}`}
        strokeWidth={1.75}
      />
      {overall.kind === "fail" ? (
        <span className={`tabular-nums leading-none ${failClass}`}>
          {overall.failed}
        </span>
      ) : null}
      {selected ? (
        <span className="absolute inset-x-0 bottom-0 h-0.5 bg-content" />
      ) : null}
    </button>
  );
}

const REFRESH_BUTTON =
  "grid size-6 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content disabled:cursor-default disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-content/45";

function PrCheckRow({
  check,
  cwd,
  repo,
  autoExpand,
  refreshToken,
}: {
  check: GithubPrCheck;
  cwd: string;
  repo: string;
  autoExpand: boolean;
  refreshToken: unknown;
}) {
  const jobId = githubActionsJobId(check.url, repo);
  const expandable = Boolean(cwd && jobId);
  const [expanded, setExpanded] = useState(autoExpand && expandable);
  const [details, setDetails] = useState<GithubCheckDetails | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [retry, setRetry] = useState(0);
  const detailsId = useId();
  useEffect(() => {
    if (autoExpand && expandable) setExpanded(true);
  }, [autoExpand, expandable]);
  useEffect(() => {
    setDetails(null);
  }, [refreshToken]);
  useEffect(() => {
    if (!expanded || !jobId || !cwd) return;
    let active = true;
    setLoading(true);
    setError(null);
    setDetails(null);
    fetchGithubCheckDetails(cwd, repo, jobId)
      .then(
        (result) => {
          if (active) setDetails(result);
        },
        (reason: unknown) => {
          if (active)
            setError(reason instanceof Error ? reason.message : String(reason));
        },
      )
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [expanded, cwd, repo, jobId, refreshToken, retry]);
  const mark = checkMark(check.state);
  const status = checkStateLabel(check.state);
  const duration = checkDuration(check.startedAt, check.completedAt);
  const workflow = check.workflow.trim();
  const meta = [workflow, status, duration].filter((part) => part).join(" · ");
  const title = `${check.name} · ${status}${duration ? `, took ${duration}` : ""}${workflow ? `, ${workflow}` : ""}`;
  const url = check.url;
  const linked = isHttpUrl(url);
  const body = (
    <>
      <mark.Icon
        className={`size-4 shrink-0 ${mark.className}`}
        strokeWidth={1.75}
      />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="min-w-0 truncate text-[13px] leading-snug text-content">
          {check.name}
        </span>
        {meta ? (
          <span className="min-w-0 truncate text-[11px] leading-snug text-content/45">
            {meta}
          </span>
        ) : null}
        {details?.steps.some((step) => step.state === "fail") ? (
          <span className="truncate text-[11px] text-rose-400/90">
            Failed at{" "}
            {details.steps
              .filter((step) => step.state === "fail")
              .map((step) => step.name)
              .join(", ")}
          </span>
        ) : null}
      </span>
      {expandable ? (
        <ChevronRight
          className={`size-3 shrink-0 text-content/45 ${expanded ? "rotate-90" : ""}`}
          strokeWidth={1.75}
        />
      ) : linked ? (
        <ExternalLink
          className="size-3 shrink-0 text-content/35"
          strokeWidth={1.75}
        />
      ) : null}
    </>
  );
  const className =
    "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-content/5";
  return (
    <li>
      <div className="flex min-w-0 items-center gap-1">
        {expandable ? (
          <button
            type="button"
            aria-label={`${check.name} details`}
            aria-expanded={expanded}
            aria-controls={detailsId}
            onClick={() => setExpanded(!expanded)}
            className={`${className} min-w-0 flex-1 focus-visible:outline focus-visible:outline-1 focus-visible:outline-content/50`}
          >
            {body}
          </button>
        ) : linked ? (
          // The desktop WebView cannot rely on target=_blank, so rows open
          // through the same opener as every other external link.
          <button
            type="button"
            title={title}
            aria-label={title}
            onClick={() => void openUrl(url)}
            className={className}
          >
            {body}
          </button>
        ) : (
          <div title={title} className={className}>
            {body}
          </div>
        )}
        {expandable && linked ? (
          <button
            type="button"
            title="View full log on GitHub"
            aria-label={`View ${check.name} on GitHub`}
            onClick={() => void openUrl(url)}
            className={REFRESH_BUTTON}
          >
            <ExternalLink className="size-3" strokeWidth={1.75} />
          </button>
        ) : null}
      </div>
      {expanded ? (
        <div
          id={detailsId}
          className="mb-3 ml-3.5 mr-2 min-w-0 border-l border-stroke pl-4 py-2 text-[12px]"
        >
          {loading ? (
            <p
              role="status"
              className="flex items-center gap-2 px-2 py-1 text-content/50"
            >
              <LoaderCircle
                className="size-4 shrink-0 animate-spin"
                strokeWidth={1.75}
              />
              Loading steps…
            </p>
          ) : null}
          {error ? (
            <div role="alert" className="space-y-2 text-content/60">
              <p>Could not load job details.</p>
              <p className="break-words text-[11px]">{error}</p>
              <button
                type="button"
                onClick={() => setRetry((value) => value + 1)}
                className="rounded px-2 py-1 hover:bg-content/5"
              >
                Retry details
              </button>
            </div>
          ) : null}
          {details ? (
            <>
              {details.steps.length ? (
                <ol className="space-y-1" aria-label={`${check.name} steps`}>
                  {details.steps.map((step, index) => {
                    const stepMark = checkMark(step.state);
                    return (
                      <li
                        key={index}
                        className={`flex items-center gap-2 rounded px-2 py-1 ${step.state === "fail" ? "bg-rose-400/5" : ""}`}
                      >
                        <stepMark.Icon
                          className={`size-4 shrink-0 ${stepMark.className}`}
                          strokeWidth={1.75}
                        />
                        <span className="min-w-0 flex-1 break-words text-content/80">
                          {step.name}
                          <span className="sr-only">
                            : {checkStateLabel(step.state)}
                          </span>
                        </span>
                        <span className="shrink-0 tabular-nums text-content/45">
                          {checkDuration(step.startedAt, step.completedAt)}
                        </span>
                      </li>
                    );
                  })}
                </ol>
              ) : (
                <p className="text-content/50">
                  No steps reported for this job.
                </p>
              )}
              {details.annotations.map((annotation, index) => (
                <div key={index} className="mt-3 border-t border-stroke pt-3">
                  <p
                    className={`mb-1 break-all text-[11px] ${annotation.level === "failure" ? "text-rose-400/90" : "text-content/60"}`}
                  >
                    {annotation.path}
                    {annotation.line > 0 ? `:${annotation.line}` : ""}
                  </p>
                  <pre className="max-h-60 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] text-content/80">
                    {annotation.message}
                  </pre>
                </div>
              ))}
              {check.state === "fail" &&
              !details.annotations.length &&
              !details.notice ? (
                <p className="mt-3 text-content/50">
                  No error annotations reported. View the full log on GitHub.
                </p>
              ) : null}
              {details.notice ? (
                <p role="status" className="mt-3 text-content/50">
                  {details.notice}
                </p>
              ) : null}
            </>
          ) : null}
        </div>
      ) : null}
    </li>
  );
}

/**
 * Checks tab body: manual refresh, initial loading, the no-checks state and a
 * load error with retry. A failed refresh keeps the previous rows on screen
 * behind an explicit out-of-date notice.
 */
export function InboxPrChecks({
  view,
  onRefresh,
  cwd = "",
  repo = "",
}: {
  view: GithubPrChecksView;
  onRefresh: () => void;
  cwd?: string;
  repo?: string;
}) {
  const { checks, loading, refreshing, error, stale } = view;
  if (loading) {
    return (
      <div className="flex justify-center py-10 text-content/40">
        <LoaderCircle className="size-4 animate-spin" strokeWidth={1.75} />
      </div>
    );
  }
  if (!checks && error) {
    return (
      <div className="flex flex-col items-start gap-2" data-inbox-pr-checks>
        <p role="alert" className="text-[13px] text-content/50">
          {error}
        </p>
        <button
          type="button"
          title="Retry loading checks"
          aria-label="Retry loading checks"
          onClick={onRefresh}
          className="inline-flex h-7 items-center gap-1.5 rounded-md border border-content/15 px-3 text-[12px] text-content/80 hover:bg-content/5"
        >
          <RefreshCw className="size-3.5" strokeWidth={1.75} />
          Retry
        </button>
      </div>
    );
  }
  const rows = checks ? sortChecks(checks.checks) : [];
  return (
    <section
      data-inbox-pr-checks
      aria-label="Pull request checks"
      className="flex flex-col gap-2"
    >
      <div className="flex min-w-0 items-center gap-2">
        <p className="text-[11px] text-content/60">
          {describeCheckCounts(countChecks(rows))}
        </p>
        {refreshing ? (
          <LoaderCircle
            className="size-3 shrink-0 animate-spin text-content/40"
            strokeWidth={1.75}
          />
        ) : null}
        {stale && error ? (
          <p
            role="status"
            className="min-w-0 truncate text-[11px] text-content/50"
          >
            Saved results may be out of date.
          </p>
        ) : null}
        <span className="min-w-0 flex-1" />
        <button
          type="button"
          title="Refresh checks"
          aria-label="Refresh checks"
          disabled={refreshing}
          onClick={onRefresh}
          className={REFRESH_BUTTON}
        >
          {refreshing ? (
            <LoaderCircle
              className="size-3.5 animate-spin"
              strokeWidth={1.75}
            />
          ) : (
            <RefreshCw className="size-3.5" strokeWidth={1.75} />
          )}
        </button>
      </div>
      {rows.length === 0 ? (
        <p className="text-[13px] text-content/45">No checks reported</p>
      ) : (
        <ul className="flex flex-col gap-0.5">
          {rows.map((check, index) => (
            <PrCheckRow
              key={`${cwd}:${repo}:${checks?.headOid}:${check.url ?? `${index}:${check.workflow}:${check.name}`}`}
              check={check}
              cwd={cwd}
              repo={repo}
              autoExpand={
                check.state === "fail" &&
                rows.filter((row) => row.state === "fail").length === 1
              }
              refreshToken={checks}
            />
          ))}
        </ul>
      )}
    </section>
  );
}
