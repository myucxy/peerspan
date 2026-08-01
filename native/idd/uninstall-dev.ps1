[CmdletBinding(SupportsShouldProcess, ConfirmImpact = "High")]
param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",

    [ValidateSet("x64", "ARM64")]
    [string]$Platform = "x64",

    [switch]$RemoveTestCertificate,

    [switch]$AcknowledgeSystemChanges,

    [switch]$RemoveFirewallRules,

    [switch]$InstallerMode
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $AcknowledgeSystemChanges) {
    throw "This operation removes a machine driver package. Re-run with -AcknowledgeSystemChanges after reviewing the script."
}

if ($InstallerMode) {
    $ConfirmPreference = "None"
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "PeerSpan driver removal requires an elevated PowerShell session."
}

function Resolve-NativeSystemTool([string]$Name) {
    $candidates = @(
        (Join-Path $env:WINDIR "Sysnative\$Name"),
        (Join-Path $env:WINDIR "System32\$Name")
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return $candidate
        }
    }
    throw "Required Windows system tool was not found: $Name"
}

function Remove-CertificateFromLocalMachineStore(
    [string]$Thumbprint,
    [string]$StoreName
) {
    $store = [Security.Cryptography.X509Certificates.X509Store]::new(
        $StoreName,
        [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine
    )

    try {
        $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
        $matches = $store.Certificates.Find(
            [Security.Cryptography.X509Certificates.X509FindType]::FindByThumbprint,
            $Thumbprint,
            $false
        )
        foreach ($match in $matches) {
            $store.Remove($match)
        }
    } finally {
        $store.Close()
    }
}

$pnpUtil = Resolve-NativeSystemTool "pnputil.exe"

function Find-PeerSpanDriverPackagesWithPnpUtil {
    $output = @(& $pnpUtil /enum-drivers)
    if ($LASTEXITCODE -ne 0) {
        throw "pnputil failed to enumerate driver packages (exit code $LASTEXITCODE)."
    }

    $blocks = ($output -join "`n") -split "(?:\r?\n){2,}"
    @($blocks | ForEach-Object {
        $block = $_
        if ($block -match '(?i)PeerSpan Project' -and
            $block -match '(?i)\bPeerSpanIdd\.inf\b' -and
            $block -match '(?i)\b(oem\d+\.inf)\b') {
            $Matches[1]
        }
    } | Sort-Object -Unique)
}

$packageNames = @()
if ($PSVersionTable.PSEdition -ne "Core") {
    try {
        $packageNames = @(Get-WindowsDriver -Online -All | Where-Object {
            $_.ProviderName -eq "PeerSpan Project" -and
            $_.OriginalFileName -and
            (Split-Path -Leaf $_.OriginalFileName) -eq "PeerSpanIdd.inf"
        } | ForEach-Object { $_.Driver })
    } catch {
        Write-Warning "Get-WindowsDriver is unavailable; falling back to pnputil package enumeration: $($_.Exception.Message)"
    }
}

if (-not $packageNames) {
    $packageNames = @(Find-PeerSpanDriverPackagesWithPnpUtil)
}

foreach ($packageName in $packageNames) {
    if ($PSCmdlet.ShouldProcess($packageName, "Uninstall and delete the PeerSpan IddCx driver package")) {
        & $pnpUtil /delete-driver $packageName /uninstall /force
        if ($LASTEXITCODE -ne 0) {
            throw "pnputil failed to remove $packageName (exit code $LASTEXITCODE)."
        }
    }
}

$softwareDeviceInstanceId = "SWD\PeerSpanVirtualDisplay\PeerSpanVirtualDisplay"
$deviceEnumeration = @(& $pnpUtil /enum-devices /instanceid $softwareDeviceInstanceId)
if ($LASTEXITCODE -eq 0 -and
    ($deviceEnumeration -match [Regex]::Escape($softwareDeviceInstanceId))) {
    if ($PSCmdlet.ShouldProcess(
        $softwareDeviceInstanceId,
        "Remove the disconnected PeerSpan software-device node"
    )) {
        & $pnpUtil /remove-device $softwareDeviceInstanceId
        if ($LASTEXITCODE -ne 0) {
            throw "pnputil failed to remove the PeerSpan software-device node (exit code $LASTEXITCODE)."
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
        $storeDescription = "LocalMachine $storeName certificate store"
        if ($PSCmdlet.ShouldProcess(
            $storeDescription,
            "Remove the PeerSpan development test-signing certificate"
        )) {
            Remove-CertificateFromLocalMachineStore `
                -Thumbprint $certificate.Thumbprint `
                -StoreName $storeName
        }
    }
    $certificate.Reset()
}

if ($RemoveFirewallRules) {
    foreach ($ruleName in @("PeerSpan-LAN-TCP", "PeerSpan-LAN-UDP", "PeerSpan-Sunshine-LAN-TCP", "PeerSpan-Sunshine-LAN-UDP")) {
        if ($PSCmdlet.ShouldProcess($ruleName, "Remove the PeerSpan local-subnet firewall rule")) {
            Get-NetFirewallRule -Name $ruleName -ErrorAction SilentlyContinue |
                Remove-NetFirewallRule -ErrorAction Stop
        }
    }
}

if (-not $packageNames) {
    Write-Output "No installed PeerSpan IddCx driver package was found."
} else {
    Write-Output "PeerSpan development driver removal completed."
}
