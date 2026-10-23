param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,
    [Parameter(Mandatory = $true)]
    [string]$EvidenceDirectory,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedSha256,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedAppSha256,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedAppPath,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedVersion,
    [Parameter(Mandatory = $true)]
    [string]$BaselineInstallerPath,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedBaselineSha256
)

$ErrorActionPreference = "Stop"
if ($env:GITHUB_ACTIONS -ne "true") {
    throw "安装/卸载验收只允许在临时 GitHub Actions Windows Runner 执行。"
}
if ([string]::IsNullOrWhiteSpace($env:RUNNER_TEMP) -or [string]::IsNullOrWhiteSpace($env:APPDATA) -or [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    throw "临时 Runner、APPDATA 或 LOCALAPPDATA 未配置；未开始安装验收。"
}

$runnerTemp = [System.IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\')
$installer = [System.IO.Path]::GetFullPath($InstallerPath)
$baselineInstaller = [System.IO.Path]::GetFullPath($BaselineInstallerPath)
$expectedApp = [System.IO.Path]::GetFullPath($ExpectedAppPath)
$evidenceRoot = [System.IO.Path]::GetFullPath($EvidenceDirectory)
if (-not $installer.StartsWith($runnerTemp + '\', [System.StringComparison]::OrdinalIgnoreCase) -or
    -not $baselineInstaller.StartsWith($runnerTemp + '\', [System.StringComparison]::OrdinalIgnoreCase) -or
    -not $expectedApp.StartsWith($runnerTemp + '\', [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "待验收安装包必须位于临时 Runner 目录内。"
}
if (-not (Test-Path -LiteralPath $installer -PathType Leaf) -or
    -not (Test-Path -LiteralPath $baselineInstaller -PathType Leaf) -or
    -not (Test-Path -LiteralPath $expectedApp -PathType Leaf)) {
    throw "找不到待验收安装包。"
}
if (-not $evidenceRoot.StartsWith($runnerTemp + '\', [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "验收证据目录必须位于临时 Runner 目录内。"
}
New-Item -ItemType Directory -Force -Path $evidenceRoot | Out-Null

$testRoot = Join-Path $runnerTemp ("ganmaoyuan-installer-test-" + [guid]::NewGuid().ToString("N"))
$installRoot = Join-Path $testRoot "Install"
$outsideFile = Join-Path $testRoot "unrelated-user-file.txt"
$dataRoot = Join-Path $env:APPDATA "com.ganmaoyuan.desktop"
$localDataRoot = Join-Path $env:LOCALAPPDATA "com.ganmaoyuan.desktop"
$dataMarker = Join-Path $dataRoot "m2-uninstall-preserve-marker.txt"
$localDataMarker = Join-Path $localDataRoot "m2-local-uninstall-preserve-marker.txt"
$desktopLink = Join-Path ([Environment]::GetFolderPath("Desktop")) "Ganmaoyuan.lnk"
$sendToLink = Join-Path $env:APPDATA "Microsoft\Windows\SendTo\Ganmaoyuan - SendTo.lnk"
$startMenuLink = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Ganmaoyuan.lnk"
# Model an old-version shortcut: the same product link still points at the
# previous executable name inside the install directory. It is not owned by
# this installer instance and must survive install/update/uninstall unchanged.
$testTarget = Join-Path $installRoot "Ganmaoyuan.exe"
$script:checks = [System.Collections.Generic.List[object]]::new()
$script:startedProcess = $null
$script:sentinelSnapshots = @{}
$installerHashBefore = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToUpperInvariant()
if ($ExpectedSha256.ToUpperInvariant() -ne $installerHashBefore) {
    throw "验收输入的 SHA-256 与本次构建安装包不一致。"
}
$baselineInstallerHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $baselineInstaller).Hash.ToUpperInvariant()
if ($ExpectedBaselineSha256.ToUpperInvariant() -ne $baselineInstallerHash) {
    throw "公开旧版基线安装包的 SHA-256 不匹配。"
}
if ($ExpectedAppSha256 -notmatch '^[0-9A-Fa-f]{64}$') {
    throw "本次构建 app.exe 的 SHA-256 格式无效。"
}
if ($ExpectedVersion -notmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$') {
    throw "本次构建版本号格式无效。"
}

function Add-Check {
    param([string]$Name, [bool]$Passed, [string]$Detail = "")
    $script:checks.Add([pscustomobject]@{
        name = $Name
        passed = $Passed
        detail = $Detail
    })
    if (-not $Passed) {
        throw "验收失败：$Name。$Detail"
    }
}

function New-Shortcut {
    param([string]$Path, [string]$Target)
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path) | Out-Null
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($Path)
    $shortcut.TargetPath = $Target
    $shortcut.Arguments = "--m2-sentinel"
    $shortcut.WorkingDirectory = Split-Path -Parent $Target
    $shortcut.Save()
}

function Get-ShortcutSnapshot {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $null
    }
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($Path)
    [pscustomobject]@{
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToUpperInvariant()
        target = $shortcut.TargetPath
        arguments = $shortcut.Arguments
        workingDirectory = $shortcut.WorkingDirectory
    }
}

function Assert-SentinelPreserved {
    param([string]$Path, [string]$Label)
    $current = Get-ShortcutSnapshot $Path
    $before = $script:sentinelSnapshots[$Path]
    Add-Check "preserve existing $Label shortcut" ($null -ne $current -and $null -ne $before -and $current.sha256 -eq $before.sha256 -and $current.target -eq $before.target -and $current.arguments -eq $before.arguments) "An existing same-name shortcut with a different target must remain byte-for-byte unchanged."
}

function Invoke-Installer {
    param([string]$Name, [string[]]$ExtraArguments = @())
    $log = Join-Path $evidenceRoot "$Name.log"
    $arguments = @("/S", "/LOG=$log") + $ExtraArguments
    $process = Start-Process -FilePath $installer -ArgumentList $arguments -PassThru -Wait
    Add-Check "$Name installer exit code" ($process.ExitCode -eq 0) "exit=$($process.ExitCode)"
    return $log
}

function Invoke-Uninstaller {
    param([string]$Name, [string]$InstallDirectory = $installRoot)
    $uninstaller = Join-Path $InstallDirectory "uninstall.exe"
    Add-Check "$Name uninstaller exists" (Test-Path -LiteralPath $uninstaller -PathType Leaf)
    $log = Join-Path $evidenceRoot "$Name.log"
    $process = Start-Process -FilePath $uninstaller -ArgumentList @("/S", "/LOG=$log") -PassThru -Wait
    Add-Check "$Name uninstaller exit code" ($process.ExitCode -eq 0) "exit=$($process.ExitCode)"
    return $log
}

function Get-TestInstallRegistryKey {
    param([string]$InstallDirectory = $installRoot)
    $candidates = @(
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ganmaoyuan",
        "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ganmaoyuan",
        "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Ganmaoyuan"
    )
    foreach ($candidate in $candidates) {
        if (-not (Test-Path -LiteralPath $candidate)) {
            continue
        }
        $properties = Get-ItemProperty -LiteralPath $candidate
        $location = ([string]$properties.InstallLocation).Trim('"')
        if (-not [string]::IsNullOrWhiteSpace($location) -and [System.IO.Path]::GetFullPath($location).TrimEnd('\') -eq $InstallDirectory.TrimEnd('\')) {
            return $candidate
        }
    }
    return $null
}

function Stop-TestApp {
    if ($null -eq $script:startedProcess) {
        return
    }
    try {
        $script:startedProcess.Refresh()
        if (-not $script:startedProcess.HasExited) {
            [void]$script:startedProcess.CloseMainWindow()
            if (-not $script:startedProcess.WaitForExit(10000)) {
                Stop-Process -Id $script:startedProcess.Id -Force -ErrorAction SilentlyContinue
                $script:startedProcess.WaitForExit()
            }
        }
    }
    catch {
        # Only the exact process started from this isolated test install is cleaned up.
    }
    $script:startedProcess = $null
}

function Assert-TestOwnedShortcutRemoved {
    param([string]$Path, [string]$Label)
    Add-Check "remove installer-managed $Label shortcut" (-not (Test-Path -LiteralPath $Path)) "Only the shortcut recorded as created by this test installation should be removed."
}

$failure = $null
try {
    New-Item -ItemType Directory -Force -Path $testRoot | Out-Null
    Set-Content -Encoding ASCII -LiteralPath $outsideFile -Value "unrelated user data marker"
    New-Item -ItemType Directory -Force -Path $dataRoot, $localDataRoot | Out-Null
    Set-Content -Encoding ASCII -LiteralPath $dataMarker -Value "preserve-app-data"
    Set-Content -Encoding ASCII -LiteralPath $localDataMarker -Value "preserve-local-data"

    foreach ($link in @($desktopLink, $sendToLink, $startMenuLink)) {
        if (Test-Path -LiteralPath $link) {
            throw "Hosted Runner 中已存在同名快捷方式；为保护其原内容，本脚本拒绝覆盖并停止验收。"
        }
        New-Shortcut $link $testTarget
        $script:sentinelSnapshots[$link] = Get-ShortcutSnapshot $link
    }

    Invoke-Installer "collision-install" @("/D=$installRoot") | Out-Null
    $installedApp = Join-Path $installRoot "app.exe"
    Add-Check "application installed" (Test-Path -LiteralPath $installedApp -PathType Leaf)
    $expectedAppHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $expectedApp).Hash.ToUpperInvariant()
    $installedAppHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $installedApp).Hash.ToUpperInvariant()
    $installedAppItem = Get-Item -LiteralPath $installedApp
    $installedAppVersion = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($installedApp)
    $expectedAppCopy = Join-Path $evidenceRoot "expected-build-app.exe"
    $installedAppCopy = Join-Path $evidenceRoot "installed-app.exe"
    Copy-Item -LiteralPath $expectedApp -Destination $expectedAppCopy -Force
    Copy-Item -LiteralPath $installedApp -Destination $installedAppCopy -Force
    $appHashDetail = "expected=$($ExpectedAppSha256.ToUpperInvariant()); artifact=$expectedAppHash; actual=$installedAppHash; size=$($installedAppItem.Length); fileVersion=$($installedAppVersion.FileVersion); productVersion=$($installedAppVersion.ProductVersion); expectedPath=$expectedApp; installedPath=$installedApp"
    Add-Check "installed application matches build provenance" ($installedAppHash -eq $ExpectedAppSha256.ToUpperInvariant()) $appHashDetail
    $installRegistryKey = Get-TestInstallRegistryKey
    Add-Check "isolated install registry entry found" ($null -ne $installRegistryKey) "The test must only modify the registry entry owned by this temporary installation."
    foreach ($item in @(
        @{ path = $desktopLink; label = "desktop" },
        @{ path = $sendToLink; label = "SendTo" },
        @{ path = $startMenuLink; label = "Start Menu" }
    )) {
        Assert-SentinelPreserved $item.path $item.label
    }

    Set-ItemProperty -LiteralPath $installRegistryKey -Name "MainBinaryName" -Value "..\unrelated-user-file.txt"
    Invoke-Installer "update-mode-install" @("/UPDATE", "/D=$installRoot") | Out-Null
    Add-Check "application remains after update-mode install" (Test-Path -LiteralPath $installedApp -PathType Leaf)
    Add-Check "data remains after update-mode install" ((Get-Content -Raw -LiteralPath $dataMarker) -eq "preserve-app-data")
    Add-Check "untrusted legacy binary name cannot delete outside file" ((Test-Path -LiteralPath $outsideFile -PathType Leaf) -and (Get-Content -Raw -LiteralPath $outsideFile) -eq "unrelated user data marker")
    foreach ($item in @(
        @{ path = $desktopLink; label = "desktop" },
        @{ path = $sendToLink; label = "SendTo" },
        @{ path = $startMenuLink; label = "Start Menu" }
    )) {
        Assert-SentinelPreserved $item.path $item.label
    }

    # Exercise the installer's real semver-upgrade branch without using any
    # pre-existing user installation. The lower version marker and all data
    # are synthetic and live only on the ephemeral hosted runner.
    Set-ItemProperty -LiteralPath $installRegistryKey -Name "DisplayVersion" -Value "0.0.1"
    Invoke-Installer "version-upgrade-install" @("/P", "/D=$installRoot") | Out-Null
    Add-Check "application remains after version-upgrade path" (Test-Path -LiteralPath $installedApp -PathType Leaf)
    Add-Check "app data remains after version-upgrade path" ((Test-Path -LiteralPath $dataMarker) -and (Get-Content -Raw -LiteralPath $dataMarker) -eq "preserve-app-data")
    Add-Check "local app data remains after version-upgrade path" ((Test-Path -LiteralPath $localDataMarker) -and (Get-Content -Raw -LiteralPath $localDataMarker) -eq "preserve-local-data")
    foreach ($item in @(
        @{ path = $desktopLink; label = "desktop" },
        @{ path = $sendToLink; label = "SendTo" },
        @{ path = $startMenuLink; label = "Start Menu" }
    )) {
        Assert-SentinelPreserved $item.path $item.label
    }

    Invoke-Uninstaller "collision-uninstall" | Out-Null
    foreach ($item in @(
        @{ path = $desktopLink; label = "desktop" },
        @{ path = $sendToLink; label = "SendTo" },
        @{ path = $startMenuLink; label = "Start Menu" }
    )) {
        Assert-SentinelPreserved $item.path $item.label
    }
    Add-Check "application data preserved by uninstall" ((Test-Path -LiteralPath $dataMarker) -and (Get-Content -Raw -LiteralPath $dataMarker) -eq "preserve-app-data")
    Add-Check "local application data preserved by uninstall" ((Test-Path -LiteralPath $localDataMarker) -and (Get-Content -Raw -LiteralPath $localDataMarker) -eq "preserve-local-data")
    Add-Check "unrelated file outside installation preserved" ((Get-Content -Raw -LiteralPath $outsideFile) -eq "unrelated user data marker")

    foreach ($link in @($desktopLink, $sendToLink, $startMenuLink)) {
        $snapshot = Get-ShortcutSnapshot $link
        if ($null -eq $snapshot -or $snapshot.sha256 -ne $script:sentinelSnapshots[$link].sha256) {
            throw "验收脚本拒绝清理非测试创建或已变化的快捷方式：$(Split-Path -Leaf $link)"
        }
        Remove-Item -LiteralPath $link
    }

    Invoke-Installer "managed-shortcuts-install" @("/D=$installRoot") | Out-Null
    $installedApp = Join-Path $installRoot "app.exe"
    Add-Check "application installed without shortcut collisions" (Test-Path -LiteralPath $installedApp -PathType Leaf)
    $installedAppHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $installedApp).Hash.ToUpperInvariant()
    $installedAppItem = Get-Item -LiteralPath $installedApp
    $installedAppVersion = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($installedApp)
    $appHashDetail = "expected=$($ExpectedAppSha256.ToUpperInvariant()); actual=$installedAppHash; size=$($installedAppItem.Length); fileVersion=$($installedAppVersion.FileVersion); productVersion=$($installedAppVersion.ProductVersion); path=$installedApp"
    Add-Check "installed application matches build provenance after shortcut migration" ($installedAppHash -eq $ExpectedAppSha256.ToUpperInvariant()) $appHashDetail
    foreach ($link in @($desktopLink, $sendToLink, $startMenuLink)) {
        $snapshot = Get-ShortcutSnapshot $link
        Add-Check "installer created managed shortcut $(Split-Path -Leaf $link)" ($null -ne $snapshot -and $snapshot.target -eq $installedApp) "Target must be the executable in this isolated install directory."
    }

    $legacyBinary = Join-Path $installRoot "Ganmaoyuan.exe"
    Set-Content -Encoding ASCII -LiteralPath $legacyBinary -Value "legacy binary marker"
    New-Shortcut $desktopLink $legacyBinary
    New-Shortcut $startMenuLink $legacyBinary
    Set-ItemProperty -LiteralPath $installRegistryKey -Name "MainBinaryName" -Value "Ganmaoyuan.exe"
    Invoke-Installer "managed-shortcuts-update" @("/UPDATE", "/D=$installRoot") | Out-Null
    Add-Check "application survives update-mode reinstall" (Test-Path -LiteralPath $installedApp -PathType Leaf)
    Add-Check "known legacy executable is removed only inside install directory" (-not (Test-Path -LiteralPath $legacyBinary))
    foreach ($link in @($desktopLink, $startMenuLink)) {
        $snapshot = Get-ShortcutSnapshot $link
        Add-Check "known legacy shortcut migrated $(Split-Path -Leaf $link)" ($null -ne $snapshot -and $snapshot.target -eq $installedApp) "A shortcut targeting the exact allowlisted legacy executable should be retargeted to app.exe."
    }

    $script:startedProcess = Start-Process -FilePath $installedApp -PassThru
    Start-Sleep -Seconds 10
    $script:startedProcess.Refresh()
    Add-Check "application process started" (-not $script:startedProcess.HasExited) "Process liveness only; this does not prove interactive GUI correctness."
    Stop-TestApp

    Invoke-Uninstaller "managed-shortcuts-uninstall" | Out-Null
    Assert-TestOwnedShortcutRemoved $desktopLink "desktop"
    Assert-TestOwnedShortcutRemoved $sendToLink "SendTo"
    Assert-TestOwnedShortcutRemoved $startMenuLink "Start Menu"
    Add-Check "application data preserved after managed uninstall" ((Test-Path -LiteralPath $dataMarker) -and (Get-Content -Raw -LiteralPath $dataMarker) -eq "preserve-app-data")
    Add-Check "local application data preserved after managed uninstall" ((Test-Path -LiteralPath $localDataMarker) -and (Get-Content -Raw -LiteralPath $localDataMarker) -eq "preserve-local-data")
    Add-Check "unrelated file outside installation preserved after uninstall" ((Get-Content -Raw -LiteralPath $outsideFile) -eq "unrelated user data marker")

    $baselineInstallRoot = Join-Path $testRoot "BaselineInstall"
    $baselineInstallLog = Join-Path $evidenceRoot "public-v0.1.1-baseline-install.log"
    $baselineProcess = Start-Process -FilePath $baselineInstaller -ArgumentList @("/S", "/LOG=$baselineInstallLog", "/D=$baselineInstallRoot") -PassThru -Wait
    Add-Check "public v0.1.1 baseline installer exit code" ($baselineProcess.ExitCode -eq 0) "exit=$($baselineProcess.ExitCode)"
    $baselineApp = Join-Path $baselineInstallRoot "app.exe"
    Add-Check "public v0.1.1 baseline installed" (Test-Path -LiteralPath $baselineApp -PathType Leaf)
    $baselineRegistryKey = Get-TestInstallRegistryKey $baselineInstallRoot
    Add-Check "public baseline install registration found" ($null -ne $baselineRegistryKey)
    $baselineVersion = [string](Get-ItemProperty -LiteralPath $baselineRegistryKey).DisplayVersion
    Add-Check "baseline registry reports v0.1.1" ($baselineVersion -eq "0.1.1") "version=$baselineVersion"
    foreach ($link in @($desktopLink, $sendToLink, $startMenuLink)) {
        $snapshot = Get-ShortcutSnapshot $link
        Add-Check "baseline owns expected shortcut $(Split-Path -Leaf $link)" ($null -ne $snapshot -and $snapshot.target -eq $baselineApp)
    }

    Set-Content -Encoding ASCII -LiteralPath $dataMarker -Value "preserve-cross-release-app-data"
    Set-Content -Encoding ASCII -LiteralPath $localDataMarker -Value "preserve-cross-release-local-data"
    Invoke-Installer "cross-release-v0.1.1-to-current" @("/D=$baselineInstallRoot") | Out-Null
    $upgradedApp = Join-Path $baselineInstallRoot "app.exe"
    Add-Check "cross-release upgrade installed current application" (Test-Path -LiteralPath $upgradedApp -PathType Leaf)
    Add-Check "cross-release installed app matches build provenance" ((Get-FileHash -Algorithm SHA256 -LiteralPath $upgradedApp).Hash.ToUpperInvariant() -eq $ExpectedAppSha256.ToUpperInvariant())
    $upgradedRegistryKey = Get-TestInstallRegistryKey $baselineInstallRoot
    Add-Check "cross-release upgrade registration found" ($null -ne $upgradedRegistryKey)
    $upgradedVersion = [string](Get-ItemProperty -LiteralPath $upgradedRegistryKey).DisplayVersion
    Add-Check "cross-release registry reports current version" ($upgradedVersion -eq $ExpectedVersion) "version=$upgradedVersion"
    Add-Check "cross-release upgrade preserves app data" ((Get-Content -Raw -LiteralPath $dataMarker) -eq "preserve-cross-release-app-data")
    Add-Check "cross-release upgrade preserves local app data" ((Get-Content -Raw -LiteralPath $localDataMarker) -eq "preserve-cross-release-local-data")
    foreach ($link in @($desktopLink, $sendToLink, $startMenuLink)) {
        $snapshot = Get-ShortcutSnapshot $link
        Add-Check "cross-release upgrade retains managed shortcut $(Split-Path -Leaf $link)" ($null -ne $snapshot -and $snapshot.target -eq $upgradedApp)
    }

    $script:startedProcess = Start-Process -FilePath $upgradedApp -PassThru
    Start-Sleep -Seconds 10
    $script:startedProcess.Refresh()
    Add-Check "upgraded application process started" (-not $script:startedProcess.HasExited) "Process liveness only; this does not prove interactive GUI correctness."
    Stop-TestApp
    Invoke-Uninstaller "cross-release-upgrade-uninstall" $baselineInstallRoot | Out-Null
    Assert-TestOwnedShortcutRemoved $desktopLink "cross-release desktop"
    Assert-TestOwnedShortcutRemoved $sendToLink "cross-release SendTo"
    Assert-TestOwnedShortcutRemoved $startMenuLink "cross-release Start Menu"
    Add-Check "cross-release uninstall preserves app data" ((Get-Content -Raw -LiteralPath $dataMarker) -eq "preserve-cross-release-app-data")
    Add-Check "cross-release uninstall preserves local app data" ((Get-Content -Raw -LiteralPath $localDataMarker) -eq "preserve-cross-release-local-data")
    Add-Check "installer hash unchanged by validation" ((Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToUpperInvariant() -eq $installerHashBefore)
    Add-Check "baseline installer hash unchanged by validation" ((Get-FileHash -Algorithm SHA256 -LiteralPath $baselineInstaller).Hash.ToUpperInvariant() -eq $baselineInstallerHash)
}
catch {
    $failure = $_.Exception.Message
}
finally {
    Stop-TestApp
    $result = [ordered]@{
        schemaVersion = 1
        testEnvironment = "ephemeral GitHub Actions Windows hosted runner"
        completedAtUtc = [DateTime]::UtcNow.ToString("o")
        currentVersion = $ExpectedVersion
        installerFile = Split-Path -Leaf $installer
        installerSha256 = $installerHashBefore
        appSha256 = $ExpectedAppSha256.ToUpperInvariant()
        baselineInstallerFile = Split-Path -Leaf $baselineInstaller
        baselineInstallerSha256 = $baselineInstallerHash
        installDirectory = $installRoot
        isolatedTestDirectory = $testRoot
        interactiveGuiVerified = $false
        applicationProcessStartupChecked = ($script:checks | Where-Object { $_.name -like "*application process started" -and $_.passed }).Count -gt 0
        crossReleaseUpgradeChecked = ($script:checks | Where-Object { $_.name -eq "cross-release registry reports current version" -and $_.passed }).Count -gt 0
        dataDeletionExplicitlySelected = $false
        failure = $failure
        checks = @($script:checks)
    }
    $result | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $evidenceRoot "install-validation.json")
    if (Test-Path -LiteralPath $testRoot) {
        $manifest = [ordered]@{
            testDirectory = Split-Path -Leaf $testRoot
            retainedUnderRunnerTemp = $true
            userDataAndSentinels = "synthetic test data only"
        }
        $manifest | ConvertTo-Json | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $evidenceRoot "test-fixture-manifest.json")
    }
}

if ($failure) {
    throw $failure
}
Write-Output "Windows install/update/uninstall checks passed. Interactive GUI verification was not performed."
