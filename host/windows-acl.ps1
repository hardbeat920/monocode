function Protect-MonoCodeDirectory([string] $Path) {
  $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
  if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw 'The host data directory must not be a link or junction.'
  }
  $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
  $acl = New-Object Security.AccessControl.DirectorySecurity
  $acl.SetOwner($sid)
  $acl.SetAccessRuleProtection($true, $false)
  foreach ($identity in @($sid.Value, 'S-1-5-18', 'S-1-5-32-544')) {
    $principal = New-Object Security.Principal.SecurityIdentifier($identity)
    $rule = New-Object Security.AccessControl.FileSystemAccessRule($principal, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
    $acl.AddAccessRule($rule)
  }
  Set-Acl -LiteralPath $Path -AclObject $acl -ErrorAction Stop
}
