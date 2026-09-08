import { askDialog as ask } from "./transport/dialog";
import type { FilePaneTab } from "./layout";
import { getPtyStatus } from "./pty";
import { terminalTabLabel } from "./terminalTab";

type RunningTerminal = {
  file: FilePaneTab;
  process: string;
};

async function runningTerminals(files: FilePaneTab[]): Promise<RunningTerminal[]> {
  const running: RunningTerminal[] = [];
  for (const file of files) {
    if (!file.terminal) continue;
    try {
      const { foreground } = await getPtyStatus(file.id);
      const process = foreground?.trim();
      if (process) running.push({ file, process });
    } catch {
      // PTY already gone — nothing to confirm.
    }
  }
  return running;
}

/** Confirm closing one terminal when its foreground process is not the shell. */
export async function confirmCloseTerminal(file: FilePaneTab): Promise<boolean> {
  const running = await runningTerminals([file]);
  if (running.length === 0) return true;
  const { process } = running[0];
  const label = terminalTabLabel(file);
  return ask(
    `"${process}" is still running in ${label}. Close this terminal anyway?`,
    { title: "MonoCode", kind: "warning" },
  );
}

/** Confirm closing terminals that still have a foreground process. */
export async function confirmCloseTerminals(files: FilePaneTab[]): Promise<boolean> {
  const running = await runningTerminals(files);
  if (running.length === 0) return true;
  if (running.length === 1) {
    const { file, process } = running[0];
    return ask(
      `"${process}" is still running in ${terminalTabLabel(file)}. Close this terminal anyway?`,
      { title: "MonoCode", kind: "warning" },
    );
  }
  const lines = running
    .map(({ file, process }) => `• ${terminalTabLabel(file)} (${process})`)
    .join("\n");
  return ask(
    `These terminals are still running:\n${lines}\n\nClose them anyway?`,
    { title: "MonoCode", kind: "warning" },
  );
}
