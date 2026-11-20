import { afterEach, expect, it, vi } from "vitest";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  rmSync,
  readFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { execFileSync } from "node:child_process";
import { providerLaunch, resolveProvider } from "./process";
import {
  powershell,
  powershellArgs,
  powershellEnvironment,
  powershellErrorText,
  psQuote,
  protectWindowsDirectory,
  runPowerShell,
  windowsTaskScript,
} from "./windows";

const directories: string[] = [];
afterEach(() => {
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
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

// Exercise Windows discovery on any OS, using the host's real filesystem and
// Node runtime. The .cmd/.bat content must never be interpreted or executed.
it.each(
  ["cmd", "bat"].flatMap((extension) =>
    ["npm-loader.js", "index.js"].map((entryName) => ({
      extension,
      entryName,
    })),
  ),
)(
  "resolves and runs a Windows Copilot .$extension launcher via $entryName with literal arguments",
  async ({ extension, entryName }) => {
    const directory = join(temporary(), "npm with spaces");
    const entry = join(directory, "node_modules/@github/copilot", entryName);
    mkdirSync(dirname(entry), { recursive: true });
    writeFileSync(entry, "console.log(JSON.stringify(process.argv.slice(2)))");
    if (entryName === "npm-loader.js")
      writeFileSync(
        join(dirname(entry), "index.js"),
        'throw new Error("Use the current loader")',
      );
    const candidate = join(directory, `copilot.${extension}`);
    writeFileSync(candidate, "This wrapper must not be executed");

    // A broken earlier installation must not hide the usable npm launcher.
    const brokenDirectory = temporary();
    const brokenCandidate = join(brokenDirectory, `copilot.${extension}`);
    writeFileSync(brokenCandidate, "This wrapper must not be executed");
    await expect(providerLaunch(brokenCandidate, [], "win32")).rejects.toThrow(
      "Missing npm provider entry point",
    );

    vi.stubEnv("PATH", [brokenDirectory, directory].join(delimiter));
    const windowsProcess = Object.create(process);
    Object.defineProperty(windowsProcess, "platform", { value: "win32" });
    vi.stubGlobal("process", windowsProcess);
    expect(await resolveProvider("copilot")).toBe(candidate);

    const args = [
      "--acp",
      "--stdio",
      "path with spaces",
      "a&b",
      "a|b",
      "%USERPROFILE%",
      "$(whoami)",
      'a"b',
      "line\nbreak",
    ];
    const launch = await providerLaunch(candidate, args);
    expect(launch).toEqual({
      command: process.execPath,
      args: [entry, ...args],
    });
    expect(
      JSON.parse(
        execFileSync(launch.command, launch.args, { encoding: "utf8" }),
      ),
    ).toEqual(args);
  },
);

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

it("keeps only the text of a PowerShell error written as CLIXML", () => {
  const privilege =
    "The process does not possess the 'SeSecurityPrivilege' privilege which is required for this operation.";
  expect(
    powershellErrorText(
      `#< CLIXML\r\n<Objs Version="1.1.0.1" xmlns="http://schemas.microsoft.com/powershell/2004/04"><Obj S="progress" RefId="0"><TN RefId="0"><T>System.Management.Automation.PSCustomObject</T></TN><MS><PR N="Record"><AV>Preparing modules for first use.</AV></PR></MS></Obj></Objs>${privilege}\r\n`,
    ),
  ).toBe(privilege);
  expect(powershellErrorText("Access denied.\r\n")).toBe("Access denied.");
});

it.skipIf(process.platform !== "win32")(
  "protects Windows credentials and reuses only this user's unlimited task",
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
    const result = JSON.parse(
      await runPowerShell(`
$script:registeredTask = $null
$script:registrations = 0
$script:starts = 0
function Get-ScheduledTask { param($TaskName, $ErrorAction) return $script:registeredTask }
function Register-ScheduledTask {
  param($TaskName, $Action, $Principal, $Trigger, $Settings, $Description)
  $script:registrations++
  $script:registeredTask = [PSCustomObject]@{ Principal = $Principal }
  @{ user = $Principal.UserId; sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value; logon = $Principal.LogonType.ToString(); limit = $Settings.ExecutionTimeLimit; instances = $Settings.MultipleInstances.ToString(); action = $Action.Arguments } | ConvertTo-Json -Compress | Set-Content -LiteralPath ${psQuote(join(directory, "task.json"))}
}
function Start-ScheduledTask { param($TaskName) $script:starts++ }
${windowsTaskScript(options, process.env.PATH ?? "")}
${windowsTaskScript(options, process.env.PATH ?? "")}
$definition = Get-Content -LiteralPath ${psQuote(join(directory, "task.json"))} -Raw | ConvertFrom-Json
$script:registeredTask = [PSCustomObject]@{ Principal = [PSCustomObject]@{ UserId = 'S-1-5-18' } }
$rejected = $false
try {
  ${windowsTaskScript(options, process.env.PATH ?? "")}
} catch {
  if ($_.Exception.Message -notlike '*different user*') { throw }
  $rejected = $true
}
@{ definition = $definition; registrations = $script:registrations; starts = $script:starts; rejected = $rejected; userSid = if ($definition.user -match '^S-1-') { $definition.user } else { ([Security.Principal.NTAccount]::new($definition.user)).Translate([Security.Principal.SecurityIdentifier]).Value } } | ConvertTo-Json -Compress
`),
    );
    expect(result.userSid).toBe(result.definition.sid);
    expect(result.registrations).toBe(1);
    expect(result.starts).toBe(2);
    expect(result.rejected).toBe(true);
    expect(["Interactive", "3"]).toContain(result.definition.logon);
    expect(result.definition.limit).toBe("PT0S");
    expect(["IgnoreNew", "2"]).toContain(result.definition.instances);
    expect(result.definition.action).toContain("-EncodedCommand");
  },
  // Three sequential PowerShell calls each have their own 30s process timeout.
  // Leave room for all three to settle before test teardown removes their files.
  100_000,
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
