$ErrorActionPreference = 'Stop'
$taskRoot = Join-Path $env:LOCALAPPDATA 'monocode-gpui-build'
$fixture = Get-ChildItem "$taskRoot\source\target\debug\deps" -Filter 'inline_video-*.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (!$fixture) { throw 'The native media fixture executable is missing' }
$owner = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
$nonce = [guid]::NewGuid().ToString('N')
$name = 'MonocodeNativeMediaFixture-' + $nonce
$output = Join-Path $taskRoot ('runtime\media-desktop-' + $nonce)
New-Item $output -ItemType Directory | Out-Null
$wrapper = Join-Path $env:USERPROFILE 'windows-native-media-session.ps1'
$arguments = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + $wrapper + '" -Fixture "' + $fixture.FullName + '" -OutputDirectory "' + $output + '"'
$action = New-ScheduledTaskAction -Execute (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -Argument $arguments
$principal = New-ScheduledTaskPrincipal -UserId $owner -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 60) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
$definition = New-ScheduledTask -Action $action -Principal $principal -Settings $settings -Description 'Run the isolated hidden native media fixture once, then remove this task'
$registered = $false
try {
    Register-ScheduledTask -TaskName $name -InputObject $definition | Out-Null
    $registered = $true
    Start-ScheduledTask -TaskName $name
    $deadline = [DateTime]::UtcNow.AddSeconds(35)
    while (!(Test-Path (Join-Path $output 'media.exit'))) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'The desktop media fixture did not complete within 35 seconds' }
        Start-Sleep -Milliseconds 100
    }
    $session = [int](Get-Content (Join-Path $output 'session-id.txt'))
    $code = [int](Get-Content (Join-Path $output 'media.exit'))
    if ($session -eq 0) { throw 'The media fixture must run in the existing desktop session' }
    Copy-Item (Join-Path $output 'media.log') (Join-Path $taskRoot 'native-media-desktop.log')
    Set-Content (Join-Path $taskRoot 'native-media-desktop.exit') $code
    Write-Output "Native media fixture desktop SessionId $session, exit $code"
    Get-Content (Join-Path $output 'media.log') -Tail 20
    if ($code -ne 0) { throw "The native media fixture failed with exit $code" }
} finally {
    if ($registered) {
        $task = Get-ScheduledTask -TaskName $name
        if ($task.State -eq 'Running') { Stop-ScheduledTask -TaskName $name }
        Unregister-ScheduledTask -TaskName $name -Confirm:$false
        if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) { throw 'The temporary fixture task was not removed' }
        Write-Output 'Temporary media fixture task removed'
    }
}
