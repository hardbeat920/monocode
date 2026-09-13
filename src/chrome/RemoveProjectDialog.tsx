import { useEffect, useState } from "react";
import { prettyCwd } from "../lib/paths";
import { projectSessionCount } from "../lib/projectData";
import { Modal } from "./Modal";

type Props = {
  name: string;
  path: string;
  onCancel: () => void;
  onConfirm: () => void;
};

/**
 * Delete drops the project from the rail and its saved chats. The folder on
 * disk is left alone; opening it again brings the project back empty.
 */
export function RemoveProjectDialog({
  name,
  path,
  onCancel,
  onConfirm,
}: Props) {
  const [sessions, setSessions] = useState<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    void projectSessionCount(path).then((count) => {
      if (!cancelled) setSessions(count);
    });
    return () => {
      cancelled = true;
    };
  }, [path]);

  return (
    <Modal
      title={`Delete “${name}”?`}
      description={prettyCwd(path)}
      size="sm"
      onClose={onCancel}
    >
      <div className="flex flex-col gap-4 p-4">
        <div className="flex flex-col gap-2">
          <p className="text-[13px] leading-relaxed text-content/60">
            This deletes all sessions for the project and removes it from the
            sidebar. The folder on disk stays intact; reopening it adds the
            project back empty.
          </p>
          {sessions != null && sessions > 0 ? (
            <p className="text-[12px] leading-snug text-content/45">
              {sessions === 1
                ? "1 saved session will be removed."
                : `${sessions} saved sessions will be removed.`}
            </p>
          ) : null}
        </div>

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="rounded-md px-3 py-1.5 text-[12px] text-content/70 hover:bg-content/8 hover:text-content"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className="rounded-md bg-red-500/20 px-3 py-1.5 text-[12px] font-medium text-red-300 hover:bg-red-500/30"
          >
            Delete
          </button>
        </div>
      </div>
    </Modal>
  );
}
