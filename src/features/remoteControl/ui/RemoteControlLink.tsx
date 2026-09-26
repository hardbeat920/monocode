import { useEffect, useRef, useState } from "react";
import { Check, Copy } from "../../../shared/ui/icons";
import { copyText } from "../../../platform/tauri/clipboard";
import { playCue } from "../../settings/model/sounds";
import type { BridgeStatus } from "../model/transcript";

/**
 * The bridge link, so a conversation can be picked up on a phone or a browser.
 *
 * The URL belongs to the conversation rather than to the process that printed
 * it: resuming the same conversation gets the same `cse_…` back, while a
 * separately started session gets its own. So this is not per-launch state and
 * survives a handover.
 *
 * `bridge` comes from the transcript's `bridge_status` record, which is also
 * what says remote control is running at all. That record lands shortly after
 * the pty starts, so "on, but no link yet" is a real state and gets its own
 * wording instead of looking like a failure.
 */
export function RemoteControlLink({
  bridge,
  className = "",
}: {
  bridge: BridgeStatus | undefined;
  className?: string;
}) {
  if (!bridge?.active) return null;

  return (
    <div
      className={`flex min-w-0 items-center gap-2 rounded-md border border-content/10 px-2.5 py-1.5 text-[12px] ${className}`}
    >
      <span className="shrink-0 text-content/45">Remote control</span>
      {bridge.url ? (
        <>
          <span className="min-w-0 flex-1 truncate font-mono text-content/70">
            {bridge.url}
          </span>
          <CopyLinkButton url={bridge.url} />
        </>
      ) : (
        <span className="min-w-0 flex-1 truncate text-content/45">
          Starting — the link appears once the session reports it
        </span>
      )}
    </div>
  );
}

function CopyLinkButton({ url }: { url: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    setCopied(false);
    return () => {
      if (timer.current != null) window.clearTimeout(timer.current);
    };
  }, [url]);

  const label = copied ? "Copied" : "Copy remote control link";

  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      className="shrink-0 rounded p-0.5 text-content/40 hover:bg-content/8 hover:text-content/70"
      onClick={() => {
        void copyText(url).then(
          () => {
            playCue("copy");
            setCopied(true);
            if (timer.current != null) window.clearTimeout(timer.current);
            timer.current = window.setTimeout(() => setCopied(false), 2000);
          },
          () => {},
        );
      }}
    >
      {copied ? (
        <Check className="size-3" strokeWidth={1.75} />
      ) : (
        <Copy className="size-3" strokeWidth={1.75} />
      )}
    </button>
  );
}
