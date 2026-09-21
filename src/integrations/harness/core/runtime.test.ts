import { describe, expect, it } from "vitest";
import type { HarnessRuntimeSettings } from "../../../features/settings/model/settings";
import {
  harnessRuntimeBinaryPath,
  harnessRuntimeEnv,
  harnessRuntimeExtraArgs,
} from "./runtime";

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
});
