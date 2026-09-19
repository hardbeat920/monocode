import { openUrl } from "@tauri-apps/plugin-opener";
import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type MouseEvent,
} from "react";
import {
  CircleDot,
  CircleX,
  ExternalLink,
  GitCompare,
  GitMerge,
  GitPullRequest,
  GitPullRequestClosed,
  GitPullRequestDraft,
  type IconComponent,
} from "../chrome/icons";
import { Popover } from "../chrome/Popover";
import {
  formatRelativeTime,
  githubWorkItem,
  githubWorkItemDetails,
  peekGithubWorkItem,
  peekGithubWorkItemDetails,
  type GithubWorkItem,
  type GithubWorkItemDetails,
} from "../lib/githubTasks";
import {
  fetchLinkPreviewMetadata,
  type GithubWorkItemLink,
  type LinkPreviewMetadata,
  type UserLink,
} from "../lib/linkPreview";

export function UserLinkPreview({
  link,
  cwd,
  compact = false,
}: {
  link: UserLink;
  cwd?: string;
  compact?: boolean;
}) {
  if (link.githubWorkItem) {
    const workItem = link.githubWorkItem;
    return (
      <GithubWorkItemPreview
        key={`${workItem.repo}:${workItem.kind}:${workItem.number}`}
        link={link}
        workItem={workItem}
        cwd={cwd}
        compact={compact}
      />
    );
  }
  return <GenericLinkPreview link={link} />;
}

