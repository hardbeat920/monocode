//! Port of host/windows.ts: PowerShell helpers, the data directory ACL, and
//! the per-user Task Scheduler task that runs the host on Windows.

use std::ffi::OsString;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use base64::Engine as _;

use super::exec::{ExecOptions, exec};
use super::runtime::HostProgram;

const ACL_SCRIPT: &str = include_str!("windows-acl.ps1");

pub fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub fn powershell_args(script: &str) -> Vec<String> {
    let utf16: Vec<u8> = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    vec![
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-EncodedCommand".into(),
        base64::engine::general_purpose::STANDARD.encode(utf16),
    ]
}

pub fn powershell() -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    format!("{root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe")
}

// Node may inherit PowerShell 7's module paths. Windows PowerShell 5.1 must
// build its own paths at startup so it can load its compatible system modules.
pub fn powershell_environment(
    env: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    env.into_iter()
        .filter(|(key, _)| !key.to_string_lossy().eq_ignore_ascii_case("PSMODULEPATH"))
        .collect()
}

/// Windows PowerShell writes progress records to a redirected stderr as
/// CLIXML. A message that starts with that marker also makes a parent
/// PowerShell, such as the scheduled task's runner, try to parse it as XML.
pub fn powershell_error_text(stderr: &str) -> String {
    static MARKER: OnceLock<regex::Regex> = OnceLock::new();
    static OBJECTS: OnceLock<regex::Regex> = OnceLock::new();
    let marker =
        MARKER.get_or_init(|| regex::Regex::new(r"#< CLIXML\r?\n?").expect("valid pattern"));
    let objects =
        OBJECTS.get_or_init(|| regex::Regex::new(r"(?s)<Objs .*?</Objs>").expect("valid pattern"));
    let text = marker.replace_all(stderr, "");
    objects.replace_all(&text, "").trim().to_string()
}

pub fn run_powershell(script: &str) -> Result<String, String> {
    // Send script contents through stdin, avoiding Windows' command-line limit
    // when the user's PATH or profile directory is long.
    let args = powershell_args(
        "$ErrorActionPreference = 'Stop'; [Console]::InputEncoding = [Text.UTF8Encoding]::new($false); [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); try { & ([ScriptBlock]::Create([Console]::In.ReadToEnd())) } catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }",
    );
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    exec(
        &powershell(),
        &args,
        ExecOptions {
            env: Some(powershell_environment(std::env::vars_os())),
            timeout: Duration::from_secs(30),
            max_buffer: 128 * 1024,
            stdin: Some(script.as_bytes().to_vec()),
            ..Default::default()
        },
    )
    .map(|output| output.stdout)
    .map_err(|error| {
        let text = powershell_error_text(&error.stderr);
        if text.is_empty() { error.message } else { text }
    })
}

pub fn protect_windows_directory(directory: &Path) -> Result<(), String> {
    run_powershell(&format!(
        "{ACL_SCRIPT}\nProtect-MonoCodeDirectory {}",
        ps_quote(&directory.to_string_lossy())
    ))
    .map(|_| ())
}

/// What a login service runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceOptions {
    pub directory: std::path::PathBuf,
    pub port: u16,
    pub program: HostProgram,
}

