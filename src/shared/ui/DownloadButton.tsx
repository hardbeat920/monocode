import { useRef, useState } from "react";
import {
  downloadFile,
  type DownloadSource,
} from "../../platform/tauri/download";
import { Download, X } from "./icons";

export function DownloadButton({
  name,
  source,
}: {
  name: string;
  source: DownloadSource;
}) {
  const pending = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <span className="inline-flex min-w-0 max-w-full flex-wrap items-center gap-1.5 align-middle">
      <button
        type="button"
        title={busy ? `Saving ${name}…` : `Download ${name}`}
        aria-label={`Download ${name}`}
        aria-busy={busy || undefined}
        disabled={busy}
        className="shrink-0 rounded-md p-1 text-content/40 hover:bg-content/8 hover:text-content/70 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
        onClick={(event) => {
          event.preventDefault();
          event.stopPropagation();
          if (pending.current) return;
          pending.current = true;
          setBusy(true);
          setError(null);
          void downloadFile(name, source)
            .catch((reason: unknown) => {
              const detail =
                reason instanceof Error ? reason.message : String(reason);
              setError(`Could not download ${name}: ${detail}`);
            })
            .finally(() => {
              pending.current = false;
              setBusy(false);
            });
        }}
      >
        <Download className="size-3.5" strokeWidth={1.75} aria-hidden="true" />
      </button>
      {error ? (
        <span
          role="alert"
          className="inline-flex max-w-[min(24rem,100%)] items-start gap-1 whitespace-normal text-[11px] text-content/65"
        >
          <span className="min-w-0 [overflow-wrap:anywhere]">{error}</span>
          <button
            type="button"
            aria-label="Dismiss download error"
            title="Dismiss download error"
            className="shrink-0 rounded p-0.5 hover:bg-content/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            onClick={(event) => {
              event.preventDefault();
              event.stopPropagation();
              setError(null);
            }}
          >
            <X className="size-3" aria-hidden="true" />
          </button>
        </span>
      ) : null}
    </span>
  );
}
