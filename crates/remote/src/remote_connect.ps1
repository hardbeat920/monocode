$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$releaseBase = @@PACKAGE@@
$version = @@VERSION@@
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
$target = "$arch-pc-windows-msvc"
$installed = Join-Path $env:USERPROFILE ".monocode-host\runtime\$version\monocode-host.exe"
$temp = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp | Out-Null
try {
  $program = $installed
  if (-not (Test-Path -LiteralPath $installed) -or ((& $installed --version) -ne $version)) {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $archive = "monocode-host_${version}_${target}.tar.gz"
    $archivePath = Join-Path $temp $archive
    Invoke-WebRequest -UseBasicParsing -Uri "$releaseBase/$archive" -OutFile $archivePath
    $checksums = (Invoke-WebRequest -UseBasicParsing -Uri "$releaseBase/SHA256SUMS").Content
    $lines = @($checksums -split "`n" | Where-Object { ($_ -split '\s+')[1] -eq $archive })
    if ($lines.Count -ne 1) { throw "The release has no unique checksum for $archive." }
    $expected = ($lines[0] -split '\s+')[0]
    $actual = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash
    if ($actual -ne $expected) { throw 'MonoCode Host download checksum did not match.' }
    $entries = @(& tar.exe -tzf $archivePath)
    if ($LASTEXITCODE -ne 0 -or $entries.Count -ne 1 -or $entries[0] -ne 'monocode-host.exe') {
      throw 'The MonoCode Host archive has unexpected files.'
    }
    $details = @(& tar.exe -tvzf $archivePath)
    if ($LASTEXITCODE -ne 0 -or $details.Count -ne 1 -or -not $details[0].StartsWith('-')) {
      throw 'The MonoCode Host archive must contain a regular executable.'
    }
    & tar.exe -xzf $archivePath -C $temp
    if ($LASTEXITCODE -ne 0) { throw 'Could not extract MonoCode Host.' }
    $program = Join-Path $temp 'monocode-host.exe'
    if ((& $program --version) -ne $version) { throw 'The downloaded host version does not match this desktop.' }
  }
  # Redirect progress because Windows PowerShell treats native stderr as
  # error records. ProcessStartInfo also gives connect a closed stdin.
  $start = New-Object Diagnostics.ProcessStartInfo
  $start.FileName = $program
  $start.Arguments = 'connect --json@@FLAGS@@'
  $start.UseShellExecute = $false
  $start.RedirectStandardInput = $true
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  $process = [Diagnostics.Process]::Start($start)
  $process.StandardInput.Close()
  $outputTask = $process.StandardOutput.ReadToEndAsync()
  $errorTask = $process.StandardError.ReadToEndAsync()
  $process.WaitForExit()
  if ($process.ExitCode -ne 0) { throw $errorTask.Result }
  $outputTask.Result.Trim() -split "`n" | Select-Object -Last 1
} finally {
  Remove-Item -Recurse -Force -ErrorAction SilentlyContinue -LiteralPath $temp
}
