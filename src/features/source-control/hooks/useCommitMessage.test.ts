// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../platform/tauri/fs", () => ({
  gitCommitMessage: vi.fn(),
}));

import { gitCommitMessage } from "../../../platform/tauri/fs";
import type { CommitMessage } from "../model/commitMessage";
import { useCommitMessage } from "./useCommitMessage";

const mockMessage = vi.mocked(gitCommitMessage);

let container: HTMLDivElement;
let root: Root;
let alive = false;
let observed: CommitMessage | null | undefined;

function Probe({ cwd, sha }: { cwd: string; sha: string }) {
  observed = useCommitMessage(cwd, sha);
  return null;
}

function render(cwd: string, sha: string) {
  act(() => root.render(createElement(Probe, { cwd, sha })));
}

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  mockMessage.mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  alive = true;
  observed = undefined;
});

afterEach(() => {
  if (alive) act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("useCommitMessage", () => {
  it("loads and reports the message", async () => {
    mockMessage.mockResolvedValue("Subject\n\nBody");
    render("/repo-hook-load", "sha");
    expect(observed).toBeUndefined();

    await flush();
    expect(observed).toEqual({
      raw: "Subject\n\nBody",
      subject: "Subject",
      description: "Body",
    });
  });

  it("reports null when Git cannot report the commit", async () => {
    mockMessage.mockRejectedValue(new Error("git exploded"));
    render("/repo-hook-error", "sha");

    await flush();
    expect(observed).toBeNull();
  });

  it("does not call Git without a project folder", async () => {
    render("~", "sha");
    await flush();
    expect(mockMessage).not.toHaveBeenCalled();
    expect(observed).toBeUndefined();
  });
});
