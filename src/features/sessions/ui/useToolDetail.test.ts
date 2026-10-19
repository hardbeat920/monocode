// @vitest-environment happy-dom
import { invoke } from "@tauri-apps/api/core";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import {
  ownStepBlock,
  ToolDetailSession,
  useToolDetail,
  type ToolDetail,
} from "./useToolDetail";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

let root: Root;
let seen: ToolDetail[];

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(invoke).mockReset();
  seen = [];
  root = createRoot(document.createElement("div"));
});

afterEach(() => act(() => root.unmount()));

function Probe({ block, open }: { block: Block; open: boolean }) {
  seen.push(useToolDetail(block, open));
  return null;
}

async function show(block: Block, open: boolean, sessionId = "s1") {
  await act(async () =>
    root.render(
      createElement(
        ToolDetailSession.Provider,
        { value: sessionId },
        createElement(Probe, { block, open }),
      ),
    ),
  );
}

const tool = (id: string, fields: NonNullable<Block["tool"]>): Block => ({
  id,
  role: "tool",
  text: "npm test",
  tool: { kind: "execute", status: "completed", ...fields },
});

it("returns inline output without fetching", async () => {
  await show(tool("inline", { detail: "ok" }), true);
  expect(seen.at(-1)).toEqual({ detail: "ok", omitted: false, pending: false });
  expect(invoke).not.toHaveBeenCalled();
});

it("fetches omitted output only once the row opens", async () => {
  vi.mocked(invoke).mockResolvedValue("full output");
  const block = tool("omitted", { detailOmitted: true });
  await show(block, false);
  expect(seen.at(-1)).toEqual({
    detail: undefined,
    omitted: true,
    pending: false,
  });
  expect(invoke).not.toHaveBeenCalled();

  await show(block, true);
  expect(invoke).toHaveBeenCalledWith("session_tool_detail", {
    sessionId: "s1",
    blockId: "omitted",
    stepId: null,
  });
  expect(seen.at(-1)).toEqual({
    detail: "full output",
    omitted: true,
    pending: false,
  });
  // The fetched output never lands on the block, so saves do not resend it.
  expect(block.tool).toEqual({
    kind: "execute",
    status: "completed",
    detailOmitted: true,
  });
});

it("reads a subagent step's output through its parent block", async () => {
  vi.mocked(invoke).mockResolvedValue("step output");
  const step = ownStepBlock(
    tool("step-1", { detailOmitted: true }),
    "agent-block",
    "step-1",
  );
  await show(step, true);
  expect(invoke).toHaveBeenCalledWith("session_tool_detail", {
    sessionId: "s1",
    blockId: "agent-block",
    stepId: "step-1",
  });
  expect(seen.at(-1)?.detail).toBe("step output");
});

it("says so when stored output is gone", async () => {
  vi.mocked(invoke).mockResolvedValue(null);
  await show(tool("gone", { detailOmitted: true }), true);
  expect(seen.at(-1)?.detail).toBe("Output is no longer available.");
});
