param([string]$Executable)
$ErrorActionPreference = 'Stop'
$taskRoot = Join-Path $env:LOCALAPPDATA 'monocode-gpui-build'
$exe = if ($Executable) { $Executable } else { Join-Path $taskRoot 'source\target\debug\monocode-app.exe' }
$bytes = [System.IO.File]::ReadAllBytes($exe)
$pe = [BitConverter]::ToInt32($bytes, 60)
$subsystem = [BitConverter]::ToUInt16($bytes, $pe + 24 + 68)
if ($subsystem -ne 2) { throw "Expected Windows GUI subsystem 2, got $subsystem" }
foreach ($arguments in @('--list-views', 'host --version')) {
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $exe
    $info.Arguments = $arguments
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $info
    try {
        $process.Start() | Out-Null
        $null = $process.Handle
        if (!$process.WaitForExit(10000)) { $process.Kill(); throw "The GUI-subsystem CLI did not exit for $arguments" }
        $output = $process.StandardOutput.ReadToEnd()
        $errors = $process.StandardError.ReadToEnd()
        if ($process.ExitCode -ne 0) { throw "GUI-subsystem CLI failed for $arguments. $errors" }
        if ($arguments -eq '--list-views' -and $output -notmatch 'shell') { throw 'Redirected GUI CLI output did not list the shell' }
        if ($arguments -eq 'host --version' -and $output.Trim() -ne '0.6.0') { throw 'Redirected GUI host CLI output did not report the version' }
        Write-Output "test windows_gui_subsystem_redirected_cli $arguments ... ok"
    } finally {
        if (!$process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
}
Write-Output "PE subsystem $subsystem"
Write-Output "GUI app SHA256 $((Get-FileHash $exe -Algorithm SHA256).Hash.ToLowerInvariant())"
