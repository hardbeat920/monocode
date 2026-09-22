import { afterEach, describe, expect, it, vi } from "vitest";
import { newTerminalFile } from "../../workspace/model/layout";
import { confirmCloseTerminal, confirmCloseTerminals } from "./terminalClose";

const getPtyStatus = vi.fn();
const ask = vi.fn();

vi.mock("../../../platform/tauri/pty", () => ({
  getPtyStatus: (...args: unknown[]) => getPtyStatus(...args),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: (...args: unknown[]) => ask(...args),
}));

const dialogOptions = { title: "MonoCode", kind: "warning" } as const;

describe("confirmCloseTerminal", () => {
  afterEach(() => {
    getPtyStatus.mockReset();
    ask.mockReset();
  });

  it("allows close when only the shell is foreground", async () => {
    const file = newTerminalFile("/repo");
    getPtyStatus.mockResolvedValue({ foreground: null });
    await expect(confirmCloseTerminal(file)).resolves.toBe(true);
    expect(getPtyStatus).toHaveBeenCalledWith(file.id);
    expect(ask).not.toHaveBeenCalled();
  });

  it("prompts when a process is running", async () => {
    const file = newTerminalFile("/repo", "agent-terminal");
    getPtyStatus.mockResolvedValue({ foreground: "npm" });
    ask.mockResolvedValue(false);
    await expect(confirmCloseTerminal(file)).resolves.toBe(false);
    expect(ask).toHaveBeenCalledWith(
      '"npm" is still running in agent-terminal. Close this terminal anyway?',
      dialogOptions,
    );
  });
});

describe("confirmCloseTerminals", () => {
  afterEach(() => {
    getPtyStatus.mockReset();
    ask.mockReset();
  });

  it("summarizes multiple running terminals", async () => {
    const first = newTerminalFile("/repo", "dev");
    const second = newTerminalFile("/repo", "test");
    getPtyStatus.mockImplementation(async (id: string) => {
      if (id === first.id) return { foreground: "vite" };
      if (id === second.id) return { foreground: "jest" };
      return { foreground: null };
    });
    ask.mockResolvedValue(true);
    await expect(confirmCloseTerminals([first, second])).resolves.toBe(true);
    expect(ask).toHaveBeenCalledWith(
      "These terminals are still running:\n• dev (vite)\n• test (jest)\n\nClose them anyway?",
      dialogOptions,
    );
  });
});
