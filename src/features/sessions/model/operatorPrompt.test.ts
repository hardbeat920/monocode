import { describe, expect, it, vi } from "vitest";
import { operatorTurnPrompt } from "./operatorPrompt";

describe("standing Operator instructions", () => {
  it.each([
    "ordinary follow-up",
    "<session_updates>automatic outcome</session_updates>",
  ])("retains instructions on authorized %s turns", async (text) => {
    const result = await operatorTurnPrompt(
      text,
      { operatorAccess: true, rawCommand: false },
      async () => "'/path with spaces/monocode' app",
    );
    expect(result.startsWith(text + "\n\n<monocode_app>")).toBe(true);
    expect(result).toContain("'/path with spaces/monocode' app --help");
    for (const instruction of [
      "capabilities",
      "exact returned model ID",
      "new tabs by default",
      "only when the user requests a split",
      "linkedOnly:true",
      "sessions.read",
      "sessions.send",
      "sessions.stop",
      "sessions.close",
      "delete:true",
      "sessions.focus",
      "sessions.rename",
      "never poll",
      "Approvals and answers belong to the user",
      "not verified task success",
    ])
      expect(result).toContain(instruction);
  });
  it("leaves ordinary unauthorized turns unchanged without resolving a credentialed CLI", async () => {
    const cli = vi.fn(async () => "monocode app");
    expect(
      await operatorTurnPrompt(
        "ordinary",
        { operatorAccess: false, rawCommand: false },
        cli,
      ),
    ).toBe("ordinary");
    expect(cli).not.toHaveBeenCalled();
  });
  it.each([true, false])(
    "preserves exact raw slash payload with access=%s and never resolves CLI",
    async (operatorAccess) => {
      const text = "  /model provider/model:high\r\n  exact trailing spaces  ";
      const cli = vi.fn(async () => "monocode app");
      expect(
        await operatorTurnPrompt(
          text,
          { operatorAccess, rawCommand: true },
          cli,
        ),
      ).toBe(text);
      expect(cli).not.toHaveBeenCalled();
    },
  );
});
