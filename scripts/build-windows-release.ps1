param(
    [string]$OutputDirectory = "",
    [switch]$RequireCleanTree
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repoRoot "src-tauri\target\release\ganmaoyuan-release"
}

function Get-CommandVersion {
    param([string]$Command, [string[]]$Arguments)
    $value = & $Command @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "无法读取工具版本：$Command $($Arguments -join ' ')"
    }
    return ($value | Out-String).Trim()
}

function Test-ByteSequence {
    param([byte[]]$Bytes, [byte[]]$Needle)
    if ($Needle.Length -eq 0 -or $Needle.Length -gt $Bytes.Length) {
        return $false
    }
    for ($i = 0; $i -le $Bytes.Length - $Needle.Length; $i++) {
        $matched = $true
        for ($j = 0; $j -lt $Needle.Length; $j++) {
            if ($Bytes[$i + $j] -ne $Needle[$j]) {
                $matched = $false
                break
            }
        }
        if ($matched) {
            return $true
        }
    }
    return $false
}

function Get-Sha256Hex {
    param([byte[]]$Bytes)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return (($sha.ComputeHash($Bytes) | ForEach-Object { $_.ToString("x2") }) -join "").ToUpperInvariant()
    }
    finally {
        $sha.Dispose()
    }
}

function Assert-NoBuildMachinePath {
    param([string]$FilePath, [string[]]$ForbiddenPaths)
    $bytes = [System.IO.File]::ReadAllBytes($FilePath)
    foreach ($path in $ForbiddenPaths) {
        $normalized = [System.IO.Path]::GetFullPath($path).TrimEnd('\', '/')
        $variants = @(
            $normalized,
            $normalized.Replace('\', '/'),
            $normalized.Replace('/', '\'),
            $normalized.ToLowerInvariant(),
            $normalized.ToUpperInvariant(),
            $normalized.Replace('\', '/').ToLowerInvariant(),
            $normalized.Replace('\', '/').ToUpperInvariant()
        ) | Select-Object -Unique
        foreach ($variant in $variants) {
            foreach ($encoding in @([System.Text.Encoding]::ASCII, [System.Text.Encoding]::Unicode)) {
                if (Test-ByteSequence $bytes ($encoding.GetBytes($variant))) {
                    throw "发布文件包含构建机绝对路径：$(Split-Path -Leaf $FilePath)"
                }
            }
        }
    }
}

function Write-NewTextFile {
    param([string]$Path, [string]$Content)
    $stream = [System.IO.File]::Open(
        $Path,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::None
    )
    $writer = $null
    try {
        $writer = [System.IO.StreamWriter]::new($stream, [System.Text.UTF8Encoding]::new($false))
        $writer.Write($Content)
        $writer.Flush()
    }
    finally {
        if ($writer) {
            $writer.Dispose()
        }
        else {
            $stream.Dispose()
        }
    }
}

Push-Location $repoRoot
try {
    if (-not $IsWindows -and $env:OS -ne "Windows_NT") {
        throw "Windows 安装包必须在 Windows 主机上构建。"
    }

    $package = Get-Content -Raw -Encoding UTF8 (Join-Path $repoRoot "package.json") | ConvertFrom-Json
    $tauriConfig = Get-Content -Raw -Encoding UTF8 (Join-Path $repoRoot "src-tauri\tauri.conf.json") | ConvertFrom-Json
    $cargo = Get-Content -Raw -Encoding UTF8 (Join-Path $repoRoot "src-tauri\Cargo.toml")
    $cargoVersionMatch = [regex]::Match($cargo, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $cargoVersionMatch.Success) {
        throw "无法从 Cargo.toml 读取应用版本。"
    }
    $version = [string]$tauriConfig.version
    if ($package.version -ne $version -or $cargoVersionMatch.Groups[1].Value -ne $version) {
        throw "package.json、tauri.conf.json 与 Cargo.toml 版本不一致。"
    }

    $outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
    $installerName = "{0}_{1}_x64-setup.exe" -f $tauriConfig.productName, $version
    $outputArtifactPaths = @(
        (Join-Path $outputRoot $installerName),
        (Join-Path $outputRoot ("{0}_{1}_app.exe" -f $tauriConfig.productName, $version)),
        (Join-Path $outputRoot ("{0}_{1}_context.exe" -f $tauriConfig.productName, $version)),
        (Join-Path $outputRoot "SHA256SUMS.txt"),
        (Join-Path $outputRoot "build-provenance.json"),
        (Join-Path $outputRoot "INSTALLATION-NOTES.md")
    )
    $existingArtifacts = @($outputArtifactPaths | Where-Object { Test-Path -LiteralPath $_ })
    if ($existingArtifacts.Count -gt 0) {
        $names = ($existingArtifacts | ForEach-Object { Split-Path -Leaf $_ }) -join ", "
        throw "发布输出目录已包含同名文件，拒绝覆盖：$names。请指定新的空输出目录。"
    }

    $gitRevision = (& git -C $repoRoot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $gitRevision -notmatch '^[0-9a-f]{40}$') {
        throw "无法确定构建源码 Git revision。"
    }
    $gitStatus = @(& git -C $repoRoot status --porcelain=v1 --untracked-files=all)
    if ($LASTEXITCODE -ne 0) {
        throw "无法读取构建源码 Git 状态。"
    }
    $workingTreeClean = $gitStatus.Count -eq 0
    if ($RequireCleanTree -and -not $workingTreeClean) {
        throw "发布构建要求干净 Git 工作区；当前工作区存在未提交改动。"
    }
    $statusText = $gitStatus -join "`n"
    $statusBytes = [System.Text.Encoding]::UTF8.GetBytes($statusText)
    $statusDigest = Get-Sha256Hex $statusBytes

    $targetRoot = if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
        Join-Path $repoRoot "src-tauri\target"
    } elseif ([System.IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
        $env:CARGO_TARGET_DIR
    } else {
        Join-Path $repoRoot $env:CARGO_TARGET_DIR
    }
    $targetRoot = [System.IO.Path]::GetFullPath($targetRoot)
    $releaseRoot = Join-Path $targetRoot "release"
    $appExe = Join-Path $releaseRoot "app.exe"
    $running = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
        Where-Object {
            $_.Name -eq "app.exe" -and $_.ExecutablePath -and
            [System.IO.Path]::GetFullPath($_.ExecutablePath).Equals(
                [System.IO.Path]::GetFullPath($appExe),
                [System.StringComparison]::OrdinalIgnoreCase
            )
        }
    if ($running) {
        $pids = ($running | Select-Object -ExpandProperty ProcessId) -join ", "
        throw "发布构建目标 app.exe 正在运行（PID $pids）；未关闭或覆盖该程序。请使用新的 CARGO_TARGET_DIR 或等待程序退出。"
    }

    $sourceRoot = [System.IO.Path]::GetFullPath($repoRoot).TrimEnd('\')
    $userRoot = if ($env:USERPROFILE) { [System.IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\') } else { "" }
    $clangCl = $env:GANMAOYUAN_CLANG_CL
    if ([string]::IsNullOrWhiteSpace($clangCl)) {
        $programFiles = [Environment]::GetFolderPath('ProgramFiles')
        $standardLlvm = Join-Path $programFiles "LLVM\bin\clang-cl.exe"
        if (Test-Path -LiteralPath $standardLlvm) {
            $clangCl = $standardLlvm
        } else {
            $command = Get-Command clang-cl.exe -ErrorAction SilentlyContinue
            if ($command) {
                $clangCl = $command.Source
            } else {
                $programFilesX86 = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
                $vswhere = if ($programFilesX86) { Join-Path $programFilesX86 "Microsoft Visual Studio\Installer\vswhere.exe" } else { "" }
                if ($vswhere -and (Test-Path -LiteralPath $vswhere)) {
                    $vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Llvm.Clang -property installationPath
                    if ($vs) {
                        $candidate = Join-Path $vs "VC\Tools\Llvm\bin\clang-cl.exe"
                        if (Test-Path -LiteralPath $candidate) { $clangCl = $candidate }
                    }
                }
            }
        }
    }
    if ([string]::IsNullOrWhiteSpace($clangCl) -or -not (Test-Path -LiteralPath $clangCl)) {
        throw "发布构建需要可用的 clang-cl，以映射 AWS-LC C 诊断中的源文件路径；未生成可发布包。"
    }
    $clangCl = [System.IO.Path]::GetFullPath($clangCl)
    $compilerRoot = Split-Path -Parent (Split-Path -Parent $clangCl)
    $env:CC_x86_64_pc_windows_msvc = [System.IO.Path]::GetFullPath($clangCl)
    $env:HOST_CC = $env:CC_x86_64_pc_windows_msvc
    $cargoHomeRoot = if ([string]::IsNullOrWhiteSpace($env:CARGO_HOME)) {
        ""
    } else {
        [System.IO.Path]::GetFullPath($env:CARGO_HOME).TrimEnd('\')
    }
    $rustupHomeRoot = if ([string]::IsNullOrWhiteSpace($env:RUSTUP_HOME)) {
        ""
    } else {
        [System.IO.Path]::GetFullPath($env:RUSTUP_HOME).TrimEnd('\')
    }

    $pathMappings = @(
        @{ path = $sourceRoot; replacement = "/ganmaoyuan/source" },
        @{ path = $targetRoot; replacement = "/ganmaoyuan/build-target" },
        @{ path = $userRoot; replacement = "/ganmaoyuan/build-user" },
        @{ path = $cargoHomeRoot; replacement = "/ganmaoyuan/cargo-home" },
        @{ path = $rustupHomeRoot; replacement = "/ganmaoyuan/rust-toolchain" },
        @{ path = $compilerRoot; replacement = "/ganmaoyuan/c-compiler" }
    ) + @(
        foreach ($temporaryRoot in @($env:RUNNER_TEMP, $env:TEMP, $env:TMP)) {
            if (-not [string]::IsNullOrWhiteSpace($temporaryRoot)) {
                @{ path = $temporaryRoot; replacement = "/ganmaoyuan/build-temp" }
            }
        }
    ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_.path) }

    $seenRoots = @{}
    $normalizedMappings = @(
        foreach ($mapping in $pathMappings) {
            $mappingPath = [System.IO.Path]::GetFullPath($mapping.path).TrimEnd('\')
            if (-not $seenRoots.ContainsKey($mappingPath)) {
                $seenRoots[$mappingPath] = $true
                [pscustomobject]@{
                    path = $mappingPath
                    replacement = $mapping.replacement
                }
            }
        }
    ) | Sort-Object { $_.path.Length } -Descending

    $separator = [char]31
    $flags = @()
    if (-not [string]::IsNullOrEmpty($env:CARGO_ENCODED_RUSTFLAGS)) {
        $flags += @($env:CARGO_ENCODED_RUSTFLAGS -split [regex]::Escape([string]$separator))
    }
    foreach ($mapping in $normalizedMappings) {
        $flags += "--remap-path-prefix=$($mapping.path)=$($mapping.replacement)"
    }
    $env:CARGO_ENCODED_RUSTFLAGS = $flags -join $separator
    $env:CARGO_BUILD_INCREMENTAL = "false"

    $cPrefixMaps = @(
        foreach ($mapping in $normalizedMappings) {
            # Keep each prefix-map as one CFLAGS argument even when a Windows path contains spaces.
            '"-ffile-prefix-map={0}={1}"' -f $mapping.path, $mapping.replacement
        }
    )
    if (-not [string]::IsNullOrWhiteSpace($env:HOST_CFLAGS)) {
        $cPrefixMaps = @($env:HOST_CFLAGS) + $cPrefixMaps
    }
    $env:CC_SHELL_ESCAPED_FLAGS = "1"
    $env:HOST_CFLAGS = $cPrefixMaps -join " "
    $buildStarted = [DateTime]::UtcNow

    $tauriCli = Join-Path $repoRoot "node_modules\.bin\tauri.cmd"
    if (-not (Test-Path -LiteralPath $tauriCli)) {
        throw "缺少本地 Tauri CLI；请先运行 npm ci。"
    }
    $makensis = Get-Command makensis.exe -ErrorAction SilentlyContinue
    if (-not $makensis) {
        $programFilesX86 = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
        $standardNsis = if ($programFilesX86) { Join-Path $programFilesX86 "NSIS\makensis.exe" } else { "" }
        if ($standardNsis -and (Test-Path -LiteralPath $standardNsis)) {
            $makensis = [pscustomobject]@{ Source = $standardNsis }
        }
    }
    if (-not $makensis) {
        throw "缺少 NSIS makensis.exe；未生成可发布包。"
    }
    $nsisVersion = Get-CommandVersion $makensis.Source @("/VERSION")
    & $tauriCli build --bundles nsis
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri NSIS 构建失败，退出码：$LASTEXITCODE"
    }

    $contextBridge = Join-Path $releaseRoot "ganmaoyuan-context.exe"
    if (-not (Test-Path -LiteralPath $contextBridge)) {
        & cargo build --locked --manifest-path (Join-Path $repoRoot "src-tauri\Cargo.toml") --release --bin ganmaoyuan-context
        if ($LASTEXITCODE -ne 0) {
            throw "感冒院上下文桥接程序构建失败，退出码：$LASTEXITCODE"
        }
    }
    if (-not (Test-Path -LiteralPath $contextBridge)) {
        throw "构建完成但缺少上下文桥接程序，拒绝生成不完整发布包。"
    }

    $installer = Join-Path $releaseRoot "bundle\nsis\$installerName"
    if (-not (Test-Path -LiteralPath $appExe) -or -not (Test-Path -LiteralPath $installer)) {
        throw "构建完成但缺少 app.exe 或预期 NSIS 安装包。"
    }
    $generatedNsi = Join-Path $releaseRoot "nsis\x64\installer.nsi"
    if (-not (Test-Path -LiteralPath $generatedNsi)) {
        throw "Tauri 未生成 NSIS 中间脚本，无法验证模板渲染。"
    }
    $generatedText = Get-Content -Raw -Encoding UTF8 $generatedNsi
    if ($generatedText -match '\{\{[^}]+\}\}') {
        throw "NSIS 中间脚本仍含未渲染模板占位符。"
    }

    $forbiddenPaths = @(
        $repoRoot,
        $env:USERPROFILE,
        $env:CARGO_HOME,
        $env:RUSTUP_HOME,
        $env:RUNNER_TEMP,
        $env:TEMP,
        $env:TMP,
        $targetRoot,
        $compilerRoot,
        (Split-Path -Parent $makensis.Source)
    ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | Select-Object -Unique
    Assert-NoBuildMachinePath $appExe $forbiddenPaths
    Assert-NoBuildMachinePath $installer $forbiddenPaths
    Assert-NoBuildMachinePath $contextBridge $forbiddenPaths

    New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
    $outputInstaller = Join-Path $outputRoot $installerName
    $outputApp = Join-Path $outputRoot ("{0}_{1}_app.exe" -f $tauriConfig.productName, $version)
    $outputBridge = Join-Path $outputRoot ("{0}_{1}_context.exe" -f $tauriConfig.productName, $version)
    [System.IO.File]::Copy($installer, $outputInstaller, $false)
    [System.IO.File]::Copy($appExe, $outputApp, $false)
    [System.IO.File]::Copy($contextBridge, $outputBridge, $false)

    $artifacts = @($outputInstaller, $outputApp, $outputBridge)
    $checksums = foreach ($artifact in $artifacts) {
        $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $artifact).Hash.ToUpperInvariant()
        "{0}  {1}" -f $hash, (Split-Path -Leaf $artifact)
    }
    Write-NewTextFile (Join-Path $outputRoot "SHA256SUMS.txt") (($checksums -join "`n") + "`n")

    $signature = Get-AuthenticodeSignature -LiteralPath $outputInstaller
    $buildRecord = [ordered]@{
        schemaVersion = 1
        product = $tauriConfig.productName
        version = $version
        architecture = "x64"
        sourceRevision = $gitRevision
        workingTreeClean = $workingTreeClean
        workingTreeStatusSha256 = $statusDigest
        builtAtUtc = [DateTime]::UtcNow.ToString("o")
        buildDurationSeconds = [Math]::Round(([DateTime]::UtcNow - $buildStarted).TotalSeconds, 2)
        os = (Get-CimInstance Win32_OperatingSystem | Select-Object -ExpandProperty Caption)
        node = Get-CommandVersion "node.exe" @("--version")
        npm = Get-CommandVersion "npm.cmd" @("--version")
        rustc = Get-CommandVersion "rustc" @("--version")
        cargo = Get-CommandVersion "cargo" @("--version")
        cCompiler = Get-CommandVersion $clangCl @("--version")
        nsis = $nsisVersion
        runnerImage = [ordered]@{
            name = $env:ImageOS
            version = $env:ImageVersion
        }
        installerFile = Split-Path -Leaf $outputInstaller
        installerSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $outputInstaller).Hash.ToUpperInvariant()
        installerSignatureStatus = [string]$signature.Status
        appFile = Split-Path -Leaf $outputApp
        appSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $outputApp).Hash.ToUpperInvariant()
        contextBridgeFile = Split-Path -Leaf $outputBridge
        contextBridgeSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $outputBridge).Hash.ToUpperInvariant()
        installerValidation = "pending hosted Windows runner validation"
    }
    Write-NewTextFile (Join-Path $outputRoot "build-provenance.json") ($buildRecord | ConvertTo-Json -Depth 5)

    $notes = @"
# Ganmaoyuan $version Windows package

- Source revision: $gitRevision
- Installer: $(Split-Path -Leaf $outputInstaller)
- SHA-256: $($buildRecord.installerSha256)
- Authenticode: $($buildRecord.installerSignatureStatus)
- Default per-user installation directory: `%LOCALAPPDATA%\Ganmaoyuan`
- Default application data: `%APPDATA%\com.ganmaoyuan.desktop`; an explicit `GANMAOYUAN_DATA_DIR` overrides it. The former D-drive data folder is reused only when recognized Ganmaoyuan data files are present.
- The installer may download Microsoft WebView2 Runtime when it is missing; network access is then required.
- Uninstall preserves application data by default. Data deletion is an explicit interactive choice.
- This build record does not claim successful installation or GUI testing; see `install-validation.json` when produced by the isolated hosted-runner job.
"@
    Write-NewTextFile (Join-Path $outputRoot "INSTALLATION-NOTES.md") $notes

    Write-Output "Release artifacts staged at: $outputRoot"
    Write-Output "Installer SHA-256: $($buildRecord.installerSha256)"
    Write-Output "Installer signature: $($buildRecord.installerSignatureStatus)"
    Write-Output "Source revision: $gitRevision (working tree clean: $workingTreeClean)"
}
finally {
    Pop-Location
}
