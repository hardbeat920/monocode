$ErrorActionPreference = 'Stop'
$taskRoot = Join-Path $env:LOCALAPPDATA 'monocode-gpui-build'
$sourceRoot = Join-Path $taskRoot 'source'
$compiler = Get-ChildItem "$taskRoot\tools\nsis" -Recurse -Filter makensis.exe -File | Select-Object -First 1
if (!$compiler) { throw 'The task NSIS compiler is absent' }
$env:PATH = "$($compiler.DirectoryName);$env:PATH"
$artifact = Join-Path $taskRoot ('artifacts\native-installer-checkpoint-' + [guid]::NewGuid().ToString('N'))
$binaries = Join-Path $artifact 'inputs'
$output = Join-Path $artifact 'packages'
New-Item $binaries -ItemType Directory -Force | Out-Null
Copy-Item "$sourceRoot\target\debug\monocode-app.exe" $binaries
Copy-Item "$sourceRoot\target\x86_64-pc-windows-msvc\release\monocode-host.exe" $binaries
Get-ChildItem $binaries -File | ForEach-Object { "$($_.Name) $((Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())" } | Set-Content (Join-Path $artifact 'input-hashes.txt')
$ErrorActionPreference = 'Continue'
& "$sourceRoot\target\debug\monocode-package.exe" bundle --target x86_64-pc-windows-msvc --binaries $binaries --output $output *> (Join-Path $artifact 'package.log')
if ($LASTEXITCODE -ne 0) { Get-Content (Join-Path $artifact 'package.log') -Tail 45; exit $LASTEXITCODE }
& "$sourceRoot\target\debug\monocode-package.exe" checksums --directory $output *>> (Join-Path $artifact 'package.log')
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$installer = Join-Path $output 'MonoCode_0.6.0_x86_64-pc-windows-msvc-setup.exe'
if (!(Test-Path $installer)) { throw 'The native installer was not generated' }
$hash = (Get-FileHash $installer -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Output 'test actual_native_windows_nsis_installer_compile ... ok'
Write-Output "Installer $installer"
Write-Output "Installer SHA256 $hash"
Get-Content (Join-Path $output 'SHA256SUMS')
Write-Output 'The installer was not executed'
