param([string]$Executable = '')
$ErrorActionPreference = 'Stop'
$taskRoot = Join-Path $env:LOCALAPPDATA 'monocode-gpui-build'
$exe = if ($Executable) { $Executable } else { Join-Path $taskRoot 'source\target\debug\monocode-host.exe' }
$runtimeRoot = Join-Path $taskRoot 'runtime'
$data = Join-Path $runtimeRoot ('host-smoke-' + [guid]::NewGuid().ToString('N'))
$process = $null
$passed = $false
function Invoke-HostCommand([string[]]$CommandArgs) {
    $ErrorActionPreference = 'Continue'
    $output = & $exe @CommandArgs 2>&1
    if ($LASTEXITCODE -ne 0) { throw "Host command failed with exit $LASTEXITCODE" }
    return ($output -join "`n")
}
try {
    if (!(Test-Path $exe)) { throw 'The isolated native host executable is missing' }
    New-Item $data -ItemType Directory -Force | Out-Null
    Set-Content (Join-Path $data 'network.json') '{"enabled":false,"bind":"127.0.0.1"}'
    $socket = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
    $socket.Start()
    $port = $socket.LocalEndpoint.Port
    $socket.Stop()
    Write-Output (Invoke-HostCommand @('--version'))
    $process = Start-Process -FilePath $exe -ArgumentList @('serve', '--data-dir', ('"' + $data + '"'), '--port', $port) -PassThru -RedirectStandardOutput (Join-Path $data 'stdout.log') -RedirectStandardError (Join-Path $data 'stderr.log')
    $null = $process.Handle
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    while (!(Test-Path (Join-Path $data 'running.json'))) {
        if ($process.HasExited) { throw 'The isolated host exited before startup' }
        if ([DateTime]::UtcNow -gt $deadline) { throw 'The isolated host did not start within 20 seconds' }
        Start-Sleep -Milliseconds 100
    }
    $listeners = @(Get-NetTCPConnection -State Listen -OwningProcess $process.Id -ErrorAction Stop)
    if ($listeners.Count -ne 1 -or $listeners[0].LocalAddress -ne '127.0.0.1' -or $listeners[0].LocalPort -ne $port) { throw 'The host listener must use only the selected loopback address and port' }
    $status = Invoke-HostCommand @('status', '--data-dir', $data)
    if ($status -notmatch 'is running') { throw 'The native host status command did not report a running host' }
    Write-Output $status
    Write-Output (Invoke-HostCommand @('stop', '--data-dir', $data))
    if (!$process.WaitForExit(10000)) { throw 'The native host did not stop within 10 seconds' }
    if ($process.ExitCode -ne 0) { throw "The native host exited with $($process.ExitCode)" }
    if (Test-Path (Join-Path $data 'running.json')) { throw 'The stopped native host retained its running marker' }
    $status = Invoke-HostCommand @('status', '--data-dir', $data)
    if ($status -notmatch 'Host is stopped') { throw 'The status command did not report a clean stop' }
    Write-Output $status
    $socket = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, $port)
    $socket.Start()
    $socket.Stop()
    Write-Output 'test native_host_loopback_start_status_stop_and_port_release ... ok'
    $passed = $true
} finally {
    if ($process -and !$process.HasExited) {
        try { Invoke-HostCommand @('stop', '--data-dir', $data) | Out-Null } catch {}
        if (!$process.WaitForExit(5000)) { $process.Kill(); $process.WaitForExit() }
    }
    if ($passed -and (Test-Path $data)) { Remove-Item $data -Recurse -Force }
}
