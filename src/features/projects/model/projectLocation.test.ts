import { beforeEach, describe, expect, it, vi } from "vitest";
import { resolveProjectLocation } from "../../../platform/tauri/fs";
import {
  forgetProjectLocation,
  rememberProjectLocation,
  synchronizeProjectLocation,
} from "./projectLocation";

vi.mock("../../../platform/tauri/fs", () => ({
  resolveProjectLocation: vi.fn(),
}));

function mockLocalStorage() {
  const data = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => data.get(key) ?? null,
      setItem: (key: string, value: string) => data.set(key, value),
      removeItem: (key: string) => data.delete(key),
      clear: () => data.clear(),
      key: (index: number) => [...data.keys()][index] ?? null,
      get length() {
        return data.size;
      },
    },
  });
}

beforeEach(() => {
  mockLocalStorage();
  vi.mocked(resolveProjectLocation).mockReset();
});

describe("project location synchronization", () => {
  it("records an identity and uses it to follow a rename", async () => {
    vi.mocked(resolveProjectLocation)
      .mockResolvedValueOnce({ path: "/work/monocode", identity: "unix:1:2" })
      .mockResolvedValueOnce({
        path: "/work/monocode-personal",
        identity: "unix:1:2",
      });

    await rememberProjectLocation("/work/monocode");
    await expect(synchronizeProjectLocation("/work/monocode")).resolves.toEqual(
      {
        path: "/work/monocode-personal",
        identity: "unix:1:2",
        moved: true,
      },
    );
    expect(resolveProjectLocation).toHaveBeenLastCalledWith(
      "/work/monocode",
      "unix:1:2",
    );
  });

  it("cannot guess a rename before an identity has been recorded", async () => {
    vi.mocked(resolveProjectLocation).mockResolvedValueOnce(null);
    await expect(
      synchronizeProjectLocation("/work/missing"),
    ).resolves.toBeNull();
    expect(resolveProjectLocation).toHaveBeenCalledWith(
      "/work/missing",
      undefined,
    );
  });

  it("forgets the saved identity when a project is removed", async () => {
    vi.mocked(resolveProjectLocation)
      .mockResolvedValueOnce({ path: "/work/repo", identity: "unix:1:2" })
      .mockResolvedValueOnce(null);
    await rememberProjectLocation("/work/repo");
    forgetProjectLocation("/work/repo");
    await synchronizeProjectLocation("/work/repo");
    expect(resolveProjectLocation).toHaveBeenLastCalledWith(
      "/work/repo",
      undefined,
    );
  });
});
