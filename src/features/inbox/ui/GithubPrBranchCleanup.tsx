import { useEffect, useRef, useState } from "react";
import { GitBranch, LoaderCircle } from "../../../shared/ui/icons";
import { Popover } from "../../../shared/ui/Popover";
import {
  githubPrBranch,
  githubPrDeleteBranch,
  type GithubPrBranch,
} from "../model/githubTasks";

export function GithubPrBranchCleanup({
  cwd,
  repo,
  number,
  revision = 0,
}: {
  cwd: string;
  repo: string;
  number: number;
  revision?: number;
}) {
  const [branch, setBranch] = useState<GithubPrBranch | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const [busy, setBusy] = useState(false);
  const lookupVersion = useRef(0);

  useEffect(() => {
    let cancelled = false;
    const version = ++lookupVersion.current;
    setLoading(true);
    setError(null);
    void githubPrBranch(cwd, repo, number)
      .then((next) => {
        if (!cancelled && version === lookupVersion.current) setBranch(next);
      })
      .catch((error: unknown) => {
        if (!cancelled && version === lookupVersion.current) {
          setError(error instanceof Error ? error.message : String(error));
          setBranch(null);
        }
      })
      .finally(() => {
        if (!cancelled && version === lookupVersion.current) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [cwd, repo, number, revision, retry]);

  const removeBranch = async () => {
    if (busy || !branch?.canDelete) return;
    setBusy(true);
    ++lookupVersion.current;
    setLoading(false);
    setError(null);
    try {
      // The backend checks the current PR and branch again before deleting.
      const deleted = await githubPrDeleteBranch(cwd, repo, number);
      ++lookupVersion.current;
      setLoading(false);
      setBranch(deleted);
      setAnchor(null);
    } catch (error: unknown) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  if (!loading && branch && !branch.exists && !branch.reason) {
    return (
      <span role="status" className="text-[11px] text-content/55">
        Branch deleted
      </span>
    );
  }

  return (
    <>
      <button
        type="button"
        disabled={loading || busy || !branch?.canDelete}
        title={
          branch?.reason || "Delete the merged pull request's source branch"
        }
        onClick={(event) => {
          setError(null);
          setAnchor(event.currentTarget);
        }}
        className="inline-flex h-7 items-center gap-1.5 rounded-md border border-content/15 px-2.5 text-[12px] text-content/75 hover:bg-content/8 hover:text-rose-400 disabled:cursor-default disabled:opacity-40"
      >
        {loading ? (
          <LoaderCircle className="size-3.5 animate-spin" strokeWidth={1.75} />
        ) : (
          <GitBranch className="size-3.5" strokeWidth={1.75} />
        )}
        Delete branch
      </button>
      {branch?.reason ? (
        <span className="text-[11px] text-content/55">{branch.reason}</span>
      ) : null}
      {error && !anchor ? (
        <span role="alert" className="text-[11px] text-rose-400">
          {error}{" "}
          <button
            type="button"
            className="underline"
            onClick={() => setRetry((retry) => retry + 1)}
          >
            Retry
          </button>
        </span>
      ) : null}
      {anchor && branch ? (
        <Popover
          anchor={anchor}
          width={320}
          onDismiss={() => {
            if (!busy) {
              setAnchor(null);
              setError(null);
            }
          }}
          className="p-3"
        >
          <div role="dialog" aria-label="Delete this branch?">
            <p className="text-[13px] font-medium">Delete this branch?</p>
            <p className="mt-1 break-words text-[12px] leading-snug text-content/65">
              Delete “{branch.name}” from {branch.repo} on GitHub? Your local
              branch and worktrees will be kept.
            </p>
            {error ? (
              <p
                role="alert"
                className="mt-2 break-words text-[11px] text-rose-400"
              >
                {error}
              </p>
            ) : null}
            <div className="mt-3 flex justify-end gap-2">
              <button
                type="button"
                disabled={busy}
                onClick={() => {
                  setAnchor(null);
                  setError(null);
                }}
                className="h-7 rounded-md px-3 text-[12px] text-content/65 hover:bg-content/8 disabled:opacity-40"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={busy || loading || !branch.canDelete}
                onClick={() => void removeBranch()}
                className="inline-flex h-7 items-center gap-1.5 rounded-md bg-rose-500/20 px-3 text-[12px] font-medium text-rose-700 hover:bg-rose-500/30 disabled:opacity-60 dark:text-rose-300"
              >
                {busy ? (
                  <LoaderCircle className="size-3.5 animate-spin" />
                ) : null}
                {busy ? "Deleting…" : "Delete branch"}
              </button>
            </div>
          </div>
        </Popover>
      ) : null}
    </>
  );
}
