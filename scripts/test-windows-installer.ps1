[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Repository,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Za-z0-9_.-]+$')]
    [string] $ReleaseTag,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[a-fA-F0-9]{64}$')]
    [string] $ExpectedSha256
)

$ErrorActionPreference = 'Stop'
$apiVersion = '2022-11-28'
$assetNamePattern = '^Ganmaoyuan_.+_x64-setup\.exe$'
$evidenceDir = Join-Path $env:RUNNER_TEMP 'ganmaoyuan-beta-evidence'
$runRoot = Join-Path $evidenceDir ("run-{0}" -f [guid]::NewGuid().ToString('N'))
$installRoot = Join-Path $runRoot 'install'
$dataRoot = Join-Path $runRoot 'user-data'
$transcriptPath = Join-Path $runRoot 'acceptance-transcript.txt'
$reportPath = Join-Path $runRoot 'acceptance-report.json'
$installerLog = Join-Path $runRoot 'installer.log'
$uninstallerLog = Join-Path $runRoot 'uninstaller.log'
$testStartedAt = [DateTime]::UtcNow
$appProcess = $null
$transcriptStarted = $false
$report = [ordered]@{
    repository = $Repository
    releaseTag = $ReleaseTag
    startedAtUtc = $testStartedAt.ToString('o')
    runner = [ordered]@{
        os = [Environment]::OSVersion.VersionString
        powershell = $PSVersionTable.PSVersion.ToString()
        interactiveGuiVerified = $false
    }
    checks = [ordered]@{}
    result = 'running'
    failure = $null
}

function Get-FileDigest([string] $Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-SendToSnapshot([string] $Directory) {
    $snapshot = @{}
    if (Test-Path -LiteralPath $Directory) {
        Get-ChildItem -LiteralPath $Directory -File -Filter '*.lnk' | ForEach-Object {
            $snapshot[$_.FullName] = Get-FileDigest $_.FullName
        }
    }
    return $snapshot
}

function Get-ShortcutInfo([string] $Path) {
    $shell = New-Object -ComObject WScript.Shell
    try {
        $shortcut = $shell.CreateShortcut($Path)
        return [ordered]@{
            path = $Path
            targetPath = [string]$shortcut.TargetPath
            arguments = [string]$shortcut.Arguments
            workingDirectory = [string]$shortcut.WorkingDirectory
        }
    }
    finally {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)
    }
}

function Assert-BaselineShortcutsUnchanged([hashtable] $Baseline) {
    foreach ($path in $Baseline.Keys) {
        if (-not (Test-Path -LiteralPath $path)) {
            throw "Pre-existing SendTo shortcut disappeared: $path"
        }
        $actual = Get-FileDigest $path
        if ($actual -ne $Baseline[$path]) {
            throw "Pre-existing SendTo shortcut bytes changed: $path"
        }
    }
}

function Read-AppErrorEvents([DateTime] $StartTimeUtc, [DateTime] $EndTimeUtc) {
    try {
        $events = @(Get-WinEvent -FilterHashtable @{
            LogName = 'Application'
            StartTime = $StartTimeUtc.ToLocalTime()
            EndTime = $EndTimeUtc.ToLocalTime()
        } -ErrorAction Stop |
            Where-Object { $_.ProviderName -match 'Application Error|Windows Error Reporting' -and $_.Message -match 'Ganmaoyuan|app\.exe' } |
            Select-Object -First 20 TimeCreated, ProviderName, Id, LevelDisplayName, Message)
        return [ordered]@{ available = $true; events = $events }
    }
    catch {
        if ($_.Exception.Message -match 'No events were found') {
            return [ordered]@{ available = $true; events = @() }
        }
        return [ordered]@{ available = $false; reason = $_.Exception.Message; events = @() }
    }
}

New-Item -ItemType Directory -Path $runRoot -Force | Out-Null
Start-Transcript -LiteralPath $transcriptPath -Force | Out-Null
$transcriptStarted = $true

