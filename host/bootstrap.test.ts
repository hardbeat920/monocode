import { afterEach, expect, it } from "vitest";
import {
  mkdtempSync,
  readFileSync,
  writeFileSync,
  mkdirSync,
  rmSync,
  existsSync,
} from "node:fs";
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { version } from "../package.json";

const directories: string[] = [];
afterEach(() => {
  for (const dir of directories.splice(0))
    rmSync(dir, { recursive: true, force: true });
});
const quote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;
function fixture(badChecksum = false) {
  const dir = mkdtempSync(join(tmpdir(), "monocode-bootstrap-"));
  directories.push(dir);
  const base = join(dir, "host with spaces ' and $data");
  const source = join(dir, "source");
  const bin = join(dir, "tools");
  mkdirSync(source);
  mkdirSync(bin);
  writeFileSync(
    join(source, "monocode-host"),
    `#!/bin/sh\ncase "$1" in\n--version) printf '%s\\n' ${quote(version)} ;;\nservice) printf 'service installed\\n' >> "$MONOCODE_TEST_EVENTS" ;;\nconnection-info) printf '{"port":3774,"pid":123}\\n' ;;\n*) exit 1 ;;\nesac\n`,
    { mode: 0o755 },
  );
  const archive = join(dir, "host.tar.gz");
  execFileSync("tar", ["-czf", archive, "-C", source, "."]);
  const checksum = badChecksum
    ? "0".repeat(64)
    : createHash("sha256").update(readFileSync(archive)).digest("hex");
  writeFileSync(join(dir, "checksum"), `${checksum}  package.tar.gz\n`);
  // A local download fixture: run the real installer shell, without network or
  // a user service. Never change the test runner's HOME or installed services.
  writeFileSync(
    join(bin, "curl"),
    `#!/bin/sh\nfor arg do previous="$last"; last="$arg"; done\ncase "$previous" in -o) ;; *) exit 1 ;; esac\ncase "$last" in */checksum) cp "$MONOCODE_TEST_CHECKSUM" "$last" ;; *) cp "$MONOCODE_TEST_ARCHIVE" "$last" ;; esac\nprintf 'download\\n' >> "$MONOCODE_TEST_DOWNLOADS"\n`,
    { mode: 0o755 },
  );
  const script = readFileSync("src-tauri/src/remote_bootstrap.sh", "utf8")
    .replace('BASE="$HOME/.monocode-host"', `BASE=${quote(base)}`)
    .replace("@@VERSION@@", quote(version))
    .replace("@@RELEASE@@", "'https://example.invalid/releases'");
  const run = () =>
    new Promise<{ code: number | null; out: string; error: string }>(
      (resolve, reject) => {
        const child = spawn("sh", ["-s"], {
          env: {
            ...process.env,
            PATH: `${bin}:/usr/bin:/bin`,
            MONOCODE_TEST_ARCHIVE: archive,
            MONOCODE_TEST_CHECKSUM: join(dir, "checksum"),
            MONOCODE_TEST_EVENTS: join(dir, "events"),
            MONOCODE_TEST_DOWNLOADS: join(dir, "downloads"),
          },
          signal: AbortSignal.timeout(10_000),
        });
        let out = "";
        let error = "";
        child.stdout.on("data", (data) => {
          out += data;
        });
        child.stderr.on("data", (data) => {
          error += data;
        });
        child.on("error", reject);
        child.on("close", (code) => resolve({ code, out, error }));
        child.stdin.end(script);
      },
    );
  return { dir, base, run };
}
it.skipIf(process.platform === "win32")(
  "installs a verified package, handles unusual home paths, and reuses its host on reconnect",
  async () => {
    const { dir, base, run } = fixture();
    const first = await run();
    expect(first.error).toBe("");
    expect(first.code).toBe(0);
    expect(JSON.parse(first.out).port).toBe(3774);
    expect(existsSync(join(base, "bin/monocode-host"))).toBe(true);
    expect((await run()).code).toBe(0);
    expect(
      readFileSync(join(dir, "downloads"), "utf8").trim().split("\n"),
    ).toHaveLength(2);
  },
);
it.skipIf(process.platform === "win32")(
  "rejects a corrupted package before executing or publishing it",
  async () => {
    const { dir, base, run } = fixture(true);
    const result = await run();
    expect(result.code).not.toBe(0);
    expect(result.error).toContain("checksum mismatch");
    expect(existsSync(join(base, "bin/monocode-host"))).toBe(false);
    expect(existsSync(join(dir, "events"))).toBe(false);
  },
);
