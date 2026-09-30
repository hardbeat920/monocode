import { afterEach, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join, resolve } from "node:path";

const directories: string[] = [];
afterEach(() => {
  for (const directory of directories.splice(0))
    rmSync(directory, { recursive: true, force: true });
});

// Only stub npm and sleep. Run the real release gate without registry writes.
function gate(scenario: string, token = "", archive = true) {
  const directory = mkdtempSync(join(tmpdir(), "monocode-release-test-"));
  directories.push(directory);
  mkdirSync(join(directory, "bin"));
  mkdirSync(join(directory, "build", "host-npm-package"), { recursive: true });
  if (archive)
    writeFileSync(
      join(directory, "build", "host-npm-package", "monocode-host-0.5.0.tgz"),
      "test",
    );
  writeFileSync(
    join(directory, "bin", "npm"),
    `#!/usr/bin/env bash
set -eu
printf '%s\\n' "$*" >> "$RUNNER_TEMP/calls"
if [[ "$1" == view ]]; then
  if [[ "$SCENARIO" == wrong-version ]]; then echo 0.4.0; exit 0; fi
  if [[ "$SCENARIO" == existing || -f "$RUNNER_TEMP/published" ]]; then
    echo 0.5.0
    exit 0
  fi
  exit 1
fi
if [[ "$1" == publish ]]; then
  if [[ "$SCENARIO" == publish-fails ]]; then exit 1; fi
  if [[ "$SCENARIO" != unavailable ]]; then touch "$RUNNER_TEMP/published"; fi
  exit 0
fi
exit 2
`,
    { mode: 0o700 },
  );
  writeFileSync(
    join(directory, "bin", "sleep"),
    "#!/usr/bin/env bash\nexit 0\n",
    { mode: 0o700 },
  );
  let status = 0;
  try {
    execFileSync("bash", [resolve("scripts/publish-host-npm.sh"), "0.5.0"], {
      cwd: directory,
      env: {
        ...process.env,
        PATH: `${join(directory, "bin")}${delimiter}${process.env.PATH}`,
        RUNNER_TEMP: directory,
        NPM_TOKEN: token,
        SCENARIO: scenario,
      },
      stdio: "pipe",
      timeout: 10_000,
    });
  } catch (error) {
    status = (error as { status: number }).status;
  }
  return { status, calls: readFileSync(join(directory, "calls"), "utf8") };
}

it("gates desktop assets, GitHub release, and updater publication on npm", () => {
  const workflow = readFileSync(".github/workflows/release.yml", "utf8");
  const start = workflow.indexOf(
    "      - name: Publish and verify monocode-host on npm",
  );
  expect(start).toBeGreaterThan(0);
  const step = workflow.slice(
    start,
    workflow.indexOf("      - name:", start + 1),
  );
  expect(step).toContain(
    'bash scripts/publish-host-npm.sh "${GITHUB_REF_NAME#v}"',
  );
  expect(step).not.toMatch(/\bif:|continue-on-error:/);
  for (const name of [
    "Upload updater packages to R2",
    "GitHub Release",
    "Publish updater feed",
  ])
    expect(workflow.indexOf(`      - name: ${name}`)).toBeGreaterThan(start);
});

it.skipIf(process.platform === "win32")(
  "requires a token when the matching npm version is absent",
  () => {
    const result = gate("absent");
    expect(result.status).toBe(1);
    expect(result.calls).not.toContain("publish ");
  },
);

it.skipIf(process.platform === "win32")(
  "allows a resumed release when its npm version is already public",
  () => {
    const result = gate("existing");
    expect(result.status).toBe(0);
    expect(result.calls).not.toContain("publish ");
  },
);

it.skipIf(process.platform === "win32")(
  "publishes and verifies the matching npm version",
  () => {
    const result = gate("absent", "test-token");
    expect(result.status).toBe(0);
    expect(result.calls).toContain(
      "publish build/host-npm-package/monocode-host-0.5.0.tgz --access public --registry https://registry.npmjs.org",
    );
    expect(result.calls.trim().split("\n").at(-1)).toBe(
      "view monocode-host@0.5.0 version --registry https://registry.npmjs.org",
    );
  },
);

it
  .skipIf(process.platform === "win32")
  .each(["publish-fails", "unavailable", "wrong-version"])(
  "blocks desktop publication when npm %s",
  (scenario) => {
    expect(gate(scenario, "test-token").status).toBe(1);
  },
);

it.skipIf(process.platform === "win32")(
  "blocks npm publication without the built tarball",
  () => {
    const result = gate("absent", "test-token", false);
    expect(result.status).toBe(1);
    expect(result.calls).not.toContain("publish ");
  },
);
