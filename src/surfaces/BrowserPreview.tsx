import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  RefreshCw,
} from "../chrome/icons";
import { previewUrl } from "../lib/browserPreview";
import {
  useBrowserPreview,
  type PreviewEvent,
} from "../hooks/useBrowserPreview";

const BUTTON =
  "grid size-6 shrink-0 place-items-center rounded text-content/55 hover:bg-content/10 hover:text-content disabled:opacity-30 disabled:pointer-events-none";

export function BrowserPreview({
  initialUrl,
  onFocus,
}: {
  initialUrl: string;
  onFocus: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [url, setUrl] = useState(initialUrl);
  const [address, setAddress] = useState(initialUrl);
  const [displayUrl, setDisplayUrl] = useState(initialUrl);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const editing = useRef(false);
  useEffect(() => {
    setUrl(initialUrl);
    setAddress(initialUrl);
  }, [initialUrl]);
  const onEvent = useCallback(
    (event: PreviewEvent) => {
      if (event.kind === "focus") {
        onFocus();
        return;
      }
      if (
        event.kind === "blocked" ||
        event.kind === "popup" ||
        event.kind === "download"
      ) {
        setError(
          event.kind === "download"
            ? "Use the external browser to download this file."
            : event.kind === "popup"
              ? "This page requested another window. Use the external browser to continue."
              : "This address cannot be opened in the preview.",
        );
        return;
      }
      if (event.kind !== "url") {
        setLoading(event.kind === "loading");
        setError("");
      }
      if (event.url) {
        setDisplayUrl(event.url);
        if (!editing.current) setAddress(event.url);
      }
    },
    [onFocus],
  );
  const action = useBrowserPreview(host, url, onEvent, setError);
  useEffect(() => {
    if (!loading) return;
    const timeout = setTimeout(() => {
      setError(
        "The page is taking longer than expected. Reload or open it in your browser.",
      );
      setLoading(false);
    }, 15000);
    return () => clearTimeout(timeout);
  }, [loading]);

  return (
    <div className="flex h-full min-h-0 flex-col" aria-label="Browser preview">
      <form
        className="flex h-9 shrink-0 items-center gap-1 border-b border-content/10 px-2"
        onSubmit={(event) => {
          event.preventDefault();
          const target = previewUrl(address);
          if (!target) {
            setError("Enter an HTTP or HTTPS address.");
            return;
          }
          setError("");
          if (target === url) action({ navigate: target });
          else setUrl(target);
        }}
      >
        <button
          type="button"
          className={BUTTON}
          aria-label="Back"
          disabled={!url}
          onClick={() => action("back")}
        >
          <ChevronLeft className="size-3.5" strokeWidth={1.75} />
        </button>
        <button
          type="button"
          className={BUTTON}
          aria-label="Forward"
          disabled={!url}
          onClick={() => action("forward")}
        >
          <ChevronRight className="size-3.5" strokeWidth={1.75} />
        </button>
        <button
          type="button"
          className={BUTTON}
          aria-label="Reload"
          disabled={!url}
          onClick={() => {
            setError("");
            action("reload");
          }}
        >
          <RefreshCw
            className={`size-3.5 ${loading ? "animate-spin" : ""}`}
            strokeWidth={1.75}
          />
        </button>
        <input
          aria-label="Web address"
          placeholder="http://localhost:3000"
          value={address}
          spellCheck={false}
          onChange={(event) => setAddress(event.target.value)}
          onFocus={() => {
            editing.current = true;
          }}
          onBlur={() => {
            editing.current = false;
          }}
          className="h-6 min-w-0 flex-1 rounded-md border border-content/10 bg-content/5 px-2 text-[12px] text-content outline-none placeholder:text-content/35 focus:border-accent/60"
        />
        <button
          type="button"
          className={BUTTON}
          aria-label="Open in external browser"
          disabled={!previewUrl(displayUrl)}
          onClick={() => {
            const target = previewUrl(displayUrl);
            if (target)
              void openUrl(target).catch((e: unknown) => setError(String(e)));
          }}
        >
          <ExternalLink className="size-3.5" strokeWidth={1.75} />
        </button>
      </form>
      {error ? (
        <p
          role="status"
          className="shrink-0 border-b border-content/10 px-3 py-2 text-[12px] text-content/70"
        >
          {error}
        </p>
      ) : null}
      <div ref={host} className="relative min-h-0 flex-1 bg-background-base">
        {!url ? (
          <div className="grid h-full place-items-center p-6 text-center text-[13px] text-content/50">
            Enter a web address or start a local development server.
          </div>
        ) : null}
      </div>
    </div>
  );
}
