import { afterAll, beforeAll, expect, it } from "vitest";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { createHash } from "node:crypto";
import {
  powershell,
  powershellArgs,
  powershellEnvironment,
  psQuote,
} from "./windows";
import { version } from "../package.json";

const exec = promisify(execFile);
const windows = process.platform === "win32";
const shell = windows ? powershell() : process.env.MONOCODE_TEST_PWSH;
let directory: string;
let archive: string;
const run = (script: string, env = {}) =>
  exec(shell!, powershellArgs(script), {
    env: powershellEnvironment({ ...process.env, ...env }),
    windowsHide: true,
    timeout: 90_000,
    maxBuffer: 128 * 1024,
  });

beforeAll(async () => {
  if (!shell) return;
  directory = mkdtempSync(join(tmpdir(), "monocode-windows-bootstrap-"));
  const source = join(directory, "source");
  mkdirSync(source);
  copyFileSync(process.execPath, join(source, "node.exe"));
  writeFileSync(
    join(source, "host.mjs"),
    `import { appendFileSync } from 'node:fs';
const action = process.argv[2];
if (action === '--version') console.log(${JSON.stringify(version)});
else if (action === 'service') appendFileSync(process.env.MONOCODE_TEST_EVENTS, 'service\\n');
else if (action === 'connection-info') console.log(JSON.stringify({ port: 3774, pid: 123 }));
else process.exit(1);
`,
  );
  archive = join(directory, "host.zip");
  await run(
    `$ErrorActionPreference = 'Stop'; Compress-Archive -Path ${psQuote(join(source, "*"))} -DestinationPath ${psQuote(archive)} -CompressionLevel Fastest`,
  );
}, 90_000);
afterAll(() => {
  if (directory) rmSync(directory, { recursive: true, force: true });
});

async function install(badChecksum: boolean) {
  const base = join(
    directory,
    badChecksum ? "broken" : "host with spaces ' $ 日本語",
  );
  const checksum = join(directory, badChecksum ? "bad.sha256" : "good.sha256");
  writeFileSync(
    checksum,
    `${badChecksum ? "0".repeat(64) : createHash("sha256").update(readFileSync(archive)).digest("hex")}  host.zip\n`,
  );
  const events = join(directory, badChecksum ? "bad.events" : "events");
  const downloads = join(
    directory,
    badChecksum ? "bad.downloads" : "downloads",
  );
  const overrides = `
function Download-MonoCode([string] $Url, [string] $Destination) {
  $source = if ($Url.EndsWith('.sha256')) { ${psQuote(checksum)} } else { ${psQuote(archive)} }
  Copy-Item -LiteralPath $source -Destination $Destination
  Add-Content -LiteralPath ${psQuote(downloads)} -Value 'download'
}
${
  windows
    ? ""
    : `function Protect-MonoCodeDirectory([string] $Path) { }
function Expand-Archive([string] $LiteralPath, [string] $DestinationPath) {
  Microsoft.PowerShell.Archive\\Expand-Archive -LiteralPath $LiteralPath -DestinationPath $DestinationPath
  & chmod +x (Join-Path $DestinationPath 'node.exe')
}`
}
`;
  const script = readFileSync("src-tauri/src/remote_bootstrap.ps1", "utf8")
    .replace(
      "$base = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.monocode-host'",
      `$base = ${psQuote(base)}`,
    )
    .replace("@@VERSION@@", psQuote(version))
    .replace("@@RELEASE@@", "'https://example.invalid'")
    .replace("@@ACL@@", readFileSync("host/windows-acl.ps1", "utf8"))
    .replace(
      "New-Item -ItemType Directory -Force -Path $base | Out-Null",
      overrides +
        "\nNew-Item -ItemType Directory -Force -Path $base | Out-Null",
    );
  const file = join(directory, badChecksum ? "bad.ps1" : "install.ps1");
  writeFileSync(file, script);
  const launch = `try { & ([ScriptBlock]::Create([IO.File]::ReadAllText(${psQuote(file)}))) } catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }`;
  return {
    base,
    downloads,
    events,
    run: () =>
      run(launch, {
        PROCESSOR_ARCHITECTURE: "AMD64",
        MONOCODE_TEST_EVENTS: events,
      }),
  };
}

it.skipIf(!shell)(
  "runs the PowerShell installer, verifies its package, and reuses the installed runtime",
  async () => {
    const fixture = await install(false);
    const result = await fixture.run();
    expect(JSON.parse(result.stdout)).toEqual({ port: 3774, pid: 123 });
    expect(existsSync(join(fixture.base, "bin", "monocode-host.cmd"))).toBe(
      true,
    );
    const pointer = readFileSync(join(fixture.base, "runtime-path"), "utf8");
    expect(JSON.parse((await fixture.run()).stdout).port).toBe(3774);
    expect(readFileSync(join(fixture.base, "runtime-path"), "utf8")).toBe(
      pointer,
    );
    expect(
      readFileSync(fixture.downloads, "utf8").trim().split(/\r?\n/),
    ).toHaveLength(2);
    if (windows) {
      const launcher = join(fixture.base, "bin", "monocode-host.cmd");
      const versionResult = await run(
        `& ${psQuote(launcher)} --version; if ($LASTEXITCODE -ne 0) { exit 1 }`,
      );
      expect(versionResult.stdout.trim()).toBe(version);
    }
  },
  90_000,
);

it.skipIf(!shell)(
  "rejects a bad Windows package before publishing or executing it",
  async () => {
    const fixture = await install(true);
    await expect(fixture.run()).rejects.toThrow("checksum mismatch");
    expect(existsSync(join(fixture.base, "runtime-path"))).toBe(false);
    expect(existsSync(fixture.events)).toBe(false);
  },
  90_000,
);
