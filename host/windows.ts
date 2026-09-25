import { execFile } from "node:child_process";
import { join } from "node:path";
import aclScript from "./windows-acl.ps1?raw";

export const psQuote = (value: string) => `'${value.replaceAll("'", "''")}'`;
export const powershellArgs = (script: string) => [
  "-NoLogo",
  "-NoProfile",
  "-NonInteractive",
  "-EncodedCommand",
  Buffer.from(script, "utf16le").toString("base64"),
];
export const powershell = () =>
  join(
    process.env.SystemRoot ?? "C:\\Windows",
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );

// Node may inherit PowerShell 7's module paths. Windows PowerShell 5.1 must
// build its own paths at startup so it can load its compatible system modules.
export const powershellEnvironment = (
  env: NodeJS.ProcessEnv = process.env,
): NodeJS.ProcessEnv =>
  Object.fromEntries(
    Object.entries(env).filter(([key]) => key.toUpperCase() !== "PSMODULEPATH"),
  );

export async function runPowerShell(script: string): Promise<string> {
  // Send script contents through stdin, avoiding Windows' command-line limit
  // when the user's PATH or profile directory is long.
  return new Promise((resolve, reject) => {
    const child = execFile(
      powershell(),
      powershellArgs(
        "$ErrorActionPreference = 'Stop'; [Console]::InputEncoding = [Text.UTF8Encoding]::new($false); [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); try { & ([ScriptBlock]::Create([Console]::In.ReadToEnd())) } catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }",
      ),
      {
        env: powershellEnvironment(),
        windowsHide: true,
        timeout: 30_000,
        maxBuffer: 128 * 1024,
      },
      (error, stdout, stderr) => {
        if (error) reject(new Error(stderr.trim() || error.message));
        else resolve(stdout);
      },
    );
    child.stdin!.on("error", () => {
      /* execFile reports an early exit */
    });
    child.stdin!.end(script, "utf8");
  });
}

export async function protectWindowsDirectory(
  directory: string,
): Promise<void> {
  await runPowerShell(
    `${aclScript}\nProtect-MonoCodeDirectory ${psQuote(directory)}`,
  );
}

export type WindowsServiceOptions = {
  directory: string;
  port: number;
  executable: string;
  entry: string;
};

/** Task Scheduler owns the process, independently of the SSH session. An
 * interactive token preserves this user's normal network/credential access.
 * S4U cannot access network or encrypted files and is unsuitable for agents. */
export function windowsTaskScript(
  options: WindowsServiceOptions,
  path: string,
): string {
  const runner = `$ErrorActionPreference = 'Continue'\n$env:PATH = ${psQuote(path)}\n& ${psQuote(options.executable)} ${[options.entry, "serve", "--data-dir", options.directory, "--port", String(options.port)].map(psQuote).join(" ")} >> ${psQuote(join(options.directory, "host.log"))} 2>&1\nexit $LASTEXITCODE`;
  const runnerPath = join(options.directory, "service.ps1");
  const launcher = `$ErrorActionPreference = 'Stop'; try { & ([ScriptBlock]::Create([IO.File]::ReadAllText(${psQuote(runnerPath)}))) } catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }`;
  const taskArgs = ["-WindowStyle", "Hidden", ...powershellArgs(launcher)].join(
    " ",
  );
  return `
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$sid = $identity.User.Value
$name = "MonoCode Host-$sid"
$task = Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
if ($null -eq $task) {
  [IO.File]::WriteAllText(${psQuote(runnerPath)}, ${psQuote(runner)}, [Text.UTF8Encoding]::new($false))
  $action = New-ScheduledTaskAction -Execute ${psQuote(powershell())} -Argument ${psQuote(taskArgs)}
  $principal = New-ScheduledTaskPrincipal -UserId $sid -LogonType Interactive -RunLevel Limited
  $trigger = New-ScheduledTaskTrigger -AtLogOn -User $sid
  $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1)
  Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Trigger $trigger -Settings $settings -Description 'MonoCode remote agent host for this user' | Out-Null
} else {
  $taskSid = [string] $task.Principal.UserId
  if ($taskSid -notmatch '^S-1-') {
    $taskSid = ([Security.Principal.NTAccount]::new($taskSid)).Translate([Security.Principal.SecurityIdentifier]).Value
  }
  if ($taskSid -ne $sid) { throw 'The existing MonoCode task belongs to a different user.' }
}
Start-ScheduledTask -TaskName $name
`;
}
