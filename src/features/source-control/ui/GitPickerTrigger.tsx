import { useContext } from "react";
import { NativePopupHost } from "../../../shared/ui/NativePopupHost";
import type { ComponentPropsWithoutRef } from "react";
import { FolderTree, GitBranch } from "../../../shared/ui/icons";

type Props = Omit<
  ComponentPropsWithoutRef<"button">,
  "children" | "className"
> & {
  label: string;
  loading?: boolean;
  worktree?: boolean;
  dimWhenDisabled?: boolean;
  /** Match the 11px metadata line in panel headers. */
  compact?: boolean;
};

/** Keep working-copy and branch modes visually identical in the composer. */
export function GitPickerTrigger({
  label,
  loading = false,
  worktree = false,
  dimWhenDisabled = true,
  compact = false,
  ...props
}: Props) {
  const host = useContext(NativePopupHost);
  if (host) return null;
  const Icon = worktree ? FolderTree : GitBranch;
  return (
    <button
      type="button"
      {...props}
      className={`-ml-1.5 flex min-w-0 max-w-64 items-center rounded-md px-1.5 hover:bg-content/8 hover:text-content aria-expanded:bg-content/8 aria-expanded:text-content active:scale-[0.97] ${compact ? "h-5 gap-1 text-[11px] text-content/50 disabled:hover:text-content/50" : "h-6 gap-1.5 text-[12px] text-content/55 disabled:hover:text-content/55"} disabled:hover:bg-transparent ${dimWhenDisabled ? "disabled:opacity-40" : ""}`}
    >
      <Icon
        className={`${compact ? "size-3" : "size-3.5"} shrink-0`}
        strokeWidth={compact ? 1.75 : undefined}
      />
      <span className="relative min-w-0 flex-1 truncate">
        {loading ? (
          <>
            {/* Reserve the same line box while the current branch loads. */}
            <span className="invisible">main</span>
            <span className="absolute inset-x-0 top-1/2 h-1.5 -translate-y-1/2 rounded-full bg-current opacity-50" />
          </>
        ) : (
          label
        )}
      </span>
      {worktree && (
        <span className="shrink-0 rounded bg-content/8 px-1 text-[10px] text-content/45">
          Worktree
        </span>
      )}
    </button>
  );
}
