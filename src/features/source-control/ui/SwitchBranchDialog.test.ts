// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { SwitchBranchDialog } from "./SwitchBranchDialog";
import { generateCommitMessage } from "../../../integrations/harness";

vi.mock("../../../integrations/harness", () => ({
  generateCommitMessage: vi.fn(),
}));

afterEach(() => {
  vi.mocked(generateCommitMessage).mockReset();
  vi.unstubAllGlobals();
});

it("lets the switch dialog cancel generation and ignores its late result", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  let resolveGeneration!: (message: string) => void;
  vi.mocked(generateCommitMessage).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        resolveGeneration = resolve;
      }),
  );
  const mount = document.createElement("div");
  document.body.append(mount);
  const root = createRoot(mount);
  const onCancel = vi.fn();
  try {
    await act(async () =>
      root.render(
        createElement(SwitchBranchDialog, {
          cwd: "/repo",
          branch: "other",
          busy: null,
          onStash: vi.fn(),
          onCommit: vi.fn(),
          onCancel,
        }),
      ),
    );

    await act(async () => {
      document
        .querySelector<HTMLButtonElement>(
          '[aria-label="Generate commit message"]',
        )!
        .click();
    });
    const signal = vi.mocked(generateCommitMessage).mock.calls[0]?.[2];
    expect(signal?.aborted).toBe(false);

    await act(async () => {
      document
        .querySelector<HTMLButtonElement>(
          '[aria-label="Cancel commit message generation"]',
        )!
        .click();
    });
    expect(signal?.aborted).toBe(true);
    expect(
      document.querySelector<HTMLButtonElement>(
        '[aria-label="Generate commit message"]',
      )?.disabled,
    ).toBe(false);

    await act(async () => resolveGeneration("Late message"));
    expect(document.querySelector("textarea")?.value).toBe("");
    expect(onCancel).not.toHaveBeenCalled();
  } finally {
    act(() => root.unmount());
    mount.remove();
  }
});
