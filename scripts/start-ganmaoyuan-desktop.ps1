param()

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$desktopExe = Join-Path $repoRoot "src-tauri\target\release\app.exe"
$distDir = Join-Path $repoRoot "dist"
$startPageUrl = "http://127.0.0.1:5173/index.html"
$desktopLogDir = Join-Path $repoRoot "desktop-runtime"
$serverLog = Join-Path $desktopLogDir "static-server.log"
$serverErrorLog = Join-Path $desktopLogDir "static-server-error.log"
$launcherLog = Join-Path $desktopLogDir "launcher.log"

New-Item -ItemType Directory -Path $desktopLogDir -Force | Out-Null

function Write-LauncherLog([string] $message) {
  $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
  Add-Content -LiteralPath $launcherLog -Value "[$timestamp] $message"
}

function Stop-GanmaoyuanProcesses {
  $prefix = $repoRoot.ToLowerInvariant()
  Get-CimInstance Win32_Process |
    Where-Object {
      $_.Name -in @("app.exe", "node.exe", "python.exe", "pythonw.exe") -and
      (
        ($_.ExecutablePath -and $_.ExecutablePath.ToLowerInvariant().StartsWith($prefix)) -or
        ($_.CommandLine -and $_.CommandLine.ToLowerInvariant().Contains($prefix))
      )
    } |
    ForEach-Object {
      try {
        Stop-Process -Id $_.ProcessId -Force -ErrorAction Stop
      } catch {
      }
    }
}

function Stop-ProcessesOnPort5173 {
  $owningProcessIds = Get-NetTCPConnection -LocalPort 5173 -ErrorAction SilentlyContinue |
    Select-Object -ExpandProperty OwningProcess -Unique

  foreach ($processId in $owningProcessIds) {
    if (-not $processId -or $processId -eq 0) {
      continue
    }
    try {
      Stop-Process -Id $processId -Force -ErrorAction Stop
      Write-LauncherLog "Stopped existing process on port 5173: PID $processId"
    } catch {
    }
  }
}

function Start-StaticServer {
  if (-not (Test-Path -LiteralPath $distDir)) {
    throw "dist directory not found: $distDir"
  }

  Remove-Item -LiteralPath $serverLog -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $serverErrorLog -Force -ErrorAction SilentlyContinue

  $python = (Get-Command python -ErrorAction Stop).Source
  $pythonw = Join-Path (Split-Path -Parent $python) "pythonw.exe"
  $serverHost = if (Test-Path -LiteralPath $pythonw) { $pythonw } else { $python }
  Write-LauncherLog "Starting static server on 5173 from $distDir."
  Start-Process -FilePath $serverHost `
    -ArgumentList "-m", "http.server", "5173" `
    -WorkingDirectory $distDir `
    -RedirectStandardOutput $serverLog `
    -RedirectStandardError $serverErrorLog `
    -WindowStyle Hidden | Out-Null

  for ($index = 0; $index -lt 30; $index++) {
    Start-Sleep -Milliseconds 200
    try {
      $response = Invoke-WebRequest -UseBasicParsing $startPageUrl -TimeoutSec 2
      if ($response.StatusCode -eq 200) {
        Write-LauncherLog "Static server responded with HTTP 200."
        return
      }
    } catch {
    }
  }

  throw "Static server did not become ready on 5173."
}

Set-Content -LiteralPath $launcherLog -Value ""
Write-LauncherLog "Launcher start."
Stop-GanmaoyuanProcesses
Stop-ProcessesOnPort5173
Start-StaticServer

if (-not (Test-Path -LiteralPath $desktopExe)) {
  throw "Desktop shell not found: $desktopExe"
}

Write-LauncherLog "Starting desktop shell: $desktopExe"
Start-Process -FilePath $desktopExe -WorkingDirectory (Split-Path -Parent $desktopExe)
