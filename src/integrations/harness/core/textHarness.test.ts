import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessId } from "../../../features/sessions/model/session";

const installed = new Set<HarnessId>();

vi.mock("./availability", () => ({
  isHarnessAvailable: (id: HarnessId) => installed.has(id),
}));
vi.mock("./registry", () => ({
  generateHarnessCommitMessage: vi.fn(async () => "generated"),
  generateHarnessPrContent: vi.fn(async () => null),
  warmupHarnessText: vi.fn(async () => {}),
}));

const { savePickerProviderVisible } =
  await import("../../../features/sessions/model/models");
const { setProjectProviderHidden } =
  await import("../../../features/sessions/model/projectProviders");
const {
  generateHarnessCommitMessage,
  generateHarnessPrContent,
  warmupHarnessText,
} = await import("./registry");
const {
  generateCommitMessage,
  generatePrContent,
  pickTextHarness,
  warmupText,
} = await import("./textHarness");

function stubLocalStorage() {
  const data = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => data.clear(),
  });
}

describe("pickTextHarness", () => {
  beforeEach(() => {
    installed.clear();
    stubLocalStorage();
  });

  afterEach(() => {
    vi.clearAllMocks();
    vi.unstubAllGlobals();
  });

  it("uses the active provider when it is installed", () => {
    installed.add("codex").add("cursor");
    expect(pickTextHarness("codex")).toBe("codex");
  });

  it("keeps the fallback order when nothing is hidden", () => {
    installed.add("cursor").add("codex");
    expect(pickTextHarness("pi")).toBe("cursor");
  });

  // Regression: a hidden Cursor was still chosen for background titles,
  // commit messages, and branch names, and its text backend opened the
  // Cursor login page.
  it("skips an installed provider hidden from the picker", () => {
    installed.add("cursor").add("codex");
    savePickerProviderVisible("cursor", false);
    expect(pickTextHarness("pi")).toBe("codex");
    expect(pickTextHarness()).toBe("codex");
  });

  it("still honours the active provider even if it is hidden", () => {
    installed.add("cursor").add("codex");
    savePickerProviderVisible("cursor", false);
    expect(pickTextHarness("cursor")).toBe("cursor");
  });

  it("returns no provider when the only installed one is hidden", () => {
    installed.add("cursor");
    savePickerProviderVisible("cursor", false);
    expect(pickTextHarness()).toBeNull();
    expect(pickTextHarness("pi")).toBeNull();
  });

  it("does not return an uninstalled preferred provider", () => {
    expect(pickTextHarness("codex")).toBeNull();
  });

  it("honours the project's hidden providers for fallbacks", () => {
    installed.add("cursor").add("codex");
    setProjectProviderHidden("/repo", "cursor", true);
    expect(pickTextHarness("pi", "/repo")).toBe("codex");
    expect(pickTextHarness(undefined, "/other")).toBe("cursor");
    expect(pickTextHarness("cursor", "/repo")).toBe("cursor");
  });

  it("does not launch text backends when none is eligible", async () => {
    installed.add("cursor");
    savePickerProviderVisible("cursor", false);
    await expect(generateCommitMessage("/repo")).rejects.toThrow(
      /No text provider is available/,
    );
    await expect(generatePrContent("/repo")).rejects.toThrow(
      /No text provider is available/,
    );
    await expect(warmupText("/repo")).resolves.toBeUndefined();
    expect(generateHarnessCommitMessage).not.toHaveBeenCalled();
    expect(generateHarnessPrContent).not.toHaveBeenCalled();
    expect(warmupHarnessText).not.toHaveBeenCalled();
  });

  it("uses the project root while generating in a worktree", async () => {
    installed.add("cursor").add("codex");
    setProjectProviderHidden("/repo", "cursor", true);
    await expect(
      generateCommitMessage("/worktree", undefined, undefined, "/repo"),
    ).resolves.toBe("generated");
    expect(generateHarnessCommitMessage).toHaveBeenCalledWith(
      "codex",
      "/worktree",
      undefined,
    );
  });
});
