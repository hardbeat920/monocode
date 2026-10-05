# Run the search qualification on QRK-GLUON's Windows from a fresh copy of the source archive.
param([Parameter(Mandatory = $true)][string]$Archive)
$ErrorActionPreference = 'Stop'
$taskRoot = Join-Path $env:LOCALAPPDATA 'monocode-gpui-build'
$source = Join-Path $taskRoot 'search-source'
$output = Join-Path $taskRoot 'search-output'
$tools = Join-Path $taskRoot 'tools'
$nasm = Get-ChildItem "$tools\nasm" -Filter nasm.exe -File -Recurse | Select-Object -First 1
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$cmake = "$vs\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
$env:PATH = "$($nasm.DirectoryName);$cmake;$env:USERPROFILE\.cargo\bin;$env:ProgramFiles\nodejs;$env:PATH"
$env:CARGO_HOME = "$taskRoot\cargo"
$env:CARGO_TARGET_DIR = "$taskRoot\source\target"
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
$env:GIT_CONFIG_COUNT = '1'
$env:GIT_CONFIG_KEY_0 = 'core.longpaths'
$env:GIT_CONFIG_VALUE_0 = 'true'
$env:PYTHONUTF8 = '1'
foreach ($path in $source, $output) {
    if (Test-Path $path) { Remove-Item $path -Recurse -Force }
    New-Item $path -ItemType Directory | Out-Null
}
$ErrorActionPreference = 'Continue'
& tar.exe -xmzf $Archive -C $source
if ($LASTEXITCODE -ne 0) { throw 'Extracting the source archive failed' }
Set-Location $source
& cmd.exe /c "(ver & rustc -Vv & cargo -V & python -V) > `"$output\platform.log`" 2>&1"
(Get-FileHash $Archive -Algorithm SHA256).Hash.ToLowerInvariant() | Set-Content "$output\archive-sha256.txt"
& cmd.exe /c "cargo test --workspace --lib --bins --all-features --locked -j4 --no-run --message-format=json > `"$output\compile.log`" 2> `"$output\compile-stderr.log`""
if ($LASTEXITCODE -ne 0) { Get-Content "$output\compile-stderr.log" -Tail 60; exit $LASTEXITCODE }
& python reports\native-qualification\javascript-search\check-after.py --source $source --output $output --compile-log "$output\compile.log"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& cmd.exe /c "cargo clippy --workspace --all-targets --all-features --locked -j4 -- -D warnings > `"$output\clippy.log`" 2>&1"
if ($LASTEXITCODE -ne 0) { Get-Content "$output\clippy.log" -Tail 60; exit $LASTEXITCODE }
Write-Output complete
