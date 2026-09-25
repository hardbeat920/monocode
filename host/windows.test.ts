import { afterEach, expect, it } from "vitest";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  rmSync,
  readFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { providerLaunch } from "./process";
import {
  powershell,
  powershellArgs,
  powershellEnvironment,
  psQuote,
  protectWindowsDirectory,
  runPowerShell,
  windowsTaskScript,
} from "./windows";

const directories: string[] = [];
afterEach(() => {
  for (const dir of directories.splice(0))
    rmSync(dir, { recursive: true, force: true });
});
const temporary = () => {
  const dir = mkdtempSync(join(tmpdir(), "monocode-win-test-"));
  directories.push(dir);
  return dir;
};

it("runs npm provider entry points directly with literal arguments and bundled Node", async () => {
  const directory = temporary();
  const entry = join(directory, "node_modules/@openai/codex/bin/codex.js");
  mkdirSync(join(directory, "node_modules/@openai/codex/bin"), {
    recursive: true,
  });
  writeFileSync(entry, "console.log(JSON.stringify(process.argv.slice(2)))");
  const args = [
    "path with spaces",
    "a&b",
    "a|b",
    "%USERPROFILE%",
    "$(whoami)",
    'a"b',
    "line\nbreak",
  ];
  const launch = await providerLaunch(
    join(directory, "codex.cmd"),
    args,
    "win32",
  );
  expect(launch.command).toBe(process.execPath);
  expect(
    JSON.parse(execFileSync(launch.command, launch.args, { encoding: "utf8" })),
  ).toEqual(args);
  await expect(
    providerLaunch(join(directory, "unknown.cmd"), args, "win32"),
  ).rejects.toThrow("Unsupported");
});

it("uses an unlimited, unelevated per-user task and preserves literal paths", () => {
  const options = {
    directory: "C:\\Users\\Nick's $PC\\.monocode-host",
    executable: "C:\\Runtime\\node.exe",
    entry: "C:\\Runtime\\host.mjs",
    port: 3774,
  };
  const script = windowsTaskScript(options, "C:\\bin;C:\\User's tools");
  expect(script).toContain("-LogonType Interactive -RunLevel Limited");
  expect(script).toContain("-ExecutionTimeLimit ([TimeSpan]::Zero)");
  expect(script).toContain("-MultipleInstances IgnoreNew");
  expect(script).not.toMatch(
    /Stop-ScheduledTask|Unregister-ScheduledTask|S4U|RunLevel Highest/,
  );
  expect(psQuote("Nick's $PC")).toBe("'Nick''s $PC'");
  expect(
    Buffer.from(powershellArgs("$x = '日本語'").at(-1)!, "base64").toString(
      "utf16le",
    ),
  ).toBe("$x = '日本語'");
});

it.skipIf(process.platform !== "win32")(
  "protects Windows credentials and renders a task with the expected identity and lifetime",
  async () => {
    const directory = temporary();
    await protectWindowsDirectory(directory);
    writeFileSync(join(directory, "credential.json"), "secret");
    const acl = JSON.parse(
      await runPowerShell(`
$acl = Get-Acl -LiteralPath ${psQuote(directory)}
$file = Get-Acl -LiteralPath ${psQuote(join(directory, "credential.json"))}
@{ protected = $acl.AreAccessRulesProtected; owner = $acl.Owner; rules = @($file.Access | ForEach-Object { $_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value }) } | ConvertTo-Json -Compress
`),
    );
    expect(acl.protected).toBe(true);
    expect(acl.rules).not.toContain("S-1-1-0");
    expect(acl.rules).not.toContain("S-1-5-32-545");
    // Use the real ScheduledTasks constructors but never register a task in the
    // developer's account. Capture the resulting definition for assertions.
    const options = {
      directory,
      port: 3774,
      executable: process.execPath,
      entry: join(directory, "host.mjs"),
    };
    const definition = JSON.parse(
      await runPowerShell(`
function Get-ScheduledTask { param($TaskName, $ErrorAction) return $null }
function Register-ScheduledTask {
  param($TaskName, $Action, $Principal, $Trigger, $Settings, $Description)
  @{ user = $Principal.UserId; logon = $Principal.LogonType.ToString(); limit = $Settings.ExecutionTimeLimit; instances = $Settings.MultipleInstances.ToString(); action = $Action.Arguments } | ConvertTo-Json -Compress | Set-Content -LiteralPath ${psQuote(join(directory, "task.json"))}
}
function Start-ScheduledTask { param($TaskName) }
${windowsTaskScript(options, process.env.PATH ?? "")}
Get-Content -LiteralPath ${psQuote(join(directory, "task.json"))} -Raw
`),
    );
    expect(definition.user).toMatch(/^S-1-5-/);
    expect(["Interactive", "3"]).toContain(definition.logon);
    expect(definition.limit).toBe("PT0S");
    expect(["IgnoreNew", "2"]).toContain(definition.instances);
    expect(definition.action).toContain("-EncodedCommand");
  },
  30_000,
);

it.skipIf(process.platform !== "win32")(
  "parses the Windows bootstrap with Windows PowerShell",
  () => {
    const script = readFileSync("src-tauri/src/remote_bootstrap.ps1", "utf8")
      .replace("@@VERSION@@", "'test'")
      .replace("@@RELEASE@@", "'https://example.invalid'")
      .replace("@@ACL@@", readFileSync("host/windows-acl.ps1", "utf8"));
    const file = join(temporary(), "bootstrap.ps1");
    writeFileSync(file, script);
    const result = execFileSync(
      powershell(),
      powershellArgs(
        `$tokens = $null; $errors = $null; $null = [Management.Automation.Language.Parser]::ParseFile(${psQuote(file)}, [ref] $tokens, [ref] $errors); if ($errors.Count) { $errors | Out-String | Write-Output; exit 1 }`,
      ),
      { encoding: "utf8", env: powershellEnvironment() },
    );
    expect(result).toBe("");
  },
);
