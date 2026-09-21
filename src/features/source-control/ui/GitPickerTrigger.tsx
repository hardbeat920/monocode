import type { ComponentPropsWithoutRef } from "react";
import { FolderTree, GitBranch } from "../../../shared/ui/icons";

type Props = Omit<
  ComponentPropsWithoutRef<"button">,
  "children" | "className"
> & {
  label: string;
  loading?: boolean;
  worktree?: boolean;
};

/** Keep working-copy and branch modes visually identical in the composer. */
export function GitPickerTrigger({
  label,
  loading = false,
  worktree = false,
  ...props
}: Props) {
  const Icon = worktree ? FolderTree : GitBranch;
  return (
    <button
      type="button"
      {...props}
      className="-ml-1.5 flex h-6 min-w-0 max-w-64 items-center gap-1.5 rounded-md px-1.5 text-[12px] text-content/55 hover:bg-content/8 hover:text-content aria-expanded:bg-content/8 aria-expanded:text-content disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-content/55 active:scale-[0.97]"
    >
      <Icon className="size-3.5 shrink-0" />
      <span className="relative truncate">
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
