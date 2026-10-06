import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Pin } from "../../../shared/ui/icons";
import { savedPromptError } from "../model/savedPrompts";

export type SavedPromptDraft = { title: string; body: string; pinned: boolean };

/**
 * Title + text + pin editor, shared by Settings → Prompts and the Save prompt
 * dialog. Ctrl/Cmd+Enter saves; Escape is left to the host (closes it).
 */
export function SavedPromptForm({
  initial,
  submitLabel = "Save prompt",
  onSave,
  onCancel,
}: {
  initial: SavedPromptDraft;
  submitLabel?: string;
  onSave: (draft: SavedPromptDraft) => Promise<void>;
  onCancel: () => void;
}): ReactNode {
  const [title, setTitle] = useState(initial.title);
  const [body, setBody] = useState(initial.body);
  const [pinned, setPinned] = useState(initial.pinned);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const titleRef = useRef<HTMLInputElement>(null);
  const bodyId = useId();
  const titleId = useId();

  useEffect(() => {
    titleRef.current?.focus();
    titleRef.current?.select();
  }, []);

  const submit = async (): Promise<void> => {
    const invalid = savedPromptError(title, body);
    if (invalid) {
      setError(invalid);
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await onSave({ title: title.trim(), body, pinned });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setSaving(false);
    }
  };

  return (
    <form
      className="flex min-h-0 flex-col gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
          event.preventDefault();
          void submit();
        }
      }}
    >
      <div className="flex flex-col gap-1">
        <label htmlFor={titleId} className="text-[12px] text-content/60">
          Title
        </label>
        <input
          id={titleId}
          ref={titleRef}
          value={title}
          disabled={saving}
          onChange={(event) => setTitle(event.target.value)}
          placeholder="Shown in the ! picker, e.g. Review this PR"
          autoComplete="off"
          className="h-8 rounded-md border border-content/10 bg-transparent px-2.5 text-[13px] text-content outline-none placeholder:text-content/35 focus:border-content/25"
        />
      </div>
      <div className="flex min-h-0 flex-col gap-1">
        <label htmlFor={bodyId} className="text-[12px] text-content/60">
          Text to insert
        </label>
        <textarea
          id={bodyId}
          value={body}
          disabled={saving}
          onChange={(event) => setBody(event.target.value)}
          placeholder="A keyword, a phrase or a whole prompt"
          rows={8}
          spellCheck
          className="min-h-32 resize-y rounded-md border border-content/10 bg-transparent px-2.5 py-2 text-[13px] leading-5 text-content outline-none placeholder:text-content/35 focus:border-content/25"
        />
      </div>
      {error ? (
        <p role="alert" className="text-[12px] text-red-400">
          {error}
        </p>
      ) : null}
      <div className="flex items-center gap-2">
        <button
          type="button"
          role="switch"
          aria-checked={pinned}
          disabled={saving}
          onClick={() => setPinned((value) => !value)}
          title="Pinned prompts stay at the top of the ! picker"
          className={`flex h-7 items-center gap-1.5 rounded-md px-2 text-[12px] ${
            pinned
              ? "bg-content/10 text-content"
              : "text-content/60 hover:bg-content/10 hover:text-content"
          }`}
        >
          <Pin className="size-3.5" />
          {pinned ? "Pinned" : "Pin"}
        </button>
        <span className="ml-auto" />
        <button
          type="button"
          disabled={saving}
          onClick={onCancel}
          className="h-7 rounded-md px-2.5 text-[12px] text-content/60 hover:bg-content/10 hover:text-content"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={saving}
          className="h-7 rounded-md bg-accent px-3 text-[12px] font-medium text-white disabled:opacity-40"
        >
          {saving ? "Saving…" : submitLabel}
        </button>
      </div>
    </form>
  );
}
