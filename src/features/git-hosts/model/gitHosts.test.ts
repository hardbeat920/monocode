import { describe, expect, it, vi } from "vitest";
import { signedInProviders } from "./gitHosts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("signedInProviders", () => {
  it("keeps signed-in providers it knows", () => {
    expect(
      signedInProviders([
        { provider: "github", installed: true, authenticated: true },
        { provider: "gitlab", installed: true, authenticated: true },
      ]),
    ).toEqual(["github"]);
    expect(signedInProviders([{ provider: "github", installed: true, authenticated: false }])).toEqual([]);
  });

  it("treats malformed replies from old or odd hosts as signed out", () => {
    for (const reply of [null, undefined, {}, "github", [null, 1, "x"], [{ provider: "github" }]])
      expect(signedInProviders(reply)).toEqual([]);
  });
});
