import { Loader, WandSparkles } from "./icons";
import { useEffect, useRef, useState } from "react";
import { generateCommitMessage } from "../lib/harness";
import { MOD } from "../lib/platform";
import { Modal } from "./Modal";

type Busy = "stash" | "commit" | null;

type Props = {
  cwd: string;
  branch: string;
  creating?: boolean;
  busy: Busy;
  error?: string | null;
  onStash: () => void;
  onCommit: (message: string) => void;
  onCancel: () => void;
};

export function SwitchBranchDialog({
  cwd,
  branch,
  creating = false,
  busy,
  error,
  onStash,
  onCommit,
  onCancel,
}: Props) {
  const [message, setMessage] = useState("");
  const [generating, setGenerating] = useState(false);
  const messageRef = useRef<HTMLTextAreaElement>(null);
  const trimmed = message.trim();
  const locked = Boolean(busy) || generating;
  const canCommit = trimmed.length > 0 && !locked;

  useEffect(() => {
    const el = messageRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [message]);

  const generate = async () => {
    if (locked) return;
    setGenerating(true);
    try {
      setMessage(await generateCommitMessage(cwd));
    } catch (err) {
      window.alert(err instanceof Error ? err.message : String(err));
    } finally {
      setGenerating(false);
      messageRef.current?.focus();
    }
  };

  return (
    <Modal
      title="Uncommitted changes"
      description={
        creating
          ? `Creating “${branch}” would overwrite your local changes.`
          : `Switching to “${branch}” would overwrite your local changes.`
      }
      size="sm"
      initialFocusRef={messageRef}
      closeDisabled={locked}
      onClose={onCancel}
    >
      <div className="flex flex-col gap-4 p-4" aria-busy={locked}>
        <p className="text-[13px] leading-relaxed text-content/60">
          Stash the changes for later, or commit them on this branch first.
        </p>

        <div className="relative">
          <textarea
            ref={messageRef}
            rows={1}
            value={message}
            placeholder={`Message (${MOD}↩ to commit)`}
            disabled={locked}
            aria-label="Commit message"
            className="max-h-40 w-full resize-none overflow-y-auto rounded-md bg-content/10 py-1 pr-8 pl-2 text-[13px] leading-5 text-content outline-none placeholder:text-content/35 disabled:opacity-40"
            onChange={(event) => setMessage(event.target.value)}
            onKeyDown={(event) => {
              if (
                (event.metaKey || event.ctrlKey) &&
                event.key === "Enter" &&
                canCommit
              ) {
                event.preventDefault();
                onCommit(trimmed);
              }
            }}
          />
          <button
            type="button"
            title="Generate commit message"
            aria-label="Generate commit message"
            disabled={locked}
            onClick={() => void generate()}
            className="absolute top-1 right-1 grid size-5 place-items-center rounded-md bg-content/10 text-content hover:bg-content/20 hover:text-content disabled:opacity-40"
          >
            {generating ? (
              <Loader className="size-3.5 animate-spin" strokeWidth={1.75} />
            ) : (
              <WandSparkles className="size-3" strokeWidth={1} />
            )}
          </button>
        </div>

        {error ? (
          <p
            role="alert"
            className="whitespace-pre-wrap text-[11px] leading-4 text-red-400/90"
          >
            {error}
          </p>
        ) : null}

        <div className="flex flex-wrap justify-end gap-2">
          <button
            type="button"
            disabled={locked}
            onClick={onCancel}
            className="rounded-md px-3 py-1.5 text-[12px] text-content/70 hover:bg-content/8 hover:text-content disabled:opacity-40"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={!canCommit}
            onClick={() => onCommit(trimmed)}
            className="inline-flex items-center gap-1.5 rounded-md bg-content/10 px-3 py-1.5 text-[12px] font-medium text-content hover:bg-content/15 disabled:opacity-40"
          >
            {busy === "commit" ? (
              <Loader className="size-3.5 animate-spin" strokeWidth={1.75} />
            ) : null}
            Commit & switch
          </button>
          <button
            type="button"
            disabled={locked}
            onClick={onStash}
            className="inline-flex items-center gap-1.5 rounded-md bg-content px-3 py-1.5 text-[12px] font-medium text-background-base hover:bg-content/80 disabled:opacity-40"
          >
            {busy === "stash" ? (
              <Loader className="size-3.5 animate-spin" strokeWidth={1.75} />
            ) : null}
            Stash & switch
          </button>
        </div>
      </div>
    </Modal>
  );
}
