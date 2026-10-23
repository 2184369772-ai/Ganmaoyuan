function Get-NsisInstalledAppSha256 {
    param([Parameter(Mandatory = $true)][string]$FilePath)

    $bytes = [System.IO.File]::ReadAllBytes($FilePath)
    $text = [System.Text.Encoding]::ASCII.GetString($bytes)
    $marker = "__TAURI_BUNDLE_TYPE_VAR_UNK"
    $offset = $text.IndexOf($marker, [System.StringComparison]::Ordinal)
    if ($offset -lt 0 -or $text.IndexOf($marker, $offset + $marker.Length, [System.StringComparison]::Ordinal) -ge 0) {
        throw "Cannot uniquely locate Tauri's unbundled app marker."
    }

    $replacement = [System.Text.Encoding]::ASCII.GetBytes("__TAURI_BUNDLE_TYPE_VAR_NSS")
    if ($replacement.Length -ne $marker.Length) {
        throw "Tauri NSIS bundle marker length changed unexpectedly."
    }
    [System.Array]::Copy($replacement, 0, $bytes, $offset, $replacement.Length)

    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return (($sha.ComputeHash($bytes) | ForEach-Object { $_.ToString("x2") }) -join "").ToUpperInvariant()
    }
    finally {
        $sha.Dispose()
    }
}
