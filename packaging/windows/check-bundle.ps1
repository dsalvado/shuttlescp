# Smoke-test the Windows folder build-bundle.sh made (target\windows\ShuttleSCP), or an
# installed copy: start it as on a PC without MSYS2, and check that it runs on its own.
#
# - MSYS2 is taken off PATH and GTK's environment variables are cleared, so the program
#   can only use what the folder carries.
# - Once its window is up, every DLL the process has loaded must come from the folder or
#   from Windows itself: one from anywhere else (an MSYS2 install, another GTK) means
#   the folder misses it, and a PC without that would fail.
# - The window is then closed as a user would close it, and the program must exit.
#
# It uses the user's real settings folder (%APPDATA%\shuttlescp), as the app always does.
#
# Usage: powershell -File packaging\windows\check-bundle.ps1 [-Folder <ShuttleSCP folder>]

param(
    [string]$Folder = (Join-Path $PSScriptRoot '..\..\target\windows\ShuttleSCP')
)

$ErrorActionPreference = 'Stop'
$Folder = (Resolve-Path $Folder).Path

# The few Windows calls needed to find and close a program's windows.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class ShuttleWindows {
    delegate bool EnumProc(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr window, StringBuilder text, int size);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr window, uint message, IntPtr w, IntPtr l);

    // The process's visible top-level windows, front (most recent) first.
    public static List<IntPtr> Visible(int processId) {
        var found = new List<IntPtr>();
        EnumWindows((window, data) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner == processId && IsWindowVisible(window)) found.Add(window);
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static string Title(IntPtr window) {
        var text = new StringBuilder(256);
        GetWindowText(window, text, text.Capacity);
        return text.ToString();
    }

    // Ask the window to close, as its title bar's X does (WM_CLOSE).
    public static void Close(IntPtr window) { PostMessage(window, 0x0010, IntPtr.Zero, IntPtr.Zero); }
}
'@
$exe = Join-Path $Folder 'bin\shuttlescp.exe'
if (-not (Test-Path $exe)) { throw "no program at $exe" }

# Only Windows on PATH, and none of the variables that point GTK elsewhere.
$windows = $env:SystemRoot
$env:PATH = "$windows\system32;$windows;$windows\System32\WindowsPowerShell\v1.0"
Get-ChildItem env: | Where-Object { $_.Name -match '^(GTK_|GDK_|GSETTINGS_|XDG_|GIO_|PANGO|FONTCONFIG)' } |
    ForEach-Object { Remove-Item "env:$($_.Name)" }

Write-Host "==> Starting $exe"
$process = Start-Process -FilePath $exe -PassThru
try {
    # Wait for the main window (a release build is slow to start the first time).
    $deadline = (Get-Date).AddSeconds(60)
    while ($process.MainWindowHandle -eq 0) {
        if ($process.HasExited) { throw "ShuttleSCP exited at start (code $($process.ExitCode))" }
        if ((Get-Date) -gt $deadline) { throw 'ShuttleSCP showed no window within 60 s' }
        Start-Sleep -Milliseconds 500
        $process.Refresh()
    }
    # Let it finish loading (icons, image loaders, the first listing).
    Start-Sleep -Seconds 5
    $process.Refresh()
    Write-Host "==> Window up: '$($process.MainWindowTitle)'"

    $modules = $process.Modules | ForEach-Object { $_.FileName }
    $foreign = $modules | Where-Object {
        -not $_.StartsWith($Folder, [StringComparison]::OrdinalIgnoreCase) -and
        -not $_.StartsWith($windows, [StringComparison]::OrdinalIgnoreCase)
    }
    $fromFolder = @($modules | Where-Object { $_.StartsWith($Folder, [StringComparison]::OrdinalIgnoreCase) })
    Write-Host "==> $($modules.Count) modules loaded, $($fromFolder.Count) from the folder"
    if ($foreign) {
        $foreign | ForEach-Object { Write-Host "    not from the folder or Windows: $_" }
        throw 'ShuttleSCP loaded files from outside its folder'
    }

    # The Login dialog may be open over the main window, which ignores closing while it
    # is: close the app's visible windows one at a time, last opened first, as a user
    # would, until it exits.
    Write-Host '==> Closing its windows'
    for ($attempt = 1; -not $process.HasExited; $attempt++) {
        if ($attempt -gt 5) { throw 'ShuttleSCP did not exit after its windows were closed' }
        $front = [ShuttleWindows]::Visible($process.Id) | Select-Object -First 1
        if ($front) {
            Write-Host "    closing '$([ShuttleWindows]::Title($front))'"
            [ShuttleWindows]::Close($front)
        }
        [void]$process.WaitForExit(5000)
    }
    Write-Host "==> OK: ran on its own and exited (code $($process.ExitCode))"
}
finally {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
}
