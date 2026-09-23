import { CheckRepairForm, type CheckRepair } from "./CheckRepairForm";
import {
  CheckRepairProgress,
  CheckRepairStatus,
  findCheckRepair,
  useCheckRepairs,
  type RepairGroup,
} from "./CheckRepairProgress";
import { CheckEvidence } from "./CheckEvidence";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useId, useRef, useState } from "react";
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
  Sparkles,
  type IconComponent,
} from "../../../shared/ui/icons";
import type { GithubPrChecksView } from "../hooks/useGithubPrChecks";
import {
  CHECK_STATES,
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
} from "../model/githubPrChecks";

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
      return {
        Icon: LoaderCircle,
        className: "animate-spin text-amber-400/70",
      };
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

function selectedChecksStillFailed(
  selected: readonly GithubPrCheck[],
  current: readonly GithubPrCheck[],
): boolean {
  const failures = new Map<string, number>();
  const identity = (check: GithubPrCheck) =>
    JSON.stringify([check.workflow, check.name, check.url]);
  for (const check of current) {
    if (check.state !== "fail") continue;
    const key = identity(check);
    failures.set(key, (failures.get(key) ?? 0) + 1);
  }
  return selected.every((check) => {
    const key = identity(check);
    const count = failures.get(key) ?? 0;
    if (count === 0) return false;
    failures.set(key, count - 1);
    return true;
  });
}

