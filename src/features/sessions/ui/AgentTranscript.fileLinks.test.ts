// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import { AgentTranscript } from "./AgentTranscript";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("file names in a reply", () => {
  it("open the file the session's tools touched, not a lookup in the open project", () => {
    const path = "/Users/me/agents/sales/notes/NOTES.md";
    const onOpenFile = vi.fn();
    const blocks: Block[] = [
      { id: "user", role: "user", text: "Update the notebook" },
      {
        id: "read",
        role: "tool",
        text: "Read NOTES.md",
        tool: {
          kind: "read",
          status: "completed",
          preview: { kind: "read", path },
        },
      },
      {
        // The shell recorded only the name it was given.
        id: "shell",
        role: "tool",
        text: "sed -i NOTES.md",
        tool: {
          kind: "shell",
          status: "completed",
          preview: { kind: "write", path: "NOTES.md" },
        },
      },
      { id: "answer", role: "assistant", text: "Updated `NOTES.md`." },
    ];
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks,
          cwd: "/Users/me",
          onOpenFile,
        }),
      ),
    );

    const link = container.querySelector<HTMLElement>('code[role="link"]');
    expect(link).not.toBeNull();
    act(() => link!.click());
    expect(onOpenFile).toHaveBeenCalledWith(path, undefined, { exact: true });
  });
});
