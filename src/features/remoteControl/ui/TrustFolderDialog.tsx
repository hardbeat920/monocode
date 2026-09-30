import { prettyCwd } from "../../../shared/lib/paths";
import { Modal } from "../../../shared/ui/Modal";

/**
 * Claude Code's folder trust check, asked here instead of in a terminal.
 *
 * The interactive CLI opens on a "Quick safety check" in any folder it has not
 * been told to trust, and waits. Handed over, that dialog used to arrive as a
 * terminal raised inside MonoCode with the cursor on "No, exit". The question
 * is the same one, put the same way; a yes is recorded where the CLI would
 * have recorded it, and the hand-over goes on without the dialog.
 */
export function TrustFolderDialog({
  folder,
  onClose,
}: {
  folder: string;
  onClose: (trusted: boolean) => void;
}) {
  return (
    <Modal
      title="Trust this folder for Claude Code?"
      size="sm"
      onClose={() => onClose(false)}
    >
      <div className="flex flex-col gap-4 p-4 text-[12px]">
        <p className="break-all font-mono text-[11px] text-content/70">
          {prettyCwd(folder)}
        </p>
        <p>
          Claude Code asks this once per folder before it will read, edit and
          run files there. Say yes for your own projects and code you trust; if
          you are not sure, look through the folder first.
        </p>
        <p className="text-[11px] text-content/45">
          Answering here records the same choice the terminal dialog would, so
          Remote Control can open without one.
        </p>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={() => onClose(false)}
            className="rounded-md px-3 py-1.5 hover:bg-content/8 active:scale-[0.97]"
          >
            Not now
          </button>
          <button
            type="button"
            autoFocus
            onClick={() => onClose(true)}
            className="rounded-md bg-accent/20 px-3 py-1.5 font-medium text-accent hover:bg-accent/30 active:scale-[0.97]"
          >
            Yes, I trust this folder
          </button>
        </div>
      </div>
    </Modal>
  );
}
