$ErrorActionPreference = 'Stop'
$taskRoot = Join-Path $env:LOCALAPPDATA 'monocode-gpui-build'
$sourceRoot = Join-Path $taskRoot 'source'
$package = Join-Path $sourceRoot 'target\debug\monocode-package.exe'
$release = Join-Path $sourceRoot 'target\x86_64-pc-windows-msvc\release'
$trial = Join-Path $taskRoot ('artifacts\portable-host-' + [guid]::NewGuid().ToString('N'))
$negative = Join-Path $trial 'dynamic-runtime-rejected'
$fixture = Join-Path $trial 'dynamic-fixture'
$positive = Join-Path $trial 'portable'
$extract = Join-Path $trial 'extracted'
New-Item $negative,$fixture,$positive,$extract -ItemType Directory -Force | Out-Null
$ErrorActionPreference = 'Continue'
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILE\windows-native-dependencies.ps1" -Portable *> "$taskRoot\native-portable-imports.log"
if ($LASTEXITCODE -ne 0) { throw 'Inspecting native PE imports failed' }
$imports = Get-Content "$taskRoot\native-portable-imports.log" -Raw
if ($imports -match 'VCRUNTIME\d|MSVCP\d|CONCRT\d|MSVCR\d') { throw 'The portable release host imports a redistributable runtime' }
Set-Content -Encoding UTF8 (Join-Path $fixture 'fixture.rs') 'fn main() { println!("0.6.0"); }'
& "$env:USERPROFILE\.cargo\bin\rustc.exe" -C target-feature=-crt-static (Join-Path $fixture 'fixture.rs') -o (Join-Path $fixture 'monocode-host.exe') *> "$taskRoot\native-portable-negative-build.log"
if ($LASTEXITCODE -ne 0) { throw 'Building the deliberately dynamic CRT fixture failed' }
& $package bundle --target x86_64-pc-windows-msvc --formats host --binaries $fixture --output $negative *> "$taskRoot\native-portable-negative.log"
if ($LASTEXITCODE -eq 0) { throw 'The package command accepted the known dynamic-runtime host' }
if (!(Get-Content "$taskRoot\native-portable-negative.log" -Raw).Contains('redistributable runtime')) { throw 'The package rejection must name the redistributable runtime' }
if (@(Get-ChildItem $negative -File).Count -ne 0) { throw 'The rejected host must not produce an archive' }
& $package bundle --target x86_64-pc-windows-msvc --formats host --binaries $release --output $positive *> "$taskRoot\native-portable-package.log"
if ($LASTEXITCODE -ne 0) { throw 'Packaging the static-runtime host failed' }
& $package checksums --directory $positive *>> "$taskRoot\native-portable-package.log"
if ($LASTEXITCODE -ne 0) { throw 'Generating native package checksums failed' }
$archive = Join-Path $positive 'monocode-host_0.6.0_x86_64-pc-windows-msvc.tar.gz'
$entries = @(& tar.exe -tzf $archive)
if ($LASTEXITCODE -ne 0 -or $entries.Count -ne 1 -or $entries[0] -ne 'monocode-host.exe') { throw 'The portable archive must contain exactly one native host executable' }
& tar.exe -xzf $archive -C $extract
if ($LASTEXITCODE -ne 0) { throw 'Extracting the native host archive failed' }
$originalHash = (Get-FileHash "$release\monocode-host.exe" -Algorithm SHA256).Hash
$extractedHash = (Get-FileHash "$extract\monocode-host.exe" -Algorithm SHA256).Hash
if ($originalHash -ne $extractedHash) { throw 'The archive did not preserve the native executable' }
$archiveHash = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant()
$checksums = Get-Content (Join-Path $positive 'SHA256SUMS') -Raw
if (!$checksums.Contains($archiveHash)) { throw 'The native archive checksum does not match SHA256SUMS' }
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILE\windows-native-host-smoke.ps1" -Executable "$extract\monocode-host.exe" *> "$taskRoot\native-portable-host-smoke.log"
if ($LASTEXITCODE -ne 0) { throw 'The extracted native host failed loopback lifecycle qualification' }
Write-Output 'test portable_windows_archive_static_runtime_imports_version_and_loopback_lifecycle ... ok'
Write-Output "archive $archive"
Write-Output "archive_sha256 $archiveHash"
Write-Output "executable_sha256 $($originalHash.ToLowerInvariant())"
