import { useEffect, useState } from "react";
import { prettyCwd } from "../../../shared/lib/paths";
import { Modal } from "../../../shared/ui/Modal";

export type SessionDeleteChoice = {
  confirmed: boolean;
  deleteWorktree: boolean;
};

export function DeleteSessionDialog({
  title,
  loadUnusedWorktree,
  cow = false,
  onClose,
}: {
  title: string;
  loadUnusedWorktree: () => Promise<string | undefined>;
  cow?: boolean;
  onClose: (choice: SessionDeleteChoice) => void;
}) {
  const [deleteWorktree, setDeleteWorktree] = useState(false);
  const [unusedWorktree, setUnusedWorktree] = useState<string>();
  const [checking, setChecking] = useState(true);
  useEffect(() => {
    let cancelled = false;
    setChecking(true);
    setUnusedWorktree(undefined);
    setDeleteWorktree(false);
    void loadUnusedWorktree()
      .catch(() => undefined)
      .then((path) => {
        if (cancelled) return;
        setUnusedWorktree(path);
        setChecking(false);
      });
    return () => {
      cancelled = true;
    };
  }, [loadUnusedWorktree]);
  return (
    <Modal
      title="Delete session?"
      size="sm"
      onClose={() => onClose({ confirmed: false, deleteWorktree: false })}
    >
      <div className="flex flex-col gap-4 p-4 text-[12px]">
        <p>“{title}” will be permanently deleted.</p>
        {checking ? (
          <p role="status" className="text-content/45">
            Checking workspace cleanup availability…
          </p>
        ) : unusedWorktree ? (
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              checked={deleteWorktree}
              onChange={(e) => setDeleteWorktree(e.target.checked)}
              className="mt-0.5 accent-accent"
            />
            <span>
              {cow
                ? "Also delete the unused copy-on-write workspace"
                : "Also delete the unused worktree"}
              <span className="mt-1 block break-all text-[11px] text-content/45">
                {prettyCwd(unusedWorktree)}
              </span>
              <span className="mt-1 block text-[11px] text-content/45">
                {cow
                  ? "The branch is kept. If files have uncommitted changes, the workspace stays."
                  : "The branch is kept. If files have uncommitted changes, the worktree stays."}
              </span>
            </span>
          </label>
        ) : null}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={() => onClose({ confirmed: false, deleteWorktree: false })}
            className="rounded-md px-3 py-1.5 hover:bg-content/8 active:scale-[0.97]"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={() => onClose({ confirmed: true, deleteWorktree })}
            className="rounded-md bg-red-500/20 px-3 py-1.5 font-medium text-red-400 hover:bg-red-500/30 active:scale-[0.97]"
          >
            Delete session
          </button>
        </div>
      </div>
    </Modal>
  );
}