try {
    if (-not $env:GH_TOKEN) {
        throw 'GH_TOKEN was not provided to the workflow.'
    }
    if (-not $env:APPDATA -or -not $env:RUNNER_TEMP) {
        throw 'Expected hosted-runner APPDATA and RUNNER_TEMP environment variables.'
    }

    $headers = @{
        Authorization = "Bearer $env:GH_TOKEN"
        Accept = 'application/vnd.github+json'
        'X-GitHub-Api-Version' = $apiVersion
    }
    $releaseUri = "https://api.github.com/repos/$Repository/releases/tags/$ReleaseTag"
    $release = Invoke-RestMethod -Method Get -Uri $releaseUri -Headers $headers
    if (-not $release.draft) {
        throw "Expected a Draft Release; $ReleaseTag is not draft."
    }
    $asset = @($release.assets | Where-Object { $_.name -match $assetNamePattern }) | Select-Object -First 1
    if (-not $asset) {
        throw "No x64 NSIS installer asset was found in Draft Release $ReleaseTag."
    }
    $report.checks.release = [ordered]@{
        url = $release.html_url
        draft = [bool]$release.draft
        installerAsset = $asset.name
        assetSize = [long]$asset.size
        releaseAssetDigest = [string]$asset.digest
    }

    $installerPath = Join-Path $runRoot $asset.name
    $binaryHeaders = @{
        Authorization = "Bearer $env:GH_TOKEN"
        Accept = 'application/octet-stream'
        'X-GitHub-Api-Version' = $apiVersion
    }
    Invoke-WebRequest -Method Get -Uri $asset.url -Headers $binaryHeaders -OutFile $installerPath
    $actualSha256 = Get-FileDigest $installerPath
    $expectedSha256 = $ExpectedSha256.ToLowerInvariant()
    if ($actualSha256 -ne $expectedSha256) {
        throw "Downloaded installer SHA-256 mismatch. Expected $expectedSha256, got $actualSha256."
    }
    if ($asset.digest -and $asset.digest -ne "sha256:$actualSha256") {
        throw "GitHub asset digest mismatch: $($asset.digest) vs sha256:$actualSha256."
    }

    $checksumAsset = @($release.assets | Where-Object { $_.name -eq 'SHA256SUMS.txt' }) | Select-Object -First 1
    if (-not $checksumAsset) {
        throw 'SHA256SUMS.txt is missing from the Draft Release.'
    }
    $checksumPath = Join-Path $runRoot 'SHA256SUMS.txt'
    Invoke-WebRequest -Method Get -Uri $checksumAsset.url -Headers $binaryHeaders -OutFile $checksumPath
    $checksumText = Get-Content -LiteralPath $checksumPath -Raw -Encoding UTF8
    $escapedName = [regex]::Escape($asset.name)
    $checksumMatch = [regex]::Match($checksumText, "(?im)^\s*([a-f0-9]{64})\s+\*?$escapedName\s*$")
    if (-not $checksumMatch.Success -or $checksumMatch.Groups[1].Value.ToLowerInvariant() -ne $actualSha256) {
        throw 'SHA256SUMS.txt does not contain the downloaded installer digest.'
    }
    $report.checks.release.sha256Expected = $expectedSha256
    $report.checks.release.sha256Actual = $actualSha256
    $report.checks.release.sha256SumsMatched = $true

    $sendToDir = Join-Path $env:APPDATA 'Microsoft\Windows\SendTo'
    New-Item -ItemType Directory -Path $sendToDir -Force | Out-Null
    $preexistingShortcut = Join-Path $sendToDir '感冒院.lnk'
    $fixtureCreated = $false
    if (-not (Test-Path -LiteralPath $preexistingShortcut)) {
        $shell = New-Object -ComObject WScript.Shell
        try {
            $fixture = $shell.CreateShortcut($preexistingShortcut)
            $fixture.TargetPath = Join-Path $PSHOME 'pwsh.exe'
            $fixture.Arguments = '-NoLogo -NoProfile'
            $fixture.Description = 'Temporary Ganmaoyuan installer acceptance fixture'
            $fixture.Save()
        }
        finally {
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)
        }
        $fixtureCreated = $true
    }
    $fixtureInfo = Get-ShortcutInfo $preexistingShortcut
    $baselineShortcuts = Get-SendToSnapshot $sendToDir
    $fixtureDigestBefore = Get-FileDigest $preexistingShortcut
    $report.checks.preexistingSendToShortcut = [ordered]@{
        path = $preexistingShortcut
        createdAsTestFixture = $fixtureCreated
        targetPath = $fixtureInfo.targetPath
        sha256BeforeInstall = $fixtureDigestBefore
    }

    New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null
    $env:GANMAOYUAN_DATA_DIR = $dataRoot
    $sentinelPath = Join-Path $dataRoot 'beta-test-user-data.txt'
    [IO.File]::WriteAllText($sentinelPath, 'GANMAOYUAN_BETA_TEST_DATA_ONLY', [Text.UTF8Encoding]::new($false))
    $sentinelDigest = Get-FileDigest $sentinelPath

    $installer = Start-Process -FilePath $installerPath -ArgumentList @('/S', "/LOG=$installerLog", "/D=$installRoot") -Wait -PassThru
    $report.checks.install = [ordered]@{
        exitCode = $installer.ExitCode
        installDirectory = $installRoot
        installerLogPresent = Test-Path -LiteralPath $installerLog
    }
    if ($installer.ExitCode -ne 0) {
        throw "Silent installer exited with code $($installer.ExitCode)."
    }

    $appPath = Join-Path $installRoot 'app.exe'
    if (-not (Test-Path -LiteralPath $appPath)) {
        $candidate = Get-ChildItem -LiteralPath $installRoot -Filter '*.exe' -File -Recurse |
            Where-Object { $_.BaseName -notmatch '(?i)uninstall|unins' } |
            Select-Object -First 1
        if ($candidate) { $appPath = $candidate.FullName }
    }
    if (-not (Test-Path -LiteralPath $appPath)) {
        throw 'Installed application executable was not found.'
    }
    $appInfo = Get-Item -LiteralPath $appPath
    if ($appInfo.Length -lt 100000) {
        throw "Installed application executable is unexpectedly small ($($appInfo.Length) bytes)."
    }
    Assert-BaselineShortcutsUnchanged $baselineShortcuts

    $ownedShortcuts = @()
    Get-ChildItem -LiteralPath $sendToDir -File -Filter '*.lnk' | ForEach-Object {
        try {
            $info = Get-ShortcutInfo $_.FullName
            if ($info.targetPath -and
                [IO.Path]::GetFullPath($info.targetPath) -ieq [IO.Path]::GetFullPath($appPath) -and
                $info.arguments -eq '--shell-source windowsSendTo') {
                $ownedShortcuts += $info
            }
        }
        catch {
            Write-Warning "Unable to inspect unrelated SendTo link $($_.FullName): $($_.Exception.Message)"
        }
    }
    if ($ownedShortcuts.Count -ne 1) {
        throw "Expected exactly one app-owned SendTo shortcut after install; found $($ownedShortcuts.Count)."
    }
    $ownedShortcutPath = $ownedShortcuts[0].path
    if ($ownedShortcutPath -ieq $preexistingShortcut) {
        throw 'Installer replaced the pre-existing same-name SendTo shortcut.'
    }
    $report.checks.install.installedExecutable = $appPath
    $report.checks.install.installedExecutableBytes = [long]$appInfo.Length
    $report.checks.install.createdSendToShortcut = $ownedShortcuts[0]
    $installEvents = Read-AppErrorEvents $testStartedAt ([DateTime]::UtcNow)
    $report.checks.install.applicationErrorEventQuery = $installEvents
    if ($installEvents.events.Count -gt 0) {
        throw 'Windows Application log contains a Ganmaoyuan/app.exe error event during installation.'
    }

    $launchStartedAt = [DateTime]::UtcNow
    $appProcess = Start-Process -FilePath $appPath -PassThru
    Start-Sleep -Seconds 12
    $appProcess.Refresh()
    $launchState = [ordered]@{
        pid = $appProcess.Id
        startedAtUtc = $launchStartedAt.ToString('o')
        checkedAfterSeconds = 12
        processAlive = -not $appProcess.HasExited
        exitCode = $null
        mainWindowHandle = [string]$appProcess.MainWindowHandle
        mainWindowTitle = [string]$appProcess.MainWindowTitle
        interactiveGuiVerified = $false
    }
    if ($appProcess.HasExited) {
        $launchState.exitCode = $appProcess.ExitCode
        if ($appProcess.ExitCode -ne 0) {
            throw "First-launch app process exited abnormally with code $($appProcess.ExitCode)."
        }
    }
    $launchEvents = Read-AppErrorEvents $launchStartedAt ([DateTime]::UtcNow)
    $launchState.applicationErrorEventQuery = $launchEvents
    $report.checks.firstLaunchProcess = $launchState
    if ($launchEvents.events.Count -gt 0) {
        throw 'Windows Application log contains a Ganmaoyuan/app.exe error event during first launch.'
    }

    if (-not $appProcess.HasExited) {
        [void]$appProcess.CloseMainWindow()
        if (-not $appProcess.WaitForExit(5000)) {
            & "$env:WINDIR\System32\taskkill.exe" /PID $appProcess.Id /T /F | Out-Null
            [void]$appProcess.WaitForExit(5000)
        }
        $appProcess.Refresh()
        if (-not $appProcess.HasExited) {
            throw "Could not stop the test-launched app process (PID $($appProcess.Id)) before uninstall."
        }
    }
    $appProcess = $null

    $uninstaller = Get-ChildItem -LiteralPath $installRoot -File -Recurse |
        Where-Object { $_.Extension -ieq '.exe' -and $_.BaseName -match '(?i)uninstall|unins' } |
        Select-Object -First 1
    if (-not $uninstaller) {
        throw 'NSIS uninstaller executable was not found under the install directory.'
    }
    $uninstall = Start-Process -FilePath $uninstaller.FullName -ArgumentList @('/S', "/LOG=$uninstallerLog") -Wait -PassThru
    $report.checks.uninstall = [ordered]@{
        uninstallerPath = $uninstaller.FullName
        exitCode = $uninstall.ExitCode
        uninstallerLogPresent = Test-Path -LiteralPath $uninstallerLog
        appExecutableRemoved = -not (Test-Path -LiteralPath $appPath)
        installDirectoryPresentAfter = Test-Path -LiteralPath $installRoot
        createdSendToShortcutRemoved = -not (Test-Path -LiteralPath $ownedShortcutPath)
    }
    if ($uninstall.ExitCode -ne 0) {
        throw "Silent uninstaller exited with code $($uninstall.ExitCode)."
    }
    if (Test-Path -LiteralPath $appPath) {
        throw 'Application executable remained after uninstall.'
    }
    if (Test-Path -LiteralPath $ownedShortcutPath) {
        throw 'Installer-owned SendTo shortcut remained after uninstall.'
    }
    Assert-BaselineShortcutsUnchanged $baselineShortcuts
    if ((Get-FileDigest $preexistingShortcut) -ne $fixtureDigestBefore) {
        throw 'Pre-existing same-name SendTo shortcut changed during install/uninstall.'
    }
    if (-not (Test-Path -LiteralPath $sentinelPath) -or (Get-FileDigest $sentinelPath) -ne $sentinelDigest) {
        throw 'Fictional user data was removed or changed by uninstall.'
    }
    $report.checks.uninstall.preexistingSendToShortcutsUnchanged = $true
    $report.checks.uninstall.fictionalUserDataPreserved = $true
    $report.checks.uninstall.sentinelSha256 = $sentinelDigest

    $report.result = 'passed'
}
catch {
    $report.result = 'failed'
    $report.failure = $_.Exception.Message
    throw
}
finally {
    if ($appProcess) {
        try {
            $appProcess.Refresh()
            if (-not $appProcess.HasExited) {
                & "$env:WINDIR\System32\taskkill.exe" /PID $appProcess.Id /T /F | Out-Null
            }
        }
        catch { }
    }
    $report.finishedAtUtc = [DateTime]::UtcNow.ToString('o')
    $report.evidence = [ordered]@{
        transcript = $transcriptPath
        installerLog = $installerLog
        uninstallerLog = $uninstallerLog
        report = $reportPath
        interactiveGuiVerified = $false
    }
    $report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $reportPath -Encoding UTF8
    if ($transcriptStarted) {
        Stop-Transcript | Out-Null
    }
}
