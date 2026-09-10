// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { announceLocalPreview } from "../lib/browserPreview";
import { newSession, type Session } from "../lib/session";
import { useLocalPreviews } from "./useLocalPreviews";

describe("new local server output", () => {
  let root: Root;
  let container: HTMLDivElement;
  const onPreview = vi.fn();
  function Harness({ sessions }: { sessions: Session[] }) {
    useLocalPreviews(sessions, onPreview);
    return null;
  }
  async function render(sessions: Session[]) {
    await act(async () => root.render(createElement(Harness, { sessions })));
  }
  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("location", new URL("http://localhost:1420"));
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    onPreview.mockClear();
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  it("does not replay historical output, but recognizes a new streamed shell result", async () => {
    const session = { ...newSession("codex", "/repo"), busy: true };
    await render([session]);
    const block = {
      id: "tool",
      role: "tool" as const,
      text: "",
      tool: {
        status: "in_progress",
        preview: {
          kind: "shell" as const,
          output: "Local: http://localhost:3",
        },
      },
    };
    await render([{ ...session, blocks: [block] }]);
    expect(onPreview).not.toHaveBeenCalled();
    block.tool.preview.output += "000/\n";
    await render([{ ...session, blocks: [{ ...block }] }]);
    expect(onPreview).toHaveBeenCalledExactlyOnceWith(
      { cwd: "/repo", url: "http://localhost:3000/" },
      session.id,
    );
    await render([{ ...session, blocks: [{ ...block }] }]);
    expect(onPreview).toHaveBeenCalledTimes(1);
    const old = {
      ...newSession("codex", "/other"),
      busy: true,
      blocks: [{ ...block, id: "old" }],
    };
    await render([session, old]);
    expect(onPreview).toHaveBeenCalledTimes(1);
  });

  it("recognizes output delivered with turn completion without replaying later history", async () => {
    const session = { ...newSession("codex", "/repo"), busy: true };
    await render([session]);
    const result = {
      id: "done",
      role: "tool" as const,
      text: "",
      tool: {
        status: "completed",
        preview: { kind: "shell" as const, output: "http://localhost:5173/" },
      },
    };
    await render([{ ...session, busy: false, blocks: [result] }]);
    expect(onPreview).toHaveBeenCalledExactlyOnceWith(
      { cwd: "/repo", url: "http://localhost:5173/" },
      session.id,
    );
    await render([
      { ...session, busy: false, blocks: [result, { ...result, id: "old" }] },
    ]);
    expect(onPreview).toHaveBeenCalledTimes(1);
  });

  it("ignores assistant prose and file-read output, and releases the terminal subscription", async () => {
    const session = { ...newSession("codex", "/repo"), busy: true };
    await render([session]);
    await render([
      {
        ...session,
        blocks: [
          { id: "a", role: "assistant", text: "http://localhost:3000/" },
          {
            id: "r",
            role: "tool",
            text: "",
            tool: {
              preview: { kind: "read", output: "http://localhost:3000/" },
            },
          },
        ],
      },
    ]);
    expect(onPreview).not.toHaveBeenCalled();
    announceLocalPreview("/repo", "http://localhost:5000/");
    expect(onPreview).toHaveBeenCalledTimes(1);
    await act(async () => root.unmount());
    announceLocalPreview("/repo", "http://localhost:6000/");
    expect(onPreview).toHaveBeenCalledTimes(1);
    root = createRoot(container);
  });
});