function GenericLinkPreview({ link }: { link: UserLink }) {
  const [metadata, setMetadata] = useState<LinkPreviewMetadata | null>(null);
  const [faviconFailed, setFaviconFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setMetadata(null);
    setFaviconFailed(false);
    void fetchLinkPreviewMetadata(link.url)
      .then((value) => {
        if (!cancelled) setMetadata(value);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [link.url]);

  const title = metadata?.title?.trim() || link.host;
  const favicon = faviconFailed ? null : metadata?.faviconDataUrl;

  return (
    <a
      href={link.url}
      data-user-link-preview
      title={link.url}
      aria-label={`Open ${title}`}
      className="user-link-preview group mx-0.5 text-sky-400/90 hover:text-sky-300 hover:underline"
      onClick={(event) => openExternalLink(event, link.url)}
    >
      <span className="mr-1 inline-flex size-4 items-center justify-center overflow-hidden rounded bg-background-base/50 align-[-0.125em] text-[9px] font-semibold uppercase text-content/55">
        {favicon ? (
          <img
            src={favicon}
            alt=""
            draggable={false}
            className="size-3.5 object-contain"
            onError={() => setFaviconFailed(true)}
          />
        ) : (
          link.host.charAt(0)
        )}
      </span>
      <span className="user-link-preview-title font-medium group-hover:underline">
        {title}
      </span>
    </a>
  );
}

const HOVER_OPEN_DELAY_MS = 220;
const HOVER_CLOSE_DELAY_MS = 100;

function GithubWorkItemPreview({
  link,
  workItem,
  cwd,
  compact,
}: {
  link: UserLink;
  workItem: GithubWorkItemLink;
  cwd?: string;
  compact: boolean;
}) {
  const anchor = useRef<HTMLAnchorElement>(null);
  const openTimer = useRef<number | null>(null);
  const closeTimer = useRef<number | null>(null);
  const mounted = useRef(true);
  const requestStarted = useRef(false);
  const tooltipId = useId();
  const [open, setOpen] = useState(false);
  const [item, setItem] = useState<GithubWorkItem | null>(() =>
    peekGithubWorkItem(workItem.repo, workItem.kind, workItem.number),
  );
  const [details, setDetails] = useState<GithubWorkItemDetails | null>(() =>
    peekGithubWorkItemDetails(workItem.repo, workItem.kind, workItem.number),
  );
  const [loadState, setLoadState] = useState<
    "idle" | "loading" | "ready" | "unavailable"
  >(item || details ? "ready" : "idle");

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (openTimer.current != null) window.clearTimeout(openTimer.current);
      if (closeTimer.current != null) window.clearTimeout(closeTimer.current);
    };
  }, []);

  const load = useCallback(() => {
    if (requestStarted.current) return;
    requestStarted.current = true;
    const cachedItem =
      item ?? peekGithubWorkItem(workItem.repo, workItem.kind, workItem.number);
    const cachedDetails =
      details ??
      peekGithubWorkItemDetails(workItem.repo, workItem.kind, workItem.number);
    if (!cachedItem || !cachedDetails) setLoadState("loading");

    void Promise.allSettled([
      cachedItem
        ? Promise.resolve(cachedItem)
        : githubWorkItem(
            cwd || ".",
            workItem.repo,
            workItem.kind,
            workItem.number,
          ),
      cachedDetails
        ? Promise.resolve(cachedDetails)
        : githubWorkItemDetails(
            cwd || ".",
            workItem.repo,
            workItem.kind,
            workItem.number,
          ),
    ]).then(([itemResult, detailsResult]) => {
      if (!mounted.current) return;
      if (itemResult.status === "fulfilled") setItem(itemResult.value);
      if (detailsResult.status === "fulfilled") {
        setDetails(detailsResult.value);
      }
      setLoadState(
        itemResult.status === "fulfilled" ||
          detailsResult.status === "fulfilled"
          ? "ready"
          : "unavailable",
      );
    });
  }, [cwd, details, item, workItem]);

  const clearOpenTimer = () => {
    if (openTimer.current == null) return;
    window.clearTimeout(openTimer.current);
    openTimer.current = null;
  };
  const clearCloseTimer = () => {
    if (closeTimer.current == null) return;
    window.clearTimeout(closeTimer.current);
    closeTimer.current = null;
  };
  const showNow = () => {
    clearOpenTimer();
    clearCloseTimer();
    setOpen(true);
    load();
  };
  const showAfterDelay = () => {
    clearCloseTimer();
    if (open || openTimer.current != null) return;
    openTimer.current = window.setTimeout(() => {
      openTimer.current = null;
      setOpen(true);
      load();
    }, HOVER_OPEN_DELAY_MS);
  };
  const hideAfterDelay = () => {
    clearOpenTimer();
    clearCloseTimer();
    closeTimer.current = window.setTimeout(() => {
      closeTimer.current = null;
      setOpen(false);
    }, HOVER_CLOSE_DELAY_MS);
  };
  const hideNow = () => {
    clearOpenTimer();
    clearCloseTimer();
    setOpen(false);
  };

  const kindLabel = workItem.kind === "pr" ? "Pull request" : "Issue";
  const compactKind = workItem.kind === "pr" ? "PR" : "Issue";
  const itemTitle = item?.title.trim();

  return (
    <>
      <a
        ref={anchor}
        href={link.url}
        data-user-link-preview
        data-github-work-item-chip={workItem.kind}
        data-compact={compact || undefined}
        aria-label={`Open ${kindLabel.toLowerCase()} #${workItem.number} in ${workItem.repo}${itemTitle ? `: ${itemTitle}` : ""}`}
        aria-describedby={open ? tooltipId : undefined}
        className={`group mx-px inline-flex max-w-full items-center gap-1 rounded-md border-0 bg-transparent p-0 text-content/70 no-underline shadow-none outline-none transition-[color,transform] duration-[140ms] ease-[var(--motion-ease-out)] hover:text-content focus-visible:ring-2 focus-visible:ring-accent/60 active:scale-[0.98] motion-reduce:transition-none pb-px ${
          compact
            ? "align-middle text-xs leading-4"
            : "align-[-0.25em] text-[11px] leading-4"
        }`}
        onMouseEnter={showAfterDelay}
        onMouseLeave={hideAfterDelay}
        onFocus={showNow}
        onBlur={hideNow}
        onClick={(event) => {
          hideNow();
          openExternalLink(event, link.url);
        }}
      >
        <span
          className={` inline-flex shrink-0 items-center font-medium ${
            compact
              ? "h-[18px] gap-0.5 rounded-[6px] px-1.5"
              : "h-5 gap-1 rounded-[5px] px-1.5"
          } ${
            workItem.kind === "pr"
              ? "bg-violet-400/10 text-violet-400/90"
              : "bg-emerald-400/10 text-emerald-400/90"
          }`}
        >
          {workItem.kind === "pr" ? (
            <GitPullRequest
              className={compact ? "size-3" : "size-3.5"}
              aria-hidden="true"
            />
          ) : (
            <CircleDot
              className={compact ? "size-3" : "size-3.5"}
              aria-hidden="true"
            />
          )}
          {compactKind} #{workItem.number}
        </span>
      </a>
      {open ? (
        <Popover
          anchor={anchor}
          side="top"
          align="start"
          gap={6}
          width={360}
          constrainHeight={false}
          onDismiss={hideNow}
          id={tooltipId}
          role="tooltip"
          data-github-work-item-popover
          className="p-3.5 font-sans text-content"
          onMouseEnter={clearCloseTimer}
          onMouseLeave={hideAfterDelay}
        >
          <GithubWorkItemCard
            parsed={workItem}
            item={item}
            details={details}
            loadState={loadState}
          />
        </Popover>
      ) : null}
    </>
  );
}

