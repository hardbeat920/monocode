import { describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { isCheckoutBlockedByChanges, listSkills } from "./fs";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("isCheckoutBlockedByChanges", () => {
  it("detects git's tracked-file checkout error", () => {
    expect(
      isCheckoutBlockedByChanges(
        "error: Your local changes to the following files would be overwritten by checkout:\n\ta.txt\nPlease commit your changes or stash them before you switch branches.",
      ),
    ).toBe(true);
  });

  it("detects git's untracked-file checkout error", () => {
    expect(
      isCheckoutBlockedByChanges(
        "error: The following untracked working tree files would be overwritten by checkout:\n\tnew.txt\nPlease move or remove them before you switch branches.",
      ),
    ).toBe(true);
  });

  it("detects the mapped app error", () => {
    expect(
      isCheckoutBlockedByChanges(
        "Your local changes would be overwritten. Commit or stash them first.",
      ),
    ).toBe(true);
  });

  it("ignores unrelated git errors", () => {
    expect(isCheckoutBlockedByChanges("Branch missing not found")).toBe(false);
    expect(isCheckoutBlockedByChanges("Not a git repository")).toBe(false);
  });
});

describe("listSkills", () => {
  it("invokes list_skills with cwd and disabledPaths", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);
    await listSkills("/repo", ["/repo/.agents/skills/review/SKILL.md"]);
    expect(invoke).toHaveBeenCalledWith("list_skills", {
      cwd: "/repo",
      disabledPaths: ["/repo/.agents/skills/review/SKILL.md"],
    });
  });

  it("passes null when disabledPaths is omitted", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);
    await listSkills("/repo");
    expect(invoke).toHaveBeenCalledWith("list_skills", {
      cwd: "/repo",
      disabledPaths: null,
    });
  });
});
