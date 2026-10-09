// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  saveProviderAccount,
  selectProviderAccount,
} from "../../../../features/providers/model/providerAccounts";
import {
  hasLiveCatalog,
  resetHarnessModelOverlays,
} from "../../../../features/sessions/model/models";
import { refreshHarnessCatalogs, registerHarness } from "../../core/registry";
import { claudeAdapter } from "./claudeAdapter";

const fixture = vi.hoisted(() => ({
  spawned: [] as unknown[][],
  fail: false,
  listeners: new Map<string, (line: string) => void>(),
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/audit-home",
}));
vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/audit/claude" }),
  spawnChild: async (...args: unknown[]) => {
    if (fixture.fail) throw new Error("spawn failed");
    fixture.spawned.push(args);
  },
  killChild: async () => undefined,
  unwatchChild: (id: string) => fixture.listeners.delete(id),
  watchChild: (id: string, listener: (line: string) => void) =>
    fixture.listeners.set(id, listener),
  writeChild: async (id: string, line: string) => {
    const message = JSON.parse(line);
    const rows =
      message.request.subtype === "list_models"
        ? {
            models: [
              {
                value: "sonnet",
                resolvedModel: "claude-sonnet-5",
                displayName: "Sonnet",
              },
            ],
          }
        : {};
    fixture.listeners.get(id)?.(
      JSON.stringify({
        type: "control_response",
        response: {
          subtype: "success",
          request_id: message.request_id,
          response: rows,
        },
      }),
    );
  },
  writeChildWithTimeout: async () => undefined,
  execChild: async () => {
    if (fixture.fail) throw new Error("version failed");
    return "2.1.287";
  },
}));

beforeEach(() => {
  localStorage.clear();
  fixture.spawned.length = 0;
  fixture.fail = false;
  resetHarnessModelOverlays();
  registerHarness(claudeAdapter);
  saveProviderAccount({
    provider: "claude",
    id: "account-work",
    label: "Work",
  });
  selectProviderAccount("claude", "/audit-project", "account-work");
});
afterEach(resetHarnessModelOverlays);

it("discovers models in the selected project", async () => {
  await refreshHarnessCatalogs(["claude"], { cwd: "/audit-project" });
  expect(fixture.spawned[0][3]).toBe("/audit-project");
});

it("discovers models with the selected account", async () => {
  await refreshHarnessCatalogs(["claude"], { cwd: "/audit-project" });
  expect(fixture.spawned[0][4]).toEqual({
    provider: "claude",
    id: "account-work",
  });
});

it("refreshes the model list when the selected account changes", async () => {
  await refreshHarnessCatalogs(["claude"], { cwd: "/audit-project" });
  selectProviderAccount("claude", "/audit-project", "default");
  await refreshHarnessCatalogs(["claude"], { cwd: "/audit-project" });
  expect(fixture.spawned).toHaveLength(2);
});

it("drops the previous account's models when the new account finds none", async () => {
  await refreshHarnessCatalogs(["claude"], { cwd: "/audit-project" });
  expect(hasLiveCatalog("claude")).toBe(true);
  fixture.fail = true;
  await refreshHarnessCatalogs(["claude"], {
    cwd: "/audit-project",
    providerAccountId: "default",
  });
  expect(hasLiveCatalog("claude")).toBe(false);
});

it("keeps the current account's models when its own refresh fails", async () => {
  await refreshHarnessCatalogs(["claude"], { cwd: "/audit-project" });
  fixture.fail = true;
  await refreshHarnessCatalogs(["claude"], { cwd: "/audit-project" });
  expect(hasLiveCatalog("claude")).toBe(true);
});
