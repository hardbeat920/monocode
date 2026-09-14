import { beforeEach, describe, expect, it, vi } from "vitest";
import { projectKey } from "./paths";
import {
  clearProjectLogo,
  droppableLogoFile,
  pickAndSetProjectLogo,
} from "./projectLogos";
import { loadTabGroupLogos, saveTabGroupLogo } from "./tabGroups";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), open: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
  convertFileSrc: (path: string) => path,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open }));

function mockLocalStorage() {
  const data = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    value: {
      getItem: (key: string) => data.get(key) ?? null,
      setItem: (key: string, value: string) => {
        data.set(key, value);
      },
      removeItem: (key: string) => {
        data.delete(key);
      },
      clear: () => data.clear(),
      key: (index: number) => [...data.keys()][index] ?? null,
      get length() {
        return data.size;
      },
    },
    configurable: true,
  });
  data.set("monocode:tab-group:key-version", "2");
}

/** The logo store announces changes on the window; nothing here listens. */
function mockWindow() {
  Object.defineProperty(globalThis, "window", {
    value: { dispatchEvent: () => true },
    configurable: true,
  });
}

const FINANCE = projectKey("/Users/me/cortex-finance/agentbase");
const CORTEX = projectKey("/Users/me/cortex/agentbase");
// Both migrated projects still point at the one file saved under the old key.
const SHARED = "/logos/agentbase.png";

describe("pickAndSetProjectLogo", () => {
  beforeEach(() => {
    mockLocalStorage();
    mockWindow();
    mocks.invoke.mockReset();
    mocks.open.mockReset();
  });

  it.each([
    ["D:\\Work\\My Project", "d:/work/my project"],
    ["D:\\", "d:"],
    ["/Users/me/My Project", "/Users/me/My Project"],
    ["\\\\server\\share\\My Project", "//server/share/my project"],
  ])(
    "opens in %s and saves the logo under its project key",
    async (directory, key) => {
      const sourcePath = "C:\\Pictures\\logo.png";
      const savedPath = "/logos/saved.png";
      mocks.open.mockResolvedValue(sourcePath);
      mocks.invoke.mockResolvedValue(savedPath);

      const result = await pickAndSetProjectLogo(directory);

      expect(mocks.open).toHaveBeenCalledWith(
        expect.objectContaining({
          defaultPath: directory,
          directory: false,
          multiple: false,
        }),
      );
      expect(mocks.invoke).toHaveBeenCalledWith("save_project_logo", {
        project: key,
        sourcePath,
      });
      expect(result).toBe(savedPath);
      expect(loadTabGroupLogos()[key]).toBe(savedPath);
    },
  );
});

describe("droppableLogoFile", () => {
  it("keeps a file another project still shows", () => {
    const logos = { [FINANCE]: SHARED, [CORTEX]: SHARED };
    expect(droppableLogoFile(logos, FINANCE)).toBeNull();
  });

  it("drops a file only this project points at", () => {
    const logos = { [FINANCE]: SHARED, [CORTEX]: "/logos/other.png" };
    expect(droppableLogoFile(logos, FINANCE)).toBe(SHARED);
  });

  it("keeps the file the project is about to point at", () => {
    const logos = { [FINANCE]: SHARED };
    expect(droppableLogoFile(logos, FINANCE, SHARED)).toBeNull();
  });

  it("has nothing to drop for a project without a logo", () => {
    expect(droppableLogoFile({}, FINANCE)).toBeNull();
  });
});

describe("clearProjectLogo", () => {
  beforeEach(() => {
    mockLocalStorage();
    mockWindow();
    mocks.invoke.mockReset();
    mocks.invoke.mockResolvedValue(undefined);
  });

  it("leaves the shared file on disk for the other project", async () => {
    saveTabGroupLogo(FINANCE, SHARED);
    saveTabGroupLogo(CORTEX, SHARED);

    await clearProjectLogo(FINANCE);

    expect(mocks.invoke).not.toHaveBeenCalledWith(
      "forget_logo_file",
      expect.anything(),
    );
    expect(mocks.invoke).toHaveBeenCalledWith("remove_project_logo", {
      project: FINANCE,
    });
  });

  it("removes the file once the last project lets go of it", async () => {
    saveTabGroupLogo(FINANCE, SHARED);
    saveTabGroupLogo(CORTEX, SHARED);

    await clearProjectLogo(FINANCE);
    await clearProjectLogo(CORTEX);

    expect(mocks.invoke).toHaveBeenCalledWith("forget_logo_file", {
      path: SHARED,
    });
  });
});
