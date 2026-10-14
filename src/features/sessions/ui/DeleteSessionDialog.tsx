import { useState } from "react";
import { prettyCwd } from "../../../shared/lib/paths";
import { Modal } from "../../../shared/ui/Modal";
import { useT } from "../../../shared/i18n/useT";

export type SessionDeleteChoice = {
  confirmed: boolean;
  deleteWorktree: boolean;
};

export function DeleteSessionDialog({
  title,
  unusedWorktree,
  onClose,
}: {
  title: string;
  unusedWorktree: string;
  onClose: (choice: SessionDeleteChoice) => void;
}) {
  const t = useT();
  const [deleteWorktree, setDeleteWorktree] = useState(false);
  return (
    <Modal
      title={t("sessions.delete.title")}
      size="sm"
      onClose={() => onClose({ confirmed: false, deleteWorktree: false })}
    >
      <div className="flex flex-col gap-4 p-4 text-[12px]">
        <p>{t("sessions.delete.body", { title })}</p>
        <label className="flex items-start gap-2">
          <input
            type="checkbox"
            checked={deleteWorktree}
            onChange={(e) => setDeleteWorktree(e.target.checked)}
            className="mt-0.5 accent-accent"
          />
          <span>
            {t("sessions.delete.alsoWorktree")}
            <span className="mt-1 block break-all text-[11px] text-content/45">
              {prettyCwd(unusedWorktree)}
            </span>
            <span className="mt-1 block text-[11px] text-content/45">
              {t("sessions.delete.worktreeNote")}
            </span>
          </span>
        </label>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={() => onClose({ confirmed: false, deleteWorktree: false })}
            className="rounded-md px-3 py-1.5 hover:bg-content/8 active:scale-[0.97]"
          >
            {t("sessions.delete.cancel")}
          </button>
          <button
            type="button"
            onClick={() => onClose({ confirmed: true, deleteWorktree })}
            className="rounded-md bg-red-500/20 px-3 py-1.5 font-medium text-red-400 hover:bg-red-500/30 active:scale-[0.97]"
          >
            {t("sessions.delete.confirm")}
          </button>
        </div>
      </div>
    </Modal>
  );
}
