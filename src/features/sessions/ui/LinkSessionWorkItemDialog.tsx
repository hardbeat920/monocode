import { useState, type FormEvent, type KeyboardEvent } from "react";
import { Modal } from "../../../shared/ui/Modal";
import type { LinkedWorkItem } from "../model/session";
import {
  formatGithubWorkItemUrls,
  parseGithubWorkItemUrls,
  sessionLinkedWorkItems,
} from "../model/sessionWorkItem";

export function LinkSessionWorkItemDialog({
  initial,
  sessionTitle,
  onSave,
  onClose,
}: {
  initial?: LinkedWorkItem | readonly LinkedWorkItem[];
  sessionTitle: string;
  onSave: (items: LinkedWorkItem[] | undefined) => void;
  onClose: () => void;
}) {
  const initialItems = sessionLinkedWorkItems({
    linkedWorkItems:
      initial == null
        ? undefined
        : Array.isArray(initial)
          ? [...initial]
          : [initial],
  });
  const [url, setUrl] = useState(() => formatGithubWorkItemUrls(initialItems));
  const [error, setError] = useState("");
  const editing = initialItems.length > 0;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const items = parseGithubWorkItemUrls(url);
    if (items == null || items.length === 0) {
      const tokenCount = url.split(/[\s,;]+/).filter(Boolean).length;
      setError(
        items == null && tokenCount > 1
          ? "Enter valid GitHub issue or pull request URLs, separated by commas."
          : "Enter a valid GitHub issue or pull request URL.",
      );
      return;
    }
    onSave(items);
  };

  return (
    <Modal
      title={editing ? "Edit GitHub link" : "Link GitHub issue or PR"}
      description={sessionTitle}
      size="sm"
      onClose={onClose}
    >
      <form onSubmit={submit} className="flex flex-col gap-4 p-4 text-[12px]">
        <label className="flex flex-col gap-1.5">
          <span className="font-medium text-content/80">
            Issue or pull request URLs
          </span>
          <textarea
            autoFocus
            rows={2}
            spellCheck={false}
            value={url}
            aria-label="GitHub issue or pull request URLs"
            aria-invalid={error ? true : undefined}
            aria-describedby={
              error ? "linked-work-item-error" : "linked-work-item-hint"
            }
            placeholder="https://github.com/owner/repo/pull/123, https://github.com/owner/repo/issues/456"
            onKeyDown={(event: KeyboardEvent<HTMLTextAreaElement>) => {
              if (event.key !== "Enter" || event.shiftKey) return;
              event.preventDefault();
              event.currentTarget.form?.requestSubmit();
            }}
            onChange={(event) => {
              setUrl(event.target.value);
              if (error) setError("");
            }}
            className={`min-h-[4.5rem] resize-none rounded-md border bg-content/5 px-2.5 py-2 text-[13px] leading-snug text-content outline-none placeholder:text-content/30 focus:border-content/30 ${
              error ? "border-red-400/60" : "border-content/10"
            }`}
          />
          {error ? (
            <span
              id="linked-work-item-error"
              role="alert"
              className="text-[11px] text-red-400"
            >
              {error}
            </span>
          ) : (
            <span
              id="linked-work-item-hint"
              className="text-[11px] text-content/45"
            >
              Paste full github.com URLs, separated by commas. The linked items
              will appear on the session card.
            </span>
          )}
        </label>
        <div className="flex items-center justify-end gap-2">
          {editing ? (
            <button
              type="button"
              onClick={() => onSave(undefined)}
              className="mr-auto rounded-md px-3 py-1.5 text-red-400 hover:bg-red-400/10 active:scale-[0.97]"
            >
              Remove link
            </button>
          ) : null}
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 hover:bg-content/8 active:scale-[0.97]"
          >
            Cancel
          </button>
          <button
            type="submit"
            className="rounded-md bg-accent px-3 py-1.5 font-medium text-white hover:brightness-110 active:scale-[0.97]"
          >
            {editing ? "Update link" : "Link"}
          </button>
        </div>
      </form>
    </Modal>
  );
}