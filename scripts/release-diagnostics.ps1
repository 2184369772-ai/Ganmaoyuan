param(
    [string]$RepoRoot = "",
    [string]$OutputDir = "D:\GanMaoYuan\Diagnostics",
    [string]$ProjectRoot = "D:\GanMaoYuan\SelfProject",
    [switch]$JsonOnly
)

$ErrorActionPreference = "Stop"

function Resolve-FullPath {
    param([string]$PathValue)
    if ([string]::IsNullOrWhiteSpace($PathValue)) {
        return ""
    }
    return [System.IO.Path]::GetFullPath($PathValue)
}

function Read-JsonFile {
    param([string]$PathValue)
    if (-not (Test-Path -LiteralPath $PathValue)) {
        return $null
    }
    return Get-Content -Raw -Encoding UTF8 -LiteralPath $PathValue | ConvertFrom-Json
}

function Get-FileSha256 {
    param([string]$PathValue)
    if (-not (Test-Path -LiteralPath $PathValue)) {
        return ""
    }
    $stream = [System.IO.File]::OpenRead($PathValue)
    try {
        $sha = [System.Security.Cryptography.SHA256]::Create()
        try {
            $hash = $sha.ComputeHash($stream)
            return (($hash | ForEach-Object { $_.ToString("x2") }) -join "").ToUpperInvariant()
        }
        finally {
            $sha.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

function Get-LnkTarget {
    param([string]$ShortcutPath)
    if (-not (Test-Path -LiteralPath $ShortcutPath)) {
        return $null
    }
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($ShortcutPath)
    [pscustomobject]@{
        path = $ShortcutPath
        targetPath = $link.TargetPath
        arguments = $link.Arguments
        workingDirectory = $link.WorkingDirectory
        targetExists = if ($link.TargetPath) { Test-Path -LiteralPath $link.TargetPath } else { $false }
    }
}

function New-Check {
    param(
        [string]$Name,
        [bool]$Passed,
        [string]$Detail = "",
        [string]$Severity = "error"
    )
    [pscustomobject]@{
        name = $Name
        passed = $Passed
        severity = if ($Passed) { "info" } else { $Severity }
        detail = $Detail
    }
}

function Mask-SecretSignal {
    param([string]$Value)
    if ([string]::IsNullOrEmpty($Value)) {
        return ""
    }
    if ($Value.Length -le 12) {
        return "$($Value.Substring(0, [Math]::Min(3, $Value.Length)))..."
    }
    return "$($Value.Substring(0, [Math]::Min(5, $Value.Length)))...$($Value.Substring($Value.Length - 4))"
}

function Find-SecretSignals {
    param([string[]]$Paths)
    $signals = @()
    $patterns = @(
        [pscustomobject]@{
            name = "secret-key-prefix"
            # Avoid false positives inside words such as "task-<uuid>".
            regex = "(?<![A-Za-z0-9])sk-[A-Za-z0-9_\-]{16,}"
        },
        [pscustomobject]@{
            name = "api-key-assignment"
            regex = "api[_-]?key\s*[:=]\s*['""][^'""]+['""]"
        },
        [pscustomobject]@{
            name = "authorization-bearer"
            regex = "authorization\s*[:=]\s*['""]?bearer\s+[A-Za-z0-9_\-\.]+"
        }
    )
    foreach ($path in $Paths) {
        if (-not (Test-Path -LiteralPath $path)) {
            continue
        }
        $files = Get-ChildItem -LiteralPath $path -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object {
                $_.FullName -notmatch "\\node_modules\\" -and
                $_.FullName -notmatch "\\target\\" -and
                $_.FullName -notmatch "\\dist\\" -and
                $_.Extension -in @(".json", ".jsonl", ".md", ".txt", ".toml", ".conf")
            }
        foreach ($file in $files) {
            $text = Get-Content -Raw -Encoding UTF8 -LiteralPath $file.FullName -ErrorAction SilentlyContinue
            if ($null -eq $text) {
                continue
            }
            foreach ($pattern in $patterns) {
                $matches = [regex]::Matches($text, $pattern.regex, [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
                if ($matches.Count -gt 0) {
                    $signals += [pscustomobject]@{
                        file = $file.FullName
                        type = $pattern.name
                        masked = @($matches | Select-Object -First 3 | ForEach-Object { Mask-SecretSignal $_.Value })
                        count = $matches.Count
                    }
                    break
                }
            }
        }
    }
    return $signals
}

if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = Split-Path -Parent $PSScriptRoot
}
$RepoRoot = Resolve-FullPath $RepoRoot
$OutputDir = Resolve-FullPath $OutputDir
$ProjectRoot = Resolve-FullPath $ProjectRoot
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$packageJsonPath = Join-Path $RepoRoot "package.json"
$tauriConfigPath = Join-Path $RepoRoot "src-tauri\tauri.conf.json"
$cargoTomlPath = Join-Path $RepoRoot "src-tauri\Cargo.toml"
$releaseExe = Join-Path $RepoRoot "src-tauri\target\release\app.exe"
$nsisInstaller = Join-Path $RepoRoot "src-tauri\target\release\bundle\nsis\Ganmaoyuan_0.1.1_x64-setup.exe"
$msiInstaller = Join-Path $RepoRoot "src-tauri\target\release\bundle\msi\Ganmaoyuan_0.1.1_x64_en-US.msi"
$globalDataDir = "D:\GanMaoYuan\AppData"
$workspaceRoot = "D:\GanMaoYuan_Workspace"
$sendToPath = Join-Path $env:APPDATA "Microsoft\Windows\SendTo\感冒院.lnk"
$desktopCandidates = @(
    (Join-Path ([Environment]::GetFolderPath("Desktop")) "感冒院.lnk"),
    (Join-Path ([Environment]::GetFolderPath("Desktop")) "Ganmaoyuan.lnk"),
    (Join-Path ([Environment]::GetFolderPath("CommonDesktopDirectory")) "感冒院.lnk"),
    (Join-Path ([Environment]::GetFolderPath("CommonDesktopDirectory")) "Ganmaoyuan.lnk")
) | Select-Object -Unique

$packageJson = Read-JsonFile $packageJsonPath
$tauriConfig = Read-JsonFile $tauriConfigPath
$projectManifestPath = Join-Path $ProjectRoot ".ganmaoyuan\project-location-manifest.json"
$projectManifest = Read-JsonFile $projectManifestPath
$registryPath = Join-Path $globalDataDir "projects.json"
$registry = Read-JsonFile $registryPath

$desktopShortcuts = @($desktopCandidates | ForEach-Object { Get-LnkTarget $_ } | Where-Object { $_ })
$sendToShortcut = Get-LnkTarget $sendToPath
$secretSignals = Find-SecretSignals @(
    $RepoRoot,
    $globalDataDir,
    (Join-Path $ProjectRoot ".ganmaoyuan")
)

$checks = @()
$checks += New-Check "package.json exists" (Test-Path -LiteralPath $packageJsonPath) $packageJsonPath
$checks += New-Check "tauri.conf.json exists" (Test-Path -LiteralPath $tauriConfigPath) $tauriConfigPath
$checks += New-Check "version matches package and Tauri" ($packageJson.version -eq $tauriConfig.version) "package=$($packageJson.version); tauri=$($tauriConfig.version)"
$checks += New-Check "release app.exe exists" (Test-Path -LiteralPath $releaseExe) $releaseExe
$checks += New-Check "NSIS installer exists" (Test-Path -LiteralPath $nsisInstaller) $nsisInstaller
$checks += New-Check "MSI installer exists" (Test-Path -LiteralPath $msiInstaller) $msiInstaller "warning"
$checks += New-Check "global AppData exists" (Test-Path -LiteralPath $globalDataDir) $globalDataDir
$checks += New-Check "workspace root exists" (Test-Path -LiteralPath $workspaceRoot) $workspaceRoot "warning"
$checks += New-Check "project manifest exists" (Test-Path -LiteralPath $projectManifestPath) $projectManifestPath
$checks += New-Check "registry contains projects" (($registry.projects | Measure-Object).Count -gt 0) $registryPath "warning"
$checks += New-Check "desktop shortcut target exists" (@($desktopShortcuts | Where-Object { $_.targetExists }).Count -gt 0) (($desktopShortcuts | ConvertTo-Json -Depth 4 -Compress)) "warning"
$checks += New-Check "SendTo shortcut target exists" ($null -ne $sendToShortcut -and $sendToShortcut.targetExists) ($sendToShortcut | ConvertTo-Json -Depth 4 -Compress) "warning"
$checks += New-Check "no plaintext API key signals in checked text files" (($secretSignals | Measure-Object).Count -eq 0) (($secretSignals | ConvertTo-Json -Depth 4 -Compress)) "error"

$artifacts = [pscustomobject]@{
    appExe = [pscustomobject]@{
        path = $releaseExe
        exists = Test-Path -LiteralPath $releaseExe
        sha256 = Get-FileSha256 $releaseExe
    }
    nsis = [pscustomobject]@{
        path = $nsisInstaller
        exists = Test-Path -LiteralPath $nsisInstaller
        sha256 = Get-FileSha256 $nsisInstaller
    }
    msi = [pscustomobject]@{
        path = $msiInstaller
        exists = Test-Path -LiteralPath $msiInstaller
        sha256 = Get-FileSha256 $msiInstaller
    }
}

$manualAcceptanceChecklist = @(
    "启动桌面快捷方式，确认打开的是当前 release app.exe 或安装版目标。",
    "打开感冒院自身项目，确认 Today Workspace 和 Work Ledger 可读取。",
    "验证打开文件、打开所在位置、SendTo、桌面拖文件入口。",
    "执行备份、恢复、项目迁移各一次，并确认原文件未被移动或删除。",
    "检查 DeepSeek 设置页只显示 hasApiKey，不暴露明文 Key。",
    "确认 NSIS 覆盖安装后桌面快捷方式和 SendTo 入口仍可用。"
)

$report = [pscustomobject]@{
    generatedAt = (Get-Date).ToString("o")
    repoRoot = $RepoRoot
    projectRoot = $ProjectRoot
    packageVersion = $packageJson.version
    tauriVersion = $tauriConfig.version
    cargoTomlPath = $cargoTomlPath
    artifacts = $artifacts
    desktopShortcuts = $desktopShortcuts
    sendToShortcut = $sendToShortcut
    globalDataDir = $globalDataDir
    workspaceRoot = $workspaceRoot
    project = if ($projectManifest) {
        [pscustomobject]@{
            id = $projectManifest.project.id
            name = $projectManifest.project.name
            fileCount = @($projectManifest.files).Count
            messageCount = @($projectManifest.messages).Count
            codexPromptCount = @($projectManifest.codexPrompts).Count
            codexReportCount = @($projectManifest.codexReports).Count
        }
    } else {
        $null
    }
    checks = $checks
    summary = [pscustomobject]@{
        passed = @($checks | Where-Object { $_.passed }).Count
        failed = @($checks | Where-Object { -not $_.passed -and $_.severity -eq "error" }).Count
        warnings = @($checks | Where-Object { -not $_.passed -and $_.severity -eq "warning" }).Count
    }
    manualAcceptanceChecklist = $manualAcceptanceChecklist
}

$jsonPath = Join-Path $OutputDir "release-diagnostic-$timestamp.json"
$markdownPath = Join-Path $OutputDir "release-acceptance-checklist-$timestamp.md"
$report | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 -LiteralPath $jsonPath

$checkLines = $checks | ForEach-Object {
    $mark = if ($_.passed) { "OK" } elseif ($_.severity -eq "warning") { "WARN" } else { "FAIL" }
    "- [$mark] $($_.name)：$($_.detail)"
}
$manualLines = $manualAcceptanceChecklist | ForEach-Object { "- [ ] $_" }
@"
# 感冒院本地发布诊断与验收清单

生成时间：$($report.generatedAt)

## 自动诊断
$($checkLines -join "`n")

## 安装/验收产物
- app.exe：$($artifacts.appExe.path)
- app.exe SHA256：$($artifacts.appExe.sha256)
- NSIS：$($artifacts.nsis.path)
- NSIS SHA256：$($artifacts.nsis.sha256)
- MSI：$($artifacts.msi.path)
- MSI SHA256：$($artifacts.msi.sha256)

## 人工验收清单
$($manualLines -join "`n")

## 说明
- 本脚本只读取和诊断，不移动、不复制、不删除用户文件。
- Secret 检查只扫描文本类配置/日志/文档信号，不读取 Credential Manager 明文。
- MSI 若需要管理员权限，应作为已知限制单独人工确认。
"@ | Set-Content -Encoding UTF8 -LiteralPath $markdownPath

if ($JsonOnly) {
    $report | ConvertTo-Json -Depth 8
} else {
    [pscustomobject]@{
        jsonPath = $jsonPath
        markdownPath = $markdownPath
        passed = $report.summary.passed
        failed = $report.summary.failed
        warnings = $report.summary.warnings
    } | ConvertTo-Json -Depth 4
}
