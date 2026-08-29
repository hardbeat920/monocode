import { describe, expect, it } from "vitest";
import type { Block, HarnessId } from "./session";
import {
  buildSecondOpinionCard,
  buildSecondOpinionPrompt,
  harnessForTurn,
  secondOpinionTargets,
  turnEditedFiles,
  turnReport,
  turnUserRequest,
} from "./secondOpinion";

function user(id: string, text: string): Block {
  return { id, role: "user", text };
}

function assistant(id: string, text: string): Block {
  return { id, role: "assistant", text };
}

function edit(id: string, path: string): Block {
  return {
    id,
    role: "tool",
    text: `Edited ${path}`,
    tool: {
      kind: "edit",
      title: `Edited ${path}`,
      status: "completed",
      preview: { kind: "write", path, fileName: path.split("/").pop() },
    },
  };
}

describe("harnessForTurn", () => {
  it("uses the session harness when there was no handoff", () => {
    const turn = [user("u", "go"), assistant("a", "done")];
    expect(harnessForTurn(turn, turn, "claude")).toBe("claude");
  });

  it("attributes a turn before a handoff to the outgoing provider", () => {
    const first = [user("u1", "go"), assistant("a1", "working")];
    const blocks: Block[] = [
      ...first,
      {
        id: "h",
        role: "handoff",
        text: "",
        handoff: { from: "claude", to: "codex", status: "ready" },
      },
      user("u2", "keep going"),
    ];
    expect(harnessForTurn(blocks, first, "codex")).toBe("claude");
  });

  it("attributes a turn after a handoff to the incoming provider", () => {
    const second = [user("u2", "keep going"), assistant("a2", "ok")];
    const blocks: Block[] = [
      user("u1", "go"),
      {
        id: "h",
        role: "handoff",
        text: "",
        handoff: { from: "claude", to: "codex", status: "ready" },
      },
      ...second,
    ];
    expect(harnessForTurn(blocks, second, "codex")).toBe("codex");
  });
});

describe("turn extracts", () => {
  it("reads the user request, report, and edited files", () => {
    const turn = [
      user("u", "fix the footer"),
      assistant("a1", "I'll patch UsageFooter."),
      edit("e", "/repo/src/chrome/UsageFooter.tsx"),
      { id: "p", role: "plan" as const, text: "## Plan\n\n- edit the chip" },
      assistant("a2", "Done."),
    ];
    expect(turnUserRequest(turn)).toBe("fix the footer");
    expect(turnReport(turn)).toBe(
      "I'll patch UsageFooter.\n\n## Plan\n\n- edit the chip\n\nDone.",
    );
    expect(turnEditedFiles(turn, "/repo")).toEqual([
      "src/chrome/UsageFooter.tsx",
    ]);
  });

  it("dedupes edited paths", () => {
    expect(
      turnEditedFiles(
        [edit("a", "src/App.tsx"), edit("b", "src/App.tsx")],
        "/repo",
      ),
    ).toEqual(["src/App.tsx"]);
  });
});

describe("secondOpinionTargets", () => {
  const all: HarnessId[] = [
    "claude",
    "codex",
    "cursor",
    "opencode",
    "pi",
    "omp",
    "fx",
  ];
  const installed = (id: HarnessId) => id === "claude" || id === "codex";
  const visible = (id: HarnessId) => all.includes(id);

  it("drops the provider that did the work", () => {
    expect(
      secondOpinionTargets("claude", {
        installed,
        visible,
        probed: true,
      }),
    ).toEqual(["codex"]);
  });

  it("hides providers the user turned off in the picker", () => {
    expect(
      secondOpinionTargets("claude", {
        installed: () => true,
        visible: (id) => id === "codex",
        probed: true,
      }),
    ).toEqual(["codex"]);
  });

  it("keeps unprobed visible providers so the menu can open", () => {
    expect(
      secondOpinionTargets("claude", {
        installed: () => false,
        visible: (id) => id === "codex" || id === "cursor",
        probed: false,
      }),
    ).toEqual(["codex", "cursor"]);
  });
});

describe("buildSecondOpinionPrompt", () => {
  it("asks the next agent to review and names the files", () => {
    const prompt = buildSecondOpinionPrompt({
      from: "claude",
      userRequest: "fix the footer",
      report: "Updated the usage chip.",
      files: ["src/chrome/UsageFooter.tsx"],
    });
    expect(prompt).toContain("Claude Code");
    expect(prompt).toContain("fix the footer");
    expect(prompt).toContain("Updated the usage chip.");
    expect(prompt).toContain("- src/chrome/UsageFooter.tsx");
    expect(prompt).toContain("Do not redo the task from scratch");
  });

  it("still builds a prompt when the turn left no summary or edits", () => {
    const prompt = buildSecondOpinionPrompt({
      from: "codex",
      userRequest: "",
      report: "",
      files: [],
    });
    expect(prompt).toContain("Codex");
    expect(prompt).toContain("(no user message on this turn)");
    expect(prompt).toContain("(no written summary");
    expect(prompt).toContain("(none recorded on this turn)");
  });
});

describe("buildSecondOpinionCard", () => {
  it("keeps a short request and the file count", () => {
    expect(
      buildSecondOpinionCard({
        from: "claude",
        to: "codex",
        userRequest: "  fix the footer\nplease  ",
        files: ["a.ts", "b.ts"],
      }),
    ).toEqual({
      from: "claude",
      to: "codex",
      request: "fix the footer please",
      files: 2,
    });
  });

  it("omits empty request and file fields", () => {
    expect(
      buildSecondOpinionCard({
        from: "cursor",
        to: "pi",
        userRequest: "   ",
        files: [],
      }),
    ).toEqual({ from: "cursor", to: "pi" });
  });
});
