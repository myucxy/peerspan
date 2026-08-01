[CmdletBinding()]
param(
    [ValidateSet("Release")]
    [string]$Configuration = "Release",

    [ValidateSet("x64")]
    [string]$Platform = "x64"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$rustRoot = "D:\Dev\Env\Rust"
$cargoBin = Join-Path $rustRoot "cargo\bin"
if (-not (Get-Command cargo.exe -ErrorAction SilentlyContinue)) {
    $cargo = Join-Path $cargoBin "cargo.exe"
    if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
        throw "Cargo was not found on PATH or at the PeerSpan environment location: $cargo"
    }
    $env:RUSTUP_HOME = Join-Path $rustRoot "rustup"
    $env:CARGO_HOME = Join-Path $rustRoot "cargo"
    $env:PATH = "$cargoBin;$env:PATH"
}

$targetRoot = Join-Path $repositoryRoot "target"
$stageRoot = Join-Path $targetRoot "installer-resources\driver"
$gameStreamStageRoot = Join-Path $targetRoot "installer-resources\gamestream"
$allowedStagePrefix = [IO.Path]::GetFullPath($targetRoot).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$resolvedStageRoot = [IO.Path]::GetFullPath($stageRoot)
$resolvedGameStreamStageRoot = [IO.Path]::GetFullPath($gameStreamStageRoot)
foreach ($resolvedPath in @($resolvedStageRoot, $resolvedGameStreamStageRoot)) {
    if (-not $resolvedPath.StartsWith($allowedStagePrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Installer staging path escaped the repository target directory: $resolvedPath"
    }
}

$gameStreamEnvironmentRoot = "D:\Dev\Env\PeerSpan"
$downloadRoot = Join-Path $gameStreamEnvironmentRoot "downloads"
$runtimeRoot = Join-Path $gameStreamEnvironmentRoot "runtimes"
New-Item -ItemType Directory -Path $downloadRoot -Force | Out-Null
New-Item -ItemType Directory -Path $runtimeRoot -Force | Out-Null

$vddPackage = @{
    Name = "VirtualDrivers VDD"
    Release = "25.7.23"
    DriverVersion = "11.30.4.434"
    Archive = Join-Path $downloadRoot "VirtualDisplayDriver-x86.Driver.Only-25.7.23.zip"
    Uri = "https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/download/25.7.23/VirtualDisplayDriver-x86.Driver.Only.zip"
    Sha256 = "E24210692B442B39AF763536330CE78B423F19342B7A7792C26DE3944E418B3A"
    Runtime = Join-Path $runtimeRoot "virtual-display-driver-25.7.23"
    PackageDirectory = "VirtualDisplayDriver"
}
if (-not (Test-Path -LiteralPath $vddPackage.Archive -PathType Leaf)) {
    Write-Output "Downloading $($vddPackage.Name) $($vddPackage.Release) to $($vddPackage.Archive)..."
    Invoke-WebRequest -Uri $vddPackage.Uri -OutFile $vddPackage.Archive
}
$vddArchiveHash = (Get-FileHash -LiteralPath $vddPackage.Archive -Algorithm SHA256).Hash
if ($vddArchiveHash -ne $vddPackage.Sha256) {
    throw "$($vddPackage.Name) archive hash mismatch. Expected $($vddPackage.Sha256), received $vddArchiveHash."
}
$vddRuntimePackage = Join-Path $vddPackage.Runtime $vddPackage.PackageDirectory
if (-not (Test-Path -LiteralPath (Join-Path $vddRuntimePackage "MttVDD.inf") -PathType Leaf)) {
    New-Item -ItemType Directory -Path $vddPackage.Runtime -Force | Out-Null
    Expand-Archive -LiteralPath $vddPackage.Archive -DestinationPath $vddPackage.Runtime -Force
}

if (Test-Path -LiteralPath $resolvedStageRoot) {
    Remove-Item -LiteralPath $resolvedStageRoot -Recurse -Force
}

$stagedPackage = Join-Path $resolvedStageRoot "package"
New-Item -ItemType Directory -Path $stagedPackage -Force | Out-Null

$files = @(
    @{
        Source = Join-Path $repositoryRoot "native\vdd\install.ps1"
        Destination = Join-Path $resolvedStageRoot "install.ps1"
    },
    @{
        Source = Join-Path $repositoryRoot "native\vdd\uninstall.ps1"
        Destination = Join-Path $resolvedStageRoot "uninstall.ps1"
    },
    @{
        Source = Join-Path $vddRuntimePackage "MttVDD.inf"
        Destination = Join-Path $stagedPackage "MttVDD.inf"
    },
    @{
        Source = Join-Path $vddRuntimePackage "mttvdd.cat"
        Destination = Join-Path $stagedPackage "mttvdd.cat"
    },
    @{
        Source = Join-Path $vddRuntimePackage "MttVDD.dll"
        Destination = Join-Path $stagedPackage "MttVDD.dll"
    },
    @{
        Source = Join-Path $vddRuntimePackage "vdd_settings.xml"
        Destination = Join-Path $stagedPackage "vdd_settings.xml"
    }
)

foreach ($file in $files) {
    if (-not (Test-Path -LiteralPath $file.Source -PathType Leaf)) {
        throw "Required installer resource was not found: $($file.Source)"
    }
    Copy-Item -LiteralPath $file.Source -Destination $file.Destination -Force
}

$runtimePackages = @(
    @{
        Name = "Sunshine"
        Version = "v2026.516.143833"
        Archive = Join-Path $downloadRoot "Sunshine-Windows-AMD64-portable.zip"
        Uri = "https://github.com/LizardByte/Sunshine/releases/download/v2026.516.143833/Sunshine-Windows-AMD64-portable.zip"
        Sha256 = "0A3AF3DDE43B8F2C94FFE04B850AD736D6E1BE2B75906779D7094A5AD9D4783B"
        Runtime = Join-Path $runtimeRoot "sunshine"
        Executable = "Sunshine\sunshine.exe"
    },
    @{
        Name = "Moonlight"
        Version = "v6.1.0"
        Archive = Join-Path $downloadRoot "MoonlightPortable-x64-6.1.0.zip"
        Uri = "https://github.com/moonlight-stream/moonlight-qt/releases/download/v6.1.0/MoonlightPortable-x64-6.1.0.zip"
        Sha256 = "95F4D0853A31C7FCED4B6D233DDF55EE41720963F2E2620A9CB49A21D112AED1"
        Runtime = Join-Path $runtimeRoot "moonlight"
        Executable = "Moonlight.exe"
    }
)

foreach ($package in $runtimePackages) {
    if (-not (Test-Path -LiteralPath $package.Archive -PathType Leaf)) {
        Write-Output "Downloading $($package.Name) $($package.Version) to $($package.Archive)..."
        Invoke-WebRequest -Uri $package.Uri -OutFile $package.Archive
    }
    $archiveHash = (Get-FileHash -LiteralPath $package.Archive -Algorithm SHA256).Hash
    if ($archiveHash -ne $package.Sha256) {
        throw "$($package.Name) archive hash mismatch. Expected $($package.Sha256), received $archiveHash."
    }
    $runtimeExecutable = Join-Path $package.Runtime $package.Executable
    if (-not (Test-Path -LiteralPath $runtimeExecutable -PathType Leaf)) {
        New-Item -ItemType Directory -Path $package.Runtime -Force | Out-Null
        Expand-Archive -LiteralPath $package.Archive -DestinationPath $package.Runtime -Force
    }
}

if (Test-Path -LiteralPath $resolvedGameStreamStageRoot) {
    Remove-Item -LiteralPath $resolvedGameStreamStageRoot -Recurse -Force
}
$stagedSunshine = Join-Path $resolvedGameStreamStageRoot "sunshine"
$stagedMoonlight = Join-Path $resolvedGameStreamStageRoot "moonlight"
$stagedLicenses = Join-Path $resolvedGameStreamStageRoot "licenses"
New-Item -ItemType Directory -Path $stagedSunshine -Force | Out-Null
New-Item -ItemType Directory -Path $stagedMoonlight -Force | Out-Null
New-Item -ItemType Directory -Path $stagedLicenses -Force | Out-Null
Copy-Item -Path (Join-Path $runtimeRoot "sunshine\*") -Destination $stagedSunshine -Recurse -Force
Copy-Item -Path (Join-Path $runtimeRoot "moonlight\*") -Destination $stagedMoonlight -Recurse -Force
$portableMarker = Join-Path $stagedMoonlight "portable.dat"
if (Test-Path -LiteralPath $portableMarker -PathType Leaf) {
    Remove-Item -LiteralPath $portableMarker -Force
}
Copy-Item -LiteralPath (Join-Path $repositoryRoot "LICENSE") -Destination (Join-Path $stagedLicenses "PeerSpan-GPLv3.txt") -Force
Copy-Item -LiteralPath (Join-Path $repositoryRoot "third_party\sunshine\LICENSE") -Destination (Join-Path $stagedLicenses "Sunshine-GPLv3.txt") -Force
Copy-Item -LiteralPath (Join-Path $repositoryRoot "third_party\moonlight-qt\LICENSE") -Destination (Join-Path $stagedLicenses "Moonlight-GPLv3.txt") -Force
Copy-Item -LiteralPath (Join-Path $repositoryRoot "third_party\virtual-display-driver\LICENSE") -Destination (Join-Path $stagedLicenses "VirtualDrivers-VDD-MIT.txt") -Force
Copy-Item -LiteralPath (Join-Path $repositoryRoot "THIRD_PARTY_NOTICES.md") -Destination (Join-Path $stagedLicenses "THIRD_PARTY_NOTICES.md") -Force
Copy-Item -LiteralPath (Join-Path $repositoryRoot "docs\source-code-offer.zh-CN.md") -Destination (Join-Path $resolvedGameStreamStageRoot "SOURCE_CODE.zh-CN.md") -Force

$gameStreamManifest = [ordered]@{
    schemaVersion = 1
    sunshineVersion = $runtimePackages[0].Version
    sunshineArchiveSha256 = $runtimePackages[0].Sha256
    sunshineSourceCommit = "14ffa6fdaa53f7b51512be2b3d24f3939695403c"
    moonlightVersion = $runtimePackages[1].Version
    moonlightArchiveSha256 = $runtimePackages[1].Sha256
    moonlightSourceCommit = "f786e94c7b2f943e24e65d7d74deb539b827fc84"
    files = @(Get-ChildItem -LiteralPath $resolvedGameStreamStageRoot -Recurse -File | ForEach-Object {
        [ordered]@{
            path = [IO.Path]::GetRelativePath($resolvedGameStreamStageRoot, $_.FullName).Replace("\", "/")
            bytes = $_.Length
            sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
        }
    })
}
$gameStreamManifest | ConvertTo-Json -Depth 5 |
    Set-Content -LiteralPath (Join-Path $resolvedGameStreamStageRoot "manifest.json") -Encoding utf8

$gitStatus = @(& git -C $repositoryRoot status --porcelain --untracked-files=all)
$resourceManifest = [ordered]@{
    schemaVersion = 1
    gitCommit = (& git -C $repositoryRoot rev-parse HEAD).Trim()
    gitDirty = $gitStatus.Count -gt 0
    builtAtUtc = [DateTime]::UtcNow.ToString("o")
    platform = $Platform
    configuration = $Configuration
    testSignedDriver = $false
    vddRelease = $vddPackage.Release
    vddDriverVersion = $vddPackage.DriverVersion
    vddArchiveSha256 = $vddPackage.Sha256
    vddSourceCommit = "d437ebc9b44a14ce6e5cc9c8b7f6beb08d6faf77"
    webview2InstallMode = "offlineInstaller"
    sunshineVersion = $runtimePackages[0].Version
    moonlightVersion = $runtimePackages[1].Version
    files = @($files | ForEach-Object {
        $item = Get-Item -LiteralPath $_.Destination
        $relativePath = [IO.Path]::GetRelativePath($resolvedStageRoot, $item.FullName).Replace("\", "/")
        [ordered]@{
            path = $relativePath
            bytes = $item.Length
            sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash
        }
    })
}
$resourceManifest | ConvertTo-Json -Depth 5 |
    Set-Content -LiteralPath (Join-Path $resolvedStageRoot "manifest.json") -Encoding utf8

Push-Location $repositoryRoot
try {
    & npm.cmd run build:installer --workspace @peerspan/desktop
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri NSIS build failed with exit code $LASTEXITCODE."
    }
} finally {
    Pop-Location
}

$bundleDirectory = Join-Path $targetRoot "$Configuration\bundle\nsis"
$installer = Get-ChildItem -LiteralPath $bundleDirectory -Filter "*.exe" -File |
    Sort-Object LastWriteTimeUtc -Descending |
    Select-Object -First 1
if (-not $installer) {
    throw "The NSIS installer was not found under $bundleDirectory."
}

$installerHash = Get-FileHash -LiteralPath $installer.FullName -Algorithm SHA256
$artifactManifest = [ordered]@{
    schemaVersion = 1
    gitCommit = $resourceManifest.gitCommit
    gitDirty = $resourceManifest.gitDirty
    builtAtUtc = [DateTime]::UtcNow.ToString("o")
    artifact = $installer.Name
    bytes = $installer.Length
    sha256 = $installerHash.Hash
    testSignedDriver = $false
    vddRelease = $vddPackage.Release
    vddDriverVersion = $vddPackage.DriverVersion
    vddArchiveSha256 = $vddPackage.Sha256
    webview2InstallMode = "offlineInstaller"
    sunshineVersion = $runtimePackages[0].Version
    moonlightVersion = $runtimePackages[1].Version
}
$artifactManifestPath = "$($installer.FullName).manifest.json"
$artifactManifest | ConvertTo-Json -Depth 3 |
    Set-Content -LiteralPath $artifactManifestPath -Encoding utf8

Write-Output "PeerSpan test installer: $($installer.FullName)"
Write-Output "SHA-256: $($installerHash.Hash)"
Write-Output "Manifest: $artifactManifestPath"
