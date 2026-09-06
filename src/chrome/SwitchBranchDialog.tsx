import { Loader, WandSparkles } from "./icons";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { generateCommitMessage } from "../lib/harness";
import { LAYER } from "../lib/layers";
import { MOD } from "../lib/platform";
import { t } from "../lib/i18n";

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
  const canCommit = trimmed.length > 0 && !busy && !generating;

  useEffect(() => {
    messageRef.current?.focus();
  }, []);

  useEffect(() => {
    const el = messageRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [message]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      if (!busy && !generating) onCancel();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [busy, generating, onCancel]);

  const generate = async () => {
    if (busy || generating) return;
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

  return createPortal(
    <div className="fixed inset-0" style={{ zIndex: LAYER.dialog }}>
      <div
        className="absolute inset-0 bg-black/30"
        onMouseDown={() => {
          if (!busy && !generating) onCancel();
        }}
      />
      <div
        role="dialog"
        aria-modal="true"
        aria-busy={Boolean(busy) || generating}
        aria-label={creating ? t("Create {0}", [branch]) : t("Switch to {0}", [branch])}
        onMouseDown={(event) => event.stopPropagation()}
        className="absolute left-1/2 top-[22%] flex w-[min(420px,calc(100vw-24px))] -translate-x-1/2 flex-col gap-3 rounded-lg border border-content/10 bg-content/5 p-4 shadow-xl backdrop-blur-xl"
      >
        <div className="flex flex-col gap-1">
          <h2 className="text-[13px] font-medium leading-tight text-content">
            {t("Uncommitted changes")}
          </h2>
          <p className="text-[12px] leading-snug text-content/55">
            {creating
              ? t("Creating \"{0}\" would overwrite your local changes. Stash them for later, or commit them on this branch first.", [branch])
              : t("Switching to \"{0}\" would overwrite your local changes. Stash them for later, or commit them on this branch first.", [branch])}
          </p>
        </div>

        <div className="relative">
          <textarea
            ref={messageRef}
            rows={1}
            value={message}
            placeholder={t("Message ({0}↩ to commit)", [MOD])}
            disabled={Boolean(busy) || generating}
            aria-label={t("Commit message")}
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
            title={t("Generate commit message")}
            aria-label={t("Generate commit message")}
            disabled={Boolean(busy) || generating}
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
          <p className="whitespace-pre-wrap text-[11px] leading-4 text-red-400/90">
            {error}
          </p>
        ) : null}

        <div className="flex flex-wrap justify-end gap-2">
          <button
            type="button"
            disabled={Boolean(busy) || generating}
            onClick={onCancel}
            className="rounded-md px-3 py-1.5 text-[12px] text-content/70 hover:bg-content/8 hover:text-content disabled:opacity-40"
          >
            {t("Cancel")}
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
            {t("Commit & switch")}
          </button>
          <button
            type="button"
            disabled={Boolean(busy) || generating}
            onClick={onStash}
            className="inline-flex items-center gap-1.5 rounded-md bg-content px-3 py-1.5 text-[12px] font-medium text-background-base hover:bg-content/80 disabled:opacity-40"
          >
            {busy === "stash" ? (
              <Loader className="size-3.5 animate-spin" strokeWidth={1.75} />
            ) : null}
            {t("Stash & switch")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