function GithubWorkItemCard({
  parsed,
  item,
  details,
  loadState,
}: {
  parsed: GithubWorkItemLink;
  item: GithubWorkItem | null;
  details: GithubWorkItemDetails | null;
  loadState: "idle" | "loading" | "ready" | "unavailable";
}) {
  const status = workItemStatus(parsed.kind, item);
  const summary = plainTextSummary(details?.body ?? "");
  const updated = item?.updatedAt ? formatRelativeTime(item.updatedAt) : "";
  const assignees = item?.assignees ?? [];
  const labels = item?.labels ?? [];
  const hasBranches =
    parsed.kind === "pr" && details?.baseRefName && details?.headRefName;

  return (
    <div className="min-w-0 text-left">
      <div className="flex min-w-0 items-center gap-2 text-[11px]">
        <status.Icon
          className={`size-4 shrink-0 ${status.className}`}
          aria-hidden="true"
        />
        <span className="min-w-0 truncate font-medium text-content/65">
          {parsed.repo}
        </span>
        <span className="shrink-0 text-content/35">·</span>
        <span className="shrink-0 font-mono tabular-nums text-content/50">
          #{parsed.number}
        </span>
        <span
          className={`ml-auto shrink-0 rounded-full bg-content/[0.07] px-2 py-0.5 font-medium ${status.className}`}
        >
          {status.label}
        </span>
      </div>

      {item ? (
        <>
          <h3 className="mt-2 line-clamp-2 text-[13px] font-semibold leading-[1.35] text-content">
            {item.title}
          </h3>
          {summary ? (
            <p className="mt-1.5 line-clamp-3 text-[11px] leading-[1.45] text-content/55">
              {summary}
            </p>
          ) : null}
        </>
      ) : loadState === "idle" || loadState === "loading" ? (
        <GithubWorkItemCardSkeleton />
      ) : (
        <div className="mt-2">
          <h3 className="text-[13px] font-semibold text-content">
            {parsed.kind === "pr" ? "Pull request" : "Issue"} #{parsed.number}
          </h3>
          {summary ? (
            <p className="mt-1.5 line-clamp-3 text-[11px] leading-[1.45] text-content/55">
              {summary}
            </p>
          ) : (
            <p className="mt-1 text-[11px] leading-relaxed text-content/50">
              Details aren&apos;t available here, but the link can still be
              opened on GitHub.
            </p>
          )}
        </div>
      )}

      {details?.author || updated ? (
        <div className="mt-2.5 flex min-w-0 items-center gap-1.5 text-[10px] text-content/45">
          {details?.author ? (
            <GithubPerson
              name={details.author}
              avatarUrl={details.authorAvatarUrl}
            />
          ) : null}
          {details?.author && updated ? <span aria-hidden>·</span> : null}
          {updated ? <span className="shrink-0">Updated {updated}</span> : null}
        </div>
      ) : null}

      {hasBranches ? (
        <div className="mt-2 flex min-w-0 items-center gap-1.5 rounded-md bg-content/[0.045] px-2 py-1.5 text-[10px] text-content/50">
          <GitCompare className="size-3.5 shrink-0" aria-hidden="true" />
          <span className="min-w-0 truncate font-mono">
            {details.baseRefName} ← {details.headRefName}
          </span>
        </div>
      ) : null}

      {labels.length > 0 || assignees.length > 0 ? (
        <div className="mt-2.5 flex min-w-0 items-center gap-1.5">
          <div className="flex min-w-0 flex-1 flex-wrap gap-1">
            {labels.slice(0, 3).map((label) => (
              <span
                key={label.name}
                className="inline-flex max-w-28 items-center gap-1 rounded-full border border-content/10 bg-content/[0.035] px-1.5 py-0.5 text-[9px] text-content/55"
              >
                <span
                  aria-hidden="true"
                  className="size-1.5 shrink-0 rounded-full"
                  style={{ backgroundColor: labelColor(label.color) }}
                />
                <span className="truncate">{label.name}</span>
              </span>
            ))}
            {labels.length > 3 ? (
              <span className="px-1 text-[9px] text-content/35">
                +{labels.length - 3}
              </span>
            ) : null}
          </div>
          {assignees.length > 0 ? (
            <div
              className="flex shrink-0 -space-x-1"
              aria-label={`Assigned to ${assignees.map((person) => person.login).join(", ")}`}
            >
              {assignees.slice(0, 3).map((person) => (
                <GithubAvatar
                  key={person.login}
                  name={person.login}
                  avatarUrl={person.avatarUrl}
                  size={18}
                />
              ))}
            </div>
          ) : null}
        </div>
      ) : null}

      <div className="mt-3 flex items-center gap-1.5 border-t border-content/[0.07] pt-2 text-[10px] text-content/35">
        <ExternalLink className="size-3" aria-hidden="true" />
        Click the chip to open on GitHub
      </div>
    </div>
  );
}

