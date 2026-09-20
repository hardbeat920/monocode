import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { normalizeBrowserUrl, openBrowserWindow } from "../lib/browserWindow";
import { Modal } from "./Modal";

const PLACEHOLDER = "localhost:3000";

/**
 * URL prompt for "New Browser Window".
 *
 * Self-contained on purpose: it owns its own menu listener so opening a browser
 * window costs App.tsx one mounted element rather than another entry in the
 * shared actions ref.
 */
export function BrowserPrompt() {
  const [open, setOpen] = useState(false);
  const [url, setUrl] = useState("");
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const unlisten = listen("new_browser_window", () => {
      setUrl("");
      setError(null);
      setOpen(true);
    });
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  if (!open) return null;

  const submit = async () => {
    const target = normalizeBrowserUrl(url || PLACEHOLDER);
    if (!target) {
      setError("Enter an http or https address.");
      inputRef.current?.focus();
      return;
    }
    try {
      await openBrowserWindow(target);
      setOpen(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <Modal
      onClose={() => setOpen(false)}
      title="New Browser Window"
      description="Opens as a separate native window."
      size="sm"
    >
      <form
        className="flex flex-col gap-3 px-4 pt-3 pb-4"
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <input
          ref={inputRef}
          type="text"
          value={url}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          placeholder={PLACEHOLDER}
          aria-label="Address"
          aria-invalid={error ? true : undefined}
          onChange={(event) => {
            setUrl(event.target.value);
            if (error) setError(null);
          }}
          className="w-full rounded-lg border border-content/15 bg-content/5 px-3 py-2 text-[13px] text-content placeholder:text-content/35 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        />
        {error ? (
          <p role="alert" className="text-[12px] leading-snug text-red-400">
            {error}
          </p>
        ) : null}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={() => setOpen(false)}
            className="rounded-md px-3 py-1.5 text-[12px] text-content/60 hover:bg-content/8 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            Cancel
          </button>
          <button
            type="submit"
            className="rounded-md bg-accent px-3 py-1.5 text-[12px] font-medium text-background-base hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            Open
          </button>
        </div>
      </form>
    </Modal>
  );
}
