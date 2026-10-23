$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class GanmaoyuanWindow {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
}
"@

$repoRoot = Split-Path -Parent $PSScriptRoot
$targetExe = [System.IO.Path]::GetFullPath(
    (Join-Path $repoRoot "src-tauri\target\release\app.exe")
)

$matching = Get-CimInstance Win32_Process |
    Where-Object {
        $_.ExecutablePath -and
        [System.IO.Path]::GetFullPath($_.ExecutablePath) -eq $targetExe
    }

foreach ($item in $matching) {
    $process = Get-Process -Id $item.ProcessId -ErrorAction SilentlyContinue
    if (-not $process) {
        continue
    }

    $postedClose = $false
    $callback = {
        param([IntPtr]$windowHandle, [IntPtr]$state)
        $windowProcessId = [uint32]0
        [void][GanmaoyuanWindow]::GetWindowThreadProcessId(
            $windowHandle,
            [ref]$windowProcessId
        )
        if (
            $windowProcessId -eq $process.Id -and
            [GanmaoyuanWindow]::IsWindowVisible($windowHandle)
        ) {
            $script:postedClose = (
                [GanmaoyuanWindow]::PostMessage(
                    $windowHandle,
                    0x0010,
                    [IntPtr]::Zero,
                    [IntPtr]::Zero
                ) -or $script:postedClose
            )
        }
        return $true
    }
    [void][GanmaoyuanWindow]::EnumWindows($callback, [IntPtr]::Zero)
    if (-not $postedClose) {
        [void]$process.CloseMainWindow()
    }
    try {
        Wait-Process -Id $process.Id -Timeout 8 -ErrorAction Stop
    }
    catch {
        throw "Ganmaoyuan process PID $($process.Id) did not close gracefully; build was cancelled to protect Codex tasks. The process was not force-terminated."
    }
}
