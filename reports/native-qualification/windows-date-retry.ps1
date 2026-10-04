$ErrorActionPreference = 'Stop'
$taskRoot = Join-Path $env:LOCALAPPDATA 'monocode-gpui-build'
$source = Join-Path $taskRoot 'source'
$tools = Join-Path $taskRoot 'tools'
$nasm = Get-ChildItem "$tools\nasm" -Filter nasm.exe -File -Recurse | Select-Object -First 1
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$cmake = "$vs\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
$env:PATH = "$($nasm.DirectoryName);$cmake;$env:USERPROFILE\.cargo\bin;$env:ProgramFiles\nodejs;$env:PATH"
$env:CARGO_HOME = "$taskRoot\cargo"
$env:CARGO_TARGET_DIR = "$source\target"
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:CARGO_BUILD_JOBS = '2'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
$env:GIT_CONFIG_COUNT = '1'
$env:GIT_CONFIG_KEY_0 = 'core.longpaths'
$env:GIT_CONFIG_VALUE_0 = 'true'
Set-Location $source
$ErrorActionPreference='Continue'
& tar.exe -xzf "$env:USERPROFILE\windows-date-retry.tar.gz"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& cargo test --workspace --lib --bins --all-features --locked -j2 --no-run *> "$taskRoot\native-date-retry-compile.log"
if ($LASTEXITCODE -ne 0) { Get-Content "$taskRoot\native-date-retry-compile.log" -Tail 60; exit $LASTEXITCODE }
$compiled=Get-Content "$taskRoot\native-date-retry-compile.log" -Raw
$platform=[regex]::Match($compiled,'monocode_platform-[a-f0-9]+\.exe')
& (Join-Path $source ('target\debug\deps\'+$platform.Value)) date_time:: --nocapture *> "$taskRoot\native-date-retry-platform.log"
$code=$LASTEXITCODE
Get-Content "$taskRoot\native-date-retry-platform.log" -Tail 35
if ($code -ne 0) { exit $code }
$transcript=[regex]::Match($compiled,'monocode_view_transcript-[a-f0-9]+\.exe')
for($i=1;$i -le 40;$i++) {
    $log="$taskRoot\native-date-retry-transcript-$i.log"
    & (Join-Path $source ('target\debug\deps\'+$transcript.Value)) *> $log
    $code=$LASTEXITCODE
    Write-Output "transcript retry attempt $i exit $code"
    if($code -ne 0) { Get-Content $log -Tail 45; exit $code }
}
& cargo clippy --workspace --all-targets --all-features --locked -j2 -- -D warnings *> "$taskRoot\native-date-retry-clippy.log"
if ($LASTEXITCODE -ne 0) { Get-Content "$taskRoot\native-date-retry-clippy.log" -Tail 65; exit $LASTEXITCODE }
& cargo test --workspace --lib --bins --all-features --locked -j2 *> "$taskRoot\native-date-retry-workspace.log"
if ($LASTEXITCODE -ne 0) { Get-Content "$taskRoot\native-date-retry-workspace.log" -Tail 65; exit $LASTEXITCODE }
& cargo test -p monocode-core --tests --locked -j2 *> "$taskRoot\native-date-retry-core.log"
if ($LASTEXITCODE -ne 0) { Get-Content "$taskRoot\native-date-retry-core.log" -Tail 65; exit $LASTEXITCODE }
& cargo test -p monocode-host --test remote_recovery --locked -j2 *> "$taskRoot\native-date-retry-recovery.log"
if ($LASTEXITCODE -ne 0) { Get-Content "$taskRoot\native-date-retry-recovery.log" -Tail 65; exit $LASTEXITCODE }
Write-Output 'native Windows date retry and broad checkpoint passed'
exit 0
