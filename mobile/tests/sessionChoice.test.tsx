import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  useSessionChoice,
  useForgetChoiceOnLeave,
} from "@/features/models/sessionChoice";
import type { ModelPick } from "@/features/models/types";
import type { Project } from "@/features/workspace/types";

const desktop = vi.hoisted(() => ({
  fingerprint: "desktop",
  configure: vi.fn(),
}));
vi.mock("react-native", () => ({ Alert: { alert: vi.fn() } }));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({
    pairing: { desktop: { fingerprint: desktop.fingerprint } },
  }),
}));
vi.mock("@/features/settings/defaultModel", () => ({
  useDefaultModel: () => null,
  useDefaultRuntimeMode: () => null,
}));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({
    sessions: [{ id: "existing", runtimeMode: "full-access" }],
    configureSession: desktop.configure,
  }),
}));

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;
const project = { id: "project", defaultRuntimeMode: "supervised" } as Project;
const pick: ModelPick = {
  harness: "codex",
  model: "codex:gpt",
  modelSettings: { effort: "high" },
};

describe("mobile account choices", () => {
  let root: ReturnType<typeof createRoot>;
  let draft: ReturnType<typeof useSessionChoice>;
  let existing: ReturnType<typeof useSessionChoice>;
  function Probe() {
    draft = useSessionChoice(project, undefined);
    existing = useSessionChoice(project, "existing");
    useForgetChoiceOnLeave(project.id, undefined);
    useForgetChoiceOnLeave(project.id, "existing");
    return null;
  }
  beforeEach(() => {
    vi.clearAllMocks();
    desktop.fingerprint = "desktop";
    root = createRoot(document.createElement("div"));
    act(() => root.render(<Probe />));
  });
  afterEach(() => act(() => root.unmount()));

  it("keeps choices separate for each provider and sends no configure command", () => {
    act(() => draft.pickAccount(pick, "GPT", "account-codex"));
    act(() =>
      draft.pickAccount(
        { ...pick, harness: "claude" },
        "Claude",
        "account-claude",
      ),
    );
    expect(draft.accounts).toEqual({
      codex: "account-codex",
      claude: "account-claude",
    });
    expect(existing.accounts).toEqual({});
    expect(desktop.configure).not.toHaveBeenCalled();
  });

  it("switches an existing chat by preparing a fresh draft with its model and permissions", () => {
    act(() => existing.pickAccount(pick, "GPT", "account-work"));
    expect(draft.chosen).toEqual({ pick, name: "GPT" });
    expect(draft.startAccess).toBe("full-access");
    expect(draft.accounts.codex).toBe("account-work");
    expect(existing.chosen).toBeNull();
    expect(existing.accounts).toEqual({});
    expect(desktop.configure).not.toHaveBeenCalled();
  });

  it("does not carry account IDs to a different desktop", () => {
    act(() => draft.pickAccount(pick, "GPT", "account-work"));
    desktop.fingerprint = "another-desktop";
    act(() => root.render(<Probe />));
    expect(draft.accounts).toEqual({});
    expect(draft.chosen).toBeNull();
  });
});
