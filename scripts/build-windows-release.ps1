param(
    [string]$CacheRoot,
    [string]$CargoTarget,
    [string]$ClangClPath,
    [switch]$UseExistingCargoCache,
    [switch]$SkipNpmCi
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$repoDrive = [IO.Path]::GetPathRoot($repoRoot)
if ($repoDrive -and $repoDrive.TrimEnd('\') -eq 'C:') {
    throw 'Release build output must be on a non-C drive.'
}

if (-not $CacheRoot) {
    $CacheRoot = Join-Path (Split-Path $repoRoot -Parent) 'publish-cache\release-hardening'
}
New-Item -ItemType Directory -Force -Path $CacheRoot | Out-Null

$cargoHome = if ($UseExistingCargoCache) {
    if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
} else {
    Join-Path $CacheRoot 'cargo-home'
}
$cargoTarget = if ($CargoTarget) { $CargoTarget } else { Join-Path $CacheRoot 'cargo-target' }
$npmCache = Join-Path $CacheRoot 'npm-cache'
$toolsDir = Join-Path $CacheRoot 'tools'
$buildDirectories = @($cargoTarget, $npmCache, $toolsDir)
if (-not $UseExistingCargoCache) { $buildDirectories += $cargoHome }
New-Item -ItemType Directory -Force -Path $buildDirectories | Out-Null
$env:CARGO_HOME = $cargoHome
$env:CARGO_NET_OFFLINE = if ($UseExistingCargoCache) { 'true' } else { $env:CARGO_NET_OFFLINE }
$env:CARGO_TARGET_DIR = $cargoTarget
$env:npm_config_cache = $npmCache
$env:TEMP = Join-Path $CacheRoot 'tmp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) {
    throw 'Visual Studio Installer (vswhere.exe) was not found; install/use the MSVC x64 Build Tools.'
}
$vsInstall = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vsInstall) {
    throw 'An MSVC x64 Build Tools installation was not found.'
}
$vsDevCmd = Join-Path $vsInstall 'Common7\Tools\VsDevCmd.bat'
$setEnvironment = 'call "{0}" -no_logo -arch=x64 -host_arch=x64 >nul && set' -f $vsDevCmd
$environmentLines = & $env:ComSpec /d /s /c $setEnvironment
if ($LASTEXITCODE -ne 0) {
    throw 'VsDevCmd failed to prepare the MSVC x64 environment.'
}
foreach ($line in $environmentLines) {
    $separator = $line.IndexOf('=')
    if ($separator -gt 0) {
        [Environment]::SetEnvironmentVariable($line.Substring(0, $separator), $line.Substring($separator + 1), 'Process')
    }
}
if (-not $ClangClPath) {
    $ClangClPath = (Get-Command clang-cl.exe -ErrorAction SilentlyContinue).Source
}
if (-not $ClangClPath -or -not (Test-Path -LiteralPath $ClangClPath -PathType Leaf)) {
    throw 'A clang-cl.exe is required: native MSVC cannot remap AWS-LC __FILE__ paths.'
}
$realCompiler = (Resolve-Path -LiteralPath $ClangClPath).Path
$env:HOST_CC = $realCompiler

if ($ClangClPath) {
    $env:CARGO_TRIM_PATHS_SCOPE = 'object'
    if (-not ('Ganmaoyuan.ReleasePathAliases' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;

public static class GanmaoyuanReleasePathAliases
{
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern uint GetShortPathName(string longPath, StringBuilder shortPath, uint capacity);
}
'@
    }
    $pathRoots = @(
        @{ Path = $repoRoot; Replacement = '.' },
        @{ Path = $cargoHome; Replacement = '.cargo' },
        @{ Path = $cargoTarget; Replacement = '.build' }
    )
    $pathRemaps = foreach ($root in $pathRoots) {
        $longPath = [IO.Path]::GetFullPath($root.Path).TrimEnd('\')
        ,"$longPath=$($root.Replacement)"
        $shortPath = [Text.StringBuilder]::new(4096)
        $shortPathLength = [GanmaoyuanReleasePathAliases]::GetShortPathName($longPath, $shortPath, [uint32]$shortPath.Capacity)
        if ($shortPathLength -gt 0 -and $shortPath.ToString() -ne $longPath) {
            ,"$($shortPath.ToString())=$($root.Replacement)"
        }
    }
    $env:CARGO_TRIM_PATHS_REMAP = $pathRemaps -join [IO.Path]::PathSeparator
    $cPathFlags = $pathRemaps | ForEach-Object { "-fmacro-prefix-map=$_" }
    $existingHostCFlags = $env:HOST_CFLAGS
    $env:HOST_CFLAGS = (@($existingHostCFlags) + $cPathFlags | Where-Object { $_ }) -join ' '
}

$encodedSeparator = [char]31
$rustFlags = @()
if ($env:CARGO_ENCODED_RUSTFLAGS) {
    $rustFlags += $env:CARGO_ENCODED_RUSTFLAGS -split [regex]::Escape([string]$encodedSeparator)
}
$rustFlags += @(
    "--remap-path-prefix=$repoRoot=.",
    "--remap-path-prefix=$cargoHome=.cargo",
    "--remap-path-prefix=$cargoTarget=.build"
)
$env:CARGO_ENCODED_RUSTFLAGS = $rustFlags -join $encodedSeparator

Push-Location $repoRoot
try {
    if (-not $SkipNpmCi) {
        npm ci --cache $npmCache
        if ($LASTEXITCODE -ne 0) { throw 'npm ci failed.' }
    }
    npx tauri build --bundles nsis
    if ($LASTEXITCODE -ne 0) { throw 'Tauri NSIS build failed.' }
} finally {
    Pop-Location
}

$appExe = Join-Path $cargoTarget 'release\app.exe'
$installer = Get-ChildItem -LiteralPath (Join-Path $cargoTarget 'release\bundle\nsis') -Filter '*-setup.exe' -File | Select-Object -First 1
if (-not (Test-Path -LiteralPath $appExe) -or -not $installer) {
    throw 'The app executable or NSIS installer was not produced.'
}

$sensitivePatterns = @(
    '(?i)[A-Z]:\\Users\\[^\\\x00 ]+\\\.cargo\\registry\\src\\',
    '(?i)sk-(?:proj-)?[A-Za-z0-9_-]{40,}',
    '(?i)gh[pousr]_[A-Za-z0-9]{30,}',
    '(?i)AKIA[A-Z0-9]{16}'
)
foreach ($artifact in @((Get-Item -LiteralPath $appExe), $installer)) {
    $bytes = [IO.File]::ReadAllBytes($artifact.FullName)
    $content = [Text.Encoding]::ASCII.GetString($bytes) + [Text.Encoding]::Unicode.GetString($bytes)
    foreach ($sensitivePattern in $sensitivePatterns) {
        if ([regex]::IsMatch($content, $sensitivePattern)) {
            throw "Sensitive path/credential pattern found in $($artifact.Name): $sensitivePattern"
        }
    }
    foreach ($privateRoot in @($env:USERPROFILE, $repoRoot, $cargoHome, $cargoTarget)) {
        if ($privateRoot -and $content.IndexOf($privateRoot, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
            throw "Build-machine path found in $($artifact.Name): $privateRoot"
        }
    }
}

$signature = (Get-AuthenticodeSignature -LiteralPath $installer.FullName).Status
[pscustomobject]@{
    AppExe = $appExe
    Installer = $installer.FullName
    Sha256 = (Get-FileHash -LiteralPath $installer.FullName -Algorithm SHA256).Hash
    Signature = $signature
} | Format-List
