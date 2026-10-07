import { describe, expect, it, vi } from "vitest";
import type { OrchestrationProposal } from "../../features/orchestration/model/orchestrationPlan";
import { buildPortableContext } from "../../features/sessions/model/portableContext";
import {
  newSession,
  type Attachment,
} from "../../features/sessions/model/session";
import {
  firstProviderRequest,
  firstProviderRequestBudget,
} from "./firstProviderRequest";

const source = {
  ...newSession("codex", "/tmp/project"),
  blocks: [
    {
      id: "history",
      role: "user" as const,
      text: "Retain this earlier request.",
    },
  ],
};

const inboxAsk = {
  key: "github:issue-1",
  title: "Approved change",
  url: "https://github.com/example/project/issues/1",
  provider: "github" as const,
};

describe("first provider request preflight", () => {
  it("rejects an oversized approved plan and wrappers before importing history", () => {
    const importHistory = vi.fn();
    const stopSource = vi.fn();
    const original = "Build approved plan";
    const text = firstProviderRequest({
      prompt: original,
      intent: "build",
      approvedPlan: "Implement the approved change.\n".repeat(2_000),
      inboxAsk,
      orchestratorPrompt: (request) => `Orchestrator instructions\n${request}`,
    });
    expect(() =>
      buildPortableContext(source, {
        windowTokens: 40_000,
        currentRequest: original,
      }),
    ).not.toThrow();
    expect(text).toContain("<approved_plan>");
    expect(text).toContain("INBOX ITEM");
    expect(text).toContain("Orchestrator instructions");
    expect(() => {
      const context = buildPortableContext(source, {
        windowTokens: 40_000,
        ...firstProviderRequestBudget({ text, attachments: [] }),
      });
      stopSource();
      importHistory(context);
    }).toThrow("remaining context");
    expect(stopSource).not.toHaveBeenCalled();
    expect(importHistory).not.toHaveBeenCalled();
  });

  it("budgets a planning proposal after its wrappers and prepared attachments", () => {
    const proposal: OrchestrationProposal = {
      version: 1,
      leadId: "lead",
      cwd: "/tmp/project",
      request: "Investigate the change",
      author: { harness: "codex", model: "codex:gpt", name: "Lead" },
      settings: {
        maxWorkers: 1,
        choices: [{ harness: "codex", model: "codex:gpt", name: "Worker" }],
      },
      status: "planning",
      title: "",
      summary: "",
      tasks: [],
    };
    const attachments: Attachment[] = [
      {
        id: "image",
        kind: "image",
        name: "reference.png",
        mimeType: "image/png",
        size: 2_000,
        data: "prepared-image",
      },
    ];
    const text = firstProviderRequest({
      prompt: proposal.request,
      intent: "orchestrate",
      proposal,
      inboxAsk,
      orchestratorPrompt: (request) => request,
    });
    const budget = firstProviderRequestBudget({ text, attachments });
    const remaining =
      new TextEncoder().encode(JSON.stringify(text)).length + 4_000 + 1_024;
    expect(() =>
      buildPortableContext(source, {
        currentRequest: proposal.request,
        windowTokens: remaining,
      }),
    ).not.toThrow();
    expect(() =>
      buildPortableContext(source, { ...budget, windowTokens: remaining }),
    ).toThrow("remaining context");
    expect(budget.attachmentTokens).toBe(4_000);
  });

  it("retains planning and manual handoff instructions in the budgeted request", () => {
    const text = firstProviderRequest({
      prompt: "Inspect the repository",
      intent: "plan",
      handoff: { from: "claude", text: "Keep the established decision." },
      earlierRequests: ["Check the earlier constraint"],
      orchestratorPrompt: (request) => request,
    });
    expect(text).toContain("You are in plan mode");
    expect(text).toContain("Keep the established decision.");
    expect(text).toContain("Check the earlier constraint");
    expect(
      firstProviderRequestBudget({ text, attachments: [] }).currentRequest,
    ).toBe(text);
  });

  it("rejects long zero-size folder references before importing history", () => {
    const importHistory = vi.fn();
    const attachments: Attachment[] = Array.from(
      { length: 20 },
      (_, index) => ({
        id: `folder-${index}`,
        kind: "file",
        name: `folder-${index}`,
        mimeType: "inode/directory",
        size: 0,
        path: `/tmp/${"nested-folder/".repeat(8)}${"多字节目录".repeat(10)}/${index}`,
      }),
    );
    expect(() => {
      const context = buildPortableContext(source, {
        windowTokens: 20_000,
        occupiedTokens: 18_800,
        ...firstProviderRequestBudget({ text: "Continue", attachments }),
      });
      importHistory(context);
    }).toThrow("remaining context");
    expect(importHistory).not.toHaveBeenCalled();
  });
});
