$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$package = @@PACKAGE@@

$npx = (Get-Command npx.cmd -ErrorAction SilentlyContinue | Select-Object -First 1).Source
if (-not $npx) {
  foreach ($dir in @(
      (Join-Path $env:ProgramFiles 'nodejs'),
      (Join-Path $env:LOCALAPPDATA 'Programs\nodejs'),
      (Join-Path $env:APPDATA 'nvm\nodejs'),
      (Join-Path $env:LOCALAPPDATA 'Volta\bin'))) {
    if ($dir -and (Test-Path -LiteralPath (Join-Path $dir 'npx.cmd'))) {
      $npx = Join-Path $dir 'npx.cmd'
      break
    }
  }
}
if (-not $npx) {
  throw 'Node.js was not found for this user. Install Node.js 22.13 or newer on the machine, then try again.'
}
$bin = Split-Path -Parent $npx
$env:PATH = "$bin;$env:PATH"
$node = Join-Path $bin 'node.exe'
if (-not (Test-Path -LiteralPath $node)) { $node = 'node.exe' }
& $node -e 'const [a, b] = process.versions.node.split(".").map(Number); process.exit(a > 22 || (a === 22 && b >= 13) ? 0 : 1)'
if ($LASTEXITCODE -ne 0) {
  throw "MonoCode Host needs Node.js 22.13 or newer; this machine has $(& $node --version). Update Node.js for this user, then try again."
}

# cmd.exe redirects the command's stderr to a file. Windows PowerShell would
# otherwise turn each progress line into an error record.
$log = [IO.Path]::GetTempFileName()
try {
  $output = & cmd.exe /d /s /c "`"`"$npx`" --yes --package $package monocode-host connect --json@@FLAGS@@ <nul 2>`"$log`"`""
  if ($LASTEXITCODE -ne 0) {
    $lines = @(Get-Content -LiteralPath $log | Where-Object { $_.Trim() })
    throw (($lines | Select-Object -Last 4) -join "`n")
  }
  $output | Select-Object -Last 1
} finally {
  Remove-Item -Force -ErrorAction SilentlyContinue -LiteralPath $log
}
