import { useCallback, useRef, useState } from "react";
import { Globe } from "../../../shared/ui/icons";
import { Popover } from "../../../shared/ui/Popover";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import {
  sessionBrowserIndicator,
  sessionBrowserTabs,
  setSessionBrowserGrant,
  useBrowserState,
} from "../../settings/model/browser";
import { BrowserTabRow } from "../../settings/ui/BrowserSettings";

const INDICATOR = {
  granted: {
    label: "Browser access on",
    className: "text-teal-600 dark:text-teal-300/80",
  },
  busy: {
    label: "Browser action running",
    className: "animate-pulse text-teal-600 dark:text-teal-300",
  },
  dialog: {
    label: "A tab is waiting on a dialog",
    className: "text-amber-600 dark:text-amber-300",
  },
} as const;

/**
 * The thread's browser indicator (spec 13.2): shown while the thread has
 * /browser, it lists the thread's tabs with the same actions as Settings.
 */
export function BrowserIndicator({ sessionId }: { sessionId: string }) {
  const state = useBrowserState();
  const indicator = sessionBrowserIndicator(state, sessionId);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const anchor = useRef<HTMLDivElement>(null);
  const run = useCallback((action: () => Promise<unknown>) => {
    setError(null);
    void action().catch((err: unknown) =>
      setError(err instanceof Error ? err.message : String(err)),
    );
  }, []);
  if (!indicator) return null;
  const tabs = sessionBrowserTabs(state, sessionId);
  const { label, className } = INDICATOR[indicator];
  return (
    <div ref={anchor} className="relative flex shrink-0 items-center">
      <button
        type="button"
        title={label}
        aria-label={label}
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className={`relative grid size-6.5 place-items-center rounded-md hover:bg-content/8 ${className}`}
      >
        <Globe className="size-3.5" strokeWidth={1.75} />
        {indicator === "dialog" ? (
          <span className="absolute top-1 right-1 size-1.5 rounded-full bg-amber-500" />
        ) : null}
      </button>
      {open ? (
        <Popover
          anchor={anchor}
          side="top"
          align="end"
          width={340}
          maxHeight={360}
          onDismiss={() => setOpen(false)}
          aria-label="Browser tabs of this thread"
          className="overflow-y-auto overscroll-contain p-2"
        >
          <div className="flex items-center gap-2 px-1 pb-2">
            <span className="min-w-0 flex-1 text-[12px] font-medium text-content">
              {label}
            </span>
            <SecondaryButton
              onClick={() =>
                run(async () => {
                  await setSessionBrowserGrant(sessionId, false);
                  setOpen(false);
                })
              }
            >
              Turn off
            </SecondaryButton>
          </div>
          {tabs.length ? (
            <ul className="space-y-1">
              {tabs.map((tab) => (
                <BrowserTabRow key={tab.tab} tab={tab} run={run} />
              ))}
            </ul>
          ) : (
            <p className="px-1 pb-1 text-[12px] text-content/45">
              The agent hasn't opened a tab yet.
            </p>
          )}
          {error ? (
            <p role="alert" className="px-1 pt-2 text-[12px] text-red-400">
              {error}
            </p>
          ) : null}
        </Popover>
      ) : null}
    </div>
  );
}
