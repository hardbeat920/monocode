// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import {
  saveHarnessRuntime,
  type HarnessRuntimeSettings,
} from "../../../features/settings/model/settings";
import {
  harnessRuntimeBinaryPath,
  harnessRuntimeEnv,
  harnessRuntimeExtraArgs,
  resolveHarnessBinary,
} from "./runtime";

beforeEach(() => {
  localStorage.clear();
});

function runtime(patch: Partial<HarnessRuntimeSettings>): HarnessRuntimeSettings {
  return { binaryPath: "", launchArgs: "", env: [], ...patch };
}

describe("harnessRuntimeBinaryPath", () => {
  it("trims the override and returns empty when unset", () => {
    expect(harnessRuntimeBinaryPath(runtime({ binaryPath: "  /bin/claude  " }))).toBe(
      "/bin/claude",
    );
    expect(harnessRuntimeBinaryPath(runtime({}))).toBe("");
  });
});

describe("harnessRuntimeExtraArgs", () => {
  it("splits on whitespace and drops empties", () => {
    expect(harnessRuntimeExtraArgs(runtime({ launchArgs: "  --chrome  --foo " }))).toEqual([
      "--chrome",
      "--foo",
    ]);
    expect(harnessRuntimeExtraArgs(runtime({}))).toEqual([]);
  });

  it("keeps a double- or single-quoted value together as one arg", () => {
    expect(
      harnessRuntimeExtraArgs(
        runtime({ launchArgs: `--config "my file.json" --other 'a b'` }),
      ),
    ).toEqual(["--config", "my file.json", "--other", "a b"]);
  });
});

describe("harnessRuntimeEnv", () => {
  it("returns undefined when there are no usable entries", () => {
    expect(harnessRuntimeEnv(runtime({}))).toBeUndefined();
    expect(
      harnessRuntimeEnv(runtime({ env: [{ key: "  ", value: "x" }] })),
    ).toBeUndefined();
  });

  it("builds a record from non-empty keys, trimming the key only", () => {
    expect(
      harnessRuntimeEnv(
        runtime({
          env: [
            { key: " FOO ", value: " bar " },
            { key: "", value: "ignored" },
          ],
        }),
      ),
    ).toEqual({ FOO: " bar " });
  });

  it("drops a key that isn't a valid env var name", () => {
    expect(
      harnessRuntimeEnv(
        runtime({
          env: [
            { key: "FOO=BAR", value: "typo" },
            { key: "2FOO", value: "leading digit" },
            { key: "GOOD_KEY", value: "kept" },
          ],
        }),
      ),
    ).toEqual({ GOOD_KEY: "kept" });
  });
});

describe("resolveHarnessBinary", () => {
  it("resolves the default when there is no override", async () => {
    const result = await resolveHarnessBinary("claude", async () => ({
      path: "/usr/local/bin/claude",
    }));
    expect(result).toEqual({ path: "/usr/local/bin/claude" });
  });

  it("keeps fields from the default resolver beyond path when overridden", async () => {
    saveHarnessRuntime("antigravity", {
      binaryPath: "/custom/agy",
      launchArgs: "",
      env: [],
    });
    const result = await resolveHarnessBinary("antigravity", async () => ({
      path: "/default/agy",
      args: ["--uid="],
    }));
    expect(result).toEqual({ path: "/custom/agy", args: ["--uid="] });
  });

  it("falls back to the override alone when the default resolver throws", async () => {
    saveHarnessRuntime("antigravity", {
      binaryPath: "/custom/agy",
      launchArgs: "",
      env: [],
    });
    const result = await resolveHarnessBinary("antigravity", async () => {
      throw new Error("not found");
    });
    expect(result).toEqual({ path: "/custom/agy" });
  });

  it("propagates the default resolver's failure when there is no override", async () => {
    await expect(
      resolveHarnessBinary("claude", async () => {
        throw new Error("not found");
      }),
    ).rejects.toThrow("not found");
  });
});