function PrCheckRow({
  check,
  cwd,
  repo,
  headOid,
  autoExpand,
  refreshToken,
  onFix,
  fixDisabled,
  fixAnchor,
  repairItem,
  wideStatus,
  revealToken,
}: {
  check: GithubPrCheck;
  cwd: string;
  repo: string;
  headOid: string;
  autoExpand: boolean;
  refreshToken: unknown;
  onFix?: (anchor: HTMLButtonElement) => void;
  fixDisabled?: boolean;
  fixAnchor?: HTMLButtonElement;
  repairItem?: RepairGroup["items"][number];
  wideStatus: boolean;
  revealToken?: number;
}) {
  const rowRef = useRef<HTMLLIElement>(null);
  const fixRef = useRef<HTMLButtonElement>(null);
  const fixOpen = Boolean(fixAnchor && fixAnchor === fixRef.current);
  const jobId = githubActionsJobId(check.url, repo);
  const expandable = Boolean(cwd && jobId);
  const [expanded, setExpanded] = useState(autoExpand && expandable);
  useEffect(() => {
    if (!revealToken) return;
    if (expandable) setExpanded(true);
    const row = rowRef.current;
    if (!row) return;
    row.focus({ preventScroll: true });
    const scroller = row.closest<HTMLElement>("[data-inbox-detail-scroll]");
    if (!scroller) return;
    const bounds = scroller.getBoundingClientRect();
    const scaleY = scroller.offsetHeight
      ? bounds.height / scroller.offsetHeight
      : 1;
    // scrollIntoView also scrolls hidden ancestors, including the desktop shell.
    // Move only the PR viewport, accounting for browser preview zoom.
    scroller.scrollTop +=
      (row.getBoundingClientRect().top - bounds.top) / (scaleY || 1) -
      scroller.clientTop;
  }, [revealToken, expandable]);
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
  const failedStep = details?.steps
    .filter((step) => step.state === "fail")
    .map((step) => step.name)
    .join(", ");
  const failureMessage = details?.annotations
    .find((annotation) => annotation.level === "failure")
    ?.message.split(/\r?\n/)
    .find((line) => line.trim());
  const subtitle =
    failureMessage || (failedStep ? `Failed at ${failedStep}` : status);
  const title = `${check.name} · ${status}${duration ? `, took ${duration}` : ""}${workflow ? `, ${workflow}` : ""}`;
  const url = check.url;
  const linked = isHttpUrl(url);
  const body = (
    <>
      <span className="grid h-7 w-5 shrink-0 place-items-center">
        <mark.Icon className={`size-4 ${mark.className}`} strokeWidth={1.75} />
      </span>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="flex min-w-0 items-baseline gap-2.5">
          <span
            data-check-name
            title={check.name}
            className="min-w-0 truncate text-[14px] font-medium leading-snug tracking-[-0.15px] text-content @max-[420px]/checks:text-[12px]"
          >
            {check.name}
          </span>
          {workflow ? (
            <span className="min-w-0 shrink-[2] truncate text-[11px] text-content/40 @max-[560px]/checks:hidden">
              {workflow}
            </span>
          ) : null}
        </span>
        {(!repairItem || expanded) && (failureMessage || failedStep) ? (
          <span
            title={subtitle}
            className="mt-0.5 min-w-0 truncate text-[12px] leading-relaxed text-content/55 @max-[420px]/checks:text-[11px]"
          >
            {subtitle}
          </span>
        ) : null}
        <span className="sr-only">
          {meta}
          {failureMessage && failedStep ? `; Failed at ${failedStep}` : ""}
        </span>
      </span>
    </>
  );
  const className =
    "flex w-full min-w-0 flex-1 items-center gap-2.5 rounded-md text-left";
  return (
    <li
      ref={rowRef}
      tabIndex={-1}
      className={`min-w-0 rounded-xl outline-none ${expanded ? "bg-content/[0.02]" : ""}`}
    >
      <div className="group/check flex min-w-0 items-center gap-2 rounded-xl px-2 py-2 hover:bg-content/[0.02] @max-[420px]/checks:gap-1">
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
        <span className={`shrink-0 text-left ${wideStatus ? "w-24" : "w-14"}`}>
          {repairItem ? (
            <CheckRepairStatus item={repairItem} />
          ) : (
            <span
              className={`text-[11px] ${mark.className.replace("animate-spin", "")}`}
            >
              {status}
            </span>
          )}
        </span>
        <span className="mr-1 w-11 shrink-0 whitespace-nowrap text-right text-[10px] tabular-nums text-content/40 @max-[480px]/checks:hidden">
          {duration}
        </span>
        {onFix ? (
          <button
            type="button"
            ref={fixRef}
            onClick={(event) => onFix(event.currentTarget)}
            disabled={fixDisabled}
            aria-haspopup="dialog"
            aria-expanded={fixOpen}
            aria-label={`Fix ${check.name} with AI`}
            title="Fix with AI"
            className="grid size-7 shrink-0 place-items-center rounded-lg bg-content/[0.03] text-content/65 hover:bg-selection hover:text-content focus-visible:outline focus-visible:outline-1 focus-visible:outline-content/50"
          >
            <Sparkles className="size-3.5" strokeWidth={1.75} />
          </button>
        ) : (
          <span className="size-7 shrink-0" aria-hidden="true" />
        )}
        {expandable ? (
          <button
            type="button"
            aria-label={`${expanded ? "Collapse" : "Expand"} ${check.name} details`}
            aria-expanded={expanded}
            aria-controls={detailsId}
            onClick={() => setExpanded(!expanded)}
            className="grid size-7 shrink-0 place-items-center rounded-lg text-content/40 hover:bg-content/5 hover:text-content focus-visible:outline focus-visible:outline-1 focus-visible:outline-content/50"
          >
            <ChevronRight
              className={`size-3 transition-transform motion-reduce:transition-none ${expanded ? "rotate-90" : ""}`}
              strokeWidth={1.75}
            />
          </button>
        ) : (
          <span className="size-7 shrink-0" aria-hidden="true" />
        )}
        {linked ? (
          <button
            type="button"
            title="View full log on GitHub"
            aria-label={`View ${check.name} on GitHub`}
            onClick={() => void openUrl(url)}
            className={`${REFRESH_BUTTON} opacity-60 group-hover/check:opacity-100 focus-visible:opacity-100`}
          >
            <ExternalLink className="size-3" strokeWidth={1.75} />
          </button>
        ) : (
          <span className="size-6 shrink-0" aria-hidden="true" />
        )}
      </div>
      {expanded ? (
        <div
          id={detailsId}
          className="min-w-0 space-y-3 py-3 pl-10 pr-3 text-[12px] @max-[420px]/checks:pl-3"
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
              {details.annotations.length ? (
                <CheckEvidence
                  annotations={details.annotations}
                  cwd={cwd}
                  repo={repo}
                  headOid={headOid}
                />
              ) : null}
              {details.steps.length ? (
                <details className="group/steps">
                  <summary className="flex cursor-pointer list-none items-center gap-1 text-[11px] text-content/50 hover:text-content [&::-webkit-details-marker]:hidden">
                    <ChevronRight className="size-3 transition-transform group-open/steps:rotate-90 motion-reduce:transition-none" />
                    View run steps
                    <span className="ml-auto pl-2 text-right text-[10px] text-content/35 @max-[420px]/checks:hidden">
                      {describeCheckCounts(countChecks(details.steps))}
                    </span>
                  </summary>
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
                </details>
              ) : (
                <p className="text-content/50">
                  No steps reported for this job.
                </p>
              )}
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
  repair,
}: {
  view: GithubPrChecksView;
  onRefresh: () => void;
  cwd?: string;
  repo?: string;
  repair?: CheckRepair;
}) {
  const { checks, loading, refreshing, error, stale } = view;
  const repairGroups = useCheckRepairs(cwd, repo, repair?.number, view);
  const revealScope = JSON.stringify([
    cwd,
    repo,
    repair?.number,
    checks?.headOid,
  ]);
  const [revealed, setRevealed] = useState<{
    name: string;
    workflow: string;
    scope: string;
    token: number;
  } | null>(null);
  const [filter, setFilter] = useState<"attention" | "all">("attention");
  const [showOthers, setShowOthers] = useState(false);
  const allFixRef = useRef<HTMLButtonElement>(null);
  const [selection, setSelection] = useState<{
    checks: GithubPrCheck[];
    anchor: HTMLButtonElement;
    scope: string;
  } | null>(null);
  const selectionValid = Boolean(
    selection &&
      checks &&
      selection.scope === revealScope &&
      selectedChecksStillFailed(selection.checks, checks.checks),
  );
  useEffect(() => {
    if (selection && !selectionValid) setSelection(null);
  }, [selection, selectionValid]);
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
  const counts = countChecks(rows);
  const attention =
    counts.fail + counts.pending + counts.cancel + counts.unknown;
  const activeFilter = attention ? filter : "all";
  const groups = CHECK_STATES.map((state) => ({
    state,
    rows: rows.filter((row) => row.state === state),
    hidden:
      activeFilter === "attention" &&
      !showOthers &&
      (state === "pass" || state === "skipping"),
  }));
  const headline = counts.fail
    ? `${counts.fail} ${counts.fail === 1 ? "check needs" : "checks need"} a fix`
    : counts.pending
      ? `${counts.pending} ${counts.pending === 1 ? "check is" : "checks are"} running`
      : attention
        ? `${attention} ${attention === 1 ? "check needs" : "checks need"} attention`
        : counts.pass
          ? "Checks passed"
          : "No checks ran";
  const summary = describeCheckCounts({ ...counts, fail: 0 });
  return (
    <section
      data-inbox-pr-checks
      aria-label="Pull request checks"
      className="@container/checks flex min-w-0 flex-col gap-2"
    >
      <div className="mb-3 flex min-w-0 flex-wrap items-start justify-between gap-3 px-2">
        <div className="min-w-0">
          {rows.length ? (
            <>
              <h2 className="text-[18px] font-medium leading-snug tracking-[-0.35px] @max-[420px]/checks:text-[16px]">
                {headline}
              </h2>
              {summary ? (
                <p className="mt-1 text-[12px] text-content/55">
                  {summary.charAt(0).toUpperCase() + summary.slice(1)}.
                </p>
              ) : null}
              <span className="sr-only">{describeCheckCounts(counts)}</span>
            </>
          ) : null}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {repair && rows.some((row) => row.state === "fail") ? (
            <button
              ref={allFixRef}
              type="button"
              disabled={refreshing || stale || Boolean(error)}
              onClick={(event) =>
                setSelection({
                  checks: rows.filter((row) => row.state === "fail"),
                  anchor: event.currentTarget,
                  scope: revealScope,
                })
              }
              aria-haspopup="dialog"
              aria-label="Fix all failed"
              aria-expanded={Boolean(
                selection && selection.anchor === allFixRef.current,
              )}
              className="primary-action inline-flex h-8 shrink-0 items-center gap-1.5 rounded-lg px-2.5 text-[12px] font-medium focus-visible:outline focus-visible:outline-1 focus-visible:outline-content/50"
            >
              <Sparkles className="size-3.5" strokeWidth={1.75} />
              Fix all failed
              <span
                aria-hidden="true"
                className="ml-1 border-l border-current/20 pl-2 text-[10px] opacity-55"
              >
                {counts.fail}
              </span>
            </button>
          ) : null}
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
      </div>
      {repair ? (
        <CheckRepairProgress
          cwd={cwd}
          repo={repo}
          repair={repair}
          view={view}
          onShowCheck={(check) => {
            setFilter("all");
            setSelection(null);
            setRevealed((previous) => ({
              name: check.name,
              workflow: check.workflow,
              scope: revealScope,
              token: (previous?.token ?? 0) + 1,
            }));
          }}
        />
      ) : null}
      {stale && error ? (
        <p role="status" className="px-2 text-[12px] text-content/55">
          Saved results may be out of date.
        </p>
      ) : null}
      {selection && selectionValid && repair ? (
        <CheckRepairForm
          key={`${selection.scope}:${JSON.stringify(selection.checks)}`}
          anchor={selection.anchor}
          checks={selection.checks}
          headOid={checks?.headOid ?? ""}
          cwd={cwd}
          repo={repo}
          repair={repair}
          blocked={refreshing || stale || Boolean(error)}
          onClose={() => setSelection(null)}
        />
      ) : null}
      {rows.length > 0 ? (
        <div className="flex items-center justify-between gap-3 py-2">
          <div
            className="inline-flex gap-0.5 rounded-lg border border-stroke bg-content/[0.02] p-0.5"
            aria-label="Filter checks"
          >
            {(
              [
                ["attention", "Needs attention", attention],
                ["all", "All checks", rows.length],
              ] as const
            ).map(([value, label, count]) => (
              <button
                key={value}
                type="button"
                aria-label={`${label}: ${count}`}
                aria-pressed={activeFilter === value}
                disabled={value === "attention" && !attention}
                onClick={() => {
                  setFilter(value);
                  setShowOthers(false);
                  setSelection(null);
                }}
                className={`inline-flex items-center gap-2 rounded-md px-2.5 py-1 text-[12px] disabled:opacity-40 ${activeFilter === value ? "bg-selection text-content shadow-sm" : "text-content/50 hover:text-content"}`}
              >
                {label}
                <span className="tabular-nums text-content/40">{count}</span>
              </button>
            ))}
          </div>
          <span className="text-[10px] text-content/40 @max-[420px]/checks:hidden">
            {counts.fail ? "Failures first" : ""}
          </span>
        </div>
      ) : null}
      {rows.length === 0 ? (
        <p className="text-[13px] text-content/45">No checks reported</p>
      ) : (
        <>
          {groups.map((group) =>
            group.rows.length ? (
              <div
                key={group.state}
                hidden={group.hidden}
                className={group.hidden ? "hidden" : ""}
              >
                <h3 className="mb-1.5 mt-3 flex items-center gap-2 px-2 text-[12px] font-normal text-content/55">
                  {checkStateLabel(group.state)}
                  <span className="text-[10px] text-content/35">
                    {group.rows.length}
                  </span>
                </h3>
                <ul className="flex flex-col gap-0.5">
                  {group.rows.map((check, index) => (
                    <PrCheckRow
                      key={JSON.stringify([
                        cwd,
                        repo,
                        repair?.number,
                        checks?.headOid,
                        check.workflow,
                        check.name,
                        check.url,
                        group.rows
                          .slice(0, index)
                          .filter(
                            (row) =>
                              row.workflow === check.workflow &&
                              row.name === check.name &&
                              row.url === check.url,
                          ).length,
                      ])}
                      check={check}
                      wideStatus={repairGroups.length > 0}
                      revealToken={
                        revealed?.scope === revealScope &&
                        revealed.name === check.name &&
                        revealed.workflow === check.workflow
                          ? revealed.token
                          : undefined
                      }
                      repairItem={
                        checks
                          ? findCheckRepair(repairGroups, check, checks)
                          : undefined
                      }
                      onFix={
                        repair &&
                        check.state === "fail"
                          ? (anchor) =>
                              setSelection({
                                checks: [check],
                                anchor,
                                scope: revealScope,
                              })
                          : undefined
                      }
                      fixDisabled={refreshing || stale || Boolean(error)}
                      fixAnchor={selection?.anchor}
                      cwd={cwd}
                      repo={repo}
                      headOid={checks?.headOid ?? ""}
                      autoExpand={
                        repairGroups.length === 0 &&
                        check === rows.find((row) => row.state === "fail")
                      }
                      refreshToken={checks}
                    />
                  ))}
                </ul>
              </div>
            ) : null,
          )}
          {activeFilter === "attention" && counts.pass + counts.skipping > 0 ? (
            <button
              type="button"
              aria-expanded={showOthers}
              onClick={() => setShowOthers(!showOthers)}
              className="mt-3 flex items-center gap-2 border-t border-stroke px-2 pt-4 text-left text-[11px] text-content/50 hover:text-content"
            >
              <ChevronRight
                className={`size-3 ${showOthers ? "rotate-90" : ""}`}
              />
              {describeCheckCounts({
                ...countChecks([]),
                pass: counts.pass,
                skipping: counts.skipping,
              })}
            </button>
          ) : null}
        </>
      )}
    </section>
  );
}
