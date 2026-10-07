import { Globe } from "../../../shared/ui/icons";
import { findProjectTerminal } from "../../projects/model/projectTerminal";
import { toggleBrowser, useBrowserState } from "../model/browserStore";

/** Status bar toggle for the current project's browser dock. */
export function BrowserStatusButton() {
  const state = useBrowserState();
  const open = !!findProjectTerminal(state.docks, state.projectPath)?.open;
  const label = open ? "Hide Browser" : "Show Browser";
  return (
    <button
      type="button"
      className={`inline-flex h-5 shrink-0 items-center gap-1.5 whitespace-nowrap rounded px-1.5 hover:bg-content/10 ${
        open ? "text-accent" : "text-content/40 hover:text-content"
      }`}
      aria-label={label}
      aria-pressed={open}
      title={label}
      onClick={toggleBrowser}
    >
      <Globe className="size-3.5" strokeWidth={1.75} aria-hidden />
      <span>Browser</span>
    </button>
  );
}
