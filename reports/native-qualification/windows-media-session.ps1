param([string]$Fixture, [string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$session = (Get-Process -Id $PID).SessionId
Set-Content (Join-Path $OutputDirectory 'session-id.txt') $session
$ErrorActionPreference = 'Continue'
& $Fixture *> (Join-Path $OutputDirectory 'media.log')
$code = $LASTEXITCODE
Set-Content (Join-Path $OutputDirectory 'media.exit') $code
exit $code
