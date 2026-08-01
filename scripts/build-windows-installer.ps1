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
$allowedStagePrefix = [IO.Path]::GetFullPath($targetRoot).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$resolvedStageRoot = [IO.Path]::GetFullPath($stageRoot)
if (-not $resolvedStageRoot.StartsWith($allowedStagePrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Installer staging path escaped the repository target directory: $resolvedStageRoot"
}

& (Join-Path $repositoryRoot "native\idd\build.ps1") `
    -Configuration $Configuration `
    -Platform $Platform
if ($LASTEXITCODE -ne 0) {
    throw "PeerSpan IddCx build failed with exit code $LASTEXITCODE."
}

if (Test-Path -LiteralPath $resolvedStageRoot) {
    Remove-Item -LiteralPath $resolvedStageRoot -Recurse -Force
}

$stagedRelease = Join-Path $resolvedStageRoot "$Platform\$Configuration"
$stagedPackage = Join-Path $stagedRelease "PeerSpanIdd"
New-Item -ItemType Directory -Path $stagedPackage -Force | Out-Null

$driverOutput = Join-Path $repositoryRoot "native\idd\$Platform\$Configuration"
$files = @(
    @{
        Source = Join-Path $repositoryRoot "native\idd\install-dev.ps1"
        Destination = Join-Path $resolvedStageRoot "install-dev.ps1"
    },
    @{
        Source = Join-Path $repositoryRoot "native\idd\uninstall-dev.ps1"
        Destination = Join-Path $resolvedStageRoot "uninstall-dev.ps1"
    },
    @{
        Source = Join-Path $driverOutput "PeerSpanIdd.cer"
        Destination = Join-Path $stagedRelease "PeerSpanIdd.cer"
    },
    @{
        Source = Join-Path $driverOutput "PeerSpanIdd\PeerSpanIdd.inf"
        Destination = Join-Path $stagedPackage "PeerSpanIdd.inf"
    },
    @{
        Source = Join-Path $driverOutput "PeerSpanIdd\peerspanidd.cat"
        Destination = Join-Path $stagedPackage "peerspanidd.cat"
    },
    @{
        Source = Join-Path $driverOutput "PeerSpanIdd\PeerSpanIdd.dll"
        Destination = Join-Path $stagedPackage "PeerSpanIdd.dll"
    }
)

foreach ($file in $files) {
    if (-not (Test-Path -LiteralPath $file.Source -PathType Leaf)) {
        throw "Required installer resource was not found: $($file.Source)"
    }
    Copy-Item -LiteralPath $file.Source -Destination $file.Destination -Force
}

$gitStatus = @(& git -C $repositoryRoot status --porcelain --untracked-files=all)
$resourceManifest = [ordered]@{
    schemaVersion = 1
    gitCommit = (& git -C $repositoryRoot rev-parse HEAD).Trim()
    gitDirty = $gitStatus.Count -gt 0
    builtAtUtc = [DateTime]::UtcNow.ToString("o")
    platform = $Platform
    configuration = $Configuration
    testSignedDriver = $true
    webview2InstallMode = "offlineInstaller"
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
    testSignedDriver = $true
    webview2InstallMode = "offlineInstaller"
}
$artifactManifestPath = "$($installer.FullName).manifest.json"
$artifactManifest | ConvertTo-Json -Depth 3 |
    Set-Content -LiteralPath $artifactManifestPath -Encoding utf8

Write-Output "PeerSpan test installer: $($installer.FullName)"
Write-Output "SHA-256: $($installerHash.Hash)"
Write-Output "Manifest: $artifactManifestPath"
