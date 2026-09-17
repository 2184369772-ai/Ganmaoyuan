param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$TauriArgs
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$firstArg = if ($TauriArgs.Count -gt 0) { $TauriArgs[0] } else { "" }

if ($firstArg -eq "build") {
    & (Join-Path $PSScriptRoot "prepare-tauri-build.ps1")
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
    }
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
