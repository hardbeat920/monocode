// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../lib/session";
import { AgentTranscript } from "./AgentTranscript";

describe("AgentTranscript file references", () => {
  let root: Root;
  let container: HTMLDivElement;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  it("passes exact tool paths when opening a shortened prose reference", async () => {
    const path = "/Users/me/other/project/platform/backup.yaml";
    const blocks: Block[] = [
      { id: "user", role: "user", text: "Inspect the backup" },
      {
        id: "edit",
        role: "tool",
        text: `Edit ${path}`,
        tool: {
          kind: "edit",
          status: "completed",
          preview: { kind: "write", path, fileName: "backup.yaml" },
        },
      },
      { id: "answer", role: "assistant", text: "Updated `backup.yaml`." },
    ];
    const onOpenFile = vi.fn();

    await act(async () => {
      root.render(
        createElement(AgentTranscript, {
          blocks,
          cwd: "/Users/me/session",
          onOpenFile,
        }),
      );
    });
    await act(async () => {
      container.querySelector<HTMLElement>('code[role="link"]')!.click();
    });

    expect(onOpenFile).toHaveBeenCalledWith(
      "/Users/me/session/backup.yaml",
      undefined,
      { candidatePaths: [path] },
    );
  });

  it("offers every file of a multi-file edit, not just the previewed one", async () => {
    const first = "/Users/me/other/project/src/one.ts";
    const second = "/Users/me/other/project/src/two.ts";
    const blocks: Block[] = [
      { id: "user", role: "user", text: "Rename the helper" },
      {
        id: "edit",
        role: "tool",
        text: `Edit ${first}`,
        tool: {
          kind: "edit",
          status: "completed",
          preview: { kind: "write", path: first, fileName: "one.ts" },
          paths: [first, second],
        },
      },
      { id: "answer", role: "assistant", text: "Renamed it in `two.ts`." },
    ];
    const onOpenFile = vi.fn();

    await act(async () => {
      root.render(
        createElement(AgentTranscript, {
          blocks,
          cwd: "/Users/me/session",
          onOpenFile,
        }),
      );
    });
    await act(async () => {
      container.querySelector<HTMLElement>('code[role="link"]')!.click();
    });

    expect(onOpenFile).toHaveBeenCalledWith(
      "/Users/me/session/two.ts",
      undefined,
      { candidatePaths: [first, second] },
    );
  });

  it("answers with the paths the turn has now, not the ones it started with", async () => {
    const early = "/Users/me/other/project/src/early.ts";
    const late = "/Users/me/other/project/platform/backup.yaml";
    const read: Block = {
      id: "read",
      role: "tool",
      text: `Read ${early}`,
      tool: {
        kind: "read",
        status: "completed",
        preview: { kind: "read", path: early, fileName: "early.ts" },
      },
    };
    const answer: Block = {
      id: "answer",
      role: "assistant",
      text: "Updated `backup.yaml`.",
    };
    const onOpenFile = vi.fn();
    const render = (blocks: Block[]) =>
      act(async () => {
        root.render(
          createElement(AgentTranscript, {
            blocks,
            cwd: "/Users/me/session",
            onOpenFile,
          }),
        );
      });

    await render([{ id: "user", role: "user", text: "Fix it" }, read, answer]);
    // The edit lands while the reader is still looking at the turn.
    await render([
      { id: "user", role: "user", text: "Fix it" },
      read,
      {
        id: "edit",
        role: "tool",
        text: `Edit ${late}`,
        tool: {
          kind: "edit",
          status: "completed",
          preview: { kind: "write", path: late, fileName: "backup.yaml" },
        },
      },
      answer,
    ]);
    await act(async () => {
      container.querySelector<HTMLElement>('code[role="link"]')!.click();
    });

    expect(onOpenFile).toHaveBeenCalledWith(
      "/Users/me/session/backup.yaml",
      undefined,
      { candidatePaths: [early, late] },
    );
  });
});
