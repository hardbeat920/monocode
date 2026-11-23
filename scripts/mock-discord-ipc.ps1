param([int]$Port = 0)

# Minimal Discord IPC server for verifying MonoCode's Rich Presence on Windows.
# Pair with $env:MONOCODE_DISCORD_IPC_PORT=<port>; npm run tauri dev.
if ($Port -lt 0 -or $Port -gt 9) {
  Write-Error "Port must be a number from 0 to 9"
  exit 2
}

function New-Frame([uint32]$Opcode, [string]$Json) {
  $body = [Text.Encoding]::UTF8.GetBytes($Json)
  $header = [byte[]]::new(8)
  [BitConverter]::GetBytes($Opcode).CopyTo($header, 0)
  [BitConverter]::GetBytes([uint32]$body.Length).CopyTo($header, 4)
  return $header + $body
}

$name = "discord-ipc-$Port"
$ready = New-Frame 1 '{"cmd":"DISPATCH","evt":"READY","data":{}}'
$pipe = [System.IO.Pipes.NamedPipeServerStream]::new(
  $name,
  [System.IO.Pipes.PipeDirection]::InOut,
  1,
  [System.IO.Pipes.PipeTransmissionMode]::Byte
)
Write-Host "Mock Discord IPC listening on \\.\pipe\$name"

try {
  while ($true) {
    $pipe.WaitForConnection()
    Write-Host "client connected"
    try {
      while ($pipe.IsConnected) {
        $header = [byte[]]::new(8)
        $read = $pipe.Read($header, 0, 8)
        if ($read -lt 8) { break }
        $opcode = [BitConverter]::ToUInt32($header, 0)
        $length = [BitConverter]::ToUInt32($header, 4)
        if ($length -gt 1048576) { Write-Host "frame too large: $length"; break }
        $body = [byte[]]::new($length)
        $offset = 0
        while ($offset -lt $length) {
          $count = $pipe.Read($body, $offset, $length - $offset)
          if ($count -le 0) { break }
          $offset += $count
        }
        if ($offset -lt $length) { break }
        Write-Host "[$opcode] $([Text.Encoding]::UTF8.GetString($body))"
        if ($opcode -eq 0) {
          $pipe.Write($ready, 0, $ready.Length)
          $pipe.Flush()
        }
      }
    } catch {
      Write-Host "client error: $($_.Exception.Message)"
    }
    Write-Host "client disconnected"
    $pipe.Disconnect()
  }
} finally {
  $pipe.Dispose()
}
