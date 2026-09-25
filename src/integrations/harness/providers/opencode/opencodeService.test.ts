import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  execChild: vi.fn(),
}));

vi.mock("../../core/child", () => ({
  execChild: mocks.execChild,
}));

import {
  parseOpenCodeServiceUrl,
  resolveOpenCodeV2Service,
  resolveOpenCodeV2ServiceUrl,
} from "./opencodeService";

describe("OpenCode v2 background service", () => {
  beforeEach(() => {
    mocks.execChild.mockReset();
  });

  it("parses only local service URLs", () => {
    expect(parseOpenCodeServiceUrl("http://127.0.0.1:49374\n")).toBe(
      "http://127.0.0.1:49374",
    );
    expect(parseOpenCodeServiceUrl("http://localhost:4096/")).toBe(
      "http://localhost:4096",
    );
    expect(parseOpenCodeServiceUrl("https://example.com:4096")).toBeNull();
  });

  it("uses an existing background service", async () => {
    mocks.execChild.mockResolvedValue("http://127.0.0.1:49374\n");

    await expect(
      resolveOpenCodeV2ServiceUrl("/opencode", "/repo"),
    ).resolves.toBe("http://127.0.0.1:49374");
    expect(mocks.execChild).toHaveBeenCalledOnce();
    expect(mocks.execChild).toHaveBeenCalledWith(
      "/opencode",
      ["service", "status"],
      "/repo",
    );
  });

  it("starts the service when it is not running", async () => {
    mocks.execChild
      .mockRejectedValueOnce(new Error("not running"))
      .mockResolvedValueOnce("")
      .mockResolvedValueOnce("http://127.0.0.1:49374");

    await expect(
      resolveOpenCodeV2ServiceUrl("/opencode", "/repo"),
    ).resolves.toBe("http://127.0.0.1:49374");
    expect(mocks.execChild.mock.calls.map(([, args]) => args)).toEqual([
      ["service", "status"],
      ["service", "start"],
      ["service", "status"],
    ]);
  });

  it("loads the service password for authenticated API requests", async () => {
    mocks.execChild
      .mockResolvedValueOnce("http://127.0.0.1:49374\n")
      .mockResolvedValueOnce("secret\n");

    await expect(
      resolveOpenCodeV2Service("/opencode", "/repo"),
    ).resolves.toEqual({
      url: "http://127.0.0.1:49374",
      password: "secret",
    });
    expect(mocks.execChild.mock.calls.map(([, args]) => args)).toEqual([
      ["service", "status"],
      ["service", "get", "password"],
    ]);
  });
});