/// Task Scheduler owns the process, independently of the SSH session. An
/// interactive token preserves this user's normal network and credential
/// access. S4U cannot access network or encrypted files and is unsuitable
/// for agents.
pub fn windows_task_script(options: &ServiceOptions, path: &str) -> String {
    let arguments = options
        .program
        .serve_args(&options.directory, options.port)
        .iter()
        .map(|arg| ps_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let log = options.directory.join("host.log");
    let runner = format!(
        "$ErrorActionPreference = 'Continue'\n$env:PATH = {}\n& {} {arguments} >> {} 2>&1\nexit $LASTEXITCODE",
        ps_quote(path),
        ps_quote(&options.program.executable.to_string_lossy()),
        ps_quote(&log.to_string_lossy()),
    );
    let runner_path = options.directory.join("service.ps1");
    let launcher = format!(
        "$ErrorActionPreference = 'Stop'; try {{ & ([ScriptBlock]::Create([IO.File]::ReadAllText({}))) }} catch {{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}",
        ps_quote(&runner_path.to_string_lossy())
    );
    let task_args = ["-WindowStyle".to_string(), "Hidden".to_string()]
        .into_iter()
        .chain(powershell_args(&launcher))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        r#"
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$sid = $identity.User.Value
$name = "MonoCode Host-$sid"
$task = Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
if ($null -eq $task) {{
  [IO.File]::WriteAllText({runner_path}, {runner}, [Text.UTF8Encoding]::new($false))
  $action = New-ScheduledTaskAction -Execute {powershell} -Argument {task_args}
  $principal = New-ScheduledTaskPrincipal -UserId $sid -LogonType Interactive -RunLevel Limited
  $trigger = New-ScheduledTaskTrigger -AtLogOn -User $sid
  $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1)
  Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Trigger $trigger -Settings $settings -Description 'MonoCode remote agent host for this user' | Out-Null
}} else {{
  $taskSid = [string] $task.Principal.UserId
  if ($taskSid -notmatch '^S-1-') {{
    $taskSid = ([Security.Principal.NTAccount]::new($taskSid)).Translate([Security.Principal.SecurityIdentifier]).Value
  }}
  if ($taskSid -ne $sid) {{ throw 'The existing MonoCode task belongs to a different user.' }}
}}
Start-ScheduledTask -TaskName $name
"#,
        runner_path = ps_quote(&runner_path.to_string_lossy()),
        runner = ps_quote(&runner),
        powershell = ps_quote(&powershell()),
        task_args = ps_quote(&task_args),
    )
}

/// Unregisters this user's task. A running host is stopped separately
/// through its lifecycle endpoint so active turns are interrupted cleanly.
pub fn windows_uninstall_script() -> String {
    r#"
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$name = "MonoCode Host-$sid"
if ($null -ne (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue)) {
  Unregister-ScheduledTask -TaskName $name -Confirm:$false
}
"#
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_an_unlimited_unelevated_per_user_task_and_preserves_literal_paths() {
        let options = ServiceOptions {
            directory: "C:\\Users\\Nick's $PC\\.monocode-host".into(),
            port: 3774,
            program: HostProgram {
                executable: "C:\\Runtime\\node.exe".into(),
                args: vec!["C:\\Runtime\\host.mjs".into()],
            },
        };
        let script = windows_task_script(&options, "C:\\bin;C:\\User's tools");
        assert!(script.contains("-LogonType Interactive -RunLevel Limited"));
        assert!(script.contains("-ExecutionTimeLimit ([TimeSpan]::Zero)"));
        assert!(script.contains("-MultipleInstances IgnoreNew"));
        for forbidden in [
            "Stop-ScheduledTask",
            "Unregister-ScheduledTask",
            "S4U",
            "RunLevel Highest",
        ] {
            assert!(!script.contains(forbidden), "{forbidden}");
        }
        assert!(script.contains("''C:\\bin;C:\\User''''s tools''"));
        assert!(
            script
                .contains("''serve'' ''--data-dir'' ''C:\\Users\\Nick''''s $PC\\.monocode-host''")
        );
        assert_eq!(ps_quote("Nick's $PC"), "'Nick''s $PC'");
        let encoded = powershell_args("$x = '日本語'").pop().unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "$x = '日本語'");
    }

    #[test]
    fn keeps_only_the_text_of_a_powershell_error_written_as_clixml() {
        let privilege = "The process does not possess the 'SeSecurityPrivilege' privilege which is required for this operation.";
        assert_eq!(
            powershell_error_text(&format!(
                "#< CLIXML\r\n<Objs Version=\"1.1.0.1\" xmlns=\"http://schemas.microsoft.com/powershell/2004/04\"><Obj S=\"progress\" RefId=\"0\"><TN RefId=\"0\"><T>System.Management.Automation.PSCustomObject</T></TN><MS><PR N=\"Record\"><AV>Preparing modules for first use.</AV></PR></MS></Obj></Objs>{privilege}\r\n"
            )),
            privilege
        );
        assert_eq!(
            powershell_error_text("Access denied.\r\n"),
            "Access denied."
        );
    }

    #[test]
    fn drops_powershell_7_module_paths() {
        let env = powershell_environment([
            (OsString::from("PSModulePath"), OsString::from("x")),
            (OsString::from("PATH"), OsString::from("y")),
        ]);
        assert_eq!(env, [(OsString::from("PATH"), OsString::from("y"))]);
        assert!(windows_uninstall_script().contains("\"MonoCode Host-$sid\""));
        assert!(ACL_SCRIPT.contains("Protect-MonoCodeDirectory"));
    }
}
