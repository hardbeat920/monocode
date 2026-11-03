// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import {
  removeProviderAccount,
  saveProviderAccount,
  selectProviderAccount,
} from "../../providers/model/providerAccounts";
import type { Session } from "../../sessions/model/session";
import {
  mobileAccountChoice,
  mobileProviderAccounts,
  mobileSessionAccountId,
} from "./mobileProviderAccounts";

beforeEach(() => {
  localStorage.clear();
  saveProviderAccount({
    id: "account-work",
    provider: "claude",
    label: "Work",
  });
  saveProviderAccount({
    id: "account-personal",
    provider: "codex",
    label: "Personal",
  });
});

describe("mobile provider accounts", () => {
  it("lists named desktop profiles and their project selections", () => {
    selectProviderAccount("claude", "/project", "account-work");
    expect(mobileProviderAccounts("/project")).toEqual({
      accounts: [
        {
          id: "default",
          provider: "claude",
          label: "Default account",
          isDefault: true,
        },
        { id: "account-work", provider: "claude", label: "Work" },
        {
          id: "default",
          provider: "codex",
          label: "Default account",
          isDefault: true,
        },
        { id: "account-personal", provider: "codex", label: "Personal" },
      ],
      selectedAccounts: { claude: "account-work", codex: "default" },
    });
    expect(mobileProviderAccounts("/other").selectedAccounts.claude).toBe(
      "default",
    );
  });

  it("pins the account for the final provider, including its project default", () => {
    selectProviderAccount("codex", "/project", "account-personal");
    expect(mobileAccountChoice("codex", "/project", {})).toBe(
      "account-personal",
    );
    expect(
      mobileAccountChoice("claude", "/project", {
        providerAccountId: "account-work",
      }),
    ).toBe("account-work");
    expect(
      mobileAccountChoice("codex", "/project", {
        providerAccountId: "default",
      }),
    ).toBe("default");
  });

  it("rejects removed profiles, profiles of another provider and malformed IDs", () => {
    for (const providerAccountId of [
      "account-work",
      "missing",
      "../auth",
      null,
      {},
      7,
    ]) {
      expect(() =>
        mobileAccountChoice("codex", "/project", { providerAccountId }),
      ).toThrow("Choose another account");
    }
    removeProviderAccount("claude", "account-work");
    expect(() =>
      mobileAccountChoice("claude", "/project", {
        providerAccountId: "account-work",
      }),
    ).toThrow("no longer available");
    expect(() =>
      mobileAccountChoice("pi", "/project", { providerAccountId: "default" }),
    ).toThrow("does not support");
    expect(mobileAccountChoice("pi", "/project", {})).toBeUndefined();
  });

  it("shows a conversation's pinned account even when it was removed", () => {
    selectProviderAccount("claude", "/project", "account-work");
    const session = {
      harness: "claude",
      cwd: "/project",
      blocks: [],
    } as unknown as Session;
    expect(mobileSessionAccountId(session)).toBe("account-work");
    expect(
      mobileSessionAccountId({ ...session, blocks: [{ role: "user" }] } as Session),
    ).toBe("default");
    expect(
      mobileSessionAccountId({ ...session, providerAccountId: "removed" }),
    ).toBe("removed");
    expect(
      mobileSessionAccountId({ ...session, harness: "pi" }),
    ).toBeUndefined();
  });
});
