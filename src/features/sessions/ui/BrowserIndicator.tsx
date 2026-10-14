import { useCallback, useRef, useState } from "react";
import { Globe } from "../../../shared/ui/icons";
import { Popover } from "../../../shared/ui/Popover";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import {
  answerBrowserRequest,
  sessionBrowserAccess,
  sessionBrowserIndicator,
  sessionBrowserTabs,
  setSessionBrowserGrant,
  useBrowserState,
} from "../../settings/model/browser";
import { AddFreeTab, BrowserTabRow } from "../../settings/ui/BrowserSettings";

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
          {state?.connection.state === "connected" ? (
            <AddFreeTab sessionId={sessionId} run={run} />
          ) : null}
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

const REQUEST_BUTTON =
  "flex h-6 shrink-0 items-center rounded-md px-1.5 hover:bg-content/10 hover:text-content disabled:opacity-50";

/**
 * "Ask" mode: an agent in this thread waits for the browser. The prompt sits
 * on the composer like the other composer notices; the agent's tool call
 * waits for the answer.
 */
export function BrowserRequestPrompt({ sessionId }: { sessionId: string }) {
  const state = useBrowserState();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!state?.toggle_on || !sessionBrowserAccess(state, sessionId)?.requested)
    return null;
  const answer = (allow: boolean) => {
    setBusy(true);
    setError(null);
    answerBrowserRequest(sessionId, allow)
      .catch((err: unknown) =>
        setError(err instanceof Error ? err.message : String(err)),
      )
      .finally(() => setBusy(false));
  };
  return (
    <div className="px-2 text-content/55" data-browser-request role="status">
      <div className="relative z-0 flex h-8 items-center gap-2 rounded-t-[10px] border border-b-0 border-teal-400/25 bg-teal-400/10 px-2 text-[12px]">
        <Globe
          className="size-3.5 shrink-0 text-teal-600 dark:text-teal-300"
          strokeWidth={1.75}
        />
        <span
          title={error ?? undefined}
          className={`min-w-0 flex-1 truncate ${error ? "text-red-400" : "text-content/85"}`}
        >
          {error ?? "This agent wants to use the browser."}
        </span>
        <button
          type="button"
          disabled={busy}
          onClick={() => answer(true)}
          className={`${REQUEST_BUTTON} text-content`}
        >
          Allow for this thread
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={() => answer(false)}
          className={REQUEST_BUTTON}
        >
          Not now
        </button>
      </div>
    </div>
  );
}
