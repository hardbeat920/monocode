param([string]$Executable, [string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
trap { $_.Exception.ToString() | Set-Content (Join-Path $OutputDirectory 'window-error.log'); Set-Content (Join-Path $OutputDirectory 'window.exit') 1; break }
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
public static class OwnedWindowCapture {
    delegate bool EnumCallback(IntPtr window, IntPtr unused);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumCallback callback, IntPtr unused);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    [StructLayout(LayoutKind.Sequential)] struct Rect { public int Left, Top, Right, Bottom; }
    public static void SetDpi() { SetProcessDpiAwarenessContext(new IntPtr(-4)); }
    public static IntPtr Find(uint owner) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr window, IntPtr unused) {
            uint pid;
            GetWindowThreadProcessId(window, out pid);
            if (pid == owner && IsWindowVisible(window)) { found = window; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
    public static string Capture(IntPtr window, uint owner, string destination) {
        uint pid;
        GetWindowThreadProcessId(window, out pid);
        if (pid != owner) throw new InvalidOperationException("The window PID changed");
        Rect rect;
        if (!GetWindowRect(window, out rect)) throw new InvalidOperationException("Window bounds unavailable");
        int width = rect.Right - rect.Left, height = rect.Bottom - rect.Top;
        if (width < 200 || height < 200 || width > 4096 || height > 4096) throw new InvalidOperationException("Unexpected owned window dimensions");
        using (Bitmap bitmap = new Bitmap(width, height, PixelFormat.Format32bppArgb)) {
            using (Graphics graphics = Graphics.FromImage(bitmap)) {
                IntPtr dc = graphics.GetHdc();
                try { if (!PrintWindow(window, dc, 2)) throw new InvalidOperationException("PrintWindow failed"); }
                finally { graphics.ReleaseHdc(dc); }
            }
            Dictionary<int, bool> colors = new Dictionary<int, bool>();
            for (int y = 0; y < height; y += 16)
                for (int x = 0; x < width; x += 16) colors[bitmap.GetPixel(x, y).ToArgb()] = true;
            bitmap.Save(destination, ImageFormat.Png);
            if (colors.Count < 25) throw new InvalidOperationException("The owned window image is blank or incomplete: " + colors.Count + " sampled colors");
            return "Window " + window + ", " + width + "x" + height + ", " + colors.Count + " sampled colors";
        }
    }
}
'@
[OwnedWindowCapture]::SetDpi()
$session = (Get-Process -Id $PID).SessionId
Set-Content (Join-Path $OutputDirectory 'session-id.txt') $session
if ($session -eq 0) { throw 'The native window fixture requires the existing desktop session' }
$profile = Join-Path $OutputDirectory 'profile'
New-Item $profile -ItemType Directory | Out-Null
$info = New-Object System.Diagnostics.ProcessStartInfo
$info.FileName = $Executable
$info.Arguments = '--view widgets --theme dark --size 1280x800 --data-dir "' + (Join-Path $profile 'data') + '"'
$info.UseShellExecute = $false
$info.RedirectStandardOutput = $true
$info.RedirectStandardError = $true
$info.EnvironmentVariables['MONOCODE_DATA_DIR'] = Join-Path $profile 'data'
$info.EnvironmentVariables['APPDATA'] = Join-Path $profile 'roaming'
$info.EnvironmentVariables['LOCALAPPDATA'] = Join-Path $profile 'local'
$process = New-Object System.Diagnostics.Process
$process.StartInfo = $info
$code = 1
try {
    $process.Start() | Out-Null
    $null = $process.Handle
    Set-Content (Join-Path $OutputDirectory 'owned-app-pid.txt') $process.Id
    Set-Content (Join-Path $OutputDirectory 'owned-app-start-ticks.txt') $process.StartTime.ToUniversalTime().Ticks
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    $window = [IntPtr]::Zero
    while ($window -eq [IntPtr]::Zero) {
        if ($process.HasExited) { throw 'The native app exited before creating its own window' }
        if ([DateTime]::UtcNow -gt $deadline) { throw 'The native app did not create a window within 20 seconds' }
        $window = [OwnedWindowCapture]::Find([uint32]$process.Id)
        Start-Sleep -Milliseconds 100
    }
    Start-Sleep -Seconds 5
    $png = Join-Path $OutputDirectory 'widgets.png'
    $result = [OwnedWindowCapture]::Capture($window, [uint32]$process.Id, $png)
    $result | Set-Content (Join-Path $OutputDirectory 'window.log')
    "App SHA256 $((Get-FileHash $Executable -Algorithm SHA256).Hash.ToLowerInvariant())" | Add-Content (Join-Path $OutputDirectory 'window.log')
    "PNG SHA256 $((Get-FileHash $png -Algorithm SHA256).Hash.ToLowerInvariant())" | Add-Content (Join-Path $OutputDirectory 'window.log')
    "SessionId $session" | Add-Content (Join-Path $OutputDirectory 'window.log')
    $code = 0
} catch {
    $_.Exception.ToString() | Set-Content (Join-Path $OutputDirectory 'window-error.log')
} finally {
    if (!$process.HasExited) { $process.Kill(); $process.WaitForExit(5000) | Out-Null }
    $process.StandardOutput.ReadToEnd() | Set-Content (Join-Path $OutputDirectory 'app.stdout.log')
    $process.StandardError.ReadToEnd() | Set-Content (Join-Path $OutputDirectory 'app.stderr.log')
    $process.Dispose()
    Set-Content (Join-Path $OutputDirectory 'window.exit') $code
}
exit $code
