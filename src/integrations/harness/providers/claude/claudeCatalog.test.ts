import { beforeEach, describe, expect, it, vi } from "vitest";

type Account = { provider: string; id: string } | undefined;

const spawnedAccounts: Account[] = [];
const handlers = new Map<string, (line: string) => void>();
const accountByProbe = new Map<string, Account>();
let gate: Promise<void> = Promise.resolve();

/** Each account's `list_models` rows; the gateway profile only has aliases. */
const ROWS: Record<string, unknown[]> = {
  default: [
    { value: "claude-opus-4-8", displayName: "Opus 4.8" },
    { value: "claude-sonnet-4-6", displayName: "Sonnet 4.6" },
  ],
  gateway: [
    {
      value: "opus",
      displayName: "Opus",
      resolvedModel: "gateway/claude-opus-5-5",
    },
  ],
  broken: [],
};

vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  execChild: async () => "2.1.0",
  spawnChild: async (
    id: string,
    _path: string,
    _args: string[],
    _cwd: string,
    account: Account,
  ) => {
    spawnedAccounts.push(account);
    accountByProbe.set(id, account);
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (id: string, line: (l: string) => void) => {
    handlers.set(id, line);
  },
  writeChild: async (id: string, line: string) => {
    const request = JSON.parse(line) as {
      request_id: string;
      request: { subtype: string };
    };
    const respond = (response: Record<string, unknown>) =>
      handlers.get(id)?.(
        JSON.stringify({
          type: "control_response",
          response: {
            subtype: "success",
            request_id: request.request_id,
            response,
          },
        }),
      );
    if (request.request.subtype === "initialize") {
      respond({});
      return;
    }
    await gate;
    const accountId = accountByProbe.get(id)?.id ?? "default";
    respond({ models: ROWS[accountId] });
  },
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home/tester",
}));

async function load() {
  vi.resetModules();
  const catalog = await import("./claudeCatalog");
  const models = await import("../../../../features/sessions/model/models");
  return { catalog, models };
}

const ids = (models: { nativeId?: string }[]) => models.map((m) => m.nativeId);

beforeEach(() => {
  spawnedAccounts.length = 0;
  handlers.clear();
  accountByProbe.clear();
  gate = Promise.resolve();
});

describe("claude per-account catalog", () => {
  it("keeps gateway alias rows as bare aliases", async () => {
    const { catalog } = await load();
    expect(ids(catalog.modelsFromClaudeListModels(ROWS.gateway))).toEqual([
      "opus",
    ]);
  });

  it("probes with the selected account and lists that account's models", async () => {
    const { catalog, models } = await load();
    catalog.selectClaudeCatalogAccount("gateway");
    await catalog.refreshClaudeCatalog();
    expect(spawnedAccounts).toEqual([{ provider: "claude", id: "gateway" }]);
    expect(ids(models.modelsFor("claude"))).toEqual(["opus"]);
  });

  it("restores a cached account's list without probing again", async () => {
    const { catalog, models } = await load();
    await catalog.refreshClaudeCatalog();
    catalog.selectClaudeCatalogAccount("gateway");
    await catalog.refreshClaudeCatalog();
    expect(spawnedAccounts).toHaveLength(2);

    catalog.selectClaudeCatalogAccount("default");
    expect(spawnedAccounts).toHaveLength(2);
    expect(ids(models.modelsFor("claude"))).toEqual([
      "claude-opus-4-8",
      "claude-sonnet-4-6",
    ]);
  });

  it("does not let a slow probe for a left account replace the list", async () => {
    const { catalog, models } = await load();
    let release!: () => void;
    gate = new Promise((resolve) => (release = resolve));
    catalog.selectClaudeCatalogAccount("gateway");
    const slow = catalog.refreshClaudeCatalog("gateway");
    await vi.waitFor(() => expect(spawnedAccounts).toHaveLength(1));

    const seen: (string | undefined)[][] = [];
    models.subscribeModels(() => seen.push(ids(models.modelsFor("claude"))));
    catalog.selectClaudeCatalogAccount("default");
    release();
    await slow;
    await vi.waitFor(() => expect(models.hasLiveCatalog("claude")).toBe(true));
    // Aliases while default probes; the gateway's ["opus"] never appears.
    expect(seen).toEqual([
      ["opus", "sonnet", "haiku"],
      ["claude-opus-4-8", "claude-sonnet-4-6"],
    ]);
  });

  it("shows only alias rows for an uncached account while its probe runs", async () => {
    const { catalog, models } = await load();
    await catalog.refreshClaudeCatalog();
    expect(ids(models.modelsFor("claude"))).toContain("claude-opus-4-8");

    let release!: () => void;
    gate = new Promise((resolve) => (release = resolve));
    catalog.selectClaudeCatalogAccount("gateway");
    expect(ids(models.modelsFor("claude"))).toEqual([
      "opus",
      "sonnet",
      "haiku",
    ]);
    release();
    await vi.waitFor(() =>
      expect(ids(models.modelsFor("claude"))).toEqual(["opus"]),
    );
  });

  it("does not cache the version fallback, so the account is probed again", async () => {
    const { catalog } = await load();
    catalog.selectClaudeCatalogAccount("broken");
    await catalog.refreshClaudeCatalog();
    expect(spawnedAccounts).toHaveLength(1);

    catalog.selectClaudeCatalogAccount("default");
    await catalog.refreshClaudeCatalog();
    catalog.selectClaudeCatalogAccount("broken");
    await vi.waitFor(() =>
      expect(spawnedAccounts.filter((a) => a?.id === "broken")).toHaveLength(2),
    );
  });
});
