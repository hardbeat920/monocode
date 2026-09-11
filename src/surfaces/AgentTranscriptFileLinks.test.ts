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
});
