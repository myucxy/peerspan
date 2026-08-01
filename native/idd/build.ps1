[CmdletBinding()]
param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",

    [ValidateSet("x64", "ARM64")]
    [string]$Platform = "x64"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$vswhereCandidates = @(
    "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe",
    "D:\Dev\Env\VisualStudio\Installer\vswhere.exe"
)
$vswhere = $vswhereCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $vswhere) {
    throw "vswhere.exe was not found. Install Visual Studio under D:\Dev\Env\VisualStudio."
}

$installationPath = & $vswhere -latest -products "*" -requires Component.Microsoft.Windows.DriverKit -property installationPath
if (-not $installationPath) {
    throw "No Visual Studio instance with the Windows Driver Kit component was found."
}

$msbuild = Join-Path $installationPath "MSBuild\Current\Bin\amd64\MSBuild.exe"
if (-not (Test-Path -LiteralPath $msbuild)) {
    throw "64-bit MSBuild was not found at $msbuild."
}

$toolVersionFile = Join-Path $installationPath "VC\Auxiliary\Build\Microsoft.VCToolsVersion.default.txt"
$toolVersion = (Get-Content -LiteralPath $toolVersionFile -Raw).Trim()
$spectreArchitecture = if ($Platform -eq "x64") { "x64" } else { "arm64" }
$spectreLib = Join-Path $installationPath "VC\Tools\MSVC\$toolVersion\lib\spectre\$spectreArchitecture"
if (-not (Test-Path -LiteralPath $spectreLib)) {
    throw "The MSVC $toolVersion Spectre-mitigated $Platform libraries are not installed."
}

$solution = Join-Path $PSScriptRoot "PeerSpanIdd.sln"
& $msbuild $solution "/m" "/nr:false" "/t:Rebuild" "/p:Configuration=$Configuration" "/p:Platform=$Platform" "/v:minimal" "/nologo"
if ($LASTEXITCODE -ne 0) {
    throw "PeerSpan IddCx build failed with exit code $LASTEXITCODE."
}

Write-Output "PeerSpan IddCx $Configuration|$Platform build completed."
