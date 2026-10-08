param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$TauriArgs
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$firstArg = if ($TauriArgs.Count -gt 0) { $TauriArgs[0] } else { "" }

if ($firstArg -eq "build") {
    & (Join-Path $PSScriptRoot "prepare-tauri-build.ps1")

    # Prevent local checkout and Cargo cache paths from being embedded in release binaries.
    $pathMappings = @(
        "--remap-path-prefix=$repoRoot=/ganmaoyuan/source"
    )
    if (-not [string]::IsNullOrWhiteSpace($env:USERPROFILE)) {
        $pathMappings += "--remap-path-prefix=$($env:USERPROFILE)=/ganmaoyuan/build-user"
    }
    $encodedSeparator = [char]31
    $existingFlags = if ([string]::IsNullOrEmpty($env:CARGO_ENCODED_RUSTFLAGS)) {
        @()
    } else {
        @($env:CARGO_ENCODED_RUSTFLAGS -split [regex]::Escape([string]$encodedSeparator))
    }
    $env:CARGO_ENCODED_RUSTFLAGS = ($existingFlags + $pathMappings) -join $encodedSeparator
}

Push-Location $repoRoot
try {
    & npx.cmd tauri @TauriArgs
    if ($LASTEXITCODE -eq 0 -and $firstArg -eq "build") {
        # Tauri builds every declared release binary. Only fall back to Cargo if
        # a future Tauri change omits the console bridge from the bundle build.
        $contextBridge = Join-Path $repoRoot "src-tauri\target\release\ganmaoyuan-context.exe"
        if (-not (Test-Path -LiteralPath $contextBridge)) {
            & cargo build --manifest-path (Join-Path $repoRoot "src-tauri\Cargo.toml") --release --bin ganmaoyuan-context
        }
        $currentAppDir = Join-Path $repoRoot "..\builds\current\release"
        $currentApp = Join-Path $currentAppDir "app.exe"
        New-Item -ItemType Directory -Force -Path $currentAppDir | Out-Null
        Copy-Item -Force (Join-Path $repoRoot "src-tauri\target\release\app.exe") $currentApp
        Write-Host "Updated stable desktop app: $currentApp"
    }
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
