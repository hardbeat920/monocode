import { Globe } from "../../../shared/ui/icons";
import { findBrowserDock, toggleBrowser, useBrowserState } from "../model/browserStore";

/** Status bar toggle for the focused session's browser. */
export function BrowserStatusButton() {
  const state = useBrowserState();
  if (!state.sessionId) return null;
  const dock = findBrowserDock(state.docks, state.sessionId);
  const open = !!dock?.open;
  // An agent is working in this session's browser while the panel is hidden.
  const agentBusy =
    !open &&
    !!dock?.pane.files.some((file) => file.id in state.agentTabs);
  const label = open ? "Hide Browser" : "Show Browser";
  return (
    <button
      type="button"
      className={`inline-flex h-5 shrink-0 items-center gap-1.5 whitespace-nowrap rounded px-1.5 hover:bg-content/10 ${
        open ? "text-accent" : "text-content/40 hover:text-content"
      }`}
      aria-label={agentBusy ? `${label} (an agent is using it)` : label}
      aria-pressed={open}
      title={agentBusy ? `${label} — an agent is using it` : label}
      onClick={toggleBrowser}
    >
      <Globe
        className={`size-3.5 ${agentBusy ? "animate-pulse text-accent" : ""}`}
        strokeWidth={1.75}
        aria-hidden
      />
      <span>Browser</span>
    </button>
  );
}