function GithubWorkItemCardSkeleton() {
  return (
    <div aria-label="Loading GitHub details" className="mt-2.5 space-y-2">
      <div className="h-3 w-4/5 rounded bg-content/10 motion-safe:animate-pulse" />
      <div className="h-2 w-full rounded bg-content/[0.07] motion-safe:animate-pulse" />
      <div className="h-2 w-2/3 rounded bg-content/[0.07] motion-safe:animate-pulse" />
    </div>
  );
}

function GithubPerson({
  name,
  avatarUrl,
}: {
  name: string;
  avatarUrl?: string;
}) {
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5">
      <GithubAvatar name={name} avatarUrl={avatarUrl} size={16} />
      <span className="min-w-0 truncate font-medium text-content/55">
        {name}
      </span>
    </span>
  );
}

function GithubAvatar({
  name,
  avatarUrl,
  size,
}: {
  name: string;
  avatarUrl?: string;
  size: number;
}) {
  const [failed, setFailed] = useState(!avatarUrl);
  if (!avatarUrl || failed) {
    return (
      <span
        aria-hidden="true"
        className="grid shrink-0 place-items-center rounded-full border border-background-base bg-content/10 font-medium text-content/50"
        style={{
          width: size,
          height: size,
          fontSize: Math.max(8, size * 0.45),
        }}
      >
        {name.trim().charAt(0).toUpperCase() || "?"}
      </span>
    );
  }
  return (
    <img
      src={avatarUrl}
      alt=""
      width={size}
      height={size}
      referrerPolicy="no-referrer"
      draggable={false}
      onError={() => setFailed(true)}
      className="shrink-0 rounded-full border border-background-base bg-content/10 object-cover"
      style={{ width: size, height: size }}
    />
  );
}

function workItemStatus(
  kind: GithubWorkItemLink["kind"],
  item: GithubWorkItem | null,
): { Icon: IconComponent; label: string; className: string } {
  if (!item) {
    return {
      Icon: kind === "pr" ? GitPullRequest : CircleDot,
      label: kind === "pr" ? "Pull request" : "Issue",
      className: "text-content/50",
    };
  }
  if (item?.draft) {
    return {
      Icon: GitPullRequestDraft,
      label: "Draft",
      className: "text-content/50",
    };
  }
  if (item?.state === "merged") {
    return {
      Icon: GitMerge,
      label: "Merged",
      className: "text-violet-400/90",
    };
  }
  if (item?.state === "closed") {
    return {
      Icon: kind === "pr" ? GitPullRequestClosed : CircleX,
      label: "Closed",
      className: "text-rose-400/90",
    };
  }
  return {
    Icon: kind === "pr" ? GitPullRequest : CircleDot,
    label: "Open",
    className: "text-emerald-400/90",
  };
}

function plainTextSummary(value: string): string {
  return value
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]+>/g, " ")
    .replace(/[`*_>#~|-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function labelColor(value: string): string {
  return /^[\da-f]{6}$/i.test(value.trim())
    ? `#${value.trim()}`
    : "currentColor";
}

function openExternalLink(event: MouseEvent<HTMLAnchorElement>, url: string) {
  event.preventDefault();
  event.stopPropagation();
  void openUrl(url).catch((error) => {
    console.error("Failed to open web link:", error);
  });
}
