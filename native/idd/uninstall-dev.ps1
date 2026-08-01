[CmdletBinding(SupportsShouldProcess, ConfirmImpact = "High")]
param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",

    [ValidateSet("x64", "ARM64")]
    [string]$Platform = "x64",

    [switch]$RemoveTestCertificate,

    [switch]$AcknowledgeSystemChanges
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $AcknowledgeSystemChanges) {
    throw "This operation removes a machine driver package. Re-run with -AcknowledgeSystemChanges after reviewing the script."
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "PeerSpan driver removal requires an elevated PowerShell session."
}

$packages = Get-WindowsDriver -Online -All | Where-Object {
    $_.ProviderName -eq "PeerSpan Project" -and
    $_.OriginalFileName -and
    (Split-Path -Leaf $_.OriginalFileName) -eq "PeerSpanIdd.inf"
}

foreach ($package in $packages) {
    if ($PSCmdlet.ShouldProcess($package.Driver, "Uninstall and delete the PeerSpan IddCx driver package")) {
        & pnputil.exe /delete-driver $package.Driver /uninstall /force
        if ($LASTEXITCODE -ne 0) {
            throw "pnputil failed to remove $($package.Driver) (exit code $LASTEXITCODE)."
        }
    }
}

if ($RemoveTestCertificate) {
    $certificatePath = Join-Path $PSScriptRoot "$Platform\$Configuration\PeerSpanIdd.cer"
    if (-not (Test-Path -LiteralPath $certificatePath -PathType Leaf)) {
        throw "The certificate used to identify the test-signing thumbprint was not found at $certificatePath."
    }
    $certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new($certificatePath)
    foreach ($storeName in @("Root", "TrustedPublisher")) {
        $storePath = "Cert:\LocalMachine\$storeName\$($certificate.Thumbprint)"
        if ((Test-Path -LiteralPath $storePath) -and $PSCmdlet.ShouldProcess(
            $storePath,
            "Remove the PeerSpan development test-signing certificate"
        )) {
            Remove-Item -LiteralPath $storePath
        }
    }
}

if (-not $packages) {
    Write-Output "No installed PeerSpan IddCx driver package was found."
} else {
    Write-Output "PeerSpan development driver removal completed."
}
