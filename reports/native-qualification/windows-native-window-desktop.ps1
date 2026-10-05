param([string]$Executable)
$ErrorActionPreference = 'Stop'
$taskRoot = Join-Path $env:LOCALAPPDATA 'monocode-gpui-build'
$exe = if ($Executable) { $Executable } else { Join-Path $taskRoot 'source\target\debug\monocode-app.exe' }
$owner = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
$nonce = [guid]::NewGuid().ToString('N')
$name = 'MonocodeNativeWindowFixture-' + $nonce
$output = Join-Path $taskRoot ('runtime\window-desktop-' + $nonce)
New-Item $output -ItemType Directory | Out-Null
$wrapper = Join-Path $env:USERPROFILE 'windows-native-window-session.ps1'
$arguments = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + $wrapper + '" -Executable "' + $exe + '" -OutputDirectory "' + $output + '"'
$action = New-ScheduledTaskAction -Execute (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -Argument $arguments
$principal = New-ScheduledTaskPrincipal -UserId $owner -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 60) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
$definition = New-ScheduledTask -Action $action -Principal $principal -Settings $settings -Description 'Capture only the native app fixture window once, then remove this task'
$registered = $false
try {
    Register-ScheduledTask -TaskName $name -InputObject $definition | Out-Null
    $registered = $true
    Start-ScheduledTask -TaskName $name
    $deadline = [DateTime]::UtcNow.AddSeconds(40)
    while (!(Test-Path (Join-Path $output 'window.exit'))) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'The owned native window fixture did not complete within 40 seconds' }
        Start-Sleep -Milliseconds 100
    }
    $code = [int](Get-Content (Join-Path $output 'window.exit'))
    Write-Output "Window artifacts $output"
    if ($code -ne 0) {
        Get-Content (Join-Path $output 'window-error.log') -ErrorAction SilentlyContinue
        Get-Content (Join-Path $output 'app.stderr.log') -Tail 35 -ErrorAction SilentlyContinue
        throw "The owned native window fixture failed with exit $code"
    }
    Get-Content (Join-Path $output 'window.log')
    Copy-Item (Join-Path $output 'widgets.png') "$env:USERPROFILE\windows-native-widgets.png"
    Copy-Item (Join-Path $output 'window.log') "$env:USERPROFILE\windows-native-window.log"
    Set-Content (Join-Path $taskRoot 'native-window-artifact-path.txt') $output
} finally {
    if (Test-Path (Join-Path $output 'owned-app-pid.txt')) {
        $appId = [int](Get-Content (Join-Path $output 'owned-app-pid.txt'))
        $started = [long](Get-Content (Join-Path $output 'owned-app-start-ticks.txt'))
        $app = Get-Process -Id $appId -ErrorAction SilentlyContinue
        if ($app -and $app.Path -eq $exe -and $app.StartTime.ToUniversalTime().Ticks -eq $started) {
            Stop-Process -Id $appId
            Wait-Process -Id $appId -Timeout 5 -ErrorAction SilentlyContinue
        }
    }
    if ($registered) {
        $task = Get-ScheduledTask -TaskName $name
        if ($task.State -eq 'Running') { Stop-ScheduledTask -TaskName $name }
        Unregister-ScheduledTask -TaskName $name -Confirm:$false
        if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) { throw 'The temporary native window task was not removed' }
        Write-Output 'Temporary native window task removed'
    }
}
